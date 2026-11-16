//! The vector table's rows, in a mapped file (issue 21 of 1.0.7).
//!
//! A start used to build a row in memory for every chunk the index holds: 64
//! bytes a row, a map from each node's id to its rows, a map from each text to
//! its rows, and the node ids themselves, about 300 bytes a chunk that grew with
//! the store. Now the start writes them once into a file with no name and maps
//! it read-only ([`Base`]); what the kernel needs it pages, and the process's
//! own memory holds only what changes: a bit a row for "alive", and four bytes a
//! row for the vector it answers with. A node that arrives after the file was
//! made (a commit) has its rows in the table's own `rows` (the *tail*), in
//! memory, as every row used to; once [`FOLD_ROWS`] have gathered, or half the
//! rows are dead, the table is written out again ([`Table::fold`]).
//!
//! **Row numbers.** The file's rows are `0..B` and the tail's are `B..`, so a
//! row has one number whichever it is in, the scan goes through them in that
//! order, and a tie goes to the earlier row as it always did. The file's rows
//! are in node-id order and a node's rows are together, in chunk order: the
//! order a start's reconcile always gave them.
//!
//! **What is in the file** (little-endian, each section on a 16-byte boundary):
//! a header; the nodes' ids end to end, with their offsets and each node's
//! first row; a 48-byte key a row (chunk, node, position, text hash, tokens,
//! session, kind, external); and the rows' numbers sorted by text hash, which
//! answers "which rows hold this text".

use std::collections::HashMap;
use std::fs::File;
use std::io;
use std::ops::Range;
use std::os::unix::fs::FileExt;
use std::path::Path;
use std::sync::atomic::Ordering;
use std::sync::Arc;

use super::entries::Holders;
use super::store::{align, tmpfile, u128_at, u32_at, u64_at, Bits, Out};
use super::{Backlog, NodeChunks, Row, Table, NO_VEC};
use crate::mapped::Map;

const MAGIC: &[u8; 8] = b"thsrows1";
const HEAD: u64 = 128;
const KEY: usize = 48;

/// The tail is written into the file at this many rows.
pub(super) const FOLD_ROWS: usize = 16_384;

/// A row's vector, packed: the file among the table's caches in the top four
/// bits and the entry in the rest. [`NO_VEC`] is none.
fn pack(v: Option<(u8, u32)>) -> u32 {
    match v {
        Some((c, e)) if c < 16 && e < 1 << 28 => u32::from(c) << 28 | e,
        _ => NO_VEC,
    }
}

fn unpack(p: u32) -> Option<(u8, u32)> {
    (p != NO_VEC).then_some(((p >> 28) as u8, p & ((1 << 28) - 1)))
}

/// What a row's key holds.
#[derive(Debug, Clone, Copy)]
pub(super) struct Key {
    pub chunk: u32,
    pub node: u32,
    pub position: u64,
    pub hash: u128,
    pub tokens: u32,
    pub session: u32,
    pub kind: u16,
    pub external: bool,
}

/// A row to write: a [`Key`] without its node, which the writer numbers.
#[derive(Debug, Clone, Copy)]
pub(super) struct RowOut {
    pub chunk: u32,
    pub position: u64,
    pub hash: u128,
    pub tokens: u32,
    pub session: u32,
    pub kind: u16,
    pub external: bool,
}

/// The mapped rows, and the state of each that the table changes.
pub(super) struct Base {
    map: Map,
    _file: File,
    rows: usize,
    nodes: usize,
    node_off: usize,
    node_first: usize,
    keys: usize,
    sh: usize,
    sr: usize,
    ids: usize,
    /// Row `i` is in the index still.
    alive: Bits,
    /// Each row's vector, packed.
    vecs: Vec<u32>,
}

impl Base {
    pub fn len(&self) -> usize {
        self.rows
    }

    pub fn alive(&self, i: usize) -> bool {
        self.alive.get(i)
    }

    pub fn kill(&mut self, i: usize) {
        self.alive.set(i, false);
    }

    pub fn vec(&self, i: usize) -> Option<(u8, u32)> {
        unpack(self.vecs[i])
    }

    pub fn set_vec(&mut self, i: usize, v: Option<(u8, u32)>) {
        self.vecs[i] = pack(v);
    }

    pub fn key(&self, i: usize) -> Key {
        let b = self.map.bytes();
        let at = self.keys + KEY * i;
        Key {
            chunk: u32_at(b, at),
            node: u32_at(b, at + 4),
            position: u64_at(b, at + 8),
            hash: u128_at(b, at + 16),
            tokens: u32_at(b, at + 32),
            session: u32_at(b, at + 36),
            kind: u16::from_le_bytes([b[at + 40], b[at + 41]]),
            external: b[at + 42] != 0,
        }
    }

    /// One field of row `i`'s key, read alone: a scan's filter wants one or two
    /// of them, not the whole key.
    pub fn position(&self, i: usize) -> u64 {
        u64_at(self.map.bytes(), self.keys + KEY * i + 8)
    }

    pub fn session(&self, i: usize) -> u32 {
        u32_at(self.map.bytes(), self.keys + KEY * i + 36)
    }

    pub fn kind(&self, i: usize) -> u16 {
        let at = self.keys + KEY * i + 40;
        let b = self.map.bytes();
        u16::from_le_bytes([b[at], b[at + 1]])
    }

    pub fn external(&self, i: usize) -> bool {
        self.map.bytes()[self.keys + KEY * i + 42] != 0
    }

    /// Node `ord`'s id.
    pub fn node_id(&self, ord: u32) -> &str {
        let b = self.map.bytes();
        let (from, to) = (
            u64_at(b, self.node_off + 8 * ord as usize) as usize,
            u64_at(b, self.node_off + 8 * (ord as usize + 1)) as usize,
        );
        std::str::from_utf8(&b[self.ids + from..self.ids + to]).unwrap_or("")
    }

    /// The node with this id, if the file has it.
    pub fn find(&self, id: &str) -> Option<u32> {
        let (mut lo, mut hi) = (0u32, self.nodes as u32);
        while lo < hi {
            let mid = lo + (hi - lo) / 2;
            match self.node_id(mid).cmp(id) {
                std::cmp::Ordering::Less => lo = mid + 1,
                std::cmp::Ordering::Greater => hi = mid,
                std::cmp::Ordering::Equal => return Some(mid),
            }
        }
        None
    }

    /// Node `ord`'s rows, alive or not.
    pub fn node_rows(&self, ord: u32) -> Range<usize> {
        let b = self.map.bytes();
        let at = |o: u32| u32_at(b, self.node_first + 4 * o as usize) as usize;
        at(ord)..at(ord + 1)
    }

    pub fn nodes(&self) -> usize {
        self.nodes
    }

    fn sorted_hash(&self, p: usize) -> u128 {
        u128_at(self.map.bytes(), self.sh + 16 * p)
    }

    fn sorted_row(&self, p: usize) -> usize {
        u32_at(self.map.bytes(), self.sr + 4 * p) as usize
    }

    /// Where, among the rows sorted by text, those with `hash` are.
    fn run(&self, hash: u128) -> Range<usize> {
        let (mut lo, mut hi) = (0usize, self.rows);
        while lo < hi {
            let mid = lo + (hi - lo) / 2;
            if self.sorted_hash(mid) < hash {
                lo = mid + 1;
            } else {
                hi = mid;
            }
        }
        let mut end = lo;
        while end < self.rows && self.sorted_hash(end) == hash {
            end += 1;
        }
        lo..end
    }

    /// The alive rows holding `hash`, in row order.
    pub fn holders(&self, hash: u128) -> Vec<u32> {
        self.run(hash)
            .map(|p| self.sorted_row(p))
            .filter(|&i| self.alive(i))
            .map(|i| i as u32)
            .collect()
    }

    pub fn holds(&self, hash: u128) -> bool {
        self.run(hash).any(|p| self.alive(self.sorted_row(p)))
    }

    /// Each alive row's text hash once, in hash order.
    pub fn each_alive_text(&self, mut f: impl FnMut(u128)) {
        let mut last: Option<u128> = None;
        for p in 0..self.rows {
            let h = self.sorted_hash(p);
            if last != Some(h) && self.alive(self.sorted_row(p)) {
                f(h);
                last = Some(h);
            }
        }
    }

    /// Read the rows' keys in now, in one run: a scan is about to touch them.
    pub fn prefetch(&self) {
        self.map.will_need(self.keys, KEY * self.rows);
    }

    /// Let the process's copy of every page go (the kernel keeps them).
    pub fn release(&self) {
        self.map.release(0, self.map.bytes().len());
    }

    /// The rows' pages out of the process and out of the kernel's cache (for
    /// measuring a search that finds its file cold).
    #[cfg(test)]
    pub fn evict(&self) {
        use std::os::fd::AsRawFd;
        self.release();
        let _ = self._file.sync_data();
        // SAFETY: a valid descriptor; advice only.
        unsafe { libc::posix_fadvise(self._file.as_raw_fd(), 0, 0, libc::POSIX_FADV_DONTNEED) };
    }

    /// What the table holds in memory for these rows.
    pub fn held(&self) -> usize {
        self.alive.bytes() + self.vecs.capacity() * 4
    }
}

/// A rows file being written, a node at a time in node-id order.
pub(super) struct BaseWriter {
    file: File,
    cap_rows: usize,
    cap_nodes: usize,
    node_off_at: u64,
    node_first_at: u64,
    keys_at: u64,
    sh_at: u64,
    sr_at: u64,
    ids_at: u64,
    keys: Out,
    ids: Out,
    node_off: Vec<u64>,
    node_first: Vec<u32>,
    pairs: Vec<(u128, u32)>,
    vecs: Vec<u32>,
    last: String,
}

impl BaseWriter {
    /// A writer for at most `nodes` nodes and `rows` rows.
    pub fn new(dir: &Path, nodes: usize, rows: usize) -> io::Result<BaseWriter> {
        let n = nodes as u64 + 1;
        let node_off_at = HEAD;
        let node_first_at = align(node_off_at + 8 * n);
        let keys_at = align(node_first_at + 4 * n);
        let sh_at = align(keys_at + KEY as u64 * rows as u64);
        let sr_at = align(sh_at + 16 * rows as u64);
        let ids_at = align(sr_at + 4 * rows as u64);
        Ok(BaseWriter {
            file: tmpfile(dir)?,
            cap_rows: rows,
            cap_nodes: nodes,
            node_off_at,
            node_first_at,
            keys_at,
            sh_at,
            sr_at,
            ids_at,
            keys: Out::new(keys_at),
            ids: Out::new(ids_at),
            node_off: Vec::with_capacity(nodes + 1),
            node_first: Vec::with_capacity(nodes + 1),
            pairs: Vec::with_capacity(rows),
            vecs: Vec::new(),
            last: String::new(),
        })
    }

    /// Node `id`'s rows, in chunk order. Nodes come in id order.
    pub fn push_node(&mut self, id: &str, rows: &[RowOut]) -> io::Result<()> {
        if self.node_first.len() >= self.cap_nodes
            || self.pairs.len() + rows.len() > self.cap_rows
            || id <= self.last.as_str() && !self.node_first.is_empty()
        {
            return Err(io::Error::other(
                "a node the rows file has no room for, or out of order",
            ));
        }
        self.last.clear();
        self.last.push_str(id);
        self.node_off.push(self.ids.pos() - self.ids_at);
        self.node_first.push(self.pairs.len() as u32);
        self.ids.put(&self.file, id.as_bytes())?;
        let ord = self.node_first.len() as u32 - 1;
        let mut k = [0u8; KEY];
        for r in rows {
            k[0..4].copy_from_slice(&r.chunk.to_le_bytes());
            k[4..8].copy_from_slice(&ord.to_le_bytes());
            k[8..16].copy_from_slice(&r.position.to_le_bytes());
            k[16..32].copy_from_slice(&r.hash.to_le_bytes());
            k[32..36].copy_from_slice(&r.tokens.to_le_bytes());
            k[36..40].copy_from_slice(&r.session.to_le_bytes());
            k[40..42].copy_from_slice(&r.kind.to_le_bytes());
            k[42] = u8::from(r.external);
            self.keys.put(&self.file, &k)?;
            self.pairs.push((r.hash, self.pairs.len() as u32));
        }
        Ok(())
    }

    /// The rows' vectors, packed, in row order, as the caller has them.
    pub fn set_vecs(&mut self, vecs: Vec<u32>) {
        self.vecs = vecs;
    }

    /// Write the rest and map it. Every row alive, and a vector for each as
    /// [`BaseWriter::set_vecs`] gave, or none.
    pub fn finish(mut self) -> io::Result<Base> {
        let rows = self.pairs.len();
        let nodes = self.node_first.len();
        self.node_off.push(self.ids.pos() - self.ids_at);
        self.node_first.push(rows as u32);
        self.keys.flush(&self.file)?;
        self.ids.flush(&self.file)?;
        let ids_len = self.ids.pos() - self.ids_at;
        let mut col: Vec<u8> = Vec::with_capacity(8 * (nodes + 1));
        for o in &self.node_off {
            col.extend_from_slice(&o.to_le_bytes());
        }
        self.file.write_all_at(&col, self.node_off_at)?;
        col.clear();
        for f in &self.node_first {
            col.extend_from_slice(&f.to_le_bytes());
        }
        self.file.write_all_at(&col, self.node_first_at)?;
        self.pairs.sort_unstable();
        let mut hs: Vec<u8> = Vec::with_capacity(16 * rows);
        let mut rs: Vec<u8> = Vec::with_capacity(4 * rows);
        for (h, r) in &self.pairs {
            hs.extend_from_slice(&h.to_le_bytes());
            rs.extend_from_slice(&r.to_le_bytes());
        }
        self.file.write_all_at(&hs, self.sh_at)?;
        self.file.write_all_at(&rs, self.sr_at)?;
        let mut head = [0u8; HEAD as usize];
        head[..8].copy_from_slice(MAGIC);
        for (i, v) in [
            rows as u64,
            nodes as u64,
            self.node_off_at,
            self.node_first_at,
            self.keys_at,
            self.sh_at,
            self.sr_at,
            self.ids_at,
        ]
        .iter()
        .enumerate()
        {
            head[8 + 8 * i..16 + 8 * i].copy_from_slice(&v.to_le_bytes());
        }
        self.file.write_all_at(&head, 0)?;
        self.file.set_len(self.ids_at + ids_len)?;
        let map = Map::random(&self.file)?;
        let mut vecs = std::mem::take(&mut self.vecs);
        vecs.resize(rows, NO_VEC);
        // The writer's lists and buffers are freed with it: give them back
        // before the table allocates what it keeps (its own vectors, the
        // names, the backlog), so those do not sit above them.
        drop((col, hs, rs));
        self.pairs = Vec::new();
        self.node_off = Vec::new();
        self.node_first = Vec::new();
        super::release_memory();
        Ok(Base {
            map,
            _file: self.file,
            rows,
            nodes,
            node_off: self.node_off_at as usize,
            node_first: self.node_first_at as usize,
            keys: self.keys_at as usize,
            sh: self.sh_at as usize,
            sr: self.sr_at as usize,
            ids: self.ids_at as usize,
            alive: Bits::filled(rows, true),
            vecs,
        })
    }
}

/// A start's rows being written, and what the table must set right when it
/// has them.
pub(super) struct Building {
    pub writer: BaseWriter,
    pub generation: u64,
}

impl Table {
    /// Rows in the file.
    pub(super) fn base_len(&self) -> usize {
        self.base.as_ref().map_or(0, Base::len)
    }

    /// Row `i`'s place in the tail, if it is there.
    pub(super) fn in_tail(&self, i: u32) -> Option<usize> {
        (i as usize).checked_sub(self.base_len())
    }

    /// Rows in all, dead or alive.
    pub(super) fn row_count(&self) -> usize {
        self.base_len() + self.rows.len()
    }

    pub(super) fn is_alive(&self, i: u32) -> bool {
        match (&self.base, self.in_tail(i)) {
            (Some(b), None) => b.alive(i as usize),
            (_, Some(k)) => self.rows[k].alive,
            _ => false,
        }
    }

    pub(super) fn row_vec(&self, i: u32) -> Option<(u8, u32)> {
        match (&self.base, self.in_tail(i)) {
            (Some(b), None) => b.vec(i as usize),
            (_, Some(k)) => self.rows[k].vec(),
            _ => None,
        }
    }

    pub(super) fn set_row_vec(&mut self, i: u32, v: Option<(u8, u32)>) {
        let tail = self.in_tail(i);
        match (self.base.as_mut(), tail) {
            (Some(b), None) => b.set_vec(i as usize, v),
            (_, Some(k)) => self.rows[k].set_vec(v),
            _ => {}
        }
    }

    pub(super) fn row_hash(&self, i: u32) -> u128 {
        match (&self.base, self.in_tail(i)) {
            (Some(b), None) => b.key(i as usize).hash,
            (_, Some(k)) => self.rows[k].hash,
            _ => 0,
        }
    }

    pub(super) fn row_position(&self, i: u32) -> u64 {
        match (&self.base, self.in_tail(i)) {
            (Some(b), None) => b.key(i as usize).position,
            (_, Some(k)) => self.rows[k].position,
            _ => 0,
        }
    }

    pub(super) fn row_tokens(&self, i: u32) -> u32 {
        match (&self.base, self.in_tail(i)) {
            (Some(b), None) => b.key(i as usize).tokens,
            (_, Some(k)) => self.rows[k].tokens,
            _ => 0,
        }
    }

    /// Row `i` is of node `id`, without making the id a string.
    pub(super) fn row_node_is(&self, i: u32, id: &str) -> bool {
        match (&self.base, self.in_tail(i)) {
            (Some(b), None) => b.node_id(b.key(i as usize).node) == id,
            (_, Some(k)) => &*self.rows[k].node == id,
            _ => false,
        }
    }

    pub(super) fn row_node(&self, i: u32) -> Arc<str> {
        match (&self.base, self.in_tail(i)) {
            (Some(b), None) => Arc::from(b.node_id(b.key(i as usize).node)),
            (_, Some(k)) => self.rows[k].node.clone(),
            _ => Arc::from(""),
        }
    }

    /// Row `i` as a whole, a copy.
    pub(super) fn row(&self, i: u32) -> Row {
        match (&self.base, self.in_tail(i)) {
            (Some(b), None) => {
                let k = b.key(i as usize);
                let (cache, entry) = b.vec(i as usize).unwrap_or((0, NO_VEC));
                Row {
                    node: Arc::from(b.node_id(k.node)),
                    chunk: k.chunk,
                    position: k.position,
                    session: k.session,
                    kind: k.kind,
                    external: k.external,
                    hash: k.hash,
                    tokens: k.tokens,
                    alive: b.alive(i as usize),
                    cache,
                    entry,
                }
            }
            (_, Some(k)) => self.rows[k].clone(),
            _ => unreachable!("a row number past the table"),
        }
    }

    /// Node `id`'s alive rows, in chunk order.
    pub(super) fn rows_of(&self, id: &str) -> Vec<u32> {
        if let Some(rows) = self.by_node.get(id) {
            return rows.clone();
        }
        let Some(b) = &self.base else {
            return Vec::new();
        };
        match b.find(id) {
            Some(ord) => b
                .node_rows(ord)
                .filter(|&i| b.alive(i))
                .map(|i| i as u32)
                .collect(),
            None => Vec::new(),
        }
    }

    /// The alive rows holding text `hash`, in row order.
    pub(super) fn holders(&self, hash: u128) -> Vec<u32> {
        let mut out = self
            .base
            .as_ref()
            .map_or_else(Vec::new, |b| b.holders(hash));
        if let Some(h) = self.by_hash.get(&hash) {
            out.extend_from_slice(h.as_slice());
        }
        out
    }

    /// An alive row holds text `hash`.
    pub(super) fn holds(&self, hash: u128) -> bool {
        self.by_hash.contains_key(&hash) || self.base.as_ref().is_some_and(|b| b.holds(hash))
    }

    /// Every text an alive row holds, once.
    pub(super) fn each_alive_text(&self, mut f: impl FnMut(u128)) {
        if let Some(b) = &self.base {
            b.each_alive_text(&mut f);
        }
        for &h in self.by_hash.keys() {
            f(h);
        }
    }

    /// Each alive row's vector, in row order.
    pub(super) fn alive_vecs(&self) -> impl Iterator<Item = Option<(u8, u32)>> + '_ {
        let base = self
            .base
            .iter()
            .flat_map(|b| (0..b.len()).filter(|&i| b.alive(i)).map(|i| b.vec(i)));
        base.chain(self.rows.iter().filter(|r| r.alive).map(Row::vec))
    }

    /// Row `i` is no longer alive (its node's other rows are the caller's).
    pub(super) fn kill(&mut self, i: u32) {
        let tail = self.in_tail(i);
        match (self.base.as_mut(), tail) {
            (Some(b), None) => b.kill(i as usize),
            (_, Some(k)) => self.rows[k].alive = false,
            _ => {}
        }
    }

    /// Let the process's copy of every page of the mapped files go: the kernel
    /// keeps them, and the next search faults back in what it reads. Done when
    /// the model unloads, so a tender left idle holds none of the table's pages.
    pub(super) fn release_pages(&self) {
        self.cold.store(true, Ordering::Relaxed);
        if let Some(b) = &self.base {
            b.release();
        }
        for c in &self.caches {
            c.release_pages();
        }
    }

    /// A search is about to scan every row: ask for the files to be read in
    /// whole and at once, which is all that happens when they are cached, and
    /// one sequential read each when the kernel had dropped them.
    pub(super) fn prefetch(&self) {
        if let Some(b) = &self.base {
            b.prefetch();
        }
        for c in &self.caches {
            if let Some(s) = &c.scan {
                s.prefetch();
            }
        }
    }

    /// Every mapped page out of the process and the kernel's cache.
    #[cfg(test)]
    pub(super) fn evict_files(&self) {
        self.cold.store(true, Ordering::Relaxed);
        if let Some(b) = &self.base {
            b.evict();
        }
        for c in &self.caches {
            if let Some(s) = &c.scan {
                s.evict();
            }
        }
    }

    /// The table's rows are written out again, with the dead gone and the tail
    /// in the file, when the tail is long or half the rows are dead. Where a
    /// table has no directory to write in (a test's) the dead rows are
    /// dropped in memory, as they always were.
    pub(super) fn fold_if_due(&mut self) {
        let long = self.dir.is_some() && self.rows.len() >= FOLD_ROWS;
        let dead = self.dead > 1024 && self.dead > self.row_count() / 2;
        if !long && !dead {
            return;
        }
        if self.dir.is_some() {
            if let Err(e) = self.fold() {
                tracing::warn!(error = %e, "index: the vector rows were not written out again; they stay in memory");
            }
        } else {
            self.compact_rows();
        }
    }

    /// Write every alive row into a new file, node by node in id order, and
    /// swap it for the old one and the tail. Row numbers change; each row
    /// keeps its vector.
    pub(super) fn fold(&mut self) -> io::Result<()> {
        let Some(dir) = self.dir.clone() else {
            return Ok(());
        };
        let mut tail: Vec<(Arc<str>, Vec<u32>)> = self
            .by_node
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        tail.sort_by(|a, b| a.0.cmp(&b.0));
        let nodes = tail.len() + self.base.as_ref().map_or(0, Base::nodes);
        let alive = self.row_count() - self.dead;
        let mut w = BaseWriter::new(&dir, nodes, alive)?;
        let mut vecs: Vec<u32> = Vec::with_capacity(alive);
        let mut tail = tail.into_iter().peekable();
        let mut buf: Vec<RowOut> = Vec::new();
        let mut put = |w: &mut BaseWriter,
                       vecs: &mut Vec<u32>,
                       id: &str,
                       rows: &mut Vec<(RowOut, u32)>|
         -> io::Result<()> {
            if rows.is_empty() {
                return Ok(());
            }
            buf.clear();
            buf.extend(rows.iter().map(|r| r.0));
            vecs.extend(rows.iter().map(|r| r.1));
            rows.clear();
            w.push_node(id, &buf)
        };
        let mut rows: Vec<(RowOut, u32)> = Vec::new();
        let base_nodes = self.base.as_ref().map_or(0, Base::nodes) as u32;
        for ord in 0..base_nodes {
            let b = self
                .base
                .as_ref()
                .ok_or_else(|| io::Error::other("no rows"))?;
            let id = b.node_id(ord);
            while let Some((tid, trows)) = tail.next_if(|t| &*t.0 < id) {
                for &i in &trows {
                    rows.push(self.out_row(i));
                }
                put(&mut w, &mut vecs, &tid, &mut rows)?;
            }
            if let Some((tid, trows)) = tail.next_if(|t| &*t.0 == id) {
                for &i in &trows {
                    rows.push(self.out_row(i));
                }
                put(&mut w, &mut vecs, &tid, &mut rows)?;
                continue;
            }
            for i in b.node_rows(ord).filter(|&i| b.alive(i)) {
                rows.push(self.out_row(i as u32));
            }
            put(&mut w, &mut vecs, id, &mut rows)?;
        }
        for (tid, trows) in tail {
            for &i in &trows {
                rows.push(self.out_row(i));
            }
            put(&mut w, &mut vecs, &tid, &mut rows)?;
        }
        w.set_vecs(vecs);
        let base = w.finish()?;
        self.base = Some(base);
        self.rows = Vec::new();
        self.by_node = HashMap::new();
        self.by_hash = HashMap::new();
        self.dead = 0;
        Ok(())
    }

    /// Row `i` as the writer takes it, and its packed vector.
    fn out_row(&self, i: u32) -> (RowOut, u32) {
        let r = self.row(i);
        (
            RowOut {
                chunk: r.chunk,
                position: r.position,
                hash: r.hash,
                tokens: r.tokens,
                session: r.session,
                kind: r.kind,
                external: r.external,
            },
            pack(r.vec()),
        )
    }

    /// A start is about to hand over `nodes` nodes of `chunks` chunks: where the
    /// table has a directory and no file of rows yet, they are written into one
    /// as they come; otherwise they are added in memory, with room made first.
    pub(super) fn start_rows(&mut self, nodes: usize, chunks: usize, to_file: bool) {
        self.building = None;
        if to_file && self.base.is_none() {
            if let Some(dir) = &self.dir {
                let made =
                    std::fs::create_dir_all(dir).and_then(|()| BaseWriter::new(dir, nodes, chunks));
                match made {
                    Ok(writer) => {
                        self.building = Some(Building {
                            writer,
                            generation: self.generation,
                        });
                        return;
                    }
                    Err(e) => {
                        tracing::warn!(error = %e, "index: the vector rows could not be written to a file; they are kept in memory")
                    }
                }
            }
        }
        self.reserve_for(nodes, chunks);
    }

    /// Whether a start's rows are going into a file.
    pub(super) fn is_building(&self) -> bool {
        self.building.is_some()
    }

    /// Write a batch of a start's nodes to the file being made.
    pub(super) fn write_nodes(&mut self, batch: &mut Vec<NodeChunks>) -> io::Result<()> {
        let mut out: Vec<RowOut> = Vec::new();
        for n in batch.drain(..) {
            let (session, kind) = (self.sessions.id(&n.session), self.kinds.id(&n.kind) as u16);
            out.clear();
            out.extend(n.chunks.iter().map(|c| RowOut {
                chunk: c.chunk,
                position: n.position,
                hash: c.hash,
                tokens: c.tokens,
                session,
                kind,
                external: n.external,
            }));
            let b = self
                .building
                .as_mut()
                .ok_or_else(|| io::Error::other("no rows are being written"))?;
            b.writer.push_node(&n.node_id, &out)?;
        }
        Ok(())
    }

    /// A start has handed over every node: the file is whole, and the table
    /// reads its rows from it.
    pub(super) fn finish_rows(&mut self) -> io::Result<()> {
        let Some(b) = self.building.take() else {
            return Ok(());
        };
        if b.generation != self.generation {
            return Ok(());
        }
        let base = b.writer.finish()?;
        self.install(base);
        Ok(())
    }

    /// Install the rows a start wrote: the tail's row numbers move up by the
    /// file's length, a node the tail holds (a commit since the read began) or
    /// one forgotten meanwhile keeps nothing of the file's, and every row is
    /// pointed at its vector.
    pub(super) fn install(&mut self, base: Base) {
        let b = base.len() as u32;
        for rows in self.by_node.values_mut() {
            rows.iter_mut().for_each(|i| *i += b);
        }
        for h in self.by_hash.values_mut() {
            let moved: Vec<u32> = h.as_slice().iter().map(|i| i + b).collect();
            let mut fresh = Holders::default();
            moved.into_iter().for_each(|i| fresh.push(i));
            *h = fresh;
        }
        self.base = Some(base);
        let mut gone: Vec<String> = self.by_node.keys().map(|k| k.to_string()).collect();
        gone.extend(self.forgot_nodes.iter().cloned());
        for id in gone {
            self.kill_base_node(&id);
        }
        let chunks: Vec<(String, u32)> = self.forgot_chunks.iter().cloned().collect();
        for (id, chunk) in chunks {
            if let Some(b) = &self.base {
                if let Some(ord) = b.find(&id) {
                    let hit = b
                        .node_rows(ord)
                        .find(|&i| b.alive(i) && b.key(i).chunk == chunk);
                    if let Some(i) = hit {
                        self.kill_base_row(i as u32);
                    }
                }
            }
        }
        self.backlog = Backlog::default();
        for i in 0..self.row_count() as u32 {
            if self.is_alive(i) {
                self.place(i);
            }
        }
    }

    fn kill_base_node(&mut self, id: &str) {
        let Some(b) = &self.base else {
            return;
        };
        let Some(ord) = b.find(id) else {
            return;
        };
        let rows: Vec<usize> = b.node_rows(ord).filter(|&i| b.alive(i)).collect();
        for i in rows {
            self.kill_base_row(i as u32);
        }
    }

    fn kill_base_row(&mut self, i: u32) {
        let hash = self.row_hash(i);
        self.kill(i);
        self.dead += 1;
        if !self.holds(hash) {
            self.backlog.remove(hash);
            self.mark(hash, true);
        }
    }
}
