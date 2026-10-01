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

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::RwLock;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::index::{Aside, Engine, IndexEntry, MovedAside, RedbIndex};
use crate::record::{kinds, NewRecord, Record, RecordKind};
use crate::wal::{History, Recovery, Wal, WalConfig};

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
    /// Latest record for every key of a kind.
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
}

pub struct WalStore {
    wal: Wal,
    index: RedbIndex,
    dir: PathBuf,
    replayed: AtomicU64,
    /// Checkpoint every N appended records (0 = manual only).
    checkpoint_every: u64,
    since_checkpoint: AtomicU64,
    /// The newest schema written per kind, as the manifest says; an append
    /// of a newer one rewrites the manifest first (F4a).
    marks: RwLock<BTreeMap<RecordKind, u16>>,
    /// Whether the manifest's rewrite is synced (the WAL's `fsync`).
    fsync: bool,
    /// Held shared by each append from its WAL write to its index write, and
    /// alone by a checkpoint: the position a checkpoint claims is then synced
    /// and indexed, which a tail-only open relies on (theseus-8ni).
    appending: RwLock<()>,
    /// How long open waited for another process to release the store.
    lock_wait_us: u64,
    /// The index file open moved aside, if it did (theseus-0b8).
    moved_aside: Option<MovedAside>,
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
    let e = match RedbIndex::open(path) {
        Ok(index) => return Ok((index, None)),
        Err(e) if RedbIndex::not_a_database(&e) => e,
        Err(e) => return Err(e.context("opening index")),
    };
    let why = e.root_cause().to_string();
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

    /// `open`, waiting at most `wait` for another process to release the
    /// store.
    pub fn open_waiting(dir: &Path, wal_cfg: WalConfig, wait: std::time::Duration) -> Result<Self> {
        let t0 = std::time::Instant::now();
        // When the try under way began: zero for the first.
        let mut began = std::time::Duration::ZERO;
        loop {
            match Self::open_once(dir, wal_cfg.clone()) {
                Ok(mut store) => {
                    store.lock_wait_us = began.as_micros() as u64;
                    if !began.is_zero() {
                        tracing::info!(
                            waited_ms = began.as_millis() as u64,
                            "store: waited for the previous process to release it"
                        );
                    }
                    return Ok(store);
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

    fn open_once(dir: &Path, wal_cfg: WalConfig) -> Result<Self> {
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
                })
                .collect();
            index.apply(&entries, false)?;
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
        let store = Self {
            wal,
            index,
            dir: dir.to_path_buf(),
            replayed: AtomicU64::new(replayed),
            checkpoint_every: 1000,
            since_checkpoint: AtomicU64::new(0),
            marks: RwLock::new(marks),
            fsync,
            appending: RwLock::new(()),
            lock_wait_us: 0,
            moved_aside,
        };
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
            store.checkpoint()?;
        }
        Ok(store)
    }

    pub fn with_checkpoint_every(mut self, n: u64) -> Self {
        self.checkpoint_every = n;
        self
    }

    pub fn recovery(&self) -> &Recovery {
        self.wal.recovery()
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// The newest schema written for each kind, as the manifest records it.
    pub fn schema_marks(&self) -> BTreeMap<RecordKind, u16> {
        self.marks.read().unwrap().clone()
    }

    /// Before records of these (kind, schema) go to the WAL: if any is newer
    /// than the manifest's mark, rewrite the manifest first, durably, with
    /// every kind this build writes at its schema (one rewrite per upgrade),
    /// so an older build refuses the store before it can read the record.
    fn mark(&self, recs: &[(RecordKind, u16)]) -> Result<()> {
        let newer = |m: &BTreeMap<RecordKind, u16>| {
            recs.iter()
                .any(|(k, s)| m.get(k).copied().unwrap_or(0) < *s)
        };
        if !newer(&self.marks.read().unwrap()) {
            return Ok(());
        }
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

    /// The full check the open leaves out (theseus-8ni): every frame before
    /// where open began checking, read-only. `pace` is called after each
    /// stretch with the time it took, for a tender that keeps to its share
    /// of a core. A corrupt frame is returned, and reads from it are refused
    /// from then on.
    pub fn verify_history(
        &self,
        pace: impl FnMut(std::time::Duration),
    ) -> std::result::Result<History, crate::wal::WalError> {
        self.wal.verify_history(pace)
    }

    /// The same check, apart from the store: a thread that runs it keeps
    /// neither the store nor its index open, so a stopping daemon's store
    /// still closes cleanly.
    pub fn history_check(&self) -> crate::wal::HistoryCheck {
        self.wal.history_check()
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
    fn append(&self, batch: &[NewRecord]) -> Result<Vec<u64>> {
        let newer = {
            let m = self.marks.read().unwrap();
            batch
                .iter()
                .any(|r| m.get(&r.kind).copied().unwrap_or(0) < r.schema)
        };
        if newer {
            let schemas: Vec<(RecordKind, u16)> =
                batch.iter().map(|r| (r.kind, r.schema)).collect();
            self.mark(&schemas)?;
        }
        let placed = {
            let _appending = self.appending.read().unwrap();
            let placed = self.wal.append(batch)?;
            let entries: Vec<IndexEntry> = placed
                .iter()
                .zip(batch)
                .map(|((pos, loc), r)| IndexEntry {
                    position: *pos,
                    kind: r.kind,
                    key: r.key.clone(),
                    scope: r.scope.clone(),
                    loc: *loc,
                })
                .collect();
            self.index.apply(&entries, false)?;
            placed
        };
        let n = self
            .since_checkpoint
            .fetch_add(batch.len() as u64, Ordering::Relaxed)
            + batch.len() as u64;
        if self.checkpoint_every > 0 && n >= self.checkpoint_every {
            self.checkpoint()?;
        }
        Ok(placed.into_iter().map(|(p, _)| p).collect())
    }

    fn get(&self, position: u64) -> Result<Option<Record>> {
        self.read(position)
    }

    fn scan(&self, from: u64, to: Option<u64>, limit: usize) -> Result<Vec<Record>> {
        let last = self.wal.last_position();
        let to = to.unwrap_or(last).min(last);
        let mut out = Vec::new();
        let mut p = from.max(1);
        while p <= to && out.len() < limit {
            if let Some(r) = self.read(p)? {
                out.push(r);
            }
            p += 1;
        }
        Ok(out)
    }

    fn latest_by_key(&self, kind: RecordKind, key: &str) -> Result<Option<Record>> {
        match self.index.latest_position(kind, key)? {
            Some(p) => self.read(p),
            None => Ok(None),
        }
    }

    fn latest_of_kind(&self, kind: RecordKind) -> Result<Vec<Record>> {
        let positions: Vec<u64> = self
            .index
            .keys_of_kind(kind)?
            .into_iter()
            .map(|(_, p)| p)
            .collect();
        self.read_many(&positions)
    }

    fn tail_of_kind(&self, kind: RecordKind, n: usize) -> Result<Vec<Record>> {
        let mut positions = self.index.positions_of_kind_rev(kind, n)?;
        positions.reverse();
        self.read_many(&positions)
    }

    fn count_of_kind(&self, kind: RecordKind) -> Result<u64> {
        self.index.count_of_kind(kind)
    }

    fn scan_scope(&self, scope: &str, after: u64, limit: usize) -> Result<Vec<Record>> {
        let positions = self.index.positions_in_scope(scope, after, limit)?;
        self.read_many(&positions)
    }

    fn count_in_scope(&self, scope: &str) -> Result<u64> {
        self.index.count_in_scope(scope)
    }

    fn last_position(&self) -> u64 {
        self.wal.last_position()
    }

    fn checkpoint(&self) -> Result<u64> {
        // No append between its WAL write and its index write: every frame
        // up to `last` is synced and indexed.
        let _alone = self.appending.write().unwrap();
        let last = self.wal.last_position();
        self.index.set_checkpoint(last)?;
        self.since_checkpoint.store(0, Ordering::Relaxed);
        Ok(last)
    }

    fn stats(&self) -> Result<StoreStats> {
        let r = self.wal.recovery();
        Ok(StoreStats {
            last_position: self.wal.last_position(),
            checkpoint: self.index.checkpoint()?,
            wal_bytes: self.wal.total_bytes(),
            wal_segments: self.wal.segment_count(),
            recovered_records: r.records,
            truncated_bytes: r.truncated_bytes,
            replayed_into_index: self.replayed.load(Ordering::Relaxed),
            frames_appended: self.wal.frames_appended(),
            syncs: self.wal.syncs(),
            history_bytes: r.history_bytes,
            index_repaired: self.index.repaired(),
            lock_wait_us: self.lock_wait_us,
            index_moved_aside: self.moved_aside.clone(),
        })
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
        let shapes: [(&str, Damage); 3] = [
            ("a 37-byte partial header", |real| real[..37].to_vec()),
            ("a few random bytes", |_| {
                vec![
                    0x5c, 0x91, 0x07, 0xee, 0x30, 0x2a, 0xd4, 0x18, 0x66, 0x0b, 0xf3,
                ]
            }),
            ("zeros the length of a new index", |real| {
                vec![0u8; real.len()]
            }),
        ];
        for (shape, damage) in shapes {
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
