//! The index: a rebuildable projection of the WAL in an embedded store.
//! Two engines behind one trait, chosen by config and by the M1 benchmark.
//!
//! Tables (all keys big-endian so lexical order is numeric order):
//! - `loc`:    position u64 → RecordLocation
//! - `bykey`:  kind u16 ‖ key bytes → latest position u64
//! - `bykind`: kind u16 ‖ position u64 → () (per-kind ordered scans)
//! - `byscope`: scope bytes ‖ 0x00 ‖ position u64 → () (per-session ordered scans, §4.4b)
//! - `meta`:   "checkpoint" → position u64
//!
//! Writes are non-durable by default; `flush_durable` + `set_checkpoint` make
//! them durable together. Startup replays the WAL past the checkpoint.

use std::path::Path;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::record::RecordKind;
pub use crate::wal::RecordLocation as Location;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Engine {
    Redb,
    Fjall,
}

impl Engine {
    pub fn as_str(self) -> &'static str {
        match self {
            Engine::Redb => "redb",
            Engine::Fjall => "fjall",
        }
    }
}

impl std::str::FromStr for Engine {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, String> {
        match s.to_ascii_lowercase().as_str() {
            "redb" => Ok(Engine::Redb),
            "fjall" => Ok(Engine::Fjall),
            other => Err(format!("unknown store engine {other:?} (redb | fjall)")),
        }
    }
}

/// One record's index entries.
#[derive(Debug, Clone)]
pub struct IndexEntry {
    pub position: u64,
    pub kind: RecordKind,
    pub key: Option<String>,
    pub scope: Option<String>,
    pub loc: Location,
}

pub trait Index: Send + Sync {
    fn engine(&self) -> Engine;
    /// Record a batch of entries. Non-durable unless `durable`.
    fn apply(&self, entries: &[IndexEntry], durable: bool) -> Result<()>;
    fn location(&self, position: u64) -> Result<Option<Location>>;
    fn latest_position(&self, kind: RecordKind, key: &str) -> Result<Option<u64>>;
    /// (key, latest position) for every key of a kind, key order.
    fn keys_of_kind(&self, kind: RecordKind) -> Result<Vec<(String, u64)>>;
    /// Positions of a kind, newest first, at most `limit`.
    fn positions_of_kind_rev(&self, kind: RecordKind, limit: usize) -> Result<Vec<u64>>;
    fn count_of_kind(&self, kind: RecordKind) -> Result<u64>;
    /// Positions in a scope with position > `after`, oldest first, at most `limit`.
    fn positions_in_scope(&self, scope: &str, after: u64, limit: usize) -> Result<Vec<u64>>;
    fn count_in_scope(&self, scope: &str) -> Result<u64>;
    fn checkpoint(&self) -> Result<Option<u64>>;
    /// Make everything durable and record the checkpoint position.
    fn set_checkpoint(&self, position: u64) -> Result<()>;
}

fn bykey(kind: RecordKind, key: &str) -> Vec<u8> {
    let mut v = Vec::with_capacity(2 + key.len());
    v.extend_from_slice(&kind.to_be_bytes());
    v.extend_from_slice(key.as_bytes());
    v
}
fn bykind(kind: RecordKind, pos: u64) -> [u8; 10] {
    let mut v = [0u8; 10];
    v[..2].copy_from_slice(&kind.to_be_bytes());
    v[2..].copy_from_slice(&pos.to_be_bytes());
    v
}
fn byscope(scope: &str, pos: u64) -> Vec<u8> {
    let mut v = Vec::with_capacity(scope.len() + 9);
    v.extend_from_slice(scope.as_bytes());
    v.push(0);
    v.extend_from_slice(&pos.to_be_bytes());
    v
}
fn scope_prefix(scope: &str) -> Vec<u8> {
    let mut v = Vec::with_capacity(scope.len() + 1);
    v.extend_from_slice(scope.as_bytes());
    v.push(0);
    v
}
fn loc_bytes(l: Location) -> [u8; 16] {
    let mut v = [0u8; 16];
    v[..4].copy_from_slice(&l.segment.to_be_bytes());
    v[4..12].copy_from_slice(&l.offset.to_be_bytes());
    v[12..].copy_from_slice(&l.len.to_be_bytes());
    v
}
fn loc_from(b: &[u8]) -> Option<Location> {
    if b.len() != 16 {
        return None;
    }
    Some(Location {
        segment: u32::from_be_bytes(b[..4].try_into().ok()?),
        offset: u64::from_be_bytes(b[4..12].try_into().ok()?),
        len: u32::from_be_bytes(b[12..].try_into().ok()?),
    })
}
fn u64_from(b: &[u8]) -> Option<u64> {
    Some(u64::from_be_bytes(b.try_into().ok()?))
}

pub fn open(engine: Engine, dir: &Path) -> Result<Box<dyn Index>> {
    Ok(match engine {
        Engine::Redb => Box::new(redb_index::RedbIndex::open(&dir.join("index.redb"))?),
        Engine::Fjall => Box::new(fjall_index::FjallIndex::open(&dir.join("index.fjall"))?),
    })
}

// ------------------------------------------------------------------ redb

mod redb_index {
    use super::*;
    use redb::{Database, Durability, ReadableDatabase, TableDefinition};

    const LOC: TableDefinition<u64, &[u8]> = TableDefinition::new("loc");
    const BYKEY: TableDefinition<&[u8], u64> = TableDefinition::new("bykey");
    const BYKIND: TableDefinition<&[u8], ()> = TableDefinition::new("bykind");
    const BYSCOPE: TableDefinition<&[u8], ()> = TableDefinition::new("byscope");
    const META: TableDefinition<&str, u64> = TableDefinition::new("meta");

    pub struct RedbIndex {
        db: Database,
    }

    impl RedbIndex {
        pub fn open(path: &Path) -> Result<Self> {
            let db =
                Database::create(path).with_context(|| format!("opening {}", path.display()))?;
            let txn = db.begin_write()?;
            {
                txn.open_table(LOC)?;
                txn.open_table(BYKEY)?;
                txn.open_table(BYKIND)?;
                txn.open_table(BYSCOPE)?;
                txn.open_table(META)?;
            }
            txn.commit()?;
            Ok(Self { db })
        }
    }

    impl Index for RedbIndex {
        fn engine(&self) -> Engine {
            Engine::Redb
        }
        fn apply(&self, entries: &[IndexEntry], durable: bool) -> Result<()> {
            let mut txn = self.db.begin_write()?;
            txn.set_durability(if durable {
                Durability::Immediate
            } else {
                Durability::None
            })?;
            {
                let mut loc = txn.open_table(LOC)?;
                let mut byk = txn.open_table(BYKEY)?;
                let mut bkd = txn.open_table(BYKIND)?;
                let mut bsc = txn.open_table(BYSCOPE)?;
                for e in entries {
                    loc.insert(e.position, loc_bytes(e.loc).as_slice())?;
                    if let Some(k) = &e.key {
                        byk.insert(bykey(e.kind, k).as_slice(), e.position)?;
                    }
                    bkd.insert(bykind(e.kind, e.position).as_slice(), ())?;
                    if let Some(sc) = &e.scope {
                        bsc.insert(byscope(sc, e.position).as_slice(), ())?;
                    }
                }
            }
            txn.commit()?;
            Ok(())
        }
        fn location(&self, position: u64) -> Result<Option<Location>> {
            let txn = self.db.begin_read()?;
            let t = txn.open_table(LOC)?;
            Ok(t.get(position)?.and_then(|v| loc_from(v.value())))
        }
        fn latest_position(&self, kind: RecordKind, key: &str) -> Result<Option<u64>> {
            let txn = self.db.begin_read()?;
            let t = txn.open_table(BYKEY)?;
            Ok(t.get(bykey(kind, key).as_slice())?.map(|v| v.value()))
        }
        fn keys_of_kind(&self, kind: RecordKind) -> Result<Vec<(String, u64)>> {
            let txn = self.db.begin_read()?;
            let t = txn.open_table(BYKEY)?;
            let lo = kind.to_be_bytes().to_vec();
            let hi = (kind + 1).to_be_bytes().to_vec();
            let mut out = Vec::new();
            for row in t.range(lo.as_slice()..hi.as_slice())? {
                let (k, v) = row?;
                let kb = k.value();
                out.push((String::from_utf8_lossy(&kb[2..]).into_owned(), v.value()));
            }
            Ok(out)
        }
        fn positions_of_kind_rev(&self, kind: RecordKind, limit: usize) -> Result<Vec<u64>> {
            let txn = self.db.begin_read()?;
            let t = txn.open_table(BYKIND)?;
            let lo = bykind(kind, 0);
            let hi = bykind(kind, u64::MAX);
            let mut out = Vec::new();
            for row in t.range(lo.as_slice()..=hi.as_slice())?.rev().take(limit) {
                let (k, _) = row?;
                if let Some(p) = u64_from(&k.value()[2..]) {
                    out.push(p);
                }
            }
            Ok(out)
        }
        fn count_of_kind(&self, kind: RecordKind) -> Result<u64> {
            let txn = self.db.begin_read()?;
            let t = txn.open_table(BYKIND)?;
            let lo = bykind(kind, 0);
            let hi = bykind(kind, u64::MAX);
            Ok(t.range(lo.as_slice()..=hi.as_slice())?.count() as u64)
        }
        fn positions_in_scope(&self, scope: &str, after: u64, limit: usize) -> Result<Vec<u64>> {
            let txn = self.db.begin_read()?;
            let t = txn.open_table(BYSCOPE)?;
            let lo = byscope(scope, after.saturating_add(1));
            let hi = byscope(scope, u64::MAX);
            let mut out = Vec::new();
            for row in t.range(lo.as_slice()..=hi.as_slice())?.take(limit) {
                let (k, _) = row?;
                let kb = k.value();
                if let Some(p) = u64_from(&kb[kb.len() - 8..]) {
                    out.push(p);
                }
            }
            Ok(out)
        }
        fn count_in_scope(&self, scope: &str) -> Result<u64> {
            let txn = self.db.begin_read()?;
            let t = txn.open_table(BYSCOPE)?;
            let lo = byscope(scope, 0);
            let hi = byscope(scope, u64::MAX);
            Ok(t.range(lo.as_slice()..=hi.as_slice())?.count() as u64)
        }
        fn checkpoint(&self) -> Result<Option<u64>> {
            let txn = self.db.begin_read()?;
            let t = txn.open_table(META)?;
            Ok(t.get("checkpoint")?.map(|v| v.value()))
        }
        fn set_checkpoint(&self, position: u64) -> Result<()> {
            let mut txn = self.db.begin_write()?;
            txn.set_durability(Durability::Immediate)?;
            {
                let mut t = txn.open_table(META)?;
                t.insert("checkpoint", position)?;
            }
            txn.commit()?;
            Ok(())
        }
    }
}

// ----------------------------------------------------------------- fjall

mod fjall_index {
    use super::*;
    use fjall::{Database, Keyspace, KeyspaceCreateOptions, PersistMode};

    pub struct FjallIndex {
        db: Database,
        loc: Keyspace,
        bykey: Keyspace,
        bykind: Keyspace,
        byscope: Keyspace,
        meta: Keyspace,
    }

    impl FjallIndex {
        pub fn open(path: &Path) -> Result<Self> {
            let db = Database::builder(path)
                .manual_journal_persist(true)
                .open()
                .with_context(|| format!("opening {}", path.display()))?;
            let loc = db.keyspace("loc", KeyspaceCreateOptions::default)?;
            let bykey = db.keyspace("bykey", KeyspaceCreateOptions::default)?;
            let bykind = db.keyspace("bykind", KeyspaceCreateOptions::default)?;
            let byscope = db.keyspace("byscope", KeyspaceCreateOptions::default)?;
            let meta = db.keyspace("meta", KeyspaceCreateOptions::default)?;
            Ok(Self {
                db,
                loc,
                bykey,
                bykind,
                byscope,
                meta,
            })
        }
    }

    impl Index for FjallIndex {
        fn engine(&self) -> Engine {
            Engine::Fjall
        }
        fn apply(&self, entries: &[IndexEntry], durable: bool) -> Result<()> {
            let mut b = self.db.batch();
            for e in entries {
                b.insert(&self.loc, e.position.to_be_bytes(), loc_bytes(e.loc));
                if let Some(k) = &e.key {
                    b.insert(&self.bykey, bykey(e.kind, k), e.position.to_be_bytes());
                }
                b.insert(&self.bykind, bykind(e.kind, e.position), []);
                if let Some(sc) = &e.scope {
                    b.insert(&self.byscope, byscope(sc, e.position), []);
                }
            }
            let b = b.durability(if durable {
                Some(PersistMode::SyncAll)
            } else {
                None
            });
            b.commit()?;
            Ok(())
        }
        fn location(&self, position: u64) -> Result<Option<Location>> {
            Ok(self
                .loc
                .get(position.to_be_bytes())?
                .and_then(|v| loc_from(&v)))
        }
        fn latest_position(&self, kind: RecordKind, key: &str) -> Result<Option<u64>> {
            Ok(self.bykey.get(bykey(kind, key))?.and_then(|v| u64_from(&v)))
        }
        fn keys_of_kind(&self, kind: RecordKind) -> Result<Vec<(String, u64)>> {
            let mut out = Vec::new();
            for g in self.bykey.prefix(kind.to_be_bytes()) {
                let (k, v) = g.into_inner()?;
                if let Some(p) = u64_from(&v) {
                    out.push((String::from_utf8_lossy(&k[2..]).into_owned(), p));
                }
            }
            Ok(out)
        }
        fn positions_of_kind_rev(&self, kind: RecordKind, limit: usize) -> Result<Vec<u64>> {
            let mut out = Vec::new();
            for g in self.bykind.prefix(kind.to_be_bytes()).rev().take(limit) {
                let k = g.key()?;
                if let Some(p) = u64_from(&k[2..]) {
                    out.push(p);
                }
            }
            Ok(out)
        }
        fn count_of_kind(&self, kind: RecordKind) -> Result<u64> {
            Ok(self.bykind.prefix(kind.to_be_bytes()).count() as u64)
        }
        fn positions_in_scope(&self, scope: &str, after: u64, limit: usize) -> Result<Vec<u64>> {
            let lo = byscope(scope, after.saturating_add(1));
            let hi = byscope(scope, u64::MAX);
            let mut out = Vec::new();
            for g in self.byscope.range(lo..=hi).take(limit) {
                let k = g.key()?;
                if let Some(p) = u64_from(&k[k.len() - 8..]) {
                    out.push(p);
                }
            }
            Ok(out)
        }
        fn count_in_scope(&self, scope: &str) -> Result<u64> {
            Ok(self.byscope.prefix(scope_prefix(scope)).count() as u64)
        }
        fn checkpoint(&self) -> Result<Option<u64>> {
            Ok(self.meta.get("checkpoint")?.and_then(|v| u64_from(&v)))
        }
        fn set_checkpoint(&self, position: u64) -> Result<()> {
            self.meta.insert("checkpoint", position.to_be_bytes())?;
            self.db.persist(PersistMode::SyncAll)?;
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn exercise(engine: Engine) {
        let dir = tempfile::tempdir().unwrap();
        let idx = open(engine, dir.path()).unwrap();
        assert_eq!(idx.engine(), engine);
        let e = |p: u64, kind: u16, key: Option<&str>| IndexEntry {
            position: p,
            kind,
            key: key.map(str::to_string),
            scope: if p % 2 == 1 {
                Some("ses_a".into())
            } else {
                Some("ses_b".into())
            },
            loc: Location {
                segment: 1,
                offset: p * 100,
                len: 10,
            },
        };
        idx.apply(
            &[
                e(1, 1, Some("s1")),
                e(2, 2, None),
                e(3, 1, Some("s2")),
                e(4, 1, Some("s1")),
                e(5, 2, None),
            ],
            false,
        )
        .unwrap();
        assert_eq!(idx.location(3).unwrap().unwrap().offset, 300);
        assert_eq!(idx.latest_position(1, "s1").unwrap(), Some(4));
        assert_eq!(idx.latest_position(1, "nope").unwrap(), None);
        let keys = idx.keys_of_kind(1).unwrap();
        assert_eq!(keys, vec![("s1".to_string(), 4), ("s2".to_string(), 3)]);
        assert_eq!(idx.positions_of_kind_rev(2, 10).unwrap(), vec![5, 2]);
        assert_eq!(idx.positions_of_kind_rev(1, 2).unwrap(), vec![4, 3]);
        assert_eq!(idx.count_of_kind(1).unwrap(), 3);
        assert_eq!(
            idx.positions_in_scope("ses_a", 0, 10).unwrap(),
            vec![1, 3, 5]
        );
        assert_eq!(idx.positions_in_scope("ses_a", 1, 10).unwrap(), vec![3, 5]);
        assert_eq!(idx.positions_in_scope("ses_b", 0, 1).unwrap(), vec![2]);
        assert_eq!(
            idx.positions_in_scope("ses_", 0, 10).unwrap(),
            Vec::<u64>::new()
        );
        assert_eq!(idx.count_in_scope("ses_b").unwrap(), 2);
        assert_eq!(idx.checkpoint().unwrap(), None);
        idx.set_checkpoint(5).unwrap();
        assert_eq!(idx.checkpoint().unwrap(), Some(5));
        drop(idx);
        let idx = open(engine, dir.path()).unwrap();
        assert_eq!(idx.checkpoint().unwrap(), Some(5));
        assert_eq!(idx.latest_position(1, "s1").unwrap(), Some(4));
    }

    #[test]
    fn redb_index() {
        exercise(Engine::Redb);
    }

    #[test]
    fn fjall_index() {
        exercise(Engine::Fjall);
    }
}
