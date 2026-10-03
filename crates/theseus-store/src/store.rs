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
//! runtime worker (`blocking`). The periodic checkpoint runs on the writer
//! too, after its answers (theseus-avvb).

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{mpsc, Arc, Mutex, RwLock};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::index::{Aside, Engine, IndexEntry, MovedAside, Projected, RedbIndex, Sums};
use crate::record::{kinds, NewRecord, Record, RecordKind};
use crate::wal::{History, RecordLocation, Recovery, Verified, Wal, WalConfig, WalError};

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
    /// Records a list read skipped since open because their reads are
    /// refused (a corrupt frame the history check found, R4): how many, and
    /// the first `REFUSED_SHOWN` of their positions, lowest first.
    #[serde(default)]
    pub refused_records: u64,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub refused_positions: Vec<u64>,
}

/// How many refused positions `StoreStats` names; the count goes on past it.
pub const REFUSED_SHOWN: usize = 16;

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
    /// Records of a kind with position > `after`, oldest first, at most
    /// `limit`: one page of a walk over a kind from a position (the ledger's
    /// pages, theseus-xo0m). This default reads the whole kind; the WAL store
    /// reads its index's range.
    fn of_kind_after(&self, kind: RecordKind, after: u64, limit: usize) -> Result<Vec<Record>> {
        Ok(self
            .tail_of_kind(kind, usize::MAX)?
            .into_iter()
            .filter(|r| r.position > after)
            .take(limit)
            .collect())
    }
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
    /// A test's way to hold a checkpoint, as a disk under writeback holds
    /// one: the next checkpoint waits until this channel's sender sends or
    /// drops.
    #[cfg(test)]
    checkpoint_hold: std::sync::Mutex<Option<mpsc::Receiver<()>>>,
    /// The positions list reads skipped because their reads are refused
    /// (R4), and how many there were past the ones kept.
    refused: Mutex<(BTreeSet<u64>, u64)>,
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

/// The refused positions a store remembers by number; past them it counts.
const REFUSED_KEPT: usize = 4096;

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

/// A test's view of an open that waits: the tries that found the store held,
/// by its directory, so a test's holder closes only once the open is known
/// to wait (theseus-so1a).
#[cfg(test)]
static HELD_TRIES: Mutex<BTreeMap<PathBuf, u64>> = Mutex::new(BTreeMap::new());

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

/// The directories that hold a name opening a store at `dir` creates: each
/// missing directory's parent. Nothing else syncs them, so the first frame's
/// sync does (theseus-gf00).
fn holders_of_new_dirs(dir: &Path) -> Vec<PathBuf> {
    dir.ancestors()
        .take_while(|a| !a.as_os_str().is_empty() && !a.exists())
        .map(|made| match made.parent() {
            Some(p) if !p.as_os_str().is_empty() => p.to_path_buf(),
            _ => PathBuf::from("."),
        })
        .collect()
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
                    #[cfg(test)]
                    {
                        let mut held = HELD_TRIES.lock().unwrap();
                        *held.entry(dir.to_path_buf()).or_default() += 1;
                    }
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
        let name_dirs = holders_of_new_dirs(dir);
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
        // The checkpoint claims only synced positions (it takes `appending`
        // alone, and the writer indexes a batch only after its sync), so a
        // frame at or before it that does not check went bad after it was
        // written: the open refuses it, not cuts it (theseus-gt12).
        let wal_cfg = WalConfig {
            synced_to: wal_cfg.synced_to.max(cp),
            ..wal_cfg
        };
        let (wal, missing) =
            Wal::open_from(&dir.join("wal"), wal_cfg, cp, at).context("opening WAL")?;
        wal.sync_with_first_frame(name_dirs);
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
            checkpoint_hold: std::sync::Mutex::default(),
            refused: Mutex::new((BTreeSet::new(), 0)),
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
        {
            let hold = self.checkpoint_hold.lock().unwrap().take();
            if let Some(hold) = hold {
                let _ = hold.recv();
            }
        }
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
    /// caller's lock still spans its read to its frame indexed (K1). The
    /// periodic checkpoint runs here too, after the answers (theseus-avvb):
    /// no append's call pays it.
    fn write_loop(&self, jobs: &mpsc::Receiver<Job>) {
        while let Ok(first) = jobs.recv() {
            {
                // A checkpoint, which takes this alone, never claims a frame
                // written and not yet synced and indexed. What queues while
                // the writer waits for it joins this batch.
                let _appending = self.appending.read().unwrap();
                let mut batch = vec![first];
                batch.extend(jobs.try_iter());
                self.commit(batch);
            }
            self.checkpoint_if_due();
        }
    }

    /// The periodic checkpoint, once `checkpoint_every` records were written
    /// since the last: on the writer, after it has answered the batch that
    /// crossed the mark, so no append's call pays it. An append that queues
    /// meanwhile waits for it, as it would wherever the checkpoint ran: it
    /// takes `appending` alone, and redb's one write transaction.
    fn checkpoint_if_due(&self) {
        let every = self.checkpoint_every.load(Ordering::Relaxed);
        if every == 0 || self.since_checkpoint.load(Ordering::Relaxed) < every {
            return;
        }
        let t = std::time::Instant::now();
        match self.checkpoint_as(true) {
            Ok(at) => tracing::debug!(
                position = at,
                ms = t.elapsed().as_secs_f64() * 1000.0,
                "store: periodic checkpoint"
            ),
            Err(e) => tracing::warn!(
                error = %format!("{e:#}"),
                "store: the periodic checkpoint failed; the next open replays from the one before"
            ),
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
    /// not know is skipped, as `read` would skip it, and so is a record whose
    /// read is refused (`listed`).
    fn read_many(&self, positions: &[u64]) -> Result<Vec<Record>> {
        let mut out = Vec::with_capacity(positions.len());
        for (p, loc) in positions.iter().zip(self.index.locations(positions)?) {
            if let Some(loc) = loc {
                out.extend(self.listed(*p, loc)?);
            }
        }
        Ok(out)
    }

    /// A list read's record at `position` (R4, theseus-15g). A record whose
    /// read is refused (its frame is corrupt) is skipped, logged once, and
    /// counted for health, so one bad record no longer fails every list
    /// that reaches it: the driver's open executions, health, the session
    /// list, a transcript, the ledger's tail. A read of that record by
    /// itself (`get`, `latest_by_key`) is still refused. Any other failure
    /// fails the read.
    fn listed(&self, position: u64, loc: RecordLocation) -> Result<Option<Record>> {
        match self.wal.read_at(loc) {
            Ok(r) => checked(position, r).map(Some),
            Err(e @ WalError::Corrupt { .. }) => {
                let mut refused = self.refused.lock().unwrap();
                let (kept, past) = &mut *refused;
                let new = if kept.len() < REFUSED_KEPT {
                    kept.insert(position)
                } else {
                    !kept.contains(&position) && {
                        *past += 1;
                        true
                    }
                };
                if new {
                    tracing::warn!(position, error = %e,
                        "store: a list read skipped a record whose read is refused; health counts it");
                }
                Ok(None)
            }
            Err(e) => Err(e.into()),
        }
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
            answered
                .recv()
                .map_err(|_| anyhow::anyhow!("the store's writer stopped before it answered"))?
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
            if let Some(loc) = self.inner.index.location(p)? {
                out.extend(self.inner.listed(p, loc)?);
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

    fn of_kind_after(&self, kind: RecordKind, after: u64, limit: usize) -> Result<Vec<Record>> {
        let positions = self
            .inner
            .index
            .positions_of_kind_after(kind, after, limit)?;
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
        let (refused_records, refused_positions) = {
            let refused = s.refused.lock().unwrap();
            let (kept, past) = &*refused;
            (
                kept.len() as u64 + past,
                kept.iter().take(REFUSED_SHOWN).copied().collect(),
            )
        };
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
            refused_records,
            refused_positions,
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
mod tests;
