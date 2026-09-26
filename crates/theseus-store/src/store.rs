//! The `Store` contract and `WalStore`, which composes the WAL (truth) with an
//! index (cache) and rebuilds the index from the WAL past the last checkpoint
//! on open.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::index::{self, Engine, Index, IndexEntry};
use crate::record::{kinds, NewRecord, Record, RecordKind};
use crate::wal::{Recovery, Wal, WalConfig};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoreStats {
    pub engine: Engine,
    pub last_position: u64,
    pub checkpoint: Option<u64>,
    pub wal_bytes: u64,
    pub wal_segments: u32,
    pub recovered_records: u64,
    pub truncated_bytes: u64,
    pub replayed_into_index: u64,
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
    fn last_position(&self) -> u64;
    /// Make the index durable and record how far it is good.
    fn checkpoint(&self) -> Result<u64>;
    fn stats(&self) -> Result<StoreStats>;
}

pub struct WalStore {
    wal: Wal,
    index: Box<dyn Index>,
    dir: PathBuf,
    replayed: AtomicU64,
    /// Checkpoint every N appended records (0 = manual only).
    checkpoint_every: u64,
    since_checkpoint: AtomicU64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Manifest {
    format: u32,
    engine: Engine,
}

impl WalStore {
    /// Open or create a store in `dir` with the given engine. If the directory
    /// already holds a store, its manifest's engine wins and a mismatch is an
    /// error (you do not silently switch engines under live data).
    pub fn open(dir: &Path, engine: Engine, wal_cfg: WalConfig) -> Result<Self> {
        std::fs::create_dir_all(dir)?;
        let manifest_path = dir.join("MANIFEST.json");
        let engine = if manifest_path.exists() {
            let m: Manifest = serde_json::from_slice(&std::fs::read(&manifest_path)?)
                .context("reading store manifest")?;
            if m.engine != engine {
                anyhow::bail!(
                    "store at {} was created with engine {} but {} was requested",
                    dir.display(),
                    m.engine.as_str(),
                    engine.as_str()
                );
            }
            m.engine
        } else {
            let m = Manifest { format: 1, engine };
            let tmp = dir.join("MANIFEST.json.tmp");
            std::fs::write(&tmp, serde_json::to_vec_pretty(&m)?)?;
            std::fs::rename(&tmp, &manifest_path)?;
            engine
        };

        let wal = Wal::open(&dir.join("wal"), wal_cfg).context("opening WAL")?;
        let index = index::open(engine, dir).context("opening index")?;

        // Rebuild whatever the index lost since its checkpoint.
        let cp = index.checkpoint()?.unwrap_or(0);
        let missing = wal.replay_from(cp)?;
        let replayed = missing.len() as u64;
        if !missing.is_empty() {
            let entries: Vec<IndexEntry> = missing
                .iter()
                .map(|(r, loc)| IndexEntry {
                    position: r.position,
                    kind: r.kind,
                    key: r.key.clone(),
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
        };
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

    pub fn engine(&self) -> Engine {
        self.index.engine()
    }

    fn read(&self, position: u64) -> Result<Option<Record>> {
        match self.index.location(position)? {
            Some(loc) => Ok(Some(self.wal.read_at(loc)?)),
            None => Ok(None),
        }
    }
}

impl Store for WalStore {
    fn append(&self, batch: &[NewRecord]) -> Result<Vec<u64>> {
        let placed = self.wal.append(batch)?;
        let entries: Vec<IndexEntry> = placed
            .iter()
            .zip(batch)
            .map(|((pos, loc), r)| IndexEntry {
                position: *pos,
                kind: r.kind,
                key: r.key.clone(),
                loc: *loc,
            })
            .collect();
        self.index.apply(&entries, false)?;
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
        let mut out = Vec::new();
        for (_, p) in self.index.keys_of_kind(kind)? {
            if let Some(r) = self.read(p)? {
                out.push(r);
            }
        }
        Ok(out)
    }

    fn tail_of_kind(&self, kind: RecordKind, n: usize) -> Result<Vec<Record>> {
        let mut positions = self.index.positions_of_kind_rev(kind, n)?;
        positions.reverse();
        let mut out = Vec::with_capacity(positions.len());
        for p in positions {
            if let Some(r) = self.read(p)? {
                out.push(r);
            }
        }
        Ok(out)
    }

    fn count_of_kind(&self, kind: RecordKind) -> Result<u64> {
        self.index.count_of_kind(kind)
    }

    fn last_position(&self) -> u64 {
        self.wal.last_position()
    }

    fn checkpoint(&self) -> Result<u64> {
        let last = self.wal.last_position();
        self.index.set_checkpoint(last)?;
        self.since_checkpoint.store(0, Ordering::Relaxed);
        Ok(last)
    }

    fn stats(&self) -> Result<StoreStats> {
        let r = self.wal.recovery();
        Ok(StoreStats {
            engine: self.index.engine(),
            last_position: self.wal.last_position(),
            checkpoint: self.index.checkpoint()?,
            wal_bytes: self.wal.total_bytes(),
            wal_segments: self.wal.segment_count(),
            recovered_records: r.records,
            truncated_bytes: r.truncated_bytes,
            replayed_into_index: self.replayed.load(Ordering::Relaxed),
        })
    }
}

/// A checkpoint marker is also a record, so the log itself says when the index was good.
pub fn checkpoint_record(position: u64) -> NewRecord {
    NewRecord::bytes(kinds::CHECKPOINT, None, position.to_le_bytes().to_vec())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn open(dir: &Path, engine: Engine) -> WalStore {
        WalStore::open(dir, engine, WalConfig::default())
            .unwrap()
            .with_checkpoint_every(0)
    }

    fn exercise(engine: Engine) {
        let dir = tempfile::tempdir().unwrap();
        let s = open(dir.path(), engine);
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

        // Checkpoint, append more (index non-durable), reopen: replay rebuilds.
        s.checkpoint().unwrap();
        s.append(&[NewRecord::json(kinds::META, Some("live"), &"glm").unwrap()])
            .unwrap();
        drop(s);
        let s = open(dir.path(), engine);
        assert_eq!(s.last_position(), 6);
        let st = s.stats().unwrap();
        assert!(st.replayed_into_index <= 6);
        assert_eq!(
            s.latest_by_key(kinds::META, "live")
                .unwrap()
                .unwrap()
                .decode::<String>()
                .unwrap(),
            "glm"
        );
        // Engine mismatch is refused.
        let other = if engine == Engine::Redb {
            Engine::Fjall
        } else {
            Engine::Redb
        };
        assert!(WalStore::open(dir.path(), other, WalConfig::default()).is_err());
    }

    #[test]
    fn redb_store() {
        exercise(Engine::Redb);
    }

    #[test]
    fn fjall_store() {
        exercise(Engine::Fjall);
    }

    #[test]
    fn index_loss_is_rebuilt_from_wal() {
        let dir = tempfile::tempdir().unwrap();
        let s = open(dir.path(), Engine::Redb);
        for i in 0..50u32 {
            s.append(&[NewRecord::json(kinds::LEDGER, None, &i).unwrap()])
                .unwrap();
        }
        s.append(&[NewRecord::json(kinds::SESSION, Some("s"), &"v1").unwrap()])
            .unwrap();
        drop(s);
        // Destroy the index entirely; the WAL is the truth.
        std::fs::remove_file(dir.path().join("index.redb")).unwrap();
        let s = open(dir.path(), Engine::Redb);
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
}
