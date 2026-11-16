//! The tier file: a vector file's records laid out by how a search wants them (issue 21 of
//! 1.0.7), `<stamp key>.tier`, beside `<stamp key>.scan`.
//!
//! The scan file keeps the records in the order they were embedded, so a search that wants only
//! some of them touches pages all over it. The tier file holds a *copy* of the int8 cuts of the
//! records it places, in the order a search wants them: first the **hot region** (what is searched
//! on every query), then the **groups** (records of similar material, each group's together, of
//! which a query reads the nearest few). A region is one run of the file, so reading it is one
//! sequential read, and the pages of a group a search did not read are never touched.
//!
//! **It is a projection of the scan file and carries no truth of its own**: the records' hashes,
//! scales and cuts are the scan file's, the 768-d vectors are the `.vec` file's, and this file
//! says only which record is in which region. Anything that makes it doubtful (absent, torn, of
//! another file, made before a compaction renumbered the records) and it is left out and the
//! search is exact, over the scan file, as it was before this file existed; it is made again by
//! the next layout. It is written beside its name, synced and renamed, never in place, so a map
//! of it is of a file that does not change, and a crash leaves the old file or the new one.
//!
//! **What says it is the file it was made from**: its length and the offsets of its columns, a
//! checksum of its order column, and the greatest entry it places with that entry's text hash read
//! back from the scan file. A compaction that drops a record at or before that entry renumbers it,
//! so its hash no longer matches; one that drops only later records changes nothing the file says.
//! Every entry is placed once at most (checked at open), so no record is in two regions.
//!
//! **Entries the file does not place** (the records embedded since, and those no alive row held
//! when it was made) are searched exactly on every query: a new text is hot until a layout places
//! it.
//!
//! Layout (little-endian, each column on a 16-byte boundary): a 192-byte header; `order`, the
//! entry numbers in region order; `scale`, their scales; `q`, their cuts; `starts`, where each
//! group begins in region order (`hot_n` for the first, then one more for the end); the groups'
//! centres (int8) and their scales.

use std::fs::{File, OpenOptions};
use std::io;
use std::ops::Range;
use std::os::unix::fs::{FileExt, OpenOptionsExt};
use std::path::{Path, PathBuf};

use super::store::{align, f32_at, u128_at, u32_at, u64_at, Bits, Out};
use crate::mapped::Map;

const MAGIC: &[u8; 8] = b"thstier1";
const HEAD: u64 = 192;

/// One region's worth of what a layout decided, to be written.
pub(super) struct Layout<'a> {
    pub cut: usize,
    /// The hot region's entries.
    pub hot: &'a [u32],
    /// Each group's entries, and its centre (int8 and its scale).
    pub groups: &'a [Group],
    /// How many cold records the centres were made from (the drift rule reads it back).
    pub made_from: u64,
}

/// A group of similar records.
pub(super) struct Group {
    pub entries: Vec<u32>,
    pub centre: Vec<i8>,
    pub scale: f32,
}

/// A tier file, mapped.
pub(super) struct TierFile {
    map: Map,
    _file: File,
    cut: usize,
    hot_n: usize,
    assigned: usize,
    groups: usize,
    order: usize,
    scale: usize,
    q: usize,
    starts: usize,
    centres: usize,
    cscale: usize,
    /// The entries the file places: a bit each.
    placed: Bits,
    /// Those of them in the hot region.
    hot: Bits,
    made_from: u64,
    /// One past the greatest entry placed: records at or after it are newer than the layout.
    covers: usize,
}

pub(super) fn path(dir: &Path, key: &str) -> PathBuf {
    dir.join(format!("{key}.tier"))
}

/// The entries of an order column as a set, each placed once and the greatest one `max_entry`.
fn placed_bits(order: &[u8], max_entry: u32) -> Result<Bits, String> {
    let n = order.len() / 4;
    let mut placed = Bits::filled(if n > 0 { max_entry as usize + 1 } else { 0 }, false);
    let mut greatest = 0;
    for p in 0..n {
        let e = u32_at(order, 4 * p);
        if e > max_entry || placed.get(e as usize) {
            return Err("an entry placed twice, or past the greatest".into());
        }
        placed.set(e as usize, true);
        greatest = greatest.max(e);
    }
    if n > 0 && greatest != max_entry {
        return Err("the greatest entry is not placed".into());
    }
    Ok(placed)
}

/// The groups' starts run in order from the end of the hot region to the end of the placed
/// records.
fn check_starts(
    b: &[u8],
    starts: usize,
    groups: usize,
    hot_n: usize,
    assigned: usize,
) -> Result<(), String> {
    let mut last = hot_n as u64;
    for g in 0..=groups {
        let s = u64_at(b, starts + 8 * g);
        if (g == 0 && s != hot_n as u64) || s < last || s > assigned as u64 {
            return Err("group starts out of order".into());
        }
        last = s;
    }
    if groups == 0 && last != hot_n as u64 || groups > 0 && last != assigned as u64 {
        return Err("the groups do not end where the file does".into());
    }
    Ok(())
}

impl TierFile {
    /// Records in the hot region.
    pub fn hot_n(&self) -> usize {
        self.hot_n
    }

    /// Records placed, hot and in groups.
    pub fn assigned(&self) -> usize {
        self.assigned
    }

    pub fn groups(&self) -> usize {
        self.groups
    }

    /// One past the greatest entry the file places.
    pub fn covers(&self) -> usize {
        self.covers
    }

    pub fn made_from(&self) -> u64 {
        self.made_from
    }

    /// Whether entry `e` is placed in a region.
    pub fn places(&self, e: u32) -> bool {
        self.placed.get(e as usize)
    }

    /// Whether entry `e` is in the hot region.
    pub fn is_hot(&self, e: u32) -> bool {
        self.hot.get(e as usize)
    }

    /// The entry at region position `p`.
    pub fn entry(&self, p: usize) -> u32 {
        u32_at(self.map.bytes(), self.order + 4 * p)
    }

    pub fn scale(&self, p: usize) -> f32 {
        f32_at(self.map.bytes(), self.scale + 4 * p)
    }

    pub fn q(&self, p: usize) -> &[i8] {
        self.map.i8s(self.q + self.cut * p, self.cut)
    }

    /// Region positions of group `g`.
    pub fn group(&self, g: usize) -> Range<usize> {
        let b = self.map.bytes();
        let at = |g: usize| u64_at(b, self.starts + 8 * g) as usize;
        at(g)..at(g + 1)
    }

    pub fn centre(&self, g: usize) -> (&[i8], f32) {
        (
            self.map.i8s(self.centres + self.cut * g, self.cut),
            f32_at(self.map.bytes(), self.cscale + 4 * g),
        )
    }

    /// Ask the kernel for the hot region now, in one run: the model has loaded and a query is
    /// coming.
    pub fn prefetch_hot(&self) {
        self.map.will_need(self.scale, 4 * self.hot_n);
        self.map.will_need(self.q, self.cut * self.hot_n);
    }

    /// Let the pages of region positions `r` go from the process (the kernel keeps them).
    pub fn release(&self, r: Range<usize>) {
        self.map
            .release(self.q + self.cut * r.start, self.cut * r.len());
        self.map.release(self.scale + 4 * r.start, 4 * r.len());
    }

    /// Let every page of the groups go (the hot region stays).
    pub fn release_groups(&self) {
        self.release(self.hot_n..self.assigned);
    }

    /// Open `path` if it is a tier file of `cut`-wide cuts that places records of a scan file of
    /// `entries` records, whose entry `e` has the hash `hash_of(e)`. `Err` says why it is not.
    pub fn open(
        path: &Path,
        cut: usize,
        entries: usize,
        hash_of: &dyn Fn(u32) -> u128,
    ) -> Result<TierFile, String> {
        let file = File::open(path).map_err(|e| format!("not opened: {e}"))?;
        let len = file.metadata().map_err(|e| e.to_string())?.len();
        let mut head = [0u8; HEAD as usize];
        file.read_exact_at(&mut head, 0)
            .map_err(|_| "no header".to_string())?;
        if &head[..8] != MAGIC {
            return Err("not a tier file".into());
        }
        if u32_at(&head, 8) as usize != cut {
            return Err("another stamp's dimensions".into());
        }
        let groups = u32_at(&head, 12) as usize;
        let hot_n = usize::try_from(u64_at(&head, 16)).map_err(|e| e.to_string())?;
        let assigned = usize::try_from(u64_at(&head, 24)).map_err(|e| e.to_string())?;
        let at = |i: usize| u64_at(&head, 32 + 8 * i) as usize;
        let [order, scale, q, starts, centres, cscale] = [0, 1, 2, 3, 4, 5].map(at);
        let total = u64_at(&head, 80);
        if total != len {
            return Err(format!("a length of {len}, not {total}"));
        }
        let want = Self::columns(assigned as u64, cut as u64, groups as u64);
        if [
            order as u64,
            scale as u64,
            q as u64,
            starts as u64,
            centres as u64,
            cscale as u64,
        ] != want[..6]
            || want[6] != total
            || hot_n > assigned
            || assigned > entries
        {
            return Err("columns that are not where the header says".into());
        }
        let max_entry = u32_at(&head, 88);
        let map = Map::random(&file).map_err(|e| format!("not mapped: {e}"))?;
        let b = map.bytes();
        if crc32fast::hash(&b[order..order + 4 * assigned]) != u32_at(&head, 92) {
            return Err("an order column that does not check".into());
        }
        if assigned > 0
            && (max_entry as usize >= entries || hash_of(max_entry) != u128_at(&head, 96))
        {
            return Err("not the scan file it was made from".into());
        }
        let placed = placed_bits(&b[order..order + 4 * assigned], max_entry)?;
        check_starts(b, starts, groups, hot_n, assigned)?;
        let mut hot = Bits::filled(
            if assigned > 0 {
                max_entry as usize + 1
            } else {
                0
            },
            false,
        );
        for p in 0..hot_n {
            hot.set(u32_at(b, order + 4 * p) as usize, true);
        }
        Ok(TierFile {
            map,
            _file: file,
            cut,
            hot_n,
            assigned,
            groups,
            order,
            scale,
            q,
            starts,
            centres,
            cscale,
            placed,
            hot,
            made_from: u64_at(&head, 120),
            covers: if assigned > 0 {
                max_entry as usize + 1
            } else {
                0
            },
        })
    }

    /// Where each column starts, and the file's length, for `assigned` placed records and
    /// `groups` groups.
    fn columns(assigned: u64, cut: u64, groups: u64) -> [u64; 7] {
        let order = HEAD;
        let scale = align(order + 4 * assigned);
        let q = align(scale + 4 * assigned);
        let starts = align(q + cut * assigned);
        let centres = align(starts + 8 * (groups + 1));
        let cscale = align(centres + cut * groups);
        [
            order,
            scale,
            q,
            starts,
            centres,
            cscale,
            cscale + 4 * groups,
        ]
    }

    /// Write `l` to `path` (beside it, synced, then renamed over it) and open it. `cut_of` gives
    /// an entry's scale and cut, `hash_of` its text hash.
    pub fn write(
        path: &Path,
        l: &Layout<'_>,
        cut_of: &dyn Fn(u32) -> (f32, Vec<i8>),
        hash_of: &dyn Fn(u32) -> u128,
        entries: usize,
    ) -> io::Result<TierFile> {
        let hot_n = l.hot.len();
        let order: Vec<u32> = l
            .hot
            .iter()
            .copied()
            .chain(l.groups.iter().flat_map(|g| g.entries.iter().copied()))
            .collect();
        let assigned = order.len();
        let groups = l.groups.len();
        let cols = Self::columns(assigned as u64, l.cut as u64, groups as u64);
        let tmp = path.with_extension("tier.tmp");
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(&tmp)?;
        let mut scales: Vec<u8> = Vec::with_capacity(4 * assigned);
        let mut q = Out::new(cols[2]);
        for &e in &order {
            let (s, cut) = cut_of(e);
            if cut.len() != l.cut {
                return Err(io::Error::other("a cut of another width"));
            }
            scales.extend_from_slice(&s.to_le_bytes());
            // SAFETY: the same bytes: `i8` and `u8` have one layout.
            let bytes = unsafe { std::slice::from_raw_parts(cut.as_ptr().cast::<u8>(), cut.len()) };
            q.put(&file, bytes)?;
        }
        q.flush(&file)?;
        let mut col: Vec<u8> = Vec::with_capacity(4 * assigned);
        for e in &order {
            col.extend_from_slice(&e.to_le_bytes());
        }
        file.write_all_at(&col, cols[0])?;
        let crc = crc32fast::hash(&col);
        file.write_all_at(&scales, cols[1])?;
        col.clear();
        let mut at = hot_n as u64;
        col.extend_from_slice(&at.to_le_bytes());
        for g in l.groups {
            at += g.entries.len() as u64;
            col.extend_from_slice(&at.to_le_bytes());
        }
        file.write_all_at(&col, cols[3])?;
        let mut cen: Vec<u8> = Vec::with_capacity(l.cut * groups);
        let mut cs: Vec<u8> = Vec::with_capacity(4 * groups);
        for g in l.groups {
            cen.extend(g.centre.iter().map(|&x| x as u8));
            cs.extend_from_slice(&g.scale.to_le_bytes());
        }
        file.write_all_at(&cen, cols[4])?;
        file.write_all_at(&cs, cols[5])?;
        let max_entry = order.iter().copied().max().unwrap_or(0);
        let mut head = [0u8; HEAD as usize];
        head[..8].copy_from_slice(MAGIC);
        head[8..12].copy_from_slice(&(l.cut as u32).to_le_bytes());
        head[12..16].copy_from_slice(&(groups as u32).to_le_bytes());
        head[16..24].copy_from_slice(&(hot_n as u64).to_le_bytes());
        head[24..32].copy_from_slice(&(assigned as u64).to_le_bytes());
        for (i, c) in cols.iter().enumerate() {
            head[32 + 8 * i..40 + 8 * i].copy_from_slice(&c.to_le_bytes());
        }
        head[88..92].copy_from_slice(&max_entry.to_le_bytes());
        head[92..96].copy_from_slice(&crc.to_le_bytes());
        if assigned > 0 {
            head[96..112].copy_from_slice(&hash_of(max_entry).to_le_bytes());
        }
        head[120..128].copy_from_slice(&l.made_from.to_le_bytes());
        file.write_all_at(&head, 0)?;
        file.set_len(cols[6])?;
        file.sync_data()?;
        std::fs::rename(&tmp, path)?;
        if let Some(dir) = path.parent() {
            File::open(dir)?.sync_all()?;
        }
        drop((col, scales, cen, cs, order));
        Self::open(path, l.cut, entries, hash_of).map_err(io::Error::other)
    }
}
