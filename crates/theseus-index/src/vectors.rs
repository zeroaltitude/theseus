//! The vector side of the index (M6 §2.2, step 29c): every chunk's
//! embedding, a flat int8 scan over the 256-d cut, the 768-d re-score, and
//! the one embedding thread that backfills them.
//!
//! - **Vectors are kept by their text**, not by node: a stamp's file,
//!   `<index>/vectors/<stamp key>.vec`, holds one record per distinct chunk
//!   text (its SHA-256, the int8 cut and its scale, the 768-d vector at f16,
//!   a CRC), appended as the thread embeds. So an index rebuilt from the WAL
//!   finds its vectors again, and a text said twice is embedded once.
//! - **Rows mirror the index**: one per chunk (node, chunk, position,
//!   session, kind, external, text hash), filled from each commit and, at
//!   start, from the index itself. A row answers once its text has a vector.
//! - **The backlog** is the texts with no vector of this stamp, newest first,
//!   batched by length: up to 8 of about one length when they are 128 tokens
//!   or less, a longer one alone (the spike's §7).
//! - **Stamps.** A stamp changed within its space (a new engine, a new
//!   precision) re-embeds in the background while the older vectors answer
//!   (a query from the new engine compares with them). A new model is a new
//!   space: its vectors start from none, and the old file goes once the new
//!   one covers every chunk.
//! - **The model** loads on first use (a backfill batch, a query that may
//!   wait, `index.embed`, or `index.warm`), on the embedding thread, never on
//!   the tender's start path; it unloads after `idle_unload`. A query embeds
//!   on its own connection's thread while a backfill batch runs, so it never
//!   waits behind one.

use std::cmp::{Ordering as CmpOrdering, Reverse};
use std::collections::{BTreeSet, BinaryHeap, HashMap, HashSet};
use std::fs::{self, File, OpenOptions};
use std::io::{self, BufReader, Read, Seek, SeekFrom, Write};
use std::os::unix::fs::{DirBuilderExt, FileExt, OpenOptionsExt};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex, RwLock};
use std::time::{Duration, Instant, SystemTime};

use anyhow::Context as _;
use sha2::{Digest, Sha256};

use crate::embedder::{self, dot_i8, quantize, stamp_key, Embedder, ModelSpec, Vector, Windows};
use crate::proto::{
    Compactions, EmbedParams, EmbedResult, EmbedStats, Filters, Neighbour, Reembed, Stamp, Task,
    VectorStatus,
};
use crate::weights::LoadError;

/// How many candidates of the int8 scan the 768-d vectors re-score.
pub const RESCORE: usize = 100;

/// Texts embedded together, when each is this many tokens or fewer.
pub const BATCH: usize = 8;
pub const BATCH_MAX_TOKENS: u32 = 128;

/// `index.embed`'s most texts per call.
pub const EMBED_MAX_TEXTS: usize = 64;

/// The longest the embedding thread sleeps without a kick.
const BACKSTOP: Duration = Duration::from_secs(60);

#[derive(Debug, Clone)]
pub struct VectorConfig {
    /// Where the models live (`[index] weights_dir`): `None` turns vectors
    /// off, and the tender answers BM25 and entities alone.
    pub weights_dir: Option<PathBuf>,
    pub spec: ModelSpec,
    /// Unload the model after this long unused (`[index] idle_unload_mins`).
    pub idle_unload: Duration,
    /// What made the vectors, for their stamp: this build's engine and
    /// embedding code ([`embedder::engine_tag`]). Tests change it to change
    /// the stamp within its space.
    pub engine: String,
    /// The longest the embedding thread waits between two pieces of work
    /// while the machine is busy (theseus-tood). Tests give zero: there the
    /// suite's own load is the pressure.
    pub yield_bound: Duration,
}

impl VectorConfig {
    /// Nomic v1.5 from `weights_dir`, unloaded after ten idle minutes.
    pub fn new(weights_dir: Option<PathBuf>) -> Self {
        Self {
            weights_dir,
            spec: ModelSpec::nomic_v1_5(),
            idle_unload: Duration::from_secs(600),
            engine: embedder::engine_tag(),
            yield_bound: theseus_store::pressure::BOUND,
        }
    }

    pub fn off() -> Self {
        Self::new(None)
    }
}

/// A chunk text's key: the first 16 bytes of its SHA-256.
pub fn text_hash(text: &str) -> u128 {
    let d = Sha256::digest(text.as_bytes());
    let mut b = [0u8; 16];
    b.copy_from_slice(&d[..16]);
    u128::from_le_bytes(b)
}

/// One chunk, as a row sees it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChunkKey {
    pub chunk: u32,
    pub hash: u128,
    /// The chunker's estimate: enough to batch texts of about one length.
    pub tokens: u32,
}

/// A node's chunks, as the index holds them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NodeChunks {
    pub node_id: String,
    pub position: u64,
    pub session: String,
    pub kind: String,
    pub external: bool,
    pub chunks: Vec<ChunkKey>,
}

/// What the vector side reads from the index: a chunk's text, to embed it,
/// and every chunk, to rebuild its rows at start.
pub trait Texts {
    fn chunk_text(&self, node_id: &str, chunk: u32) -> Option<String>;
    fn all_nodes(&self) -> anyhow::Result<Vec<NodeChunks>>;
}

// ---------------------------------------------------------------------------
// One stamp's vectors on disk.

const MAGIC: &[u8; 8] = b"thsvec01";
const HEADER: u64 = 16;

/// One stamp's vectors: the file, and in memory each record's hash, int8
/// cut, and scale (the 768-d vectors stay on disk, read for the re-score).
pub struct Cache {
    pub stamp: Stamp,
    key: String,
    dir: PathBuf,
    file: File,
    cut: usize,
    full: usize,
    rec: usize,
    by_hash: HashMap<u128, u32>,
    /// Each entry's text hash.
    hashes: Vec<u128>,
    q: Vec<i8>,
    scale: Vec<f32>,
    /// Entries whose text no chunk holds (theseus-64x): no row points at
    /// them, so they answer nothing, and the next compaction drops them.
    dead: Vec<bool>,
    dead_count: usize,
}

fn rec_len(cut: usize, full: usize) -> usize {
    16 + 4 + cut + 2 * full + 4
}

/// What a compaction did.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Compacted {
    /// Files rewritten.
    pub files: u64,
    /// Records dropped.
    pub dropped: u64,
    pub bytes_before: u64,
    pub bytes_after: u64,
}

impl Compacted {
    fn add(&mut self, o: Compacted) {
        self.files += o.files;
        self.dropped += o.dropped;
        self.bytes_before += o.bytes_before;
        self.bytes_after += o.bytes_after;
    }
}

/// A compaction's steps, at whose ends a test stops it as a crash would.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Step {
    /// Half the kept records copied.
    HalfCopied,
    /// The copy whole and synced, not yet renamed.
    Copied,
    /// Renamed over the file; the directory not yet synced.
    Renamed,
}

/// Make a rename in `dir` durable.
fn sync_dir(dir: &Path) -> io::Result<()> {
    File::open(dir)?.sync_all()
}

impl Cache {
    /// Open `<dir>/<key>.vec` for `stamp`, or start it. A torn or corrupt
    /// tail (a crash mid-append) is cut at the last whole record, and a
    /// compaction's copy that a crash left unrenamed is removed.
    pub fn open(dir: &Path, stamp: &Stamp) -> anyhow::Result<Cache> {
        let key = stamp_key(stamp);
        let path = dir.join(format!("{key}.vec"));
        let meta = dir.join(format!("{key}.json"));
        if !meta.exists() {
            crate::state::save(&meta, stamp).context("writing a stamp's file")?;
        }
        match fs::remove_file(path.with_extension("vec.tmp")) {
            Ok(()) => tracing::warn!(
                file = %path.display(),
                "index: a compaction's copy left by a crash, removed (the file is the old one)"
            ),
            Err(e) if e.kind() == io::ErrorKind::NotFound => {}
            Err(e) => return Err(e).context("removing a compaction's copy"),
        }
        let file = OpenOptions::new()
            .read(true)
            .append(true)
            .create(true)
            .open(&path)
            .with_context(|| format!("opening {}", path.display()))?;
        let [cut, full] = stamp.dims;
        let rec = rec_len(cut, full);
        let mut c = Cache {
            stamp: stamp.clone(),
            key,
            dir: dir.to_path_buf(),
            file,
            cut,
            full,
            rec,
            by_hash: HashMap::new(),
            hashes: Vec::new(),
            q: Vec::new(),
            scale: Vec::new(),
            dead: Vec::new(),
            dead_count: 0,
        };
        let len = c.file.metadata()?.len();
        let mut head = [0u8; HEADER as usize];
        let fresh = len < HEADER || {
            c.file.read_exact_at(&mut head, 0)?;
            &head[..8] != MAGIC
                || u32::from_le_bytes(head[8..12].try_into()?) as usize != cut
                || u32::from_le_bytes(head[12..16].try_into()?) as usize != full
        };
        if fresh {
            c.file.set_len(0)?;
            let mut h = MAGIC.to_vec();
            h.extend_from_slice(&(cut as u32).to_le_bytes());
            h.extend_from_slice(&(full as u32).to_le_bytes());
            (&c.file).write_all(&h)?;
            return Ok(c);
        }
        let mut f = c.file.try_clone()?;
        f.seek(SeekFrom::Start(HEADER))?;
        let mut r = BufReader::with_capacity(1 << 20, f);
        let mut buf = vec![0u8; rec];
        loop {
            match r.read_exact(&mut buf) {
                Ok(()) => {}
                Err(e) if e.kind() == io::ErrorKind::UnexpectedEof => break,
                Err(e) => return Err(e.into()),
            }
            let crc = u32::from_le_bytes(buf[rec - 4..].try_into()?);
            if crc32fast::hash(&buf[..rec - 4]) != crc {
                break;
            }
            c.push_record(&buf);
        }
        let good = HEADER + (c.len() * rec) as u64;
        if good != len {
            tracing::warn!(
                file = %path.display(),
                cut_bytes = len - good,
                "index: a vector file's torn or corrupt tail, cut"
            );
            c.file.set_len(good)?;
        }
        Ok(c)
    }

    fn push_record(&mut self, b: &[u8]) {
        let hash = u128::from_le_bytes(b[..16].try_into().unwrap_or_default());
        let scale = f32::from_le_bytes(b[16..20].try_into().unwrap_or_default());
        let entry = self.scale.len() as u32;
        self.by_hash.insert(hash, entry);
        self.hashes.push(hash);
        self.scale.push(scale);
        self.q.extend(b[20..20 + self.cut].iter().map(|&x| x as i8));
        self.dead.push(false);
    }

    pub fn len(&self) -> usize {
        self.scale.len()
    }

    pub fn is_empty(&self) -> bool {
        self.scale.is_empty()
    }

    pub fn get(&self, hash: u128) -> Option<u32> {
        self.by_hash.get(&hash).copied()
    }

    /// Records whose text no chunk holds.
    pub fn dead(&self) -> usize {
        self.dead_count
    }

    /// The file's length: its header and its whole records.
    pub fn bytes(&self) -> u64 {
        HEADER + (self.len() * self.rec) as u64
    }

    /// Mark the record of `hash`, if this file holds one, dead or alive.
    fn set_dead(&mut self, hash: u128, dead: bool) {
        if let Some(e) = self.get(hash) {
            let d = &mut self.dead[e as usize];
            if *d != dead {
                *d = dead;
                if dead {
                    self.dead_count += 1;
                } else {
                    self.dead_count -= 1;
                }
            }
        }
    }

    /// Every record dead unless `alive` holds its text.
    fn recount(&mut self, alive: &dyn Fn(u128) -> bool) {
        self.dead_count = 0;
        for (e, h) in self.hashes.iter().enumerate() {
            self.dead[e] = !alive(*h);
            self.dead_count += usize::from(self.dead[e]);
        }
    }

    fn q(&self, e: u32) -> &[i8] {
        &self.q[e as usize * self.cut..(e as usize + 1) * self.cut]
    }

    /// An entry's 768-d vector, read from the file.
    pub fn full(&self, e: u32) -> io::Result<Vec<f32>> {
        let mut b = vec![0u8; 2 * self.full];
        let at = HEADER + e as u64 * self.rec as u64 + 20 + self.cut as u64;
        self.file.read_exact_at(&mut b, at)?;
        let (pairs, _) = b.as_chunks::<2>();
        Ok(pairs
            .iter()
            .map(|x| half::f16::from_bits(u16::from_le_bytes(*x)).to_f32())
            .collect())
    }

    fn record(&self, hash: u128, v: &Vector) -> Vec<u8> {
        let (q, scale) = quantize(&v.cut);
        let mut b = Vec::with_capacity(self.rec);
        b.extend_from_slice(&hash.to_le_bytes());
        b.extend_from_slice(&scale.to_le_bytes());
        b.extend(q.iter().map(|&x| x as u8));
        for &x in &v.full {
            b.extend_from_slice(&half::f16::from_f32(x).to_bits().to_le_bytes());
        }
        let crc = crc32fast::hash(&b);
        b.extend_from_slice(&crc.to_le_bytes());
        b
    }

    /// Append vectors, in one write; their entries. A failed write leaves the
    /// file as it was.
    pub fn append(&mut self, items: &[(u128, &Vector)]) -> io::Result<Vec<u32>> {
        let mut buf = Vec::with_capacity(items.len() * self.rec);
        for (h, v) in items {
            if v.cut.len() != self.cut || v.full.len() != self.full {
                return Err(io::Error::other("a vector of another stamp's dimensions"));
            }
            buf.extend(self.record(*h, v));
        }
        if let Err(e) = (&self.file).write_all(&buf) {
            let _ = self.file.set_len(HEADER + (self.len() * self.rec) as u64);
            return Err(e);
        }
        let mut out = Vec::with_capacity(items.len());
        for r in buf.chunks_exact(self.rec) {
            self.push_record(r);
            out.push(self.len() as u32 - 1);
        }
        Ok(out)
    }

    fn paths(&self) -> (PathBuf, PathBuf) {
        (
            self.dir.join(format!("{}.vec", self.key)),
            self.dir.join(format!("{}.json", self.key)),
        )
    }

    /// Rewrite the file with only the records whose text `keep` admits
    /// (theseus-64x), atomically: a copy beside it (mode 0600) synced, then
    /// renamed over it, then the directory synced. A crash leaves the old
    /// file or the new one, never part of either: the copy is renamed only
    /// whole, and the next open removes one a crash left. Nothing is
    /// rewritten when every record stays.
    pub fn compact(&mut self, keep: &dyn Fn(u128) -> bool) -> anyhow::Result<Compacted> {
        self.rewrite(keep, &mut |_| Ok(()))
    }

    /// [`Cache::compact`], calling `at` at each step's end: a test returns an
    /// error there to stop it as a crash would.
    pub(crate) fn rewrite(
        &mut self,
        keep: &dyn Fn(u128) -> bool,
        at: &mut dyn FnMut(Step) -> io::Result<()>,
    ) -> anyhow::Result<Compacted> {
        let kept: Vec<u32> = (0..self.len() as u32)
            .filter(|&e| keep(self.hashes[e as usize]))
            .collect();
        if kept.len() == self.len() {
            return Ok(Compacted::default());
        }
        let before = self.bytes();
        let (path, _) = self.paths();
        let tmp = path.with_extension("vec.tmp");
        let mut out = OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(&tmp)
            .with_context(|| format!("creating {}", tmp.display()))?;
        {
            let mut w = io::BufWriter::with_capacity(1 << 20, &mut out);
            w.write_all(MAGIC)?;
            w.write_all(&(self.cut as u32).to_le_bytes())?;
            w.write_all(&(self.full as u32).to_le_bytes())?;
            let mut rec = vec![0u8; self.rec];
            for (n, &e) in kept.iter().enumerate() {
                if n == kept.len() / 2 {
                    w.flush()?;
                    at(Step::HalfCopied)?;
                }
                self.file
                    .read_exact_at(&mut rec, HEADER + u64::from(e) * self.rec as u64)?;
                w.write_all(&rec)?;
            }
            w.flush()?;
        }
        out.sync_all()?;
        drop(out);
        at(Step::Copied)?;
        fs::rename(&tmp, &path).with_context(|| format!("renaming over {}", path.display()))?;
        at(Step::Renamed)?;
        sync_dir(&self.dir)?;
        // The new file, and in memory its records as the copy wrote them.
        let file = OpenOptions::new()
            .read(true)
            .append(true)
            .open(&path)
            .with_context(|| format!("opening {}", path.display()))?;
        let mut by_hash = HashMap::with_capacity(kept.len());
        let mut hashes = Vec::with_capacity(kept.len());
        let mut q = Vec::with_capacity(kept.len() * self.cut);
        let mut scale = Vec::with_capacity(kept.len());
        let mut dead = Vec::with_capacity(kept.len());
        for (n, &e) in kept.iter().enumerate() {
            let h = self.hashes[e as usize];
            by_hash.insert(h, n as u32);
            hashes.push(h);
            q.extend_from_slice(self.q(e));
            scale.push(self.scale[e as usize]);
            dead.push(self.dead[e as usize]);
        }
        let dropped = (self.len() - kept.len()) as u64;
        self.dead_count = dead.iter().filter(|d| **d).count();
        (
            self.file,
            self.by_hash,
            self.hashes,
            self.q,
            self.scale,
            self.dead,
        ) = (file, by_hash, hashes, q, scale, dead);
        Ok(Compacted {
            files: 1,
            dropped,
            bytes_before: before,
            bytes_after: self.bytes(),
        })
    }

    fn remove(self) {
        let (vec, json) = self.paths();
        let _ = fs::remove_file(vec);
        let _ = fs::remove_file(json);
    }
}

// ---------------------------------------------------------------------------
// Rows, the backlog, and the scan.

#[derive(Debug, Clone)]
struct Row {
    node: Arc<str>,
    chunk: u32,
    position: u64,
    session: u32,
    kind: u16,
    external: bool,
    hash: u128,
    tokens: u32,
    alive: bool,
    /// (cache, entry): the vector this row answers with.
    vec: Option<(u8, u32)>,
}

#[derive(Debug, Default)]
struct Interner {
    names: Vec<String>,
    ids: HashMap<String, u32>,
}

impl Interner {
    fn id(&mut self, s: &str) -> u32 {
        if let Some(&i) = self.ids.get(s) {
            return i;
        }
        let i = self.names.len() as u32;
        self.names.push(s.to_string());
        self.ids.insert(s.to_string(), i);
        i
    }

    fn get(&self, s: &str) -> Option<u32> {
        self.ids.get(s).copied()
    }

    fn name(&self, i: u32) -> &str {
        self.names.get(i as usize).map_or("", String::as_str)
    }
}

/// A pending text's place in the backlog: rows with no vector at all go
/// before rows an older stamp's vector answers for; then newest first.
type Slot = (bool, Reverse<u64>, u128);

#[derive(Debug, Default)]
struct Backlog {
    items: HashMap<u128, (u32, Slot)>,
    order: BTreeSet<Slot>,
    buckets: [BTreeSet<Slot>; 4],
}

fn bucket(tokens: u32) -> usize {
    match tokens {
        0..=32 => 0,
        33..=64 => 1,
        65..=BATCH_MAX_TOKENS => 2,
        _ => 3,
    }
}

impl Backlog {
    fn add(&mut self, hash: u128, tokens: u32, position: u64, answers: bool) {
        let slot = match self.items.get(&hash) {
            Some((_, (a, Reverse(p), _))) => (*a && answers, Reverse((*p).max(position)), hash),
            None => (answers, Reverse(position), hash),
        };
        self.remove(hash);
        self.order.insert(slot);
        self.buckets[bucket(tokens)].insert(slot);
        self.items.insert(hash, (tokens, slot));
    }

    fn remove(&mut self, hash: u128) {
        if let Some((tokens, slot)) = self.items.remove(&hash) {
            self.order.remove(&slot);
            self.buckets[bucket(tokens)].remove(&slot);
        }
    }

    fn len(&self) -> usize {
        self.items.len()
    }

    /// The next batch: the first text's bucket, and up to [`BATCH`] of it
    /// when its texts are short.
    fn take(&mut self) -> Vec<u128> {
        let Some(&first) = self.order.first() else {
            return Vec::new();
        };
        let b = bucket(self.items[&first.2].0);
        let n = if b < 3 { BATCH } else { 1 };
        let picked: Vec<u128> = self.buckets[b]
            .iter()
            .filter(|s| s.0 == first.0)
            .take(n)
            .map(|s| s.2)
            .collect();
        for h in &picked {
            self.remove(*h);
        }
        picked
    }
}

/// The filters every source shares, as the rows hold them.
pub struct RowFilter {
    as_of: Option<u64>,
    exclude: HashSet<u32>,
    sessions: Option<HashSet<u32>>,
    kinds: Option<HashSet<u16>>,
    external: Option<bool>,
    skip_node: Option<Arc<str>>,
    /// A filter naming only unknown sessions or kinds admits nothing.
    none: bool,
}

/// The vector source's search behind a trait: the flat exact scan now, an
/// HNSW in its place later, once the scan's p95 passes 10 ms (§2.2).
pub trait Scan {
    /// The best `n` rows for the int8 query `q` among those `keep` admits,
    /// by the int8 dot product times each row's scale; best first, ties to
    /// the earlier row.
    fn top(&self, q: &[i8], n: usize, keep: &dyn Fn(usize) -> bool) -> Vec<(usize, f32)>;
}

/// A scored row for the heap: the worst on top.
#[derive(Debug, Clone, Copy)]
struct Scored(f32, usize);

impl PartialEq for Scored {
    fn eq(&self, o: &Self) -> bool {
        self.cmp(o) == CmpOrdering::Equal
    }
}
impl Eq for Scored {}
impl PartialOrd for Scored {
    fn partial_cmp(&self, o: &Self) -> Option<CmpOrdering> {
        Some(self.cmp(o))
    }
}
impl Ord for Scored {
    /// Better is greater: a higher score, then an earlier row.
    fn cmp(&self, o: &Self) -> CmpOrdering {
        self.0.total_cmp(&o.0).then(o.1.cmp(&self.1))
    }
}

/// The best `n` of `scored`, best first.
pub fn best(n: usize, scored: impl Iterator<Item = (usize, f32)>) -> Vec<(usize, f32)> {
    let mut heap: BinaryHeap<Reverse<Scored>> = BinaryHeap::with_capacity(n + 1);
    if n == 0 {
        return Vec::new();
    }
    for (row, s) in scored {
        let x = Scored(s, row);
        if heap.len() < n {
            heap.push(Reverse(x));
        } else if heap.peek().is_some_and(|w| x > w.0) {
            heap.pop();
            heap.push(Reverse(x));
        }
    }
    let mut out: Vec<Scored> = heap.into_iter().map(|r| r.0).collect();
    out.sort_by(|a, b| b.cmp(a));
    out.into_iter().map(|s| (s.1, s.0)).collect()
}

/// Every row with a vector, in order: the flat scan.
struct Flat<'a>(&'a Table);

impl Scan for Flat<'_> {
    fn top(&self, q: &[i8], n: usize, keep: &dyn Fn(usize) -> bool) -> Vec<(usize, f32)> {
        let t = self.0;
        best(
            n,
            t.rows.iter().enumerate().filter_map(|(i, r)| {
                let (c, e) = r.vec?;
                if !r.alive || !keep(i) {
                    return None;
                }
                let cache = &t.caches[c as usize];
                Some((i, dot_i8(cache.q(e), q) as f32 * cache.scale[e as usize]))
            }),
        )
    }
}

/// The rows, the stamp's vectors, and the backlog.
#[derive(Default)]
pub struct Table {
    generation: u64,
    rows: Vec<Row>,
    by_node: HashMap<Arc<str>, Vec<u32>>,
    /// Alive rows, by their text.
    by_hash: HashMap<u128, Vec<u32>>,
    sessions: Interner,
    kinds: Interner,
    /// This stamp's vectors first, then older stamps' of the same space.
    caches: Vec<Cache>,
    /// Other spaces' files: removed once this stamp covers every chunk.
    others: Vec<(Stamp, PathBuf, PathBuf)>,
    backlog: Backlog,
    dead: usize,
    /// The vector files are read: until then the backlog waits.
    opened: bool,
    reconciled: bool,
    /// The ingest thread has caught up with the WAL since the tender started
    /// or the index was dropped: until then the rows are not every chunk.
    caught_up: bool,
    /// A compaction asked for once the table settles, and why (a rebuild).
    compact_asked: Option<&'static str>,
    /// Nodes and chunks forgotten before the rows were reconciled with the
    /// index, which a reconcile that read the index first must not add.
    forgot_nodes: HashSet<String>,
    forgot_chunks: HashSet<(String, u32)>,
}

impl Table {
    fn resolve(&self, hash: u128) -> Option<(u8, u32)> {
        self.caches
            .iter()
            .enumerate()
            .find_map(|(i, c)| c.get(hash).map(|e| (i as u8, e)))
    }

    /// Point row `i` at its vector, or queue its text. A record its text had
    /// left dead lives again.
    fn place(&mut self, i: u32) {
        let r = &self.rows[i as usize];
        let vec = self.resolve(r.hash);
        let (hash, tokens, position) = (r.hash, r.tokens, r.position);
        self.rows[i as usize].vec = vec;
        if vec.is_some() {
            self.mark(hash, false);
        }
        if self.opened && !matches!(vec, Some((0, _))) {
            self.backlog.add(hash, tokens, position, vec.is_some());
        }
    }

    /// The rows are every chunk the index holds: the files are read, the
    /// index's chunks reconciled, and the ingest thread caught up. Only then
    /// is a record whose text has no row truly dead.
    fn settled(&self) -> bool {
        self.opened && self.reconciled && self.caught_up
    }

    /// `hash`'s records, in every file that holds one, dead or alive.
    fn mark(&mut self, hash: u128, dead: bool) {
        for c in &mut self.caches {
            c.set_dead(hash, dead);
        }
    }

    /// Every record dead unless a row holds its text.
    fn recount(&mut self) {
        let by_hash = &self.by_hash;
        for c in &mut self.caches {
            c.recount(&|h| by_hash.contains_key(&h));
        }
    }

    /// Records in the files, and the dead among them.
    fn records(&self) -> (usize, usize) {
        self.caches
            .iter()
            .fold((0, 0), |(n, d), c| (n + c.len(), d + c.dead()))
    }

    /// Row `i` is gone: its text, when no other row holds it, leaves the
    /// backlog, and its record is dead.
    fn kill_row(&mut self, i: u32) {
        let r = &mut self.rows[i as usize];
        if !r.alive {
            return;
        }
        r.alive = false;
        let h = r.hash;
        self.dead += 1;
        if let Some(v) = self.by_hash.get_mut(&h) {
            v.retain(|&x| x != i);
            if v.is_empty() {
                self.by_hash.remove(&h);
                self.backlog.remove(h);
                self.mark(h, true);
            }
        }
    }

    fn add_node(&mut self, n: &NodeChunks) {
        if let Some(rows) = self.by_node.get(n.node_id.as_str()) {
            let same = rows.len() == n.chunks.len()
                && rows.iter().zip(&n.chunks).all(|(&i, c)| {
                    let r = &self.rows[i as usize];
                    r.chunk == c.chunk && r.hash == c.hash
                });
            if same {
                // Read again (a kill between commit and cursor): the same
                // texts; only where it lives may have moved.
                let (session, kind) = (self.sessions.id(&n.session), self.kinds.id(&n.kind));
                for &i in &self.by_node[n.node_id.as_str()] {
                    let r = &mut self.rows[i as usize];
                    r.position = n.position;
                    r.session = session;
                    r.kind = kind as u16;
                    r.external = n.external;
                }
                return;
            }
            self.remove_node(&n.node_id);
        }
        let node: Arc<str> = Arc::from(n.node_id.as_str());
        let session = self.sessions.id(&n.session);
        let kind = self.kinds.id(&n.kind) as u16;
        let mut ids = Vec::with_capacity(n.chunks.len());
        for c in &n.chunks {
            let i = self.rows.len() as u32;
            self.rows.push(Row {
                node: node.clone(),
                chunk: c.chunk,
                position: n.position,
                session,
                kind,
                external: n.external,
                hash: c.hash,
                tokens: c.tokens,
                alive: true,
                vec: None,
            });
            self.by_hash.entry(c.hash).or_default().push(i);
            ids.push(i);
            self.place(i);
        }
        self.by_node.insert(node, ids);
    }

    /// A node's rows go; the texts it held, by hash.
    fn remove_node(&mut self, node: &str) -> Vec<u128> {
        let Some(ids) = self.by_node.remove(node) else {
            return Vec::new();
        };
        let mut held = Vec::with_capacity(ids.len());
        for i in ids {
            held.push(self.rows[i as usize].hash);
            self.kill_row(i);
        }
        self.maybe_compact_rows();
        held
    }

    /// One chunk's row goes; its text's hash, if the table had the chunk.
    fn remove_chunk(&mut self, node: &str, chunk: u32) -> Option<u128> {
        let ids = self.by_node.get(node)?;
        let at = ids
            .iter()
            .position(|&i| self.rows[i as usize].chunk == chunk)?;
        let ids = self.by_node.get_mut(node)?;
        let i = ids.remove(at);
        if ids.is_empty() {
            self.by_node.remove(node);
        }
        let h = self.rows[i as usize].hash;
        self.kill_row(i);
        self.maybe_compact_rows();
        Some(h)
    }

    fn maybe_compact_rows(&mut self) {
        if self.dead > 1024 && self.dead > self.rows.len() / 2 {
            self.compact_rows();
        }
    }

    fn compact_rows(&mut self) {
        let old = std::mem::take(&mut self.rows);
        self.by_node.clear();
        self.by_hash.clear();
        for r in old.into_iter().filter(|r| r.alive) {
            let i = self.rows.len() as u32;
            self.by_node.entry(r.node.clone()).or_default().push(i);
            self.by_hash.entry(r.hash).or_default().push(i);
            self.rows.push(r);
        }
        self.dead = 0;
    }

    /// Every row goes (the index was dropped): until the ingest thread has
    /// caught up again, no record counts as dead, and then the files are
    /// compacted.
    fn clear(&mut self) {
        self.generation += 1;
        self.rows.clear();
        self.by_node.clear();
        self.by_hash.clear();
        self.backlog = Backlog::default();
        self.dead = 0;
        self.caught_up = false;
        self.compact_asked = Some("rebuild");
    }

    /// Point every row at its vector again (the files were read, or a
    /// cache changed), and rebuild the backlog.
    fn resolve_all(&mut self) {
        self.backlog = Backlog::default();
        for i in 0..self.rows.len() as u32 {
            if self.rows[i as usize].alive {
                self.place(i);
            }
        }
    }

    fn alive(&self) -> impl Iterator<Item = &Row> {
        self.rows.iter().filter(|r| r.alive)
    }

    fn filter(&self, as_of: Option<u64>, exclude: &[String], f: &Filters) -> RowFilter {
        let mut none = false;
        let mut ids = |names: &[String], i: &Interner| -> Option<HashSet<u32>> {
            if names.is_empty() {
                return None;
            }
            let s: HashSet<u32> = names.iter().filter_map(|n| i.get(n)).collect();
            none |= s.is_empty();
            Some(s)
        };
        let sessions = ids(&f.sessions, &self.sessions);
        let kinds = ids(&f.kinds, &self.kinds).map(|s| s.into_iter().map(|k| k as u16).collect());
        RowFilter {
            as_of,
            exclude: exclude
                .iter()
                .filter_map(|s| self.sessions.get(s))
                .collect(),
            sessions,
            kinds,
            external: f.external,
            skip_node: None,
            none,
        }
    }

    fn admits(&self, f: &RowFilter, i: usize) -> bool {
        let r = &self.rows[i];
        !f.none
            && f.as_of.is_none_or(|a| r.position < a)
            && !f.exclude.contains(&r.session)
            && f.sessions.as_ref().is_none_or(|s| s.contains(&r.session))
            && f.kinds.as_ref().is_none_or(|k| k.contains(&r.kind))
            && f.external.is_none_or(|e| r.external == e)
            && f.skip_node.as_ref().is_none_or(|n| *n != r.node)
    }

    fn full_of(&self, i: usize) -> io::Result<Option<Vec<f32>>> {
        match self.rows[i].vec {
            Some((c, e)) => self.caches[c as usize].full(e).map(Some),
            None => Ok(None),
        }
    }

    /// The scan, then the re-score: the best `fetch` rows by the cosine of
    /// their 768-d vectors with `full`, ties to the earlier position.
    fn search(
        &self,
        full: &[f32],
        cut: usize,
        f: &RowFilter,
        fetch: usize,
    ) -> io::Result<Vec<(usize, f64)>> {
        let short = embedder::l2_normalize(&full[..cut]);
        let (q, _) = quantize(&short);
        let cands = Flat(self).top(&q, fetch.max(RESCORE), &|i| self.admits(f, i));
        let mut scored = Vec::with_capacity(cands.len());
        for (i, _) in cands {
            if let Some(v) = self.full_of(i)? {
                scored.push((i, embedder::cosine(full, &v)));
            }
        }
        scored.sort_by(|a, b| {
            b.1.total_cmp(&a.1)
                .then(self.rows[a.0].position.cmp(&self.rows[b.0].position))
                .then(a.0.cmp(&b.0))
        });
        scored.truncate(fetch);
        Ok(scored)
    }
}

// ---------------------------------------------------------------------------
// The shared side: the model, the table, and the embedding thread.

enum Model {
    Off,
    NoWeights,
    Unloaded,
    Loading,
    Loaded(Arc<Embedder>),
    /// A file is not the pinned one, or would not load. Tried again when the
    /// weights file changes.
    Refused {
        why: String,
        sig: Option<(u64, SystemTime)>,
    },
}

impl Model {
    fn name(&self) -> &'static str {
        match self {
            Model::Off => "off",
            Model::NoWeights => "no_weights",
            Model::Unloaded => "unloaded",
            Model::Loading => "loading",
            Model::Loaded(_) => "loaded",
            Model::Refused { .. } => "refused",
        }
    }
}

struct ModelState {
    model: Model,
    /// A load was asked for (`index.warm`, a query that may wait, `index.embed`).
    want: bool,
    last_used: Instant,
    loads: u64,
    unloads: u64,
    load_ms: f64,
    loaded_at_ms: u64,
}

/// What `index.forget` did to the vector files.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Forgot {
    /// Chunks not forgotten that hold a text asked to go: its record stays.
    pub held: Vec<(String, u32)>,
    pub compacted: Compacted,
}

/// A hit of the vector source.
#[derive(Debug, Clone, PartialEq)]
pub struct VectorHit {
    pub node_id: Arc<str>,
    pub chunk: u32,
    pub position: u64,
    /// Cosine of the 768-d vectors.
    pub score: f64,
}

pub struct Vectors {
    cfg: VectorConfig,
    dir: PathBuf,
    stamp: Stamp,
    table: RwLock<Table>,
    state: Mutex<ModelState>,
    /// The model's state changed (loaded, refused, unloaded).
    changed: Condvar,
    /// Work for the embedding thread.
    kick: Mutex<bool>,
    kicked: Condvar,
    stop: AtomicBool,
    stats: Mutex<(EmbedStats, Option<String>, u64)>,
    compactions: Mutex<CompactState>,
    /// Held while the files are read, so they are read once.
    opening: Mutex<()>,
}

/// The compactions so far, and when a failed one may be tried again.
#[derive(Debug, Default)]
struct CompactState {
    stats: Compactions,
    retry_at: Option<Instant>,
}

/// What one turn of the embedding thread did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Work {
    /// Something: go again.
    Did,
    /// Nothing to do: sleep this long, or until kicked.
    Idle(Duration),
}

fn now_ms() -> u64 {
    crate::tender::now_ms()
}

fn file_sig(path: &Path) -> Option<(u64, SystemTime)> {
    let m = fs::metadata(path).ok()?;
    Some((m.len(), m.modified().ok()?))
}

/// This thread's CPU time, in ms.
fn thread_cpu_ms() -> u64 {
    let mut ts = libc::timespec {
        tv_sec: 0,
        tv_nsec: 0,
    };
    // SAFETY: a valid out-pointer.
    unsafe { libc::clock_gettime(libc::CLOCK_THREAD_CPUTIME_ID, &mut ts) };
    ts.tv_sec as u64 * 1000 + ts.tv_nsec as u64 / 1_000_000
}

/// Batches for texts of these token counts: by length, up to [`BATCH`] of
/// at most [`BATCH_MAX_TOKENS`] each, longer ones alone.
pub fn plan_batches(tokens: &[usize]) -> Vec<Vec<usize>> {
    let mut idx: Vec<usize> = (0..tokens.len()).collect();
    idx.sort_by_key(|&i| (tokens[i], i));
    let mut out: Vec<Vec<usize>> = Vec::new();
    for i in idx {
        let short = tokens[i] <= BATCH_MAX_TOKENS as usize;
        match out.last_mut() {
            Some(b) if short && b.len() < BATCH && tokens[b[0]] <= BATCH_MAX_TOKENS as usize => {
                b.push(i)
            }
            _ => out.push(vec![i]),
        }
    }
    out
}

impl Vectors {
    /// Nothing is read here but two `stat`s: whether the weights are there
    /// at all. The model loads on first use, on the embedding thread.
    pub fn new(cfg: VectorConfig, index_dir: &Path) -> Self {
        let model = match &cfg.weights_dir {
            None => Model::Off,
            Some(d) if cfg.spec.weights_path(d).exists() && cfg.spec.tokenizer_path(d).exists() => {
                Model::Unloaded
            }
            Some(_) => Model::NoWeights,
        };
        let stamp = cfg.spec.stamp(&cfg.engine);
        Self {
            dir: index_dir.join("vectors"),
            stamp,
            table: RwLock::new(Table::default()),
            state: Mutex::new(ModelState {
                model,
                want: false,
                last_used: Instant::now(),
                loads: 0,
                unloads: 0,
                load_ms: 0.0,
                loaded_at_ms: 0,
            }),
            changed: Condvar::new(),
            kick: Mutex::new(false),
            kicked: Condvar::new(),
            stop: AtomicBool::new(false),
            stats: Mutex::new((EmbedStats::default(), None, 0)),
            compactions: Mutex::new(CompactState::default()),
            opening: Mutex::new(()),
            cfg,
        }
    }

    pub fn enabled(&self) -> bool {
        self.cfg.weights_dir.is_some()
    }

    pub fn stamp(&self) -> &Stamp {
        &self.stamp
    }

    /// `hybrid` while vectors can answer, or will once loaded; `bm25_only`
    /// when off, without weights, or refused.
    pub fn mode(&self) -> &'static str {
        match self.state.lock().unwrap().model {
            Model::Unloaded | Model::Loading | Model::Loaded(_) => "hybrid",
            _ => "bm25_only",
        }
    }

    pub fn wake(&self) {
        *self.kick.lock().unwrap() = true;
        self.kicked.notify_all();
    }

    pub fn request_stop(&self) {
        self.stop.store(true, Ordering::SeqCst);
        self.wake();
        self.changed.notify_all();
    }

    fn error(&self, e: String) {
        tracing::warn!(error = %e, "index: vectors");
        let mut s = self.stats.lock().unwrap();
        s.1 = Some(e);
        s.2 = now_ms();
    }

    // -- What the ingest thread tells it.

    /// A commit's nodes: their rows replaced, their new texts queued.
    pub fn on_commit(&self, nodes: &[NodeChunks]) {
        if !self.enabled() || nodes.is_empty() {
            return;
        }
        {
            let mut t = self.table.write().unwrap();
            for n in nodes {
                t.add_node(n);
            }
        }
        self.wake();
    }

    /// The index was dropped (a rebuild, or a tender that builds its index
    /// from the WAL's start): every row goes; the files stay, so the rebuilt
    /// chunks find their vectors again, and once the ingest thread has caught
    /// up the files are compacted to the texts the index still holds.
    pub fn on_clear(&self) {
        if self.enabled() {
            self.table.write().unwrap().clear();
        }
    }

    /// Nodes a re-ingest skipped (their record now gives the index nothing,
    /// as an erased payload will): their rows go, and their texts' records
    /// die unless another chunk holds them.
    pub fn on_remove(&self, nodes: &[String]) {
        if !self.enabled() || nodes.is_empty() {
            return;
        }
        let mut t = self.table.write().unwrap();
        for n in nodes {
            t.remove_node(n);
        }
        drop(t);
        self.wake();
    }

    /// The ingest thread has read the WAL to its end, after the tender's
    /// start or a rebuild: the rows are every chunk now, so a record no row
    /// holds is dead, and a rebuild's compaction is due.
    pub fn on_caught_up(&self) {
        if !self.enabled() {
            return;
        }
        let mut t = self.table.write().unwrap();
        if t.caught_up {
            return;
        }
        t.caught_up = true;
        Self::on_settled(&mut t);
        drop(t);
        self.wake();
    }

    /// Count the dead once the table has every row.
    fn on_settled(t: &mut Table) {
        if t.settled() {
            t.recount();
        }
    }

    /// Why the files should be compacted now, if they should: a quarter of
    /// a file dead, or a rebuild that asked. Never while the rows are not yet
    /// every chunk (a start, a rebuild's backfill): a text would look dead
    /// only because its row has not come back yet.
    fn compaction_due(&self) -> Option<&'static str> {
        if self
            .compactions
            .lock()
            .unwrap()
            .retry_at
            .is_some_and(|at| Instant::now() < at)
        {
            return None;
        }
        let t = self.table.read().unwrap();
        if !t.settled() {
            return None;
        }
        if let Some(why) = t.compact_asked {
            return Some(why);
        }
        t.caches
            .iter()
            .any(|c| 4 * c.dead() > c.len())
            .then_some("dead")
    }

    /// Rewrite every vector file without the records `gone` names: this
    /// stamp's, its space's older ones, and other spaces' (theirs answer
    /// nothing, but they hold the same texts' meaning). The rows are pointed
    /// at the new files.
    fn compact_files(
        t: &mut Table,
        dir: &Path,
        gone: &dyn Fn(u128) -> bool,
    ) -> anyhow::Result<Compacted> {
        let mut done = Compacted::default();
        let mut failed: Option<anyhow::Error> = None;
        for c in &mut t.caches {
            match c.compact(&|h| !gone(h)) {
                Ok(x) => done.add(x),
                Err(e) => {
                    failed.get_or_insert(e);
                }
            }
        }
        for (stamp, _, _) in &t.others {
            match Cache::open(dir, stamp).and_then(|mut c| c.compact(&|h| !gone(h))) {
                Ok(x) => done.add(x),
                Err(e) => {
                    failed.get_or_insert(e);
                }
            }
        }
        t.resolve_all();
        match failed {
            Some(e) => Err(e.context("compacting the vector files")),
            None => Ok(done),
        }
    }

    /// Drop every dead record from the files, now (the quarter rule, or a
    /// rebuild's ask).
    fn compact_now(&self, why: &'static str) {
        let t0 = Instant::now();
        let mut t = self.table.write().unwrap();
        if !t.settled() {
            return;
        }
        t.compact_asked = None;
        let live: HashSet<u128> = t.by_hash.keys().copied().collect();
        let r = Self::compact_files(&mut t, &self.dir, &|h| !live.contains(&h));
        drop(t);
        self.compacted(why, t0, &r);
    }

    fn compacted(&self, why: &str, t0: Instant, r: &anyhow::Result<Compacted>) {
        let ms = t0.elapsed().as_secs_f64() * 1e3;
        match r {
            Ok(c) => {
                if c.files > 0 {
                    tracing::info!(
                        why,
                        files = c.files,
                        dropped = c.dropped,
                        bytes_before = c.bytes_before,
                        bytes_after = c.bytes_after,
                        ms,
                        "index: vector files compacted"
                    );
                }
                let mut s = self.compactions.lock().unwrap();
                s.retry_at = None;
                if c.files == 0 {
                    return;
                }
                s.stats.count += 1;
                s.stats.dropped += c.dropped;
                s.stats.last_at_ms = now_ms();
                s.stats.last_why = why.to_string();
                s.stats.last_ms = ms;
                s.stats.last_bytes_before = c.bytes_before;
                s.stats.last_bytes_after = c.bytes_after;
            }
            Err(e) => {
                // Not again for a minute: a disk that refused a rewrite
                // would otherwise be asked again at once, forever.
                self.compactions.lock().unwrap().retry_at = Some(Instant::now() + BACKSTOP);
                self.error(format!("{e:#}"));
            }
        }
    }

    /// `index.forget`'s vector side (theseus-64x), once the index has
    /// dropped `nodes` whole and `chunks` one by one: their rows go now, and
    /// every file is rewritten without the records of `asked` (the texts
    /// they held, and those named) unless a chunk not forgotten still holds
    /// one (`held` names those chunks), and, once the rows are every chunk,
    /// without every other dead record too.
    pub fn forget(
        &self,
        nodes: &[String],
        chunks: &[(String, u32)],
        asked: &HashSet<u128>,
    ) -> anyhow::Result<Forgot> {
        if !self.enabled() {
            return Ok(Forgot::default());
        }
        let t0 = Instant::now();
        // The files are read first, so their records can be dropped even
        // before the embedding thread has started.
        self.open_files()?;
        let mut t = self.table.write().unwrap();
        for n in nodes {
            t.remove_node(n);
        }
        for (n, c) in chunks {
            t.remove_chunk(n, *c);
        }
        if !t.reconciled {
            // A reconcile reading the index now must not bring them back.
            t.forgot_nodes.extend(nodes.iter().cloned());
            t.forgot_chunks.extend(chunks.iter().cloned());
        }
        let mut held: Vec<(String, u32)> = asked
            .iter()
            .filter_map(|h| t.by_hash.get(h))
            .flatten()
            .map(|&i| {
                let r = &t.rows[i as usize];
                (r.node.to_string(), r.chunk)
            })
            .collect();
        held.sort();
        let settled = t.settled();
        let live: HashSet<u128> = t.by_hash.keys().copied().collect();
        let r = Self::compact_files(&mut t, &self.dir, &|h| {
            !live.contains(&h) && (settled || asked.contains(&h))
        });
        if settled {
            t.compact_asked = None;
        }
        drop(t);
        self.compacted("forget", t0, &r);
        Ok(Forgot {
            held,
            compacted: r?,
        })
    }

    // -- The embedding thread.

    /// Read this stamp's file, and any older stamp's of its space (they
    /// answer until re-embedded), once: the embedding thread at its start,
    /// or a forget that comes first.
    pub fn open_files(&self) -> anyhow::Result<()> {
        if !self.enabled() {
            return Ok(());
        }
        let _one = self.opening.lock().unwrap();
        if self.table.read().unwrap().opened {
            return Ok(());
        }
        fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(&self.dir)
            .context("creating the vectors directory")?;
        let current = Cache::open(&self.dir, &self.stamp)?;
        let mut older: Vec<(SystemTime, Cache)> = Vec::new();
        let mut others = Vec::new();
        for e in fs::read_dir(&self.dir)? {
            let path = e?.path();
            if path.extension().and_then(|x| x.to_str()) != Some("json") {
                continue;
            }
            let Some(stamp) = crate::state::load::<Stamp>(&path) else {
                continue;
            };
            if stamp == self.stamp {
                continue;
            }
            if stamp.same_space(&self.stamp) {
                let when = fs::metadata(&path)
                    .and_then(|m| m.modified())
                    .unwrap_or(SystemTime::UNIX_EPOCH);
                older.push((when, Cache::open(&self.dir, &stamp)?));
            } else {
                let vec = path.with_extension("vec");
                others.push((stamp, vec, path));
            }
        }
        older.sort_by_key(|x| Reverse(x.0));
        let mut t = self.table.write().unwrap();
        t.caches = std::iter::once(current)
            .chain(older.into_iter().map(|x| x.1))
            .collect();
        t.others = others;
        t.opened = true;
        t.resolve_all();
        Self::on_settled(&mut t);
        Ok(())
    }

    /// Rows for every chunk the index holds (at start: the index outlives
    /// the process, the rows do not). Nodes a commit has added meanwhile are
    /// kept as they are.
    pub fn reconcile(&self, texts: &dyn Texts) -> anyhow::Result<()> {
        if !self.enabled() {
            return Ok(());
        }
        let generation = self.table.read().unwrap().generation;
        let nodes = texts.all_nodes()?;
        let mut t = self.table.write().unwrap();
        if t.generation == generation {
            for n in &nodes {
                if t.by_node.contains_key(n.node_id.as_str()) || t.forgot_nodes.contains(&n.node_id)
                {
                    continue;
                }
                let mut n = n.clone();
                n.chunks
                    .retain(|c| !t.forgot_chunks.contains(&(n.node_id.clone(), c.chunk)));
                if !n.chunks.is_empty() {
                    t.add_node(&n);
                }
            }
        }
        t.reconciled = true;
        t.forgot_nodes.clear();
        t.forgot_chunks.clear();
        Self::on_settled(&mut t);
        Ok(())
    }

    /// The embedding thread: read the files, rebuild the rows, then embed
    /// the backlog, load and unload the model, until stopped. At nice 19
    /// within the tender's 10, so a backfill yields to the tender's own
    /// queries and ingest; and between two pieces of work it waits while the
    /// machine is busy (theseus-tood). Not in `SCHED_IDLE`, which can't be
    /// left: this thread also loads the model a waiting query needs, and
    /// candle's thread pool takes the policy of the thread that first runs it.
    pub fn run(&self, texts: &dyn Texts) {
        if !self.enabled() {
            return;
        }
        // SAFETY: no pointers. On Linux the nice value is the calling
        // thread's: this one only.
        unsafe { libc::setpriority(libc::PRIO_PROCESS, 0, 19) };
        if let Err(e) = self.open_files() {
            self.error(format!("reading the vector files: {e:#}"));
        }
        if let Err(e) = self.reconcile(texts) {
            self.error(format!("reading the index's chunks: {e:#}"));
        }
        while !self.stop.load(Ordering::SeqCst) {
            match self.work_once(texts) {
                Work::Did => {
                    let yielded = theseus_store::pressure::quiet_blocking_unless(
                        self.cfg.yield_bound,
                        || self.stop.load(Ordering::SeqCst),
                    );
                    if !yielded.is_zero() {
                        tracing::debug!(
                            ?yielded,
                            "index: the embedding thread waited while the machine was busy"
                        );
                    }
                }
                Work::Idle(d) => {
                    let k = self.kick.lock().unwrap();
                    let (mut k, _) = self
                        .kicked
                        .wait_timeout_while(k, d, |k| !*k && !self.stop.load(Ordering::SeqCst))
                        .unwrap();
                    *k = false;
                }
            }
        }
    }

    /// One turn of the embedding thread.
    pub fn work_once(&self, texts: &dyn Texts) -> Work {
        let Some(dir) = self.cfg.weights_dir.clone() else {
            return Work::Idle(BACKSTOP);
        };
        if let Some(why) = self.compaction_due() {
            self.compact_now(why);
            return Work::Did;
        }
        let (opened, pending) = {
            let t = self.table.read().unwrap();
            (t.opened && t.reconciled, t.backlog.len())
        };
        let mut st = self.state.lock().unwrap();
        // Weights that have arrived, or changed since they were refused.
        let wpath = self.cfg.spec.weights_path(&dir);
        let retry = match &st.model {
            Model::NoWeights => wpath.exists(),
            Model::Refused { sig, .. } => file_sig(&wpath) != *sig,
            _ => false,
        };
        if retry {
            st.model = Model::Unloaded;
        }
        enum Next {
            Load,
            Embed(Arc<Embedder>),
            Unload(Duration),
            Wait(Duration),
        }
        let need = st.want || (opened && pending > 0);
        let next = match &st.model {
            Model::Unloaded if need => Next::Load,
            Model::Loaded(emb) if opened && pending > 0 => Next::Embed(emb.clone()),
            Model::Loaded(_) => {
                let idle = st.last_used.elapsed();
                if idle >= self.cfg.idle_unload {
                    Next::Unload(idle)
                } else {
                    Next::Wait((self.cfg.idle_unload - idle).min(BACKSTOP))
                }
            }
            _ => Next::Wait(BACKSTOP),
        };
        match next {
            Next::Load => {
                st.model = Model::Loading;
                drop(st);
                self.load(&dir);
                Work::Did
            }
            Next::Embed(emb) => {
                drop(st);
                self.embed_batch(&emb, texts);
                Work::Did
            }
            Next::Unload(idle) => {
                st.model = Model::Unloaded;
                st.unloads += 1;
                drop(st);
                release_memory();
                tracing::info!(?idle, "index: the embedding model unloaded, unused");
                self.changed.notify_all();
                Work::Did
            }
            Next::Wait(d) => Work::Idle(d),
        }
    }

    fn load(&self, dir: &Path) {
        let t0 = Instant::now();
        let r = Embedder::load(dir, &self.cfg.spec);
        let ms = t0.elapsed().as_secs_f64() * 1e3;
        let mut st = self.state.lock().unwrap();
        st.want = false;
        match r {
            Ok(e) => {
                tracing::info!(ms, "index: the embedding model loaded");
                st.model = Model::Loaded(Arc::new(e));
                st.loads += 1;
                st.load_ms = ms;
                st.loaded_at_ms = now_ms();
                st.last_used = Instant::now();
            }
            Err(LoadError::Missing(p)) => {
                st.model = Model::NoWeights;
                drop(st);
                self.error(format!(
                    "no weights: {} is missing; BM25 alone answers",
                    p.display()
                ));
            }
            Err(e) => {
                let why = format!("{e:#}");
                st.model = Model::Refused {
                    why: why.clone(),
                    sig: file_sig(&self.cfg.spec.weights_path(dir)),
                };
                drop(st);
                release_memory();
                self.error(format!(
                    "the weights would not load; BM25 alone answers: {why}"
                ));
            }
        }
        self.changed.notify_all();
    }

    /// Embed the backlog's next batch, and point its rows at the vectors.
    fn embed_batch(&self, emb: &Embedder, texts: &dyn Texts) {
        // The texts, each by a row that still holds it (read from the index
        // after the lock is let go).
        let picked: Vec<(u128, Arc<str>, u32)> = {
            let mut t = self.table.write().unwrap();
            let hashes = t.backlog.take();
            hashes
                .into_iter()
                .filter_map(|h| {
                    let i = *t.by_hash.get(&h)?.first()?;
                    let r = &t.rows[i as usize];
                    Some((h, r.node.clone(), r.chunk))
                })
                .collect()
        };
        let batch: Vec<(u128, String)> = picked
            .into_iter()
            .filter_map(|(h, node, chunk)| Some((h, texts.chunk_text(&node, chunk)?)))
            .collect();
        if batch.is_empty() {
            return;
        }
        let (t0, c0) = (Instant::now(), thread_cpu_ms());
        let rows: Vec<Windows> = batch
            .iter()
            .map(|(_, text)| emb.tokenize(Task::SearchDocument, text))
            .collect();
        let tokens: u64 = rows.iter().map(|r| r.tokens() as u64).sum();
        let truncated = rows.iter().filter(|r| r.truncated).count() as u64;
        let windowed = rows.iter().filter(|r| r.ids.len() > 1).count() as u64;
        let r = emb.embed_windows(&rows);
        let (wall, cpu) = (t0.elapsed().as_millis() as u64, thread_cpu_ms() - c0);
        self.state.lock().unwrap().last_used = Instant::now();
        let vectors = match r {
            Ok(v) => v,
            Err(e) => {
                let mut s = self.stats.lock().unwrap();
                s.0.failed += batch.len() as u64;
                drop(s);
                self.error(format!("a batch of {} would not embed: {e:#}", batch.len()));
                return;
            }
        };
        let mut t = self.table.write().unwrap();
        // Only texts a chunk still holds: one forgotten while it was being
        // embedded must not come back into the file (theseus-64x).
        let items: Vec<(u128, &Vector)> = batch
            .iter()
            .map(|(h, _)| *h)
            .zip(&vectors)
            .filter(|(h, _)| t.by_hash.contains_key(h))
            .collect();
        let Some(cache) = t.caches.first_mut() else {
            return;
        };
        match cache.append(&items) {
            Ok(entries) => {
                for ((h, _), e) in items.iter().zip(entries) {
                    for i in t.by_hash.get(h).cloned().unwrap_or_default() {
                        t.rows[i as usize].vec = Some((0, e));
                    }
                }
            }
            Err(e) => {
                drop(t);
                self.error(format!("writing vectors: {e}"));
                return;
            }
        }
        if t.backlog.len() == 0 {
            Self::finish_stamp(&mut t);
        }
        drop(t);
        let mut s = self.stats.lock().unwrap();
        s.0.texts += batch.len() as u64;
        s.0.batches += 1;
        s.0.tokens += tokens;
        s.0.truncated += truncated;
        s.0.windowed += windowed;
        s.0.wall_ms += wall;
        s.0.cpu_ms += cpu;
    }

    /// The backlog is empty: once every chunk has this stamp's vector, the
    /// older stamps' files go. (Dead records go by the compaction rules, in
    /// `work_once`.)
    fn finish_stamp(t: &mut Table) {
        let covered = t.alive().all(|r| matches!(r.vec, Some((0, _))));
        if !covered {
            return;
        }
        if t.caches.len() > 1 || !t.others.is_empty() {
            for c in t.caches.drain(1..) {
                tracing::info!(stamp = ?c.stamp, "index: an older stamp's vectors re-embedded, its file removed");
                c.remove();
            }
            for (_, vec, json) in t.others.drain(..) {
                let _ = fs::remove_file(vec);
                let _ = fs::remove_file(json);
            }
        }
    }

    /// Embed everything pending, now, on this thread (tests, and a check).
    pub fn settle(&self, texts: &dyn Texts) -> anyhow::Result<()> {
        {
            let t = self.table.read().unwrap();
            if !t.opened {
                drop(t);
                self.open_files()?;
            }
        }
        if !self.table.read().unwrap().reconciled {
            self.reconcile(texts)?;
        }
        loop {
            let pending = self.table.read().unwrap().backlog.len();
            let model = self.state.lock().unwrap().model.name();
            if pending == 0 || !matches!(model, "unloaded" | "loading" | "loaded") {
                return Ok(());
            }
            self.work_once(texts);
        }
    }

    // -- What the socket's threads ask.

    /// The model, waiting up to `wait` for a load; or why not.
    pub fn model(&self, wait: Duration) -> Result<Arc<Embedder>, String> {
        let deadline = Instant::now() + wait;
        let mut st = self.state.lock().unwrap();
        loop {
            match &st.model {
                Model::Loaded(e) => {
                    let e = e.clone();
                    st.last_used = Instant::now();
                    return Ok(e);
                }
                Model::Off => return Err("vectors are off: no weights_dir".into()),
                Model::NoWeights => {
                    return Err(format!(
                        "bm25_only: no weights in {}",
                        self.cfg
                            .weights_dir
                            .as_deref()
                            .unwrap_or(Path::new(""))
                            .display()
                    ))
                }
                Model::Refused { why, .. } => return Err(format!("bm25_only: {why}")),
                Model::Unloaded | Model::Loading => {
                    if !st.want {
                        st.want = true;
                        self.wake();
                    }
                }
            }
            let now = Instant::now();
            if now >= deadline || self.stop.load(Ordering::SeqCst) {
                return Err("the model is loading".into());
            }
            st = self.changed.wait_timeout(st, deadline - now).unwrap().0;
        }
    }

    /// `index.warm`: start a load if none is running, and answer at once.
    pub fn warm(&self) -> String {
        let mut st = self.state.lock().unwrap();
        st.last_used = Instant::now();
        if matches!(st.model, Model::Unloaded) {
            if !st.want {
                st.want = true;
                drop(st);
                self.wake();
            }
            return "loading".into();
        }
        st.model.name().into()
    }

    /// The vector source: the query embedded, the scan, the re-score.
    /// `Err` says why the source did not answer.
    pub fn search(
        &self,
        text: &str,
        as_of: Option<u64>,
        exclude: &[String],
        filters: &Filters,
        fetch: usize,
        wait: Duration,
    ) -> Result<(Vec<VectorHit>, f64, f64), String> {
        let emb = self.model(wait)?;
        let t0 = Instant::now();
        let v = emb
            .embed(Task::SearchQuery, &[text])
            .map_err(|e| format!("the query would not embed: {e:#}"))?
            .remove(0);
        let embed_ms = t0.elapsed().as_secs_f64() * 1e3;
        let t1 = Instant::now();
        let t = self.table.read().unwrap();
        let f = t.filter(as_of, exclude, filters);
        let hits = t
            .search(&v.full, emb.spec().cut, &f, fetch)
            .map_err(|e| format!("reading vectors: {e}"))?
            .into_iter()
            .map(|(i, score)| {
                let r = &t.rows[i];
                VectorHit {
                    node_id: r.node.clone(),
                    chunk: r.chunk,
                    position: r.position,
                    score,
                }
            })
            .collect();
        Ok((hits, embed_ms, t1.elapsed().as_secs_f64() * 1e3))
    }

    /// The nodes nearest `node_id`, by the mean of its chunks' 768-d
    /// vectors; each by its best chunk. No model needed: the vectors are
    /// stored.
    pub fn neighbours(
        &self,
        node_id: &str,
        k: usize,
        as_of: Option<u64>,
    ) -> Result<(Vec<Neighbour>, f64), String> {
        let t0 = Instant::now();
        let t = self.table.read().unwrap();
        let rows = t
            .by_node
            .get(node_id)
            .ok_or_else(|| format!("no node {node_id} in the index"))?;
        let mut sum: Option<Vec<f32>> = None;
        for &i in rows {
            let Some(v) = t.full_of(i as usize).map_err(|e| e.to_string())? else {
                continue;
            };
            match &mut sum {
                Some(s) => s.iter_mut().zip(&v).for_each(|(a, b)| *a += b),
                None => sum = Some(v),
            }
        }
        let full = embedder::l2_normalize(
            &sum.ok_or_else(|| format!("node {node_id} has no vector yet"))?,
        );
        let mut f = t.filter(as_of, &[], &Filters::default());
        f.skip_node = rows.first().map(|&i| t.rows[i as usize].node.clone());
        let k = k.clamp(1, 100);
        let scored = t
            .search(
                &full,
                self.cfg.spec.cut.min(full.len()),
                &f,
                (4 * k).max(RESCORE),
            )
            .map_err(|e| e.to_string())?;
        let mut seen = HashSet::new();
        let mut out = Vec::new();
        for (i, score) in scored {
            let r = &t.rows[i];
            if !seen.insert(r.node.clone()) {
                continue;
            }
            out.push(Neighbour {
                node_id: r.node.to_string(),
                chunk: u64::from(r.chunk),
                score,
                position: r.position,
                session_id: t.sessions.name(r.session).to_string(),
                kind: t.kinds.name(u32::from(r.kind)).to_string(),
            });
            if out.len() == k {
                break;
            }
        }
        Ok((out, t0.elapsed().as_secs_f64() * 1e3))
    }

    /// `index.embed`: vectors for texts, on the caller's thread, in batches
    /// planned as the backfill's are.
    pub fn embed(&self, p: &EmbedParams) -> anyhow::Result<EmbedResult> {
        anyhow::ensure!(
            p.texts.len() <= EMBED_MAX_TEXTS,
            "{} texts: at most {EMBED_MAX_TEXTS} a call",
            p.texts.len()
        );
        let emb = self
            .model(Duration::from_millis(p.wait_ms))
            .map_err(anyhow::Error::msg)?;
        let full = emb.spec().config.hidden;
        let dims = p.dims.unwrap_or(full);
        anyhow::ensure!(
            dims == full || dims == emb.spec().cut,
            "dims {dims}: {full} or {}",
            emb.spec().cut
        );
        let t0 = Instant::now();
        let rows: Vec<Windows> = p.texts.iter().map(|t| emb.tokenize(p.task, t)).collect();
        let lens: Vec<usize> = rows.iter().map(Windows::tokens).collect();
        let mut vectors = vec![Vec::new(); rows.len()];
        for b in plan_batches(&lens) {
            let batch: Vec<Windows> = b.iter().map(|&i| rows[i].clone()).collect();
            for (i, v) in b.iter().zip(emb.embed_windows(&batch)?) {
                vectors[*i] = if dims == full { v.full } else { v.cut };
            }
        }
        self.state.lock().unwrap().last_used = Instant::now();
        Ok(EmbedResult {
            vectors,
            dims,
            stamp: self.stamp.clone(),
            tokens: lens,
            embed_ms: t0.elapsed().as_secs_f64() * 1e3,
        })
    }

    pub fn status(&self) -> VectorStatus {
        let st = self.state.lock().unwrap();
        let model = st.model.name().to_string();
        let since = |i: Instant| now_ms().saturating_sub(i.elapsed().as_millis() as u64);
        let mut s = VectorStatus {
            model,
            weights_dir: self
                .cfg
                .weights_dir
                .as_ref()
                .map(|d| d.display().to_string()),
            stamp: self.enabled().then(|| self.stamp.clone()),
            loads: st.loads,
            unloads: st.unloads,
            load_ms: st.load_ms,
            loaded_at_ms: st.loaded_at_ms,
            last_used_ms: since(st.last_used),
            idle_unload_secs: self.cfg.idle_unload.as_secs(),
            threads: format!(
                "RAYON_NUM_THREADS={} CANDLE_NUM_THREADS={}",
                std::env::var("RAYON_NUM_THREADS").unwrap_or_else(|_| "unset".into()),
                std::env::var("CANDLE_NUM_THREADS").unwrap_or_else(|_| "unset".into())
            ),
            ..VectorStatus::default()
        };
        if let Model::Refused { why, .. } = &st.model {
            s.last_error = Some(why.clone());
        }
        drop(st);
        {
            let t = self.table.read().unwrap();
            let mut done = 0u64;
            for r in t.alive() {
                s.chunks += 1;
                if r.vec.is_some() {
                    s.vectors += 1;
                }
                if matches!(r.vec, Some((0, _))) {
                    done += 1;
                }
            }
            s.pending = t.backlog.len() as u64;
            let (records, dead) = t.records();
            (s.records, s.dead) = (records as u64, dead as u64);
            if t.caches.len() > 1 || !t.others.is_empty() {
                s.reembed = Some(Reembed {
                    from: t
                        .caches
                        .iter()
                        .skip(1)
                        .map(|c| c.stamp.clone())
                        .chain(t.others.iter().map(|o| o.0.clone()))
                        .collect(),
                    done,
                    total: s.chunks,
                });
            }
        }
        let stats = self.stats.lock().unwrap();
        s.backfill = stats.0.clone();
        if s.last_error.is_none() {
            s.last_error = stats.1.clone();
        }
        s.last_error_ms = stats.2;
        drop(stats);
        s.compactions = self.compactions.lock().unwrap().stats.clone();
        s
    }
}

/// Give freed model memory back to the system: glibc keeps freed heap
/// chunks below its (growing) mmap threshold; musl returns them itself.
#[cfg(all(target_os = "linux", target_env = "gnu"))]
fn release_memory() {
    // SAFETY: no pointers.
    unsafe {
        libc::malloc_trim(0);
    }
}

#[cfg(not(all(target_os = "linux", target_env = "gnu")))]
fn release_memory() {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::weights::SplitMix;
    use std::os::unix::fs::PermissionsExt;

    fn stamp(cut: usize, full: usize) -> Stamp {
        Stamp {
            model: "m@1".into(),
            weights: "w".into(),
            tokenizer: "t".into(),
            engine: "e".into(),
            precision: "f32".into(),
            dims: [cut, full],
        }
    }

    fn random_vector(rng: &mut SplitMix, full: usize, cut: usize) -> Vector {
        let raw: Vec<f32> = (0..full).map(|_| rng.unit()).collect();
        let (f, c) = embedder::finish(&raw, cut);
        Vector {
            full: f,
            cut: c,
            tokens: 1,
            windowed: false,
            truncated: false,
        }
    }

    /// A table of `n` rows over random vectors, with random positions,
    /// sessions, kinds, and external flags; some rows dead, some without a
    /// vector.
    fn table(dir: &Path, n: usize, seed: u64) -> Table {
        let mut rng = SplitMix(seed);
        let mut t = Table::default();
        let mut cache = Cache::open(dir, &stamp(16, 48)).unwrap();
        let vs: Vec<Vector> = (0..n).map(|_| random_vector(&mut rng, 48, 16)).collect();
        let items: Vec<(u128, &Vector)> =
            vs.iter().enumerate().map(|(i, v)| (i as u128, v)).collect();
        cache.append(&items).unwrap();
        t.caches.push(cache);
        t.opened = true;
        for i in 0..n {
            let r = rng.next_u64();
            t.add_node(&NodeChunks {
                node_id: format!("nd_{i}"),
                position: r % 1000,
                session: format!("ses_{}", r % 5),
                kind: ["user_message", "tool_result"][(r >> 8) as usize % 2].into(),
                external: (r >> 16).is_multiple_of(3),
                // Every seventh row's text has no vector yet.
                chunks: vec![ChunkKey {
                    chunk: 0,
                    hash: if i % 7 == 6 {
                        u128::MAX - i as u128
                    } else {
                        i as u128
                    },
                    tokens: 10,
                }],
            });
        }
        for i in (0..n).step_by(11) {
            t.remove_node(&format!("nd_{i}"));
        }
        t
    }

    #[test]
    fn the_flat_scan_matches_a_naive_loop() {
        let tmp = tempfile::tempdir().unwrap();
        let t = table(tmp.path(), 500, 7);
        let mut rng = SplitMix(99);
        let filters = [
            (None, vec![], Filters::default()),
            (Some(500), vec![], Filters::default()),
            (None, vec!["ses_1".to_string()], Filters::default()),
            (
                Some(800),
                vec![],
                Filters {
                    sessions: vec!["ses_2".into(), "ses_3".into()],
                    kinds: vec!["tool_result".into()],
                    external: Some(false),
                    ..Filters::default()
                },
            ),
            (
                None,
                vec![],
                Filters {
                    sessions: vec!["ses_nowhere".into()],
                    ..Filters::default()
                },
            ),
        ];
        for (as_of, exclude, f) in &filters {
            for n in [1, 10, 100, 1000] {
                let q: Vec<i8> = (0..16).map(|_| (rng.next_u64() % 255) as i8).collect();
                let rf = t.filter(*as_of, exclude, f);
                let got = Flat(&t).top(&q, n, &|i| t.admits(&rf, i));
                // The naive loop: every admitted row with a vector, scored,
                // sorted by score and then row.
                let mut want: Vec<(usize, f32)> = (0..t.rows.len())
                    .filter(|&i| t.rows[i].alive && t.admits(&rf, i))
                    .filter_map(|i| {
                        let (c, e) = t.rows[i].vec?;
                        let cache = &t.caches[c as usize];
                        Some((i, dot_i8(cache.q(e), &q) as f32 * cache.scale[e as usize]))
                    })
                    .collect();
                want.sort_by(|a, b| b.1.total_cmp(&a.1).then(a.0.cmp(&b.0)));
                want.truncate(n);
                assert_eq!(got, want, "as_of {as_of:?}, exclude {exclude:?}, n {n}");
                for &(i, _) in &got {
                    let r = &t.rows[i];
                    assert!(as_of.is_none_or(|a| r.position < a));
                    assert!(!exclude.contains(&t.sessions.name(r.session).to_string()));
                }
            }
        }
        // The unknown session admits nothing; the rest admit something.
        let none = t.filter(None, &[], &filters[4].2);
        assert!(Flat(&t)
            .top(&[1; 16], 10, &|i| t.admits(&none, i))
            .is_empty());
    }

    #[test]
    fn the_re_score_orders_by_the_full_vectors_cosine() {
        let tmp = tempfile::tempdir().unwrap();
        let t = table(tmp.path(), 60, 3);
        let mut rng = SplitMix(5);
        let q = random_vector(&mut rng, 48, 16);
        let rf = t.filter(None, &[], &Filters::default());
        let got = t.search(&q.full, 16, &rf, 30).unwrap();
        // Fewer rows than RESCORE: every one with a vector is re-scored.
        let mut want: Vec<(usize, f64)> = (0..t.rows.len())
            .filter(|&i| t.rows[i].alive)
            .filter_map(|i| Some((i, embedder::cosine(&q.full, &t.full_of(i).unwrap()?))))
            .collect();
        want.sort_by(|a, b| {
            b.1.total_cmp(&a.1)
                .then(t.rows[a.0].position.cmp(&t.rows[b.0].position))
                .then(a.0.cmp(&b.0))
        });
        want.truncate(30);
        assert_eq!(got, want);
    }

    #[test]
    fn a_vector_file_round_trips_and_its_torn_tail_is_cut() {
        let tmp = tempfile::tempdir().unwrap();
        let s = stamp(16, 48);
        let mut rng = SplitMix(1);
        let vs: Vec<Vector> = (0..5).map(|_| random_vector(&mut rng, 48, 16)).collect();
        let mut c = Cache::open(tmp.path(), &s).unwrap();
        let items: Vec<(u128, &Vector)> = vs
            .iter()
            .enumerate()
            .map(|(i, v)| (i as u128 + 10, v))
            .collect();
        assert_eq!(c.append(&items).unwrap(), vec![0, 1, 2, 3, 4]);
        drop(c);
        let path = tmp.path().join(format!("{}.vec", stamp_key(&s)));
        let len = fs::metadata(&path).unwrap().len();
        // A record whose bytes changed fails its check: it and all after go.
        let mut b = fs::read(&path).unwrap();
        let rec = rec_len(16, 48) as u64;
        b[(HEADER + 3 * rec + 30) as usize] ^= 1;
        fs::write(&path, &b).unwrap();
        let c = Cache::open(tmp.path(), &s).unwrap();
        assert_eq!(c.len(), 3);
        assert_eq!(fs::metadata(&path).unwrap().len(), HEADER + 3 * rec);
        for (i, v) in vs.iter().take(3).enumerate() {
            let e = c.get(i as u128 + 10).unwrap();
            let back = c.full(e).unwrap();
            assert!(embedder::cosine(&back, &v.full) > 0.999_99);
            assert_eq!(c.q(e), quantize(&v.cut).0.as_slice());
        }
        assert!(len > HEADER + 3 * rec);
        // A header that is not this format's starts the file over.
        let mut b = fs::read(&path).unwrap();
        b[0] = b'X';
        fs::write(&path, &b).unwrap();
        assert!(Cache::open(tmp.path(), &s).unwrap().is_empty());
        assert_eq!(fs::metadata(&path).unwrap().len(), HEADER);
    }

    #[test]
    fn the_backlog_batches_short_texts_of_one_length_newest_first() {
        let mut b = Backlog::default();
        // (hash, tokens, position, answers)
        for (h, tok, pos, ans) in [
            (1, 20, 10, false),
            (2, 20, 50, false),
            (3, 300, 60, false),
            (4, 100, 40, false),
            (5, 20, 70, true),
            (6, 30, 5, false),
        ] {
            b.add(h, tok, pos, ans);
        }
        // Rows with no vector first, newest first: 3 (300 tokens) goes alone.
        assert_eq!(b.take(), vec![3]);
        // Then 2, with the other short ones of its bucket, newest first.
        assert_eq!(b.take(), vec![2, 1, 6]);
        // Then 4 (100 tokens), the only one of its bucket.
        assert_eq!(b.take(), vec![4]);
        // Last, the one an older stamp's vector answers for.
        assert_eq!(b.take(), vec![5]);
        assert!(b.take().is_empty());
        // Added again with a newer position, it moves up; removed, it goes.
        b.add(1, 20, 1, false);
        b.add(2, 20, 2, false);
        b.add(1, 20, 9, false);
        assert_eq!(b.order.first().map(|s| s.2), Some(1));
        b.remove(1);
        assert_eq!(b.take(), vec![2]);
    }

    #[test]
    fn batches_are_planned_by_length() {
        assert_eq!(
            plan_batches(&[5, 600, 7, 128, 129, 6, 6, 6, 6, 6, 6, 6]),
            vec![vec![0, 5, 6, 7, 8, 9, 10, 11], vec![2, 3], vec![4], vec![1]]
        );
        assert!(plan_batches(&[]).is_empty());
    }

    /// The scan at the design's scale: 100,000 chunks at 256 int8 dimensions
    /// (25.6 MB), and their 768-d vectors on disk. Prints the scan's and the
    /// scan-and-re-score's p50 and p95; HNSW waits until the scan's p95
    /// passes 10 ms (§2.2). Run in release:
    /// `cargo test --release -p theseus-index --lib -- --ignored --nocapture bench_`.
    #[test]
    #[ignore = "a benchmark: 181 MB of vectors, timed (run in release)"]
    fn bench_the_flat_scan_over_100k_chunks() {
        let n = 100_000;
        let tmp = tempfile::tempdir().unwrap();
        let mut rng = SplitMix(2026);
        let mut t = Table::default();
        let mut cache = Cache::open(tmp.path(), &stamp(256, 768)).unwrap();
        for start in (0..n).step_by(1000) {
            let vs: Vec<Vector> = (0..1000)
                .map(|_| random_vector(&mut rng, 768, 256))
                .collect();
            let items: Vec<(u128, &Vector)> = vs
                .iter()
                .enumerate()
                .map(|(i, v)| ((start + i) as u128, v))
                .collect();
            cache.append(&items).unwrap();
        }
        t.caches.push(cache);
        t.opened = true;
        for i in 0..n {
            t.add_node(&NodeChunks {
                node_id: format!("nd_{i}"),
                position: i as u64,
                session: format!("ses_{}", i % 50),
                kind: "tool_result".into(),
                external: false,
                chunks: vec![ChunkKey {
                    chunk: 0,
                    hash: i as u128,
                    tokens: 100,
                }],
            });
        }
        let time = |label: &str, f: &mut dyn FnMut()| {
            let mut ms: Vec<f64> = (0..40)
                .map(|_| {
                    let t0 = Instant::now();
                    f();
                    t0.elapsed().as_secs_f64() * 1e3
                })
                .collect();
            ms.sort_by(f64::total_cmp);
            eprintln!(
                "{label}: p50 {:.2} ms, p95 {:.2} ms, max {:.2} ms",
                ms[ms.len() / 2],
                ms[ms.len() * 95 / 100],
                ms[ms.len() - 1]
            );
        };
        let q = random_vector(&mut rng, 768, 256);
        let (qi, _) = quantize(&q.cut);
        let all = t.filter(None, &[], &Filters::default());
        let half = t.filter(Some(n as u64 / 2), &["ses_7".into()], &Filters::default());
        eprintln!("{n} chunks; int8 cuts {} MB in memory", n * 256 / 1_000_000);
        time("the int8 scan, best 100, no filter", &mut || {
            assert_eq!(
                Flat(&t).top(&qi, RESCORE, &|i| t.admits(&all, i)).len(),
                RESCORE
            );
        });
        time(
            "the int8 scan, best 100, as_of and an excluded session",
            &mut || {
                assert_eq!(
                    Flat(&t).top(&qi, RESCORE, &|i| t.admits(&half, i)).len(),
                    RESCORE
                );
            },
        );
        time(
            "the scan and the 768-d re-score of its best 100",
            &mut || {
                assert_eq!(t.search(&q.full, 256, &all, 30).unwrap().len(), 30);
            },
        );
    }

    /// A file of 40 records (hashes 0..40), and the bytes a compaction that
    /// keeps the even hashes must leave.
    fn forty(dir: &Path) -> (Cache, Vec<u8>, Vec<u8>) {
        let mut rng = SplitMix(64);
        let mut c = Cache::open(dir, &stamp(16, 48)).unwrap();
        let vs: Vec<Vector> = (0..40).map(|_| random_vector(&mut rng, 48, 16)).collect();
        let items: Vec<(u128, &Vector)> =
            vs.iter().enumerate().map(|(i, v)| (i as u128, v)).collect();
        c.append(&items).unwrap();
        let path = dir.join(format!("{}.vec", stamp_key(&stamp(16, 48))));
        let old = fs::read(&path).unwrap();
        let rec = c.rec;
        let mut new = old[..HEADER as usize].to_vec();
        for e in (0..40).step_by(2) {
            let at = HEADER as usize + e * rec;
            new.extend_from_slice(&old[at..at + rec]);
        }
        (c, old, new)
    }

    fn even(h: u128) -> bool {
        h.is_multiple_of(2)
    }

    /// theseus-64x: a compaction stopped at each of its steps, as a crash
    /// would stop it, leaves the old file whole (the copy unfinished, or
    /// whole but not renamed) or the new one whole (renamed), never a part of
    /// either; the next open removes a copy left behind and reads every
    /// record of whichever file it finds. Uninterrupted, the file is exactly
    /// the kept records, and every record still reads back.
    #[test]
    fn a_crash_during_the_rewrite_leaves_the_old_file_or_the_new_never_a_torn_one() {
        for (stop, want_new) in [
            (Step::HalfCopied, false),
            (Step::Copied, false),
            (Step::Renamed, true),
        ] {
            let tmp = tempfile::tempdir().unwrap();
            let (mut c, old, new) = forty(tmp.path());
            let path = c.paths().0;
            let copy = path.with_extension("vec.tmp");
            let err = c
                .rewrite(&even, &mut |s| {
                    if s == stop {
                        Err(io::Error::other("killed (test)"))
                    } else {
                        Ok(())
                    }
                })
                .unwrap_err();
            assert!(format!("{err:#}").contains("killed"), "{stop:?}: {err:#}");
            drop(c);
            let on_disk = fs::read(&path).unwrap();
            assert!(
                on_disk == if want_new { new } else { old },
                "{stop:?}: the file is neither the old one nor the new"
            );
            // The copy is left where a crash before the rename leaves it.
            assert_eq!(copy.exists(), !want_new, "{stop:?}");
            let again = Cache::open(tmp.path(), &stamp(16, 48)).unwrap();
            assert!(!copy.exists(), "{stop:?}: the open removes the copy");
            let mut hashes: Vec<u128> = again.by_hash.keys().copied().collect();
            hashes.sort_unstable();
            let want: Vec<u128> = (0..40).filter(|h| !want_new || even(*h)).collect();
            assert_eq!(hashes, want, "{stop:?}");
            assert_eq!(fs::read(&path).unwrap(), on_disk, "{stop:?}: nothing cut");
        }

        // Uninterrupted: exactly the kept records, in memory as on disk.
        let tmp = tempfile::tempdir().unwrap();
        let (mut c, old, new) = forty(tmp.path());
        let full_before: Vec<Vec<f32>> = (0..40).map(|e| c.full(e).unwrap()).collect();
        let done = c.compact(&even).unwrap();
        assert_eq!(
            done,
            Compacted {
                files: 1,
                dropped: 20,
                bytes_before: old.len() as u64,
                bytes_after: new.len() as u64,
            }
        );
        let path = c.paths().0;
        assert_eq!(fs::read(&path).unwrap(), new);
        assert!(!path.with_extension("vec.tmp").exists());
        for h in (0..40u128).step_by(2) {
            let e = c.get(h).unwrap();
            assert_eq!(c.full(e).unwrap(), full_before[h as usize]);
        }
        assert!((1..40u128).step_by(2).all(|h| c.get(h).is_none()));
        // The copy was made 0600, as the file is private.
        assert_eq!(
            fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
        // Keeping everything rewrites nothing; an append after a compaction
        // goes to the new file.
        assert_eq!(c.compact(&|_| true).unwrap(), Compacted::default());
        let mut rng = SplitMix(5);
        let v = random_vector(&mut rng, 48, 16);
        c.append(&[(1000, &v)]).unwrap();
        let again = Cache::open(tmp.path(), &stamp(16, 48)).unwrap();
        assert_eq!(again.len(), 21);
        assert!(again.get(1000).is_some());
    }

    /// A text that loses its last row is dead at once, in every file that
    /// holds it, and alive again when a row holds it again; a recount from
    /// the rows agrees with the marks.
    #[test]
    fn a_record_is_dead_once_no_row_holds_its_text() {
        let tmp = tempfile::tempdir().unwrap();
        let mut t = table(tmp.path(), 60, 3);
        t.reconciled = true;
        t.caught_up = true;
        t.recount();
        let marks = |t: &Table| -> Vec<bool> { t.caches[0].dead.clone() };
        let counted = marks(&t);
        // Rows removed by `table` (every 11th) left their texts dead.
        for e in (0..60).step_by(11) {
            assert!(counted[e], "{e}");
        }
        let (n, dead) = t.records();
        assert_eq!((n, dead), (60, counted.iter().filter(|d| **d).count()));
        // A node removed now: its record dies at once.
        assert!(!t.caches[0].dead[5]);
        t.remove_node("nd_5");
        assert!(t.caches[0].dead[5]);
        // Its text said again by another node: alive again.
        t.add_node(&NodeChunks {
            node_id: "nd_again".into(),
            position: 1,
            session: "ses_1".into(),
            kind: "user_message".into(),
            external: false,
            chunks: vec![ChunkKey {
                chunk: 0,
                hash: 5,
                tokens: 10,
            }],
        });
        assert!(!t.caches[0].dead[5]);
        // One chunk of a node goes alone.
        assert_eq!(t.remove_chunk("nd_again", 0), Some(5));
        assert!(t.caches[0].dead[5] && !t.by_node.contains_key("nd_again"));
        assert_eq!(t.remove_chunk("nd_none", 0), None);
        // Incremental marks and a recount from the rows agree.
        let incremental = marks(&t);
        t.recount();
        assert_eq!(marks(&t), incremental);
    }

    /// theseus-64x at the design's scale: a file of 100,000 records at
    /// [256, 768] (181.6 MB) compacted with a quarter of them dead, which
    /// holds the table's write lock (a vector query waits that long); and
    /// the open that reads it. Records are written raw (each with its CRC),
    /// so building the file costs no model. Prints the times. Run with
    /// `cargo nextest run --workspace --run-ignored only --no-capture -E
    /// 'test(bench_a_compaction)'` (a debug build: the copy is I/O).
    #[test]
    #[ignore = "a benchmark: 182 MB written, compacted, timed"]
    fn bench_a_compaction_of_100k_records() {
        let tmp = tempfile::tempdir().unwrap();
        let st = stamp(256, 768);
        let n = 100_000usize;
        {
            let c = Cache::open(tmp.path(), &st).unwrap();
            let mut rng = SplitMix(11);
            let mut w = io::BufWriter::with_capacity(1 << 20, &c.file);
            let mut rec = vec![0u8; c.rec];
            for i in 0..n {
                rec[..16].copy_from_slice(&(i as u128).to_le_bytes());
                for b in rec[16..c.rec - 4].iter_mut() {
                    *b = (rng.next_u64() & 0x3f) as u8;
                }
                let at = c.rec - 4;
                let crc = crc32fast::hash(&rec[..at]);
                rec[at..].copy_from_slice(&crc.to_le_bytes());
                w.write_all(&rec).unwrap();
            }
            w.flush().unwrap();
        }
        let t0 = Instant::now();
        let mut c = Cache::open(tmp.path(), &st).unwrap();
        let open = t0.elapsed();
        assert_eq!(c.len(), n);
        let t1 = Instant::now();
        let done = c.compact(&|h| !h.is_multiple_of(4)).unwrap();
        let compact = t1.elapsed();
        assert_eq!(done.dropped, n as u64 / 4);
        eprintln!(
            "{n} records, {:.1} MB: the open {:.0} ms; a compaction dropping a quarter {:.0} ms, {:.1} MB left",
            done.bytes_before as f64 / 1e6,
            open.as_secs_f64() * 1e3,
            compact.as_secs_f64() * 1e3,
            done.bytes_after as f64 / 1e6,
        );
    }
}
