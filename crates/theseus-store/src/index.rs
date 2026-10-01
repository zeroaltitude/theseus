//! The index: a rebuildable projection of the WAL in redb, the M1 benchmark's
//! pick (fjall, the engine it lost to, is gone: theseus-0g4).
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
use redb::{Database, Durability, ReadableDatabase, TableDefinition};
use serde::{Deserialize, Serialize};

use crate::record::RecordKind;
pub use crate::wal::RecordLocation as Location;

/// The engine a store's manifest and the config's `store_engine` name. redb is
/// the only one; either naming anything else is refused when it is read.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Engine {
    #[default]
    Redb,
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

const LOC: TableDefinition<u64, &[u8]> = TableDefinition::new("loc");
const BYKEY: TableDefinition<&[u8], u64> = TableDefinition::new("bykey");
const BYKIND: TableDefinition<&[u8], ()> = TableDefinition::new("bykind");
const BYSCOPE: TableDefinition<&[u8], ()> = TableDefinition::new("byscope");
const META: TableDefinition<&str, u64> = TableDefinition::new("meta");

pub struct RedbIndex {
    db: Database,
    repaired: bool,
}

impl RedbIndex {
    pub fn open(path: &Path) -> Result<Self> {
        // redb repairs a file its last process did not close, which costs
        // this start several syncs: say so (theseus-8ni).
        let repair = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let seen = repair.clone();
        let db = Database::builder()
            .set_repair_callback(move |_| seen.store(true, std::sync::atomic::Ordering::Relaxed))
            .create(path)
            .with_context(|| format!("opening {}", path.display()))?;
        let repaired = repair.load(std::sync::atomic::Ordering::Relaxed);
        let txn = db.begin_write()?;
        {
            txn.open_table(LOC)?;
            txn.open_table(BYKEY)?;
            txn.open_table(BYKIND)?;
            txn.open_table(BYSCOPE)?;
            txn.open_table(META)?;
        }
        txn.commit()?;
        Ok(Self { db, repaired })
    }

    /// Whether the open repaired the file: its last process did not close it.
    pub fn repaired(&self) -> bool {
        self.repaired
    }

    /// Whether `open` failed because another process has the file open: redb
    /// holds a lock on it from its open to its close (theseus-qa0 F4b).
    pub fn held_elsewhere(e: &anyhow::Error) -> bool {
        matches!(
            e.downcast_ref::<redb::DatabaseError>(),
            Some(redb::DatabaseError::DatabaseAlreadyOpen)
        )
    }

    /// Whether `open` failed because the file is not a redb database at all
    /// (theseus-0b8): its first bytes are not redb's magic number
    /// (`InvalidData`), or it ends inside the header (`UnexpectedEof`). A
    /// kill inside the store's first open, while redb writes the header,
    /// leaves such a file. Told by redb's error kind, never its text: redb
    /// takes its lock before it reads a byte, so a file another process holds
    /// is `DatabaseAlreadyOpen`, and a real database that fails some other way
    /// (a newer format, a bad commit slot, a file cut short) is `Corrupted`.
    pub fn not_a_database(e: &anyhow::Error) -> bool {
        matches!(
            e.downcast_ref::<redb::DatabaseError>(),
            Some(redb::DatabaseError::Storage(redb::StorageError::Io(io)))
                if matches!(io.kind(), std::io::ErrorKind::InvalidData | std::io::ErrorKind::UnexpectedEof)
        )
    }

    /// Record a batch of entries. Non-durable unless `durable`.
    pub fn apply(&self, entries: &[IndexEntry], durable: bool) -> Result<()> {
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

    pub fn location(&self, position: u64) -> Result<Option<Location>> {
        let txn = self.db.begin_read()?;
        let t = txn.open_table(LOC)?;
        Ok(t.get(position)?.and_then(|v| loc_from(v.value())))
    }

    /// The locations of many positions, in one read transaction.
    pub fn locations(&self, positions: &[u64]) -> Result<Vec<Option<Location>>> {
        let txn = self.db.begin_read()?;
        let t = txn.open_table(LOC)?;
        positions
            .iter()
            .map(|p| Ok(t.get(*p)?.and_then(|v| loc_from(v.value()))))
            .collect()
    }

    pub fn latest_position(&self, kind: RecordKind, key: &str) -> Result<Option<u64>> {
        let txn = self.db.begin_read()?;
        let t = txn.open_table(BYKEY)?;
        Ok(t.get(bykey(kind, key).as_slice())?.map(|v| v.value()))
    }

    /// (key, latest position) for every key of a kind, key order.
    pub fn keys_of_kind(&self, kind: RecordKind) -> Result<Vec<(String, u64)>> {
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

    /// Positions of a kind, newest first, at most `limit`.
    pub fn positions_of_kind_rev(&self, kind: RecordKind, limit: usize) -> Result<Vec<u64>> {
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

    pub fn count_of_kind(&self, kind: RecordKind) -> Result<u64> {
        let txn = self.db.begin_read()?;
        let t = txn.open_table(BYKIND)?;
        let lo = bykind(kind, 0);
        let hi = bykind(kind, u64::MAX);
        Ok(t.range(lo.as_slice()..=hi.as_slice())?.count() as u64)
    }

    /// Positions in a scope with position > `after`, oldest first, at most `limit`.
    pub fn positions_in_scope(&self, scope: &str, after: u64, limit: usize) -> Result<Vec<u64>> {
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

    pub fn count_in_scope(&self, scope: &str) -> Result<u64> {
        let txn = self.db.begin_read()?;
        let t = txn.open_table(BYSCOPE)?;
        let lo = byscope(scope, 0);
        let hi = byscope(scope, u64::MAX);
        Ok(t.range(lo.as_slice()..=hi.as_slice())?.count() as u64)
    }

    pub fn checkpoint(&self) -> Result<Option<u64>> {
        let txn = self.db.begin_read()?;
        let t = txn.open_table(META)?;
        Ok(t.get("checkpoint")?.map(|v| v.value()))
    }

    /// Make everything durable and record the checkpoint position.
    pub fn set_checkpoint(&self, position: u64) -> Result<()> {
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

/// redb's magic number, the first bytes of every database it writes (its
/// `MAGICNUMBER`, which the crate keeps to itself), and the length of its
/// header: a file shorter than that never held a database.
const REDB_MAGIC: [u8; 9] = [b'r', b'e', b'd', b'b', 0x1A, 0x0A, 0xA9, 0x0D, 0x0A];
const REDB_HEADER_LEN: u64 = 320;

/// An index file that was not a redb database, which the store's open moved
/// aside before it built a new index from the WAL (theseus-0b8).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MovedAside {
    /// Where it is now: `index.redb.bad-<unix ms>`, beside it.
    pub path: String,
    pub bytes: u64,
    /// What redb said of it.
    pub why: String,
}

/// What `move_aside` did.
#[derive(Debug)]
pub enum Aside {
    /// Moved: the name is free for a new index.
    Moved(MovedAside),
    /// Another process holds the file, or moved it first: the caller opens
    /// again, as for any store another process holds.
    Held,
    /// Under the lock the file is a redb database after all: left as it is.
    Database,
}

/// Move the file at `path`, which redb refused as not a database
/// (`RedbIndex::not_a_database`), to `<name>.bad-<unix ms>` beside it: kept,
/// never deleted (theseus-0b8).
///
/// It moves under the file's lock, which redb and this lock respect in both
/// directions: a process whose redb holds the file keeps it (`Held`), and no
/// open takes the file while it moves. The lock is on the file the name
/// still names, and that file is checked again under it, so two processes
/// that both found it bad move it once, and neither moves the index the
/// other made in its place.
pub fn move_aside(path: &Path, why: &str) -> Result<Aside> {
    use std::io::Read as _;
    use std::os::unix::fs::MetadataExt as _;
    let mut f = match std::fs::File::open(path) {
        Ok(f) => f,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Aside::Held),
        Err(e) => return Err(e).with_context(|| format!("opening {}", path.display())),
    };
    match f.try_lock() {
        Ok(()) => {}
        Err(std::fs::TryLockError::WouldBlock) => return Ok(Aside::Held),
        Err(std::fs::TryLockError::Error(e)) => {
            return Err(e).with_context(|| format!("locking {}", path.display()))
        }
    }
    let held = f.metadata()?;
    match std::fs::metadata(path) {
        Ok(now) if (now.dev(), now.ino()) == (held.dev(), held.ino()) => {}
        _ => return Ok(Aside::Held),
    }
    let mut head = Vec::with_capacity(REDB_MAGIC.len());
    (&mut f)
        .take(REDB_MAGIC.len() as u64)
        .read_to_end(&mut head)?;
    if held.len() >= REDB_HEADER_LEN && head == REDB_MAGIC {
        return Ok(Aside::Database);
    }
    let name = path
        .file_name()
        .context("an index path names a file")?
        .to_string_lossy()
        .into_owned();
    let ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    let mut to = path.with_file_name(format!("{name}.bad-{ms}"));
    let mut n = 1;
    while to.exists() {
        n += 1;
        to = path.with_file_name(format!("{name}.bad-{ms}-{n}"));
    }
    std::fs::rename(path, &to)
        .with_context(|| format!("moving {} aside to {}", path.display(), to.display()))?;
    if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
        std::fs::File::open(dir)?.sync_all()?;
    }
    Ok(Aside::Moved(MovedAside {
        path: to.display().to_string(),
        bytes: held.len(),
        why: why.to_string(),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redb_index() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("index.redb");
        let idx = RedbIndex::open(&path).unwrap();
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
        let idx = RedbIndex::open(&path).unwrap();
        assert_eq!(idx.checkpoint().unwrap(), Some(5));
        assert_eq!(idx.latest_position(1, "s1").unwrap(), Some(4));
    }
}
