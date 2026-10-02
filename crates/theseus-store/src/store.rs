//! The `Store` contract and `WalStore`, which composes the WAL (truth) with an
//! index (cache) and rebuilds the index from the WAL past the last checkpoint
//! on open.
//!
//! **Versions** (theseus-qa0 F4a). `MANIFEST.json` names the store's format
//! and, from format 3, the newest schema written for each record kind. Open
//! reads it first and refuses a store holding a kind newer than this build
//! knows, before anything is written. An append whose record is newer than
//! the manifest's mark rewrites the manifest first, durably, so no newer
//! record is ever on disk under a manifest that hides it. A format-2 store
//! (every record schema 1) stays format 2 until this build writes a newer
//! record into it, so an older binary still opens a store this one only read.
//!
//! **One writer** (theseus-vni9). A `WalStore` owns a thread, `store-writer`,
//! which owns every append: a caller hands it a frame and waits for its
//! answer, and the writer writes every frame queued, syncs once for all of
//! them, indexes them in one transaction, and answers each. The wait holds no
//! runtime worker (`blocking`).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{mpsc, Arc, RwLock};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::index::{Aside, Engine, IndexEntry, MovedAside, Projected, RedbIndex, Sums};
use crate::record::{kinds, NewRecord, Record, RecordKind};
use crate::wal::{History, RecordLocation, Recovery, Verified, Wal, WalConfig};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoreStats {
    pub last_position: u64,
    pub checkpoint: Option<u64>,
    pub wal_bytes: u64,
    pub wal_segments: u32,
    pub recovered_records: u64,
    pub truncated_bytes: u64,
    pub replayed_into_index: u64,
    /// Frames appended and fdatasync calls since open (group commit ratio).
    pub frames_appended: u64,
    pub syncs: u64,
    /// WAL bytes open did not check, before the frame after the checkpoint
    /// (theseus-8ni): `verify_history` checks them after serving.
    #[serde(default)]
    pub history_bytes: u64,
    /// The index was repaired at open: the last process did not close it.
    #[serde(default)]
    pub index_repaired: bool,
    /// How long open waited for the previous process to release the store
    /// (theseus-qa0 F4b): a start at once after a stop.
    #[serde(default)]
    pub lock_wait_us: u64,
    /// The index file open found was not a redb database: where it went
    /// before the index was built again from the WAL (theseus-0b8).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub index_moved_aside: Option<MovedAside>,
    /// The index's terms were not whole at its checkpoint (a store an older
    /// build wrote last, or a new projection), and are built again after
    /// serving (`WalStore::build_terms`); until then the readers by state
    /// read every record (theseus-lv2).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub terms_pending: bool,
}

/// What the kernel writes through. Every method is durable when it returns.
pub trait Store: Send + Sync {
    /// Append records as one atomic frame; returns their positions in order.
    fn append(&self, batch: &[NewRecord]) -> Result<Vec<u64>>;
    /// Settle a completion and the continuation state it implies, atomically (§3.16).
    fn settle(&self, completion: NewRecord, continuation: NewRecord) -> Result<(u64, u64)> {
        let v = self.append(&[completion, continuation])?;
        Ok((v[0], v[1]))
    }
    fn get(&self, position: u64) -> Result<Option<Record>>;
    /// Records with `from <= position <= to` (inclusive), at most `limit`, in order.
    fn scan(&self, from: u64, to: Option<u64>, limit: usize) -> Result<Vec<Record>>;
    /// The latest record for (kind, key): current state of an entity.
    fn latest_by_key(&self, kind: RecordKind, key: &str) -> Result<Option<Record>>;
    /// Latest record for every key of a kind, in key order (the bytes'):
    /// the kernel transaction merges what it staged into it by key.
    fn latest_of_kind(&self, kind: RecordKind) -> Result<Vec<Record>>;
    /// Newest `n` records of a kind, oldest first.
    fn tail_of_kind(&self, kind: RecordKind, n: usize) -> Result<Vec<Record>>;
    fn count_of_kind(&self, kind: RecordKind) -> Result<u64>;
    /// Records in a scope (a session) with position > `after`, oldest first,
    /// at most `limit`: the per-session tail walk (§4.4b).
    fn scan_scope(&self, scope: &str, after: u64, limit: usize) -> Result<Vec<Record>>;
    fn count_in_scope(&self, scope: &str) -> Result<u64>;
    fn last_position(&self) -> u64;
    /// Make the index durable and record how far it is good.
    fn checkpoint(&self) -> Result<u64>;
    fn stats(&self) -> Result<StoreStats>;

    /// The latest record of each key of `kind` whose terms in the store's
    /// projection include one in `lo..hi`, each key once, in key order
    /// (theseus-lv2). `None` when the store keeps no terms: the caller reads
    /// every record of the kind instead, and asks the projection's own
    /// function.
    fn latest_by_terms(
        &self,
        _kind: RecordKind,
        _lo: &str,
        _hi: &str,
    ) -> Result<Option<Vec<Record>>> {
        Ok(None)
    }
    /// How many (term, key) pairs of `kind` have a term in `lo..hi`; `None`
    /// when the store keeps no terms.
    fn count_by_terms(&self, _kind: RecordKind, _lo: &str, _hi: &str) -> Result<Option<u64>> {
        Ok(None)
    }
    /// The latest record of every key of `kind` that starts with `prefix`,
    /// in key order.
    fn latest_with_prefix(&self, kind: RecordKind, prefix: &str) -> Result<Vec<Record>> {
        Ok(self
            .latest_of_kind(kind)?
            .into_iter()
            .filter(|r| r.key.as_deref().is_some_and(|k| k.starts_with(prefix)))
            .collect())
    }
    /// How many keys `kind` has (its entities), where `count_of_kind` counts
    /// every record.
    fn count_keys(&self, kind: RecordKind) -> Result<u64> {
        Ok(self.latest_of_kind(kind)?.len() as u64)
    }
    /// The numbers of every key of `kind`, added up, as the projection gives
    /// them (theseus-lv2); `None` when the store keeps none whole, and the
    /// caller adds up every record itself.
    fn totals(&self, _kind: RecordKind) -> Result<Option<Sums>> {
        Ok(None)
    }
}

/// What a store's index keeps beside each keyed record of the kinds it names
/// (theseus-lv2): its terms, so a reader asks for the keys with a term (a
/// state) instead of reading every record of the kind, and its numbers, which
/// the index adds up per kind. The index is a projection of the WAL, and so
/// are these: every append and every replay keeps them.
#[derive(Debug)]
pub struct Projection {
    /// The terms are whole at the index's checkpoint only when its mark under
    /// this name says so: a writer that kept no terms (an older build, an
    /// open with no projection) moves the checkpoint alone, and the next open
    /// with this projection builds them again after serving. A change to what
    /// `terms` or `sums` returns gives it a new name.
    pub name: &'static str,
    pub kinds: &'static [RecordKind],
    /// A record's terms, from its kind and payload. A term is not empty and
    /// holds no 0x00.
    pub terms: fn(RecordKind, &[u8]) -> Vec<String>,
    /// A record's numbers, for the kinds whose records are added up; `None`
    /// for the rest.
    pub sums: fn(RecordKind, &[u8]) -> Option<Sums>,
}

impl Projection {
    /// The terms of a keyed record of a kind this projects.
    fn of(&self, kind: RecordKind, key: Option<&str>, payload: &[u8]) -> Option<Vec<String>> {
        (key.is_some() && self.kinds.contains(&kind)).then(|| (self.terms)(kind, payload))
    }

    /// The numbers of a keyed record of a kind this adds up.
    fn sums_of(&self, kind: RecordKind, key: Option<&str>, payload: &[u8]) -> Option<Sums> {
        (key.is_some() && self.kinds.contains(&kind))
            .then(|| (self.sums)(kind, payload))
            .flatten()
    }
}

/// The store: a handle on what its writer thread shares with it, and the
/// writer's queue (theseus-vni9). Every append goes through the queue; every
/// read reads the shared WAL and index.
pub struct WalStore {
    inner: Arc<Inner>,
    /// The writer's queue. Closed when the store drops, which ends the
    /// writer once it has answered what it holds.
    queue: Option<mpsc::Sender<Job>>,
    writer: Option<std::thread::JoinHandle<()>>,
}

/// What the store's handle and its writer share.
struct Inner {
    wal: Wal,
    index: RedbIndex,
    dir: PathBuf,
    replayed: AtomicU64,
    /// Checkpoint every N appended records (0 = manual only).
    checkpoint_every: AtomicU64,
    since_checkpoint: AtomicU64,
    /// The position the index's last checkpoint claims: a checkpoint with
    /// nothing written since costs nothing (theseus-pfv).
    checkpointed: AtomicU64,
    /// The position the last durable checkpoint claims: a stop's checkpoint
    /// is made durable by redb's close, not by itself (theseus-02k).
    durable_to: AtomicU64,
    /// The newest schema written per kind, as the manifest says; an append
    /// of a newer one rewrites the manifest first (F4a).
    marks: RwLock<BTreeMap<RecordKind, u16>>,
    /// Whether the manifest's rewrite is synced (the WAL's `fsync`).
    fsync: bool,
    /// Held shared by the writer from a batch's first WAL write to its index
    /// write, and alone by a checkpoint and a manifest's rewrite: the
    /// position a checkpoint claims is then synced and indexed, which a
    /// tail-only open relies on (theseus-8ni), and no frame is written while
    /// the manifest moves (Review 2's R8).
    appending: RwLock<()>,
    /// How long open waited for another process to release the store.
    lock_wait_us: u64,
    /// The index file open moved aside, if it did (theseus-0b8).
    moved_aside: Option<MovedAside>,
    /// The terms the index keeps with every append (theseus-lv2).
    projection: Option<&'static Projection>,
    /// Whether the index's terms are whole: readers by term may use them,
    /// and a checkpoint marks them. False from an open that found them not
    /// whole until `build_terms` has walked every key.
    terms_whole: std::sync::atomic::AtomicBool,
    /// How far the last history check proved the WAL, as the index kept it
    /// (theseus-0dq), read at open.
    verified: Option<Verified>,
    /// A newer mark, handed over by the check that made it: the next
    /// checkpoint writes it.
    pending_verified: VerifiedSlot,
    /// Appends handed to the writer and not yet answered (a test's way to
    /// see them queued): counted once sent, so it may dip below zero while
    /// the writer answers one before its sender counts it.
    queued: std::sync::atomic::AtomicI64,
    /// A test's way to make a checkpoint slow, as a disk under writeback is.
    #[cfg(test)]
    checkpoint_delay: std::sync::Mutex<std::time::Duration>,
}

/// One append, as the writer takes it: its records, each one's terms and
/// numbers in the store's projection (worked out by the caller, so the
/// writer only writes), and where its answer goes.
struct Job {
    records: Vec<NewRecord>,
    projected: Vec<(Option<Vec<String>>, Option<Sums>)>,
    answer: mpsc::SyncSender<Result<Vec<u64>>>,
}

/// Run `f`, which waits (for the disk, or for a lock held across it),
/// without holding a runtime worker (theseus-vni9). On a worker of a
/// multi-thread tokio runtime, `block_in_place` first hands the worker's
/// role (its queue of tasks, its timers) to another thread, so every worker
/// keeps serving while this thread waits; the thread that waits is the one
/// that called, so what it holds by thread (an execution's or a session's
/// lock) stays its own. Anywhere else (a plain thread, the blocking pool, a
/// current-thread runtime, where `block_in_place` would panic) it simply
/// runs `f`.
pub fn blocking<R>(f: impl FnOnce() -> R) -> R {
    match tokio::runtime::Handle::try_current() {
        Ok(h) if h.runtime_flavor() == tokio::runtime::RuntimeFlavor::MultiThread => {
            tokio::task::block_in_place(f)
        }
        _ => f(),
    }
}

/// Where a history check hands over the mark it made (theseus-0dq), apart
/// from the store: the check's thread holds this, never the store, so a
/// stopping daemon's store still closes. The store's next checkpoint writes
/// the mark into the index, in its own commit.
#[derive(Clone, Default)]
pub struct VerifiedSlot(Arc<std::sync::Mutex<Option<Verified>>>);

impl VerifiedSlot {
    pub fn set(&self, v: Verified) {
        *self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(v);
    }
    fn take(&self) -> Option<Verified> {
        self.0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take()
    }
}

/// The index's keys for a history check's mark (theseus-0dq).
const VERIFIED_KEYS: [&str; 5] = [
    "verified.segment",
    "verified.offset",
    "verified.first",
    "verified.position",
    "verified.full_at",
];

fn verified_meta(v: &Verified) -> [(&'static str, u64); 5] {
    [
        (VERIFIED_KEYS[0], u64::from(v.segment)),
        (VERIFIED_KEYS[1], v.offset),
        (VERIFIED_KEYS[2], v.first),
        (VERIFIED_KEYS[3], v.position),
        (VERIFIED_KEYS[4], v.full_at_unix_ms),
    ]
}

/// How long an open waits for another process to release the store before
/// it fails (theseus-qa0 F4b). `theseus shutdown` returns on the daemon's
/// answer. The daemon then removes its socket, flushes telemetry (bounded at
/// 1 s), and closes the store as its runtime ends, so a start that follows at
/// once finds the store held for the rest of that stop: about 15 ms here, up
/// to a second more with a telemetry batch to send. The bound covers a stop
/// whose flush takes its whole second, on a slow disk. A second daemon beside
/// one that is not stopping fails once it has passed.
pub const LOCK_WAIT: std::time::Duration = std::time::Duration::from_secs(3);

/// How often a held store is tried again.
const LOCK_POLL: std::time::Duration = std::time::Duration::from_micros(500);

/// A replay of at least this many records builds the index in key order
/// (`RedbIndex::apply_bulk`, theseus-byu).
const BULK: usize = 4096;

/// Bumped when the WAL record layout or the manifest changes. 2 = scope
/// field (M2). 3 = the newest schema written for each kind (F4a).
const MANIFEST_FORMAT: u32 = 3;
/// The oldest format this build reads. Format 2 lists no kinds: every
/// record in it is schema 1.
const MANIFEST_OLDEST: u32 = 2;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Manifest {
    format: u32,
    engine: Engine,
    /// From format 3: every kind written, with the newest schema written.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    kinds: Vec<KindMark>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct KindMark {
    kind: RecordKind,
    /// For a reader that does not know the kind, and for people.
    name: String,
    schema: u16,
}

/// What an operator is told when this build is older than the store.
const INSTALL_NEWER: &str = "install the newer theseusd: an older build never writes over a store \
     a newer one wrote, and a rollback is a restore of a copy taken before the upgrade";

impl Manifest {
    fn marks(&self) -> BTreeMap<RecordKind, u16> {
        if self.format == MANIFEST_OLDEST {
            // Everything a format-2 store holds is schema 1.
            return kinds::SCHEMAS.iter().map(|(k, _)| (*k, 1)).collect();
        }
        self.kinds.iter().map(|m| (m.kind, m.schema)).collect()
    }

    fn of(marks: &BTreeMap<RecordKind, u16>) -> Self {
        Self {
            format: MANIFEST_FORMAT,
            engine: Engine::Redb,
            kinds: marks
                .iter()
                .map(|(k, s)| KindMark {
                    kind: *k,
                    name: kinds::name(*k).to_string(),
                    schema: *s,
                })
                .collect(),
        }
    }

    /// Refuse a store this build is too old for: a format past its own, or a
    /// kind written at a schema newer than it reads (P5b, theseus-qa0).
    fn check(&self, dir: &Path) -> Result<()> {
        if self.format > MANIFEST_FORMAT {
            anyhow::bail!(
                "store at {} is format {}, and this build reads formats {MANIFEST_OLDEST} to \
                 {MANIFEST_FORMAT}: {INSTALL_NEWER}",
                dir.display(),
                self.format,
            );
        }
        if self.format < MANIFEST_OLDEST {
            anyhow::bail!(
                "store at {} is format {}, from before M2, and this build reads formats \
                 {MANIFEST_OLDEST} to {MANIFEST_FORMAT}; refusing to open",
                dir.display(),
                self.format,
            );
        }
        for m in &self.kinds {
            let known = kinds::schema(m.kind);
            if m.schema > known {
                let reads = if known == 0 {
                    "a kind this build does not know".to_string()
                } else {
                    format!(
                        "and this build reads {} records up to schema {known}",
                        m.name
                    )
                };
                anyhow::bail!(
                    "store at {} holds {} records (kind {}) at schema {}, {reads}: {INSTALL_NEWER}",
                    dir.display(),
                    m.name,
                    m.kind,
                    m.schema,
                );
            }
        }
        Ok(())
    }
}

/// Whether this build may open the store in `dir`, judged by its manifest
/// alone: nothing else is read, and nothing is written (theseus-7hh). The
/// installer asks before it copies a stopped daemon's store for the binary it
/// installs, so an older build is never handed a newer store (F4a). A
/// directory with no manifest is a store this build would create. The WAL's
/// own records are checked when the store opens.
pub fn check_manifest(dir: &Path) -> Result<()> {
    let path = dir.join("MANIFEST.json");
    let bytes = match std::fs::read(&path) {
        Ok(b) => b,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(e) => {
            return Err(
                anyhow::Error::new(e).context(format!("reading store manifest {}", path.display()))
            )
        }
    };
    let m: Manifest = serde_json::from_slice(&bytes)
        .with_context(|| format!("reading store manifest {}", path.display()))?;
    m.check(dir)
}

/// Replace the manifest: a temporary file, synced, renamed over it, and
/// the directory synced, so a reader finds the old one or the new one.
fn write_manifest(dir: &Path, m: &Manifest, fsync: bool) -> Result<()> {
    use std::io::Write as _;
    let tmp = dir.join("MANIFEST.json.tmp");
    let mut f = std::fs::File::create(&tmp)?;
    f.write_all(&serde_json::to_vec_pretty(m)?)?;
    if fsync {
        f.sync_all()?;
    }
    std::fs::rename(&tmp, dir.join("MANIFEST.json"))?;
    if fsync {
        std::fs::File::open(dir)?.sync_all()?;
    }
    Ok(())
}

/// Open the index at `path`. A file there that is not a redb database (a
/// kill inside the store's first open, while redb wrote its header) is moved
/// aside, never deleted, and a new index takes its place, which the open
/// then builds from the WAL, as `restore` does (theseus-0b8). A file another
/// process holds is that process's, and its error goes back as it came, so
/// the open waits and then refuses as it always has. A redb database that
/// fails some other way is left where it is, and refused.
fn open_index(path: &Path) -> Result<(RedbIndex, Option<MovedAside>)> {
    let (e, why) = match RedbIndex::open(path) {
        Ok(index) => return Ok((index, None)),
        Err(e) => match RedbIndex::why_not_a_database(&e) {
            Some(why) => (e, why),
            None => return Err(e.context("opening index")),
        },
    };
    match crate::index::move_aside(path, &why)? {
        Aside::Moved(m) => {
            tracing::warn!(
                moved_to = %m.path,
                bytes = m.bytes,
                why = %m.why,
                "store: index.redb was not a redb database (a kill inside the store's first open \
                 leaves one); it is moved aside, and the index is built again from the WAL"
            );
            let index = RedbIndex::open(path).context("opening index")?;
            Ok((index, Some(m)))
        }
        Aside::Held => Err(anyhow::Error::new(redb::DatabaseError::DatabaseAlreadyOpen)
            .context(format!("opening {}", path.display()))
            .context("opening index")),
        Aside::Database => Err(e.context("opening index")),
    }
}

impl WalStore {
    /// Open or create a store in `dir`. A manifest naming another engine, a
    /// format this build does not read, or a kind newer than it knows is
    /// refused before anything is written, never converted (F4a).
    ///
    /// The WAL is checked from the frame after the index's checkpoint to its
    /// end, for the next position and a torn frame (theseus-8ni): store open
    /// grows with the tail since the checkpoint, never with history.
    /// `verify_history` checks the rest, after serving.
    ///
    /// A store another process still has open is waited for, at most
    /// `LOCK_WAIT` (theseus-qa0 F4b): the lock is redb's, taken when the
    /// index opens, and each try reads the manifest again, so the one that
    /// gets the lock checks the manifest the last holder left.
    pub fn open(dir: &Path, wal_cfg: WalConfig) -> Result<Self> {
        Self::open_waiting(dir, wal_cfg, LOCK_WAIT)
    }

    /// `open`, keeping `projection`'s terms with every record (theseus-lv2):
    /// the open builds them from the WAL when the index's are not whole.
    pub fn open_projected(
        dir: &Path,
        wal_cfg: WalConfig,
        projection: &'static Projection,
    ) -> Result<Self> {
        Self::open_with(dir, wal_cfg, LOCK_WAIT, Some(projection))
    }

    /// `open`, waiting at most `wait` for another process to release the
    /// store.
    pub fn open_waiting(dir: &Path, wal_cfg: WalConfig, wait: std::time::Duration) -> Result<Self> {
        Self::open_with(dir, wal_cfg, wait, None)
    }

    fn open_with(
        dir: &Path,
        wal_cfg: WalConfig,
        wait: std::time::Duration,
        projection: Option<&'static Projection>,
    ) -> Result<Self> {
        let t0 = std::time::Instant::now();
        // When the try under way began: zero for the first.
        let mut began = std::time::Duration::ZERO;
        loop {
            match Inner::open_once(dir, wal_cfg.clone(), projection) {
                Ok(mut inner) => {
                    inner.lock_wait_us = began.as_micros() as u64;
                    if !began.is_zero() {
                        tracing::info!(
                            waited_ms = began.as_millis() as u64,
                            "store: waited for the previous process to release it"
                        );
                    }
                    return Self::start(inner);
                }
                Err(e) if RedbIndex::held_elsewhere(&e) && t0.elapsed() < wait => {
                    std::thread::sleep(LOCK_POLL);
                    began = t0.elapsed();
                }
                Err(e) if RedbIndex::held_elsewhere(&e) => {
                    return Err(e.context(format!(
                        "another process has held the store at {} for {} ms and still does: \
                         is another theseusd serving it?",
                        dir.display(),
                        t0.elapsed().as_millis()
                    )));
                }
                Err(e) => return Err(e),
            }
        }
    }

    /// The opened store, with its writer.
    fn start(inner: Inner) -> Result<Self> {
        let inner = Arc::new(inner);
        let (queue, jobs) = mpsc::channel();
        let shared = inner.clone();
        let writer = std::thread::Builder::new()
            .name("store-writer".into())
            .spawn(move || shared.write_loop(&jobs))
            .context("starting the store's writer thread")?;
        Ok(Self {
            inner,
            queue: Some(queue),
            writer: Some(writer),
        })
    }

    pub fn with_checkpoint_every(self, n: u64) -> Self {
        self.inner.checkpoint_every.store(n, Ordering::Relaxed);
        self
    }

    /// Whether the index's terms are whole: readers by term use them.
    pub fn terms_whole(&self) -> bool {
        self.inner.terms_whole()
    }

    /// Build the projection's terms again, `n` keys from `cursor`, after
    /// serving (theseus-lv2): an open that found them not whole at the
    /// index's checkpoint (a store an older build, or an open with no
    /// projection, wrote last) leaves them to this, and readers by term read
    /// every record until they are whole. Each key's terms are put only
    /// while its latest record is the one read (`put_terms_if_latest`): an
    /// append since then put its own. Returns the cursor to go on from, or
    /// `None` once every key of every kind is done; the terms are whole from
    /// then on, and the next checkpoint marks them.
    pub fn build_terms(
        &self,
        cursor: Option<(RecordKind, String)>,
        n: usize,
    ) -> Result<Option<(RecordKind, String)>> {
        let s = &self.inner;
        let Some(p) = s.projection else {
            return Ok(None);
        };
        if s.terms_whole() {
            return Ok(None);
        }
        let start = cursor
            .as_ref()
            .and_then(|(k, _)| p.kinds.iter().position(|x| x == k))
            .unwrap_or(0);
        for (i, &kind) in p.kinds.iter().enumerate().skip(start) {
            let after = cursor
                .as_ref()
                .filter(|(k, _)| i == start && *k == kind)
                .map(|(_, key)| key.as_str());
            let keys = s.index.keys_of_kind_after(kind, after, n.max(1))?;
            let Some((last, _)) = keys.last().cloned() else {
                continue;
            };
            let positions: Vec<u64> = keys.iter().map(|(_, pos)| *pos).collect();
            let rows: Vec<Projected> = s
                .read_many(&positions)?
                .into_iter()
                .filter_map(|r| {
                    let key = r.key.clone()?;
                    let terms = (p.terms)(kind, &r.payload);
                    let sums = (p.sums)(kind, &r.payload);
                    Some((kind, key, r.position, terms, sums))
                })
                .collect();
            s.index.put_terms_if_latest(&rows)?;
            return Ok(Some((kind, last)));
        }
        s.terms_whole.store(true, Ordering::Release);
        Ok(None)
    }

    pub fn recovery(&self) -> &Recovery {
        self.inner.wal.recovery()
    }

    pub fn dir(&self) -> &Path {
        &self.inner.dir
    }

    /// The newest schema written for each kind, as the manifest records it.
    pub fn schema_marks(&self) -> BTreeMap<RecordKind, u16> {
        self.inner.marks.read().unwrap().clone()
    }

    /// The full check the open leaves out (theseus-8ni): every frame before
    /// where open began checking, read-only. `pace` is called after each
    /// stretch with the time it took, for a tender that keeps to its share
    /// of a core. A corrupt frame is returned, and reads from it are refused
    /// from then on.
    pub fn verify_history(
        &self,
        pace: impl FnMut(std::time::Duration),
    ) -> std::result::Result<History, crate::wal::WalError> {
        self.inner.wal.verify_history(pace)
    }

    /// The same check, apart from the store: a thread that runs it keeps
    /// neither the store nor its index open, so a stopping daemon's store
    /// still closes cleanly.
    pub fn history_check(&self) -> crate::wal::HistoryCheck {
        self.inner.wal.history_check()
    }

    /// How far the last history check proved the WAL, as the index kept it
    /// at this open (theseus-0dq): a check may start there
    /// (`HistoryCheck::from_mark`).
    pub fn verified(&self) -> Option<Verified> {
        self.inner.verified
    }

    /// Where a check hands over its mark; the next checkpoint writes it.
    pub fn verified_slot(&self) -> VerifiedSlot {
        self.inner.pending_verified.clone()
    }

    /// The checkpoint of a stop (theseus-02k): the index's checkpoint, its
    /// terms' mark, and a history check's mark, in one commit with no sync of
    /// its own. redb's close, which follows as the store drops, is a durable
    /// commit, and makes this one durable with it: a stop then pays one
    /// commit's syncs, not two. A kill between the two only makes the next
    /// open replay from the checkpoint before.
    pub fn checkpoint_for_close(&self) -> Result<u64> {
        blocking(|| self.inner.checkpoint_as(false))
    }

    /// Appends handed to the writer and not yet answered.
    #[cfg(test)]
    fn queued(&self) -> i64 {
        self.inner.queued.load(Ordering::SeqCst)
    }
}

impl Drop for WalStore {
    /// Closing the queue ends the writer once it has answered what it holds.
    /// Nothing else is queued: an appender holds the store until its answer.
    /// The index closes after, as the last handle on it drops.
    fn drop(&mut self) {
        drop(self.queue.take());
        if let Some(w) = self.writer.take() {
            let _ = w.join();
        }
    }
}

impl Inner {
    #[expect(clippy::too_many_lines, reason = "shape budget: split it")]
    fn open_once(
        dir: &Path,
        wal_cfg: WalConfig,
        projection: Option<&'static Projection>,
    ) -> Result<Self> {
        std::fs::create_dir_all(dir)?;
        let fsync = wal_cfg.fsync;
        let manifest_path = dir.join("MANIFEST.json");
        let marks = if manifest_path.exists() {
            let m: Manifest = serde_json::from_slice(&std::fs::read(&manifest_path)?)
                .with_context(|| format!("reading store manifest {}", manifest_path.display()))?;
            m.check(dir)?;
            m.marks()
        } else {
            // A new store: every kind this build writes, at its schema.
            let marks: BTreeMap<RecordKind, u16> = kinds::SCHEMAS.iter().copied().collect();
            write_manifest(dir, &Manifest::of(&marks), fsync)?;
            marks
        };

        let (index, moved_aside) = open_index(&dir.join("index.redb"))?;
        // The WAL from the frame after the checkpoint's record: the index
        // knows where that record lies.
        let cp = index.checkpoint()?.unwrap_or(0);
        let at = if cp > 0 { index.location(cp)? } else { None };
        let (wal, missing) =
            Wal::open_from(&dir.join("wal"), wal_cfg, cp, at).context("opening WAL")?;
        // A record newer than this build (a manifest that lags its WAL: a
        // WAL copied in by hand) is refused too.
        if let Some(r) = missing
            .iter()
            .map(|(r, _)| r)
            .find(|r| r.schema > kinds::schema(r.kind))
        {
            anyhow::bail!(
                "store at {} holds a {} record (kind {}, position {}) at schema {}, and this build \
                 reads up to schema {}: {INSTALL_NEWER}",
                dir.display(),
                kinds::name(r.kind),
                r.kind,
                r.position,
                r.schema,
                kinds::schema(r.kind),
            );
        }

        // Whether the index's terms were whole at its checkpoint: then the
        // replay below keeps them so (theseus-lv2). With no checkpoint, the
        // replay is every record, and builds them whole (a new store, a
        // restore).
        let terms_whole = match projection {
            Some(p) => cp == 0 || index.terms_mark(p.name)? == Some(cp),
            None => true,
        };
        // Rebuild whatever the index lost since its checkpoint.
        let replayed = missing.len() as u64;
        if !missing.is_empty() {
            let entries: Vec<IndexEntry> = missing
                .iter()
                .map(|(r, loc)| IndexEntry {
                    position: r.position,
                    kind: r.kind,
                    key: r.key.clone(),
                    scope: r.scope.clone(),
                    loc: *loc,
                    terms: projection.and_then(|p| p.of(r.kind, r.key.as_deref(), &r.payload)),
                    sums: projection.and_then(|p| p.sums_of(r.kind, r.key.as_deref(), &r.payload)),
                })
                .collect();
            // A long replay (a restore, a store with no checkpoint) is built
            // in key order, a table at a time (theseus-byu).
            if entries.len() >= BULK {
                index.apply_bulk(&entries)?;
            } else {
                index.apply(&entries, false)?;
            }
        }
        let last = wal.last_position();
        if cp > last {
            // Index claims more than the WAL has (WAL truncated below a durable
            // checkpoint). Only possible with a torn tail after a checkpoint that
            // included it, which our ordering forbids; treat as corruption.
            anyhow::bail!(
                "index checkpoint {cp} is past the WAL's last position {last}; refusing to open"
            );
        }
        let mut store = Self {
            wal,
            index,
            dir: dir.to_path_buf(),
            replayed: AtomicU64::new(replayed),
            checkpoint_every: AtomicU64::new(1000),
            since_checkpoint: AtomicU64::new(0),
            checkpointed: AtomicU64::new(cp),
            durable_to: AtomicU64::new(cp),
            marks: RwLock::new(marks),
            fsync,
            appending: RwLock::new(()),
            lock_wait_us: 0,
            moved_aside,
            projection,
            terms_whole: std::sync::atomic::AtomicBool::new(terms_whole),
            verified: None,
            pending_verified: VerifiedSlot::default(),
            queued: std::sync::atomic::AtomicI64::new(0),
            #[cfg(test)]
            checkpoint_delay: std::sync::Mutex::default(),
        };
        store.verified = match store.index.meta(&VERIFIED_KEYS)?[..] {
            [Some(segment), Some(offset), Some(first), Some(position), Some(full_at)] => {
                Some(Verified {
                    segment: u32::try_from(segment).unwrap_or(u32::MAX),
                    offset,
                    first,
                    position,
                    full_at_unix_ms: full_at,
                })
            }
            _ => None,
        };
        if !terms_whole {
            // Built after serving (`build_terms`): a start writes nothing,
            // and reads nothing that grows with history, for them.
            tracing::info!(
                checkpoint = cp,
                "store: the index's terms are not whole at its checkpoint (an older build wrote \
                 last); they are built again after serving"
            );
        }
        // A manifest that lags its WAL's tail catches up now.
        let tail: Vec<(RecordKind, u16)> =
            missing.iter().map(|(r, _)| (r.kind, r.schema)).collect();
        store.mark(&tail)?;
        if replayed > 0 {
            tracing::info!(
                replayed,
                checkpoint = cp,
                last,
                "store: index rebuilt from WAL"
            );
            store.checkpoint_as(true)?;
        }
        Ok(store)
    }

    fn terms_whole(&self) -> bool {
        self.terms_whole.load(Ordering::Acquire)
    }

    /// Before records of these (kind, schema) go to the WAL: if any is newer
    /// than the manifest's mark, rewrite the manifest first, durably, with
    /// every kind this build writes at its schema (one rewrite per upgrade),
    /// so an older build refuses the store before it can read the record.
    /// It takes `appending` alone (Review 2's R8), so no frame is written
    /// while the manifest moves; never call it holding that lock.
    fn mark(&self, recs: &[(RecordKind, u16)]) -> Result<()> {
        let newer = |m: &BTreeMap<RecordKind, u16>| {
            recs.iter()
                .any(|(k, s)| m.get(k).copied().unwrap_or(0) < *s)
        };
        if !newer(&self.marks.read().unwrap()) {
            return Ok(());
        }
        let _alone = self.appending.write().unwrap();
        let mut marks = self.marks.write().unwrap();
        if !newer(&marks) {
            return Ok(());
        }
        let mut next = marks.clone();
        for (k, s) in kinds::SCHEMAS.iter().copied().chain(recs.iter().copied()) {
            let m = next.entry(k).or_insert(0);
            *m = (*m).max(s);
        }
        write_manifest(&self.dir, &Manifest::of(&next), self.fsync)
            .context("marking the store's manifest with a newer record schema")?;
        tracing::info!(
            kinds = ?next,
            "store: manifest marked with this build's record schemas"
        );
        *marks = next;
        Ok(())
    }

    fn checkpoint_as(&self, durable: bool) -> Result<u64> {
        // No batch between its WAL write and its index write: every frame up
        // to `last` is synced and indexed.
        let _alone = self.appending.write().unwrap();
        #[cfg(test)]
        std::thread::sleep(*self.checkpoint_delay.lock().unwrap());
        let last = self.wal.last_position();
        let verified = self.pending_verified.take();
        // Nothing written since the last checkpoint: the index has `last`
        // already, and only an append writes it between checkpoints. So a
        // clean stop's last checkpoint, after its own, is free when nothing
        // came between them (theseus-pfv). A durable one is free only when
        // the last durable one claimed `last` too.
        let done = if durable {
            &self.durable_to
        } else {
            &self.checkpointed
        };
        if done.load(Ordering::Relaxed) == last && verified.is_none() {
            return Ok(last);
        }
        let meta = verified.as_ref().map(verified_meta);
        // The terms are marked whole only while they are (theseus-lv2).
        self.index.set_checkpoint_with(
            last,
            self.projection
                .filter(|_| self.terms_whole())
                .map(|p| p.name),
            meta.as_ref().map_or(&[][..], |m| &m[..]),
            durable,
        )?;
        self.checkpointed.store(last, Ordering::Relaxed);
        if durable {
            self.durable_to.store(last, Ordering::Relaxed);
        }
        self.since_checkpoint.store(0, Ordering::Relaxed);
        Ok(last)
    }

    /// The writer (theseus-vni9): it takes every append queued, writes their
    /// frames back to back, syncs once for all of them, indexes them in one
    /// transaction, and answers each. So one fdatasync commits whatever
    /// queued while the last one ran: group commit across turns, by
    /// construction. The answer comes once the frame is indexed, so a
    /// caller's lock still spans its read to its frame indexed (K1).
    fn write_loop(&self, jobs: &mpsc::Receiver<Job>) {
        while let Ok(first) = jobs.recv() {
            // A checkpoint, which takes this alone, never claims a frame
            // written and not yet synced and indexed. What queues while the
            // writer waits for it joins this batch.
            let _appending = self.appending.read().unwrap();
            let mut batch = vec![first];
            batch.extend(jobs.try_iter());
            self.commit(batch);
        }
    }

    /// Write, sync, index, and answer one batch.
    fn commit(&self, batch: Vec<Job>) {
        // Each frame, unsynced: a frame the log refuses fails alone.
        let written: Vec<Result<Vec<(u64, RecordLocation)>>> = batch
            .iter()
            .map(|j| self.wal.write(&j.records).map_err(anyhow::Error::from))
            .collect();
        let wrote = written
            .iter()
            .any(|w| w.as_ref().is_ok_and(|p| !p.is_empty()));
        // One sync for every frame of the batch.
        let synced = if wrote && self.fsync {
            self.wal.sync().map_err(anyhow::Error::from)
        } else {
            Ok(())
        };
        // Then the index, in one transaction: a reader never sees a frame
        // the disk may yet lose.
        let mut records = 0u64;
        let indexed = synced.and_then(|()| {
            let mut entries = Vec::new();
            for (job, placed) in batch.iter().zip(&written) {
                let Ok(placed) = placed else { continue };
                records += placed.len() as u64;
                for (((pos, loc), r), (terms, sums)) in
                    placed.iter().zip(&job.records).zip(&job.projected)
                {
                    entries.push(IndexEntry {
                        position: *pos,
                        kind: r.kind,
                        key: r.key.clone(),
                        scope: r.scope.clone(),
                        loc: *loc,
                        terms: terms.clone(),
                        sums: *sums,
                    });
                }
            }
            if entries.is_empty() {
                Ok(())
            } else {
                self.index.apply(&entries, false)
            }
        });
        if indexed.is_ok() {
            self.since_checkpoint.fetch_add(records, Ordering::Relaxed);
        }
        for (job, placed) in batch.into_iter().zip(written) {
            let answer = match (placed, &indexed) {
                (Err(e), _) => Err(e),
                (Ok(placed), Ok(())) => Ok(placed.into_iter().map(|(p, _)| p).collect()),
                // A sync or an index write that failed fails every frame it
                // covered, as a failed group sync failed each one it led.
                (Ok(_), Err(e)) => Err(anyhow::anyhow!("{e:#}")),
            };
            self.queued.fetch_sub(1, Ordering::SeqCst);
            let _ = job.answer.send(answer);
        }
    }

    fn read(&self, position: u64) -> Result<Option<Record>> {
        match self.index.location(position)? {
            Some(loc) => Ok(Some(checked(position, self.wal.read_at(loc)?)?)),
            None => Ok(None),
        }
    }

    /// Records by position, in order: one index transaction for all of
    /// them, then one `pread` each (theseus-qa0). A position the index does
    /// not know is skipped, as `read` would skip it.
    fn read_many(&self, positions: &[u64]) -> Result<Vec<Record>> {
        let mut out = Vec::with_capacity(positions.len());
        for (p, loc) in positions.iter().zip(self.index.locations(positions)?) {
            if let Some(loc) = loc {
                out.push(checked(*p, self.wal.read_at(loc)?)?);
            }
        }
        Ok(out)
    }
}

/// A record read where the index put `position` must be that record: open
/// no longer reads the WAL's history, so a read is where a stale index shows.
fn checked(position: u64, r: Record) -> Result<Record> {
    if r.position != position {
        anyhow::bail!(
            "the index puts position {position} where the WAL holds position {}: the index \
             does not match the WAL",
            r.position
        );
    }
    Ok(r)
}

impl Store for WalStore {
    /// Hand the frame to the writer and wait for its answer: durable and
    /// indexed when it returns. The wait holds no runtime worker
    /// (`blocking`), and the caller's thread keeps what it holds by thread.
    fn append(&self, batch: &[NewRecord]) -> Result<Vec<u64>> {
        if batch.is_empty() {
            return Ok(Vec::new());
        }
        blocking(|| {
            let s = &self.inner;
            let newer = {
                let m = s.marks.read().unwrap();
                batch
                    .iter()
                    .any(|r| m.get(&r.kind).copied().unwrap_or(0) < r.schema)
            };
            if newer {
                let schemas: Vec<(RecordKind, u16)> =
                    batch.iter().map(|r| (r.kind, r.schema)).collect();
                s.mark(&schemas)?;
            }
            let projected = batch
                .iter()
                .map(|r| {
                    let key = r.key.as_deref();
                    (
                        s.projection.and_then(|p| p.of(r.kind, key, &r.payload)),
                        s.projection
                            .and_then(|p| p.sums_of(r.kind, key, &r.payload)),
                    )
                })
                .collect();
            let (answer, answered) = mpsc::sync_channel(1);
            let queue = self
                .queue
                .as_ref()
                .expect("a store's queue lives as long as it");
            let job = Job {
                records: batch.to_vec(),
                projected,
                answer,
            };
            if queue.send(job).is_err() {
                anyhow::bail!("the store's writer has stopped");
            }
            s.queued.fetch_add(1, Ordering::SeqCst);
            let placed = answered
                .recv()
                .map_err(|_| anyhow::anyhow!("the store's writer stopped before it answered"))??;
            let every = s.checkpoint_every.load(Ordering::Relaxed);
            if every > 0 && s.since_checkpoint.load(Ordering::Relaxed) >= every {
                s.checkpoint_as(true)?;
            }
            Ok(placed)
        })
    }

    fn get(&self, position: u64) -> Result<Option<Record>> {
        self.inner.read(position)
    }

    fn scan(&self, from: u64, to: Option<u64>, limit: usize) -> Result<Vec<Record>> {
        let last = self.inner.wal.last_position();
        let to = to.unwrap_or(last).min(last);
        let mut out = Vec::new();
        let mut p = from.max(1);
        while p <= to && out.len() < limit {
            if let Some(r) = self.inner.read(p)? {
                out.push(r);
            }
            p += 1;
        }
        Ok(out)
    }

    fn latest_by_key(&self, kind: RecordKind, key: &str) -> Result<Option<Record>> {
        match self.inner.index.latest_position(kind, key)? {
            Some(p) => self.inner.read(p),
            None => Ok(None),
        }
    }

    fn latest_of_kind(&self, kind: RecordKind) -> Result<Vec<Record>> {
        let positions: Vec<u64> = self
            .inner
            .index
            .keys_of_kind(kind)?
            .into_iter()
            .map(|(_, p)| p)
            .collect();
        self.inner.read_many(&positions)
    }

    fn tail_of_kind(&self, kind: RecordKind, n: usize) -> Result<Vec<Record>> {
        let mut positions = self.inner.index.positions_of_kind_rev(kind, n)?;
        positions.reverse();
        self.inner.read_many(&positions)
    }

    fn count_of_kind(&self, kind: RecordKind) -> Result<u64> {
        self.inner.index.count_of_kind(kind)
    }

    fn scan_scope(&self, scope: &str, after: u64, limit: usize) -> Result<Vec<Record>> {
        let positions = self.inner.index.positions_in_scope(scope, after, limit)?;
        self.inner.read_many(&positions)
    }

    fn count_in_scope(&self, scope: &str) -> Result<u64> {
        self.inner.index.count_in_scope(scope)
    }

    fn last_position(&self) -> u64 {
        self.inner.wal.last_position()
    }

    fn checkpoint(&self) -> Result<u64> {
        blocking(|| self.inner.checkpoint_as(true))
    }

    fn stats(&self) -> Result<StoreStats> {
        let s = &self.inner;
        let r = s.wal.recovery();
        Ok(StoreStats {
            last_position: s.wal.last_position(),
            checkpoint: s.index.checkpoint()?,
            wal_bytes: s.wal.total_bytes(),
            wal_segments: s.wal.segment_count(),
            recovered_records: r.records,
            truncated_bytes: r.truncated_bytes,
            replayed_into_index: s.replayed.load(Ordering::Relaxed),
            frames_appended: s.wal.frames_appended(),
            syncs: s.wal.syncs(),
            history_bytes: r.history_bytes,
            index_repaired: s.index.repaired(),
            lock_wait_us: s.lock_wait_us,
            index_moved_aside: s.moved_aside.clone(),
            terms_pending: !s.terms_whole(),
        })
    }

    fn latest_by_terms(&self, kind: RecordKind, lo: &str, hi: &str) -> Result<Option<Vec<Record>>> {
        let s = &self.inner;
        if !s.projection.is_some_and(|p| p.kinds.contains(&kind)) || !s.terms_whole() {
            return Ok(None);
        }
        let positions: Vec<u64> = s
            .index
            .keys_by_terms(kind, lo, hi)?
            .into_iter()
            .map(|(_, p)| p)
            .collect();
        Ok(Some(s.read_many(&positions)?))
    }

    fn count_by_terms(&self, kind: RecordKind, lo: &str, hi: &str) -> Result<Option<u64>> {
        let s = &self.inner;
        if !s.projection.is_some_and(|p| p.kinds.contains(&kind)) || !s.terms_whole() {
            return Ok(None);
        }
        Ok(Some(s.index.count_by_terms(kind, lo, hi)?))
    }

    fn latest_with_prefix(&self, kind: RecordKind, prefix: &str) -> Result<Vec<Record>> {
        let positions: Vec<u64> = self
            .inner
            .index
            .keys_with_prefix(kind, prefix)?
            .into_iter()
            .map(|(_, p)| p)
            .collect();
        self.inner.read_many(&positions)
    }

    fn count_keys(&self, kind: RecordKind) -> Result<u64> {
        self.inner.index.count_keys(kind)
    }

    fn totals(&self, kind: RecordKind) -> Result<Option<Sums>> {
        let s = &self.inner;
        if !s.projection.is_some_and(|p| p.kinds.contains(&kind)) || !s.terms_whole() {
            return Ok(None);
        }
        Ok(Some(s.index.totals(kind)?))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::record::kinds;

    fn open(dir: &Path) -> WalStore {
        WalStore::open(dir, WalConfig::default())
            .unwrap()
            .with_checkpoint_every(0)
    }

    /// Wait until `n` appends are queued for the writer.
    fn until_queued(s: &WalStore, n: i64) {
        let t0 = std::time::Instant::now();
        while s.queued() < n {
            assert!(
                t0.elapsed() < std::time::Duration::from_secs(20),
                "{} of {n} appends queued",
                s.queued()
            );
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
    }

    /// Group commit by construction (theseus-vni9): the appends that queue
    /// while the writer is busy are written back to back and made durable by
    /// one fdatasync, each answered once its frame is synced and indexed. The
    /// writer is held here as a checkpoint holds it, by `appending`.
    #[test]
    fn the_writer_commits_every_queued_frame_with_one_sync() {
        let dir = tempfile::tempdir().unwrap();
        let s = Arc::new(open(dir.path()));
        s.append(&[NewRecord::json(kinds::LEDGER, None, &"first").unwrap()])
            .unwrap();
        let before = s.stats().unwrap();
        let held = s.inner.appending.write().unwrap();
        let appenders: Vec<_> = (0..12)
            .map(|i| {
                let s = s.clone();
                std::thread::spawn(move || {
                    let key = format!("k{i}");
                    s.append(&[
                        NewRecord::json(kinds::META, Some(&key), &i).unwrap(),
                        NewRecord::json(kinds::LEDGER, None, &i).unwrap(),
                    ])
                    .unwrap()
                })
            })
            .collect();
        until_queued(&s, 12);
        drop(held);
        let mut positions: Vec<u64> = appenders
            .into_iter()
            .flat_map(|a| a.join().unwrap())
            .collect();
        let after = s.stats().unwrap();
        assert_eq!(after.frames_appended - before.frames_appended, 12);
        assert_eq!(
            after.syncs - before.syncs,
            1,
            "twelve frames queued together are made durable by one fdatasync"
        );
        positions.sort_unstable();
        assert_eq!(positions, (2..=25).collect::<Vec<u64>>());
        for i in 0..12 {
            let r = s.latest_by_key(kinds::META, &format!("k{i}")).unwrap();
            assert_eq!(
                r.unwrap().decode::<i32>().unwrap(),
                i,
                "indexed when answered"
            );
        }
        drop(s);
        let s = open(dir.path());
        assert_eq!(s.last_position(), 25, "every one durable");
    }

    /// The wait for the writer holds no runtime worker (theseus-vni9): with
    /// the writer held, as a long fdatasync holds it, appends from every
    /// worker of a two-worker runtime still leave a third task served.
    #[test]
    fn an_append_that_waits_for_the_disk_holds_no_runtime_worker() {
        let dir = tempfile::tempdir().unwrap();
        let s = Arc::new(open(dir.path()));
        let rt = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
            .unwrap();
        let held = s.inner.appending.write().unwrap();
        let appends: Vec<_> = (0..4)
            .map(|i| {
                let s = s.clone();
                rt.spawn(async move {
                    s.append(&[NewRecord::json(kinds::LEDGER, None, &i).unwrap()])
                        .unwrap()
                })
            })
            .collect();
        until_queued(&s, 4);
        let served = rt.block_on(async {
            tokio::time::timeout(
                std::time::Duration::from_secs(5),
                tokio::spawn(async { "served" }),
            )
            .await
        });
        drop(held);
        assert!(
            matches!(served, Ok(Ok("served"))),
            "a task waited for a worker while the appends waited for the disk"
        );
        for a in appends {
            rt.block_on(a).unwrap();
        }
    }

    /// The manifest moves under `appending` alone (Review 2's R8): a newer
    /// record's mark waits while a batch is being written, and no frame is
    /// written while it moves.
    #[test]
    fn a_manifests_mark_waits_for_the_batch_being_written() {
        let dir = tempfile::tempdir().unwrap();
        let s = open(dir.path());
        s.append(&[NewRecord::json(kinds::LEDGER, None, &"a").unwrap()])
            .unwrap();
        drop(s);
        std::fs::write(
            dir.path().join("MANIFEST.json"),
            r#"{"format": 2, "engine": "redb"}"#,
        )
        .unwrap();
        let s = Arc::new(open(dir.path()));
        let batch = s.inner.appending.read().unwrap();
        let newer = {
            let s = s.clone();
            std::thread::spawn(move || {
                s.append(&[NewRecord::json(kinds::SESSION, Some("s1"), &"v").unwrap()])
                    .unwrap()
            })
        };
        std::thread::sleep(std::time::Duration::from_millis(300));
        assert_eq!(
            manifest(dir.path())["format"],
            2,
            "the manifest moved while a batch was being written"
        );
        drop(batch);
        newer.join().unwrap();
        assert_eq!(manifest(dir.path())["format"], 3);
    }

    /// A start at once after a stop (theseus-qa0 F4b): the stopping process
    /// still holds the store for a while after its socket is gone. The next
    /// open waits for it, and serves what the last holder wrote.
    #[test]
    fn an_open_waits_for_the_last_holder_to_close() {
        let dir = tempfile::tempdir().unwrap();
        let first = open(dir.path());
        let rec = NewRecord::json(kinds::SESSION, Some("s1"), &serde_json::json!({"turns": 1}));
        first.append(&[rec.unwrap()]).unwrap();
        let closer = std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(80));
            drop(first);
        });
        let t0 = std::time::Instant::now();
        let next = WalStore::open(dir.path(), WalConfig::default()).unwrap();
        let waited = t0.elapsed();
        closer.join().unwrap();
        assert!(waited >= std::time::Duration::from_millis(60), "{waited:?}");
        let st = next.stats().unwrap();
        assert!(st.lock_wait_us >= 60_000, "{}", st.lock_wait_us);
        assert!(!st.index_repaired, "the last holder closed it");
        let s1 = next.latest_by_key(kinds::SESSION, "s1").unwrap().unwrap();
        assert_eq!(s1.decode::<serde_json::Value>().unwrap()["turns"], 1);
        // An open that found the store free waited for nothing.
        drop(next);
        let again = WalStore::open(dir.path(), WalConfig::default()).unwrap();
        assert_eq!(again.stats().unwrap().lock_wait_us, 0);
    }

    /// A second process beside one that is not stopping still fails, once
    /// its wait has passed, and says why. The daemon's wait covers a stop
    /// whose telemetry flush takes its whole second.
    #[test]
    fn an_open_beside_a_holder_that_stays_fails_after_the_wait() {
        let dir = tempfile::tempdir().unwrap();
        let _held = open(dir.path());
        let wait = std::time::Duration::from_millis(150);
        let t0 = std::time::Instant::now();
        let e = WalStore::open_waiting(dir.path(), WalConfig::default(), wait)
            .err()
            .expect("the store is held");
        assert!(t0.elapsed() >= wait);
        let msg = format!("{e:#}");
        assert!(msg.contains("is another theseusd serving it?"), "{msg}");
        assert!(RedbIndex::held_elsewhere(&e), "{msg}");
        assert!(LOCK_WAIT >= std::time::Duration::from_secs(2));
    }

    #[test]
    fn redb_store() {
        let dir = tempfile::tempdir().unwrap();
        let s = open(dir.path());
        let p = s
            .append(&[NewRecord::json(
                kinds::SESSION,
                Some("s1"),
                &serde_json::json!({"turns": 0}),
            )
            .unwrap()])
            .unwrap();
        assert_eq!(p, vec![1]);
        s.append(&[NewRecord::json(kinds::LEDGER, None, &serde_json::json!({"k": "a"})).unwrap()])
            .unwrap();
        s.append(&[
            NewRecord::json(kinds::SESSION, Some("s1"), &serde_json::json!({"turns": 1})).unwrap(),
        ])
        .unwrap();
        let (c, k) = s
            .settle(
                NewRecord::json(
                    kinds::COMPLETION,
                    Some("act_1"),
                    &serde_json::json!({"ok": true}),
                )
                .unwrap(),
                NewRecord::json(
                    kinds::EXECUTION,
                    Some("ex_1"),
                    &serde_json::json!({"state": "runnable"}),
                )
                .unwrap(),
            )
            .unwrap();
        assert_eq!((c, k), (4, 5));
        let latest = s.latest_by_key(kinds::SESSION, "s1").unwrap().unwrap();
        assert_eq!(latest.position, 3);
        assert_eq!(latest.decode::<serde_json::Value>().unwrap()["turns"], 1);
        assert_eq!(s.latest_of_kind(kinds::SESSION).unwrap().len(), 1);
        assert_eq!(s.tail_of_kind(kinds::LEDGER, 5).unwrap()[0].position, 2);
        assert_eq!(s.count_of_kind(kinds::SESSION).unwrap(), 2);
        assert_eq!(s.scan(2, Some(4), 10).unwrap().len(), 3);
        s.append(&[
            NewRecord::json(kinds::LEDGER, None, &"a")
                .unwrap()
                .scoped("ses_x"),
            NewRecord::json(kinds::LEDGER, None, &"b")
                .unwrap()
                .scoped("ses_y"),
            NewRecord::json(kinds::LEDGER, None, &"c")
                .unwrap()
                .scoped("ses_x"),
        ])
        .unwrap();
        let sx = s.scan_scope("ses_x", 0, 10).unwrap();
        assert_eq!(sx.len(), 2);
        assert_eq!(sx[1].decode::<String>().unwrap(), "c");
        assert_eq!(s.scan_scope("ses_x", sx[0].position, 10).unwrap().len(), 1);
        assert_eq!(s.count_in_scope("ses_y").unwrap(), 1);

        // Checkpoint, append more (index non-durable), reopen: replay rebuilds.
        s.checkpoint().unwrap();
        s.append(&[NewRecord::json(kinds::META, Some("live"), &"glm").unwrap()])
            .unwrap();
        drop(s);
        let s = open(dir.path());
        assert_eq!(s.last_position(), 9);
        assert_eq!(s.count_in_scope("ses_x").unwrap(), 2);
        let st = s.stats().unwrap();
        assert!(st.replayed_into_index <= 9);
        assert_eq!(
            s.latest_by_key(kinds::META, "live")
                .unwrap()
                .unwrap()
                .decode::<String>()
                .unwrap(),
            "glm"
        );
    }

    /// A toy projection: an execution's payload, a JSON string, is its one
    /// term; a session's, a number, is added up as [1, the number].
    fn toy_terms(kind: RecordKind, payload: &[u8]) -> Vec<String> {
        if kind != kinds::EXECUTION {
            return Vec::new();
        }
        serde_json::from_slice::<String>(payload)
            .map(|s| vec![s])
            .unwrap_or_default()
    }
    fn toy_sums(kind: RecordKind, payload: &[u8]) -> Option<Sums> {
        let n: u128 = serde_json::from_slice::<u64>(payload).ok()?.into();
        (kind == kinds::SESSION).then_some([1, n, 0, 0, 0, 0, 0, 0])
    }
    static TOY: Projection = Projection {
        name: "terms.toy.1",
        kinds: &[kinds::EXECUTION, kinds::SESSION],
        terms: toy_terms,
        sums: toy_sums,
    };

    fn by_term(s: &WalStore, t: &str) -> Vec<String> {
        s.latest_by_terms(kinds::EXECUTION, t, &format!("{t}\u{1}"))
            .unwrap()
            .expect("a projected store")
            .into_iter()
            .map(|r| r.key.unwrap())
            .collect()
    }

    /// The terms follow every append and every replay, and survive a reopen
    /// (theseus-lv2). A writer that kept none (an open with no projection, as
    /// an older build is) leaves them stale at a newer checkpoint: the next
    /// projected open reads every record for them until they are built again
    /// after serving, a stretch of keys at a time, beside appends.
    #[test]
    fn the_terms_follow_the_wal_through_reopens_and_a_writer_that_kept_none() {
        let dir = tempfile::tempdir().unwrap();
        let ex =
            |key: &str, state: &str| NewRecord::json(kinds::EXECUTION, Some(key), &state).unwrap();
        let projected = || {
            WalStore::open_projected(dir.path(), WalConfig::default(), &TOY)
                .unwrap()
                .with_checkpoint_every(0)
        };
        let s = projected();
        assert!(
            !s.stats().unwrap().terms_pending,
            "a new store has none to build"
        );
        s.append(&[ex("e1", "waiting"), ex("e2", "running")])
            .unwrap();
        s.append(&[ex("e3", "waiting")]).unwrap();
        s.append(&[ex("e1", "queued")]).unwrap();
        assert_eq!(by_term(&s, "waiting"), ["e3"]);
        assert_eq!(by_term(&s, "queued"), ["e1"]);
        assert_eq!(
            s.count_by_terms(kinds::EXECUTION, "a", "z").unwrap(),
            Some(3)
        );
        // A kind the projection does not name keeps no terms; a store with no
        // projection, none.
        assert!(s.latest_by_terms(kinds::META, "a", "z").unwrap().is_none());
        s.checkpoint().unwrap();
        // The tail after the checkpoint is replayed with its terms.
        s.append(&[ex("e2", "waiting")]).unwrap();
        drop(s);
        let s = projected();
        let st = s.stats().unwrap();
        assert_eq!((st.replayed_into_index, st.terms_pending), (1, false));
        assert_eq!(by_term(&s, "waiting"), ["e2", "e3"]);
        assert!(by_term(&s, "running").is_empty());
        drop(s);
        // A writer with no projection moves the checkpoint past the terms.
        let plain = open(dir.path());
        assert!(plain
            .latest_by_terms(kinds::EXECUTION, "a", "z")
            .unwrap()
            .is_none());
        plain.append(&[ex("e3", "complete")]).unwrap();
        plain.checkpoint().unwrap();
        drop(plain);
        // Not whole: no terms are answered, and a checkpoint marks none.
        let s = projected();
        assert!(s.stats().unwrap().terms_pending);
        assert!(s
            .latest_by_terms(kinds::EXECUTION, "a", "z")
            .unwrap()
            .is_none());
        s.checkpoint_for_close().unwrap();
        drop(s);
        let s = projected();
        assert!(s.stats().unwrap().terms_pending, "still not whole");
        // The build after serving, two keys a stretch, with an append between
        // its stretches: e1 moves on after the build read it.
        let at = s.build_terms(None, 2).unwrap();
        assert_eq!(at, Some((kinds::EXECUTION, "e2".to_string())));
        s.append(&[ex("e1", "complete"), ex("e4", "waiting")])
            .unwrap();
        assert!(s.build_terms(at, 2).unwrap().is_some(), "e3 and e4");
        assert!(!s.terms_whole());
        let mut at = Some((kinds::EXECUTION, "e4".to_string()));
        while let Some(next) = s.build_terms(at, 2).unwrap() {
            at = Some(next);
        }
        assert!(s.terms_whole() && !s.stats().unwrap().terms_pending);
        assert_eq!(by_term(&s, "waiting"), ["e2", "e4"]);
        assert_eq!(by_term(&s, "complete"), ["e1", "e3"]);
        assert!(
            by_term(&s, "queued").is_empty(),
            "e1's own append replaced it"
        );
        s.checkpoint().unwrap();
        drop(s);
        let s = projected();
        assert!(!s.stats().unwrap().terms_pending, "whole at its checkpoint");
        assert_eq!(by_term(&s, "waiting"), ["e2", "e4"]);
    }

    /// theseus-byu: a long replay builds the index in key order, a table at a
    /// time, and answers every read as the index built record by record does:
    /// the same WAL, opened with no index.
    #[test]
    fn an_index_built_in_bulk_answers_as_one_built_record_by_record() {
        let one_by_one = tempfile::tempdir().unwrap();
        // 1,700 frames: unsynced, which changes nothing a read answers.
        let unsynced = WalConfig {
            fsync: false,
            ..WalConfig::default()
        };
        let s = WalStore::open_projected(one_by_one.path(), unsynced, &TOY)
            .unwrap()
            .with_checkpoint_every(0);
        let states = ["waiting", "running", "queued", "complete"];
        let mut n = 0usize;
        for i in 0..1700u32 {
            let k = format!("e{:04}", (i * 7919) % 900);
            s.append(&[
                NewRecord::json(kinds::EXECUTION, Some(&k), &states[i as usize % 4])
                    .unwrap()
                    .scoped(&format!("ses_{}", i % 37)),
                NewRecord::json(kinds::LEDGER, None, &i)
                    .unwrap()
                    .scoped(&format!("ses_{}", i % 37)),
                NewRecord::json(kinds::SESSION, Some(&format!("s{}", i % 300)), &i).unwrap(),
            ])
            .unwrap();
            n += 3;
        }
        assert!(n >= BULK, "a replay long enough for the bulk path");
        s.checkpoint().unwrap();
        drop(s);
        // The same WAL, and no index: the open replays every record.
        let bulk = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(bulk.path().join("wal")).unwrap();
        for seg in crate::wal::list_segments(&one_by_one.path().join("wal")).unwrap() {
            let name = format!("{seg:09}.seg");
            std::fs::copy(
                one_by_one.path().join("wal").join(&name),
                bulk.path().join("wal").join(&name),
            )
            .unwrap();
        }
        let a = WalStore::open_projected(one_by_one.path(), WalConfig::default(), &TOY).unwrap();
        let b = WalStore::open_projected(bulk.path(), WalConfig::default(), &TOY).unwrap();
        assert_eq!(b.stats().unwrap().replayed_into_index, n as u64);
        let keyed = |s: &WalStore, kind| -> Vec<(Option<String>, u64)> {
            s.latest_of_kind(kind)
                .unwrap()
                .into_iter()
                .map(|r| (r.key, r.position))
                .collect()
        };
        for kind in [kinds::EXECUTION, kinds::SESSION, kinds::LEDGER] {
            assert_eq!(keyed(&a, kind), keyed(&b, kind), "kind {kind}");
            assert_eq!(
                a.count_of_kind(kind).unwrap(),
                b.count_of_kind(kind).unwrap()
            );
        }
        for scope in ["ses_0", "ses_5", "ses_36"] {
            let at = |s: &WalStore| -> Vec<u64> {
                s.scan_scope(scope, 0, usize::MAX)
                    .unwrap()
                    .iter()
                    .map(|r| r.position)
                    .collect()
            };
            assert_eq!(at(&a), at(&b), "{scope}");
        }
        for state in states {
            assert_eq!(by_term(&a, state), by_term(&b, state), "{state}");
        }
        assert_eq!(
            a.tail_of_kind(kinds::LEDGER, 5).unwrap(),
            b.tail_of_kind(kinds::LEDGER, 5).unwrap()
        );
        // The totals: each session's latest number, added up, either way.
        let latest: u128 = a
            .latest_of_kind(kinds::SESSION)
            .unwrap()
            .iter()
            .map(|r| u128::from(r.decode::<u64>().unwrap()))
            .sum();
        let want = Some([300, latest, 0, 0, 0, 0, 0, 0]);
        assert_eq!(a.totals(kinds::SESSION).unwrap(), want);
        assert_eq!(b.totals(kinds::SESSION).unwrap(), want);
    }

    /// Keys by prefix and the count of keys come from the key table alone.
    #[test]
    fn keys_by_prefix_and_their_count() {
        let dir = tempfile::tempdir().unwrap();
        let s = open(dir.path());
        for (k, v) in [("q_1", 1), ("a_1", 2), ("q_2", 3), ("q_1", 4)] {
            s.append(&[NewRecord::json(kinds::COMPLETION, Some(k), &v).unwrap()])
                .unwrap();
        }
        let q: Vec<(String, i64)> = s
            .latest_with_prefix(kinds::COMPLETION, "q_")
            .unwrap()
            .into_iter()
            .map(|r| (r.key.clone().unwrap(), r.decode().unwrap()))
            .collect();
        assert_eq!(q, [("q_1".to_string(), 4), ("q_2".to_string(), 3)]);
        assert_eq!(s.count_keys(kinds::COMPLETION).unwrap(), 3);
        assert_eq!(s.count_of_kind(kinds::COMPLETION).unwrap(), 4);
    }

    /// A store this build did not write is refused and left as it was: another
    /// format (format 1 is the pre-M2 record layout) or another engine.
    #[test]
    fn another_format_or_engine_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        for (name, manifest, says) in [
            ("format1", r#"{"format": 1, "engine": "redb"}"#, "format 1"),
            ("format9", r#"{"format": 9, "engine": "redb"}"#, "format 9"),
            ("fjall", r#"{"format": 2, "engine": "fjall"}"#, "fjall"),
        ] {
            let store_dir = dir.path().join(name);
            std::fs::create_dir_all(&store_dir).unwrap();
            std::fs::write(store_dir.join("MANIFEST.json"), manifest).unwrap();
            let e = WalStore::open(&store_dir, WalConfig::default())
                .err()
                .unwrap_or_else(|| panic!("{name} opened"));
            assert!(format!("{e:#}").contains(says), "{name}: {e:#}");
            assert!(!store_dir.join("wal").exists(), "{name} was written to");
        }
    }

    /// Every file under `dir`, with its length and bytes: what "nothing was
    /// written" compares.
    fn snapshot(dir: &Path) -> Vec<(PathBuf, Vec<u8>)> {
        let mut out = Vec::new();
        let mut stack = vec![dir.to_path_buf()];
        while let Some(d) = stack.pop() {
            for e in std::fs::read_dir(&d).unwrap() {
                let p = e.unwrap().path();
                if p.is_dir() {
                    stack.push(p);
                } else {
                    out.push((p.clone(), std::fs::read(&p).unwrap()));
                }
            }
        }
        out.sort();
        out
    }

    fn manifest(dir: &Path) -> serde_json::Value {
        serde_json::from_slice(&std::fs::read(dir.join("MANIFEST.json")).unwrap()).unwrap()
    }

    /// F4a: a store marked with a schema newer than this build knows is
    /// refused, with a message that names the kind, both schemas, and what to
    /// do, and nothing in it is written: not the WAL's tail, not the index,
    /// not the manifest.
    #[test]
    fn a_newer_schema_is_refused_and_nothing_is_written() {
        let dir = tempfile::tempdir().unwrap();
        let s = open(dir.path());
        s.append(&[NewRecord::json(kinds::SESSION, Some("s1"), &"v1").unwrap()])
            .unwrap();
        s.checkpoint().unwrap();
        s.append(&[NewRecord::json(kinds::LEDGER, None, &"after").unwrap()])
            .unwrap();
        drop(s);
        let session = kinds::schema(kinds::SESSION);
        let mut m = manifest(dir.path());
        for k in m["kinds"].as_array_mut().unwrap() {
            if k["kind"] == kinds::SESSION {
                k["schema"] = (session + 1).into();
            }
        }
        std::fs::write(
            dir.path().join("MANIFEST.json"),
            serde_json::to_vec_pretty(&m).unwrap(),
        )
        .unwrap();
        // A torn frame too, which an open would cut: it must stay.
        {
            use std::io::Write as _;
            let mut f = std::fs::OpenOptions::new()
                .append(true)
                .open(dir.path().join("wal").join("000000001.seg"))
                .unwrap();
            f.write_all(&crate::wal::MAGIC.to_le_bytes()).unwrap();
        }
        let before = snapshot(dir.path());
        let e = format!(
            "{:#}",
            WalStore::open(dir.path(), WalConfig::default())
                .err()
                .unwrap()
        );
        for says in [
            "session records (kind 1)".to_string(),
            format!("at schema {}", session + 1),
            format!("up to schema {session}"),
            "install the newer theseusd".to_string(),
        ] {
            assert!(e.contains(&says), "{says:?} missing from: {e}");
        }
        assert_eq!(
            snapshot(dir.path()),
            before,
            "the refused store was written to"
        );

        // A kind this build does not know, and a newer format, are refused
        // the same way.
        for (manifest, says) in [
            (
                r#"{"format": 3, "engine": "redb", "kinds": [{"kind": 77, "name": "hold", "schema": 1}]}"#,
                "hold records (kind 77) at schema 1, a kind this build does not know",
            ),
            (r#"{"format": 4, "engine": "redb"}"#, "is format 4"),
        ] {
            std::fs::write(dir.path().join("MANIFEST.json"), manifest).unwrap();
            let before = snapshot(dir.path());
            let e = format!(
                "{:#}",
                WalStore::open(dir.path(), WalConfig::default())
                    .err()
                    .unwrap()
            );
            assert!(
                e.contains(says) && e.contains("install the newer theseusd"),
                "{e}"
            );
            assert_eq!(snapshot(dir.path()), before);
        }
    }

    /// theseus-7hh: the manifest alone says what `open` would, for a store
    /// this build reads, one it is too old for, and a directory with no
    /// store yet, and the check writes nothing.
    #[test]
    fn the_manifest_alone_says_whether_this_build_may_open_a_store() {
        let dir = tempfile::tempdir().unwrap();
        check_manifest(&dir.path().join("none")).unwrap();
        let s = open(dir.path());
        s.append(&[NewRecord::json(kinds::SESSION, Some("s1"), &"v1").unwrap()])
            .unwrap();
        drop(s);
        check_manifest(dir.path()).unwrap();
        for (manifest, says) in [
            (
                r#"{"format": 3, "engine": "redb", "kinds": [{"kind": 77, "name": "hold", "schema": 1}]}"#,
                "a kind this build does not know",
            ),
            (r#"{"format": 4, "engine": "redb"}"#, "is format 4"),
            ("{", "reading store manifest"),
        ] {
            std::fs::write(dir.path().join("MANIFEST.json"), manifest).unwrap();
            let before = snapshot(dir.path());
            let e = format!("{:#}", check_manifest(dir.path()).err().unwrap());
            assert!(e.contains(says), "{says:?} missing from: {e}");
            assert_eq!(snapshot(dir.path()), before, "the check wrote");
        }
    }

    /// F4a: a format-2 store (an older build's) opens as it is and stays
    /// format 2 while this build writes only what the older one could
    /// (schema-1 records), so a rollback still opens it. The first record of
    /// a newer schema marks the manifest first, with every kind this build
    /// writes, once.
    #[test]
    fn a_format_2_store_is_marked_at_its_first_newer_record() {
        let dir = tempfile::tempdir().unwrap();
        let s = open(dir.path());
        s.append(&[NewRecord::json(kinds::LEDGER, None, &"a").unwrap()])
            .unwrap();
        drop(s);
        std::fs::write(
            dir.path().join("MANIFEST.json"),
            r#"{"format": 2, "engine": "redb"}"#,
        )
        .unwrap();
        let s = open(dir.path());
        assert!(s.schema_marks().values().all(|v| *v == 1));
        s.append(&[NewRecord::json(kinds::LEDGER, None, &"b").unwrap()])
            .unwrap();
        assert_eq!(
            manifest(dir.path())["format"],
            2,
            "a schema-1 write keeps format 2"
        );
        let session = NewRecord::json(kinds::SESSION, Some("s1"), &"v").unwrap();
        assert_eq!(session.schema, kinds::schema(kinds::SESSION));
        assert!(session.schema > 1, "the session kind is bumped (T1's hold)");
        s.append(&[session]).unwrap();
        let m = manifest(dir.path());
        assert_eq!(m["format"], 3);
        let marked: BTreeMap<u16, u16> = m["kinds"]
            .as_array()
            .unwrap()
            .iter()
            .map(|k| {
                (
                    k["kind"].as_u64().unwrap() as u16,
                    k["schema"].as_u64().unwrap() as u16,
                )
            })
            .collect();
        let want: BTreeMap<u16, u16> = kinds::SCHEMAS.iter().copied().collect();
        assert_eq!(marked, want, "every kind this build writes, at its schema");
        let modified = std::fs::metadata(dir.path().join("MANIFEST.json"))
            .unwrap()
            .modified()
            .unwrap();
        s.append(&[NewRecord::json(kinds::EXECUTION, Some("e1"), &"x").unwrap()])
            .unwrap();
        assert_eq!(
            std::fs::metadata(dir.path().join("MANIFEST.json"))
                .unwrap()
                .modified()
                .unwrap(),
            modified,
            "marked once per upgrade"
        );
        drop(s);
        // And the marked store opens again under this build.
        let s = open(dir.path());
        assert_eq!(s.last_position(), 4);
        assert_eq!(
            s.latest_by_key(kinds::SESSION, "s1")
                .unwrap()
                .unwrap()
                .schema,
            kinds::schema(kinds::SESSION)
        );
    }

    /// theseus-8ni: open checks only the WAL after the checkpoint. Corrupt
    /// an old segment's body and make another unreadable: the store still
    /// opens, finds the next position, and cuts a torn tail. Then the
    /// history check finds the corrupt segment, and reads from it are
    /// refused.
    #[test]
    fn open_reads_only_the_tail_and_the_history_check_finds_an_old_corrupt_segment() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let cfg = WalConfig {
            segment_bytes: 400,
            ..Default::default()
        };
        let s = WalStore::open(dir.path(), cfg.clone())
            .unwrap()
            .with_checkpoint_every(0);
        for i in 0..40u32 {
            s.append(&[NewRecord::json(kinds::LEDGER, None, &[i; 20]).unwrap()])
                .unwrap();
        }
        let cp = s.checkpoint().unwrap();
        for i in 40..43u32 {
            s.append(&[NewRecord::json(kinds::LEDGER, None, &[i; 20]).unwrap()])
                .unwrap();
        }
        let segments = s.stats().unwrap().wal_segments;
        assert!(segments >= 4, "{segments} segments");
        drop(s);
        let seg = |n: u32| dir.path().join("wal").join(format!("{n:09}.seg"));
        // Segment 1: a byte of its first record's payload flipped (header
        // 12, count 4, record header 28), so the record still decodes and
        // only its frame's crc knows. Segment 2: unreadable.
        let mut b = std::fs::read(seg(1)).unwrap();
        b[12 + 4 + 28 + 3] ^= 0x01;
        std::fs::write(seg(1), &b).unwrap();
        std::fs::set_permissions(seg(2), std::fs::Permissions::from_mode(0o000)).unwrap();
        // A torn frame at the end of the last segment.
        let last_len = std::fs::metadata(seg(segments)).unwrap().len();
        {
            use std::io::Write as _;
            let mut f = std::fs::OpenOptions::new()
                .append(true)
                .open(seg(segments))
                .unwrap();
            f.write_all(&crate::wal::MAGIC.to_le_bytes()).unwrap();
            f.write_all(&[9u8; 7]).unwrap();
        }

        let s = WalStore::open(dir.path(), cfg.clone()).unwrap();
        std::fs::set_permissions(seg(2), std::fs::Permissions::from_mode(0o644)).unwrap();
        let st = s.stats().unwrap();
        assert_eq!(st.last_position, 43);
        assert_eq!(st.truncated_bytes, 11, "the torn frame is cut");
        assert_eq!(std::fs::metadata(seg(segments)).unwrap().len(), last_len);
        assert_eq!(st.replayed_into_index, 43 - cp, "only the tail is replayed");
        assert!(st.history_bytes > 0);
        let r = s.recovery();
        assert_eq!(r.records, 43 - cp, "only the tail is checked");
        assert!(r.checked_from.is_some());
        // Appends go on from the next position.
        let p = s
            .append(&[NewRecord::json(kinds::LEDGER, None, &"next").unwrap()])
            .unwrap();
        assert_eq!(p, vec![44]);

        let before = s.get(1);
        assert!(before.is_ok(), "no check has run yet: {before:?}");
        let e = s.verify_history(|_| {}).unwrap_err();
        assert!(
            matches!(e, crate::wal::WalError::Corrupt { segment: 1, .. }),
            "{e}"
        );
        let refused = format!("{:#}", s.get(1).unwrap_err());
        assert!(refused.contains("corrupt frame"), "{refused}");
        assert!(s.get(2).unwrap().is_some(), "the next frame still reads");
        assert!(s.get(cp).unwrap().is_some(), "a later segment still reads");

        // A whole history checks, and `pace` hears of each stretch.
        let clean = tempfile::tempdir().unwrap();
        let s = WalStore::open(clean.path(), cfg.clone())
            .unwrap()
            .with_checkpoint_every(0);
        for i in 0..40u32 {
            s.append(&[NewRecord::json(kinds::LEDGER, None, &[i; 20]).unwrap()])
                .unwrap();
        }
        s.checkpoint().unwrap();
        drop(s);
        let s = WalStore::open(clean.path(), cfg).unwrap();
        let h = s.verify_history(|_| {}).unwrap();
        assert_eq!(h.records, 40);
        assert!(h.segments >= 3 && !h.checked_at_open, "{h:?}");
    }

    /// theseus-0dq: a history check starts where the last one ended. Its
    /// mark reaches the index with the next checkpoint, and the check after a
    /// restart checks the mark's frame again and then only what was written
    /// since; a mark whose frame no longer checks, or holds other positions,
    /// is not believed, and the whole log is checked, which finds what is
    /// wrong.
    #[test]
    fn a_history_check_starts_at_the_last_checks_mark() {
        let dir = tempfile::tempdir().unwrap();
        let cfg = WalConfig {
            segment_bytes: 400,
            ..Default::default()
        };
        let reopen = || {
            WalStore::open(dir.path(), cfg.clone())
                .unwrap()
                .with_checkpoint_every(0)
        };
        let rows = |s: &WalStore, from: u32, to: u32| {
            for i in from..to {
                s.append(&[NewRecord::json(kinds::LEDGER, None, &[i; 20]).unwrap()])
                    .unwrap();
            }
            s.checkpoint().unwrap()
        };
        let s = reopen();
        rows(&s, 0, 40);
        drop(s);
        // The first check reads the whole history and leaves its mark.
        let s = reopen();
        assert_eq!(s.verified(), None);
        let h = s
            .history_check()
            .from_mark(s.verified())
            .run(|_| {})
            .unwrap();
        assert_eq!((h.records, h.from_position), (40, None));
        let mark = h.verified.expect("a mark");
        assert_eq!(mark.position, 40);
        assert!(mark.full_at_unix_ms > 0, "a whole check sets its time");
        s.verified_slot().set(mark);
        rows(&s, 40, 45);
        drop(s);
        // The next reads the mark's frame and what came after it, and keeps
        // the time of the last whole check.
        let s = reopen();
        assert_eq!(s.verified(), Some(mark), "the checkpoint wrote it");
        let h = s
            .history_check()
            .from_mark(s.verified())
            .run(|_| {})
            .unwrap();
        assert_eq!(h.from_position, Some(40));
        assert_eq!(
            h.records, 6,
            "the mark's frame again, and the five after it"
        );
        let next = h.verified.unwrap();
        assert_eq!(next.position, 45);
        assert_eq!(next.full_at_unix_ms, mark.full_at_unix_ms);
        // A mark that names other positions for its frame is not believed.
        let wrong = Verified {
            first: mark.first + 1,
            ..mark
        };
        let h = s
            .history_check()
            .from_mark(Some(wrong))
            .run(|_| {})
            .unwrap();
        assert_eq!((h.records, h.from_position), (45, None), "the whole log");
        drop(s);
        // Nor one whose frame no longer checks: the whole check that follows
        // finds the corrupt frame.
        let seg = dir
            .path()
            .join("wal")
            .join(format!("{:09}.seg", mark.segment));
        let mut b = std::fs::read(&seg).unwrap();
        b[mark.offset as usize + 12 + 4 + 28 + 3] ^= 0x01;
        std::fs::write(&seg, &b).unwrap();
        let s = reopen();
        let e = s
            .history_check()
            .from_mark(Some(mark))
            .run(|_| {})
            .unwrap_err();
        assert!(
            matches!(e, crate::wal::WalError::Corrupt { segment, .. } if segment == mark.segment),
            "{e}"
        );
    }

    /// theseus-02k: a stop's checkpoint syncs nothing of its own, and redb's
    /// close makes it durable, so the next open replays nothing and repairs
    /// nothing, and a history check's mark rides along.
    #[test]
    fn a_checkpoint_for_close_is_made_durable_by_the_close() {
        let dir = tempfile::tempdir().unwrap();
        let s = open(dir.path());
        for i in 0..5u32 {
            s.append(&[NewRecord::json(kinds::LEDGER, None, &i).unwrap()])
                .unwrap();
        }
        s.checkpoint().unwrap();
        s.append(&[NewRecord::json(kinds::LEDGER, None, &"stopping").unwrap()])
            .unwrap();
        let mark = Verified {
            segment: 1,
            offset: 0,
            first: 1,
            position: 5,
            full_at_unix_ms: 7,
        };
        s.verified_slot().set(mark);
        assert_eq!(s.checkpoint_for_close().unwrap(), 6);
        // A durable checkpoint after it is not free: nothing synced 6 yet.
        assert_eq!(s.inner.durable_to.load(Ordering::Relaxed), 5);
        assert_eq!(s.checkpoint_for_close().unwrap(), 6, "and a second is free");
        drop(s);
        let s = open(dir.path());
        let st = s.stats().unwrap();
        assert_eq!(st.checkpoint, Some(6));
        assert_eq!(st.replayed_into_index, 0, "the close made it durable");
        assert!(!st.index_repaired);
        assert_eq!(s.verified(), Some(mark));
    }

    /// When the index's checkpoint does not match the WAL (a WAL copied in
    /// under an old index, say), open checks every segment, as it always did,
    /// and a read through a stale index entry is refused, never believed.
    #[test]
    fn a_checkpoint_the_wal_does_not_match_falls_back_to_the_full_check() {
        let dir = tempfile::tempdir().unwrap();
        let s = open(dir.path());
        for i in 0..5u32 {
            s.append(&[NewRecord::json(kinds::LEDGER, None, &i).unwrap()])
                .unwrap();
        }
        s.checkpoint().unwrap();
        drop(s);
        // Another WAL of seven longer records: position 5 is elsewhere.
        let other = tempfile::tempdir().unwrap();
        let w = crate::wal::Wal::open(other.path(), WalConfig::default()).unwrap();
        for i in 0..7u32 {
            w.append(&[
                NewRecord::json(kinds::LEDGER, None, &format!("a longer record {i}")).unwrap(),
            ])
            .unwrap();
        }
        drop(w);
        std::fs::copy(
            other.path().join("000000001.seg"),
            dir.path().join("wal").join("000000001.seg"),
        )
        .unwrap();
        let s = open(dir.path());
        assert!(s.recovery().checked_from.is_none(), "every segment checked");
        assert_eq!(s.last_position(), 7);
        assert_eq!(
            s.get(6).unwrap().unwrap().decode::<String>().unwrap(),
            "a longer record 5"
        );
        assert!(s.get(2).is_err(), "a stale entry's read is refused");
    }

    /// A checkpoint with nothing written since the last one claims the same
    /// position and commits nothing (theseus-pfv: a clean stop's last
    /// checkpoint, after its own). One after new records claims them, and the
    /// next open replays nothing either way.
    #[test]
    fn a_checkpoint_with_nothing_new_claims_the_same_and_the_next_open_replays_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let s = open(dir.path());
        for i in 0..5u32 {
            s.append(&[NewRecord::json(kinds::LEDGER, None, &i).unwrap()])
                .unwrap();
        }
        assert_eq!(s.checkpoint().unwrap(), 5);
        assert_eq!(s.checkpoint().unwrap(), 5, "nothing new");
        assert_eq!(s.inner.index.checkpoint().unwrap(), Some(5));
        drop(s);
        let s = open(dir.path());
        assert_eq!(s.stats().unwrap().replayed_into_index, 0);
        assert_eq!(s.checkpoint().unwrap(), 5, "nothing new since the open");
        s.append(&[NewRecord::json(kinds::LEDGER, None, &9u32).unwrap()])
            .unwrap();
        assert_eq!(s.checkpoint().unwrap(), 6);
        assert_eq!(s.inner.index.checkpoint().unwrap(), Some(6));
        drop(s);
        let s = open(dir.path());
        assert_eq!(s.stats().unwrap().replayed_into_index, 0);
        assert_eq!(s.count_of_kind(kinds::LEDGER).unwrap(), 6);
    }

    #[test]
    fn index_loss_is_rebuilt_from_wal() {
        let dir = tempfile::tempdir().unwrap();
        let s = open(dir.path());
        for i in 0..50u32 {
            s.append(&[NewRecord::json(kinds::LEDGER, None, &i).unwrap()])
                .unwrap();
        }
        s.append(&[NewRecord::json(kinds::SESSION, Some("s"), &"v1").unwrap()])
            .unwrap();
        drop(s);
        // Destroy the index entirely; the WAL is the truth.
        std::fs::remove_file(dir.path().join("index.redb")).unwrap();
        let s = open(dir.path());
        assert_eq!(s.last_position(), 51);
        assert_eq!(s.stats().unwrap().replayed_into_index, 51);
        assert_eq!(s.count_of_kind(kinds::LEDGER).unwrap(), 50);
        assert_eq!(
            s.latest_by_key(kinds::SESSION, "s")
                .unwrap()
                .unwrap()
                .decode::<String>()
                .unwrap(),
            "v1"
        );
    }

    /// Every record a store reads: position, kind, key, scope, and payload.
    type Rows = Vec<(u64, RecordKind, Option<String>, Option<String>, Vec<u8>)>;

    /// A store of sessions, ledger rows, and scoped nodes, checkpointed and
    /// closed, with every record as the store reads it.
    fn written(dir: &Path) -> Rows {
        let s = open(dir);
        for i in 0..40u32 {
            let mut batch = vec![NewRecord::json(kinds::LEDGER, None, &i).unwrap()];
            if i % 3 == 0 {
                batch.push(
                    NewRecord::json(kinds::SESSION, Some(&format!("ses_{}", i % 4)), &i).unwrap(),
                );
            }
            if i % 5 == 0 {
                batch.push(
                    NewRecord::json(kinds::NODE, Some(&format!("n{i}")), &i)
                        .unwrap()
                        .scoped("ses_0"),
                );
            }
            s.append(&batch).unwrap();
        }
        s.checkpoint().unwrap();
        let all = everything(&s);
        assert!(all.len() > 50);
        all
    }

    fn everything(s: &WalStore) -> Rows {
        s.scan(1, None, usize::MAX)
            .unwrap()
            .into_iter()
            .map(|r| (r.position, r.kind, r.key, r.scope, r.payload))
            .collect()
    }

    /// The `index.redb.bad-*` files beside the index.
    fn moved(dir: &Path) -> Vec<PathBuf> {
        let mut v: Vec<PathBuf> = std::fs::read_dir(dir)
            .unwrap()
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| {
                p.file_name()
                    .is_some_and(|n| n.to_string_lossy().starts_with("index.redb.bad-"))
            })
            .collect();
        v.sort();
        v
    }

    /// A kill inside the store's first open leaves an `index.redb` that is
    /// not a redb database (theseus-0b8): a header cut short, bytes that were
    /// never a header, or the zeros redb sized the file with before it wrote
    /// one. The next open moves the file aside, keeping it byte for byte,
    /// builds the index again from the WAL, and reads every record the WAL
    /// holds. The open after that finds a good index and moves nothing.
    #[test]
    fn an_index_that_is_not_a_database_is_moved_aside_and_rebuilt_from_the_wal() {
        type Damage = fn(&[u8]) -> Vec<u8>;
        // Each shape, and why the open says it is not a database.
        let shapes: [(&str, Damage, &str); 3] = [
            (
                "a 37-byte partial header",
                |real| real[..37].to_vec(),
                "the file ends inside redb's header",
            ),
            (
                "a few random bytes",
                |_| {
                    vec![
                        0x5c, 0x91, 0x07, 0xee, 0x30, 0x2a, 0xd4, 0x18, 0x66, 0x0b, 0xf3,
                    ]
                },
                "the file does not start with redb's magic number",
            ),
            (
                "zeros the length of a new index",
                |real| vec![0u8; real.len()],
                "the file does not start with redb's magic number",
            ),
        ];
        for (shape, damage, says) in shapes {
            let dir = tempfile::tempdir().unwrap();
            let before = written(dir.path());
            let index = dir.path().join("index.redb");
            let bad = damage(&std::fs::read(&index).unwrap());
            std::fs::write(&index, &bad).unwrap();

            let s = open(dir.path());
            let st = s.stats().unwrap();
            let m = st.index_moved_aside.clone().expect(shape);
            let aside = moved(dir.path());
            assert_eq!(aside.len(), 1, "{shape}: {aside:?}");
            assert_eq!(m.path, aside[0].display().to_string(), "{shape}");
            assert_eq!(
                std::fs::read(&aside[0]).unwrap(),
                bad,
                "{shape}: kept as it was"
            );
            assert_eq!(m.bytes, bad.len() as u64);
            assert!(m.why.starts_with(says), "{shape}: {}", m.why);
            assert!(m.why.contains("(redb: "), "{shape}: {}", m.why);
            assert_eq!(st.replayed_into_index, before.len() as u64, "{shape}");
            assert_eq!(everything(&s), before, "{shape}");
            assert_eq!(s.count_of_kind(kinds::LEDGER).unwrap(), 40, "{shape}");
            assert_eq!(s.count_in_scope("ses_0").unwrap(), 8, "{shape}");
            assert_eq!(
                s.latest_by_key(kinds::SESSION, "ses_3")
                    .unwrap()
                    .unwrap()
                    .decode::<u32>()
                    .unwrap(),
                39,
                "{shape}"
            );
            drop(s);

            let s = open(dir.path());
            let st = s.stats().unwrap();
            assert!(st.index_moved_aside.is_none(), "{shape}");
            assert_eq!(
                st.replayed_into_index, 0,
                "{shape}: the rebuild was checkpointed"
            );
            assert_eq!(everything(&s), before, "{shape}");
            assert_eq!(moved(dir.path()).len(), 1, "{shape}");
        }
    }

    /// A store another process holds is waited for and refused with the
    /// message it always had, and nothing is moved (theseus-0b8): the held
    /// index is that process's. That holds even while the index is not a
    /// database yet, as when its holder is inside its own first open: redb
    /// takes its lock before it reads a byte, so the error says held.
    #[test]
    fn a_held_index_is_refused_and_never_moved_even_before_it_is_a_database() {
        let dir = tempfile::tempdir().unwrap();
        written(dir.path());
        let holder = open(dir.path());
        let wait = std::time::Duration::from_millis(150);
        let e = WalStore::open_waiting(dir.path(), WalConfig::default(), wait)
            .err()
            .expect("the store is held");
        let msg = format!("{e:#}");
        assert!(msg.contains("is another theseusd serving it?"), "{msg}");
        assert!(RedbIndex::held_elsewhere(&e) && !RedbIndex::not_a_database(&e));
        assert!(moved(dir.path()).is_empty());
        drop(holder);

        // A holder inside its first open: the file is zeros, and locked.
        let dir = tempfile::tempdir().unwrap();
        written(dir.path());
        let index = dir.path().join("index.redb");
        let zeros = vec![0u8; 4096];
        std::fs::write(&index, &zeros).unwrap();
        let lock = std::fs::File::open(&index).unwrap();
        lock.try_lock().unwrap();
        let e = WalStore::open_waiting(dir.path(), WalConfig::default(), wait)
            .err()
            .expect("the index is held");
        let msg = format!("{e:#}");
        assert!(msg.contains("is another theseusd serving it?"), "{msg}");
        assert!(moved(dir.path()).is_empty());
        assert_eq!(std::fs::read(&index).unwrap(), zeros, "left as it was");
        drop(lock);
        // Released, it is the holder's leftover: moved aside, and rebuilt.
        let s = open(dir.path());
        assert!(s.stats().unwrap().index_moved_aside.is_some());
        assert_eq!(moved(dir.path()).len(), 1);
    }

    /// A file that is a redb database but fails some other way (a format
    /// newer than redb reads, or a file cut short of its layout) is not this
    /// case: it stays where it is, and the open refuses as it did before.
    #[test]
    fn a_redb_index_that_fails_another_way_is_left_and_refused() {
        type Damage = fn(&mut Vec<u8>);
        let cases: [(&str, Damage); 2] = [
            ("a newer file format", |b| {
                // Each commit slot's first byte is its format version.
                b[64] = 99;
                b[192] = 99;
            }),
            ("a file cut short", |b| b.truncate(b.len() / 2)),
        ];
        for (case, damage) in cases {
            let dir = tempfile::tempdir().unwrap();
            written(dir.path());
            let index = dir.path().join("index.redb");
            let mut bytes = std::fs::read(&index).unwrap();
            damage(&mut bytes);
            std::fs::write(&index, &bytes).unwrap();
            let e = WalStore::open(dir.path(), WalConfig::default())
                .err()
                .expect(case);
            let msg = format!("{e:#}");
            assert!(msg.contains("opening index"), "{case}: {msg}");
            assert!(!RedbIndex::not_a_database(&e), "{case}: {msg}");
            assert!(!RedbIndex::held_elsewhere(&e), "{case}: {msg}");
            assert!(moved(dir.path()).is_empty(), "{case}");
            assert_eq!(std::fs::read(&index).unwrap(), bytes, "{case}: untouched");
        }
    }
}
