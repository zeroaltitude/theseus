//! The index: a rebuildable projection of the WAL in redb, the M1 benchmark's
//! pick (fjall, the engine it lost to, is gone: theseus-0g4).
//!
//! Tables (all keys big-endian so lexical order is numeric order):
//! - `loc`:    position u64 → RecordLocation
//! - `bykey`:  kind u16 ‖ key bytes → latest position u64
//! - `bykind`: kind u16 ‖ position u64 → () (per-kind ordered scans)
//! - `byscope`: scope bytes ‖ 0x00 ‖ position u64 → () (per-session ordered scans, §4.4b)
//! - `terms`:  kind u16 ‖ term ‖ 0x00 ‖ key bytes → latest position u64: the terms a
//!   projection gives each key's latest record (theseus-lv2), so a reader asks for
//!   the keys with a term instead of reading every record of the kind
//! - `termsof`: kind u16 ‖ key bytes → that key's terms, each ended by 0x00, so the
//!   key's next record replaces them
//! - `meta`:   "checkpoint" → position u64; a projection's name → the checkpoint
//!   its terms were whole at; `SHAPE` → the checkpoint the tables below were
//!   whole at
//! - `counts`: kind u16 → how many records it has; `keycounts`: kind u16 → how
//!   many keys; `scopecounts`: scope bytes → how many records (theseus-vm3n.5:
//!   a count reads one row, not a range that grows with history)
//! - `termcounts`: kind u16 ‖ term → how many keys have it, kept with `terms`
//! - `born`:   kind u16 ‖ key → the key's first position; `bybirth`: kind u16 ‖
//!   that position → the key: the newest keys of a kind, by birth
//! - `clock`:  kind u16 → the kind's clock: the newest frame time its records
//!   have had, so it never steps back (see `pages.rs`)
//! - `bytime`: kind u16 ‖ minute u64 → the first position of the kind whose
//!   clock is in that minute: a time window's first position in O(log n)
//! - `tagged`: kind u16 ‖ tag ‖ 0x00 ‖ position u64 → (): a record's tags
//!   (`pages::tags_of`: a ledger row's kind and session), so a filtered read
//!   costs its answer, not a scan
//!
//! Writes are non-durable by default; `flush_durable` + `set_checkpoint` make
//! them durable together. Startup replays the WAL past the checkpoint.

use std::collections::BTreeMap;
use std::path::Path;

use anyhow::{Context, Result};
use redb::{Database, Durability, ReadableDatabase, ReadableTable, TableDefinition};
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
    /// The frame's time, as the WAL wrote it.
    pub at_unix_ms: u64,
    pub kind: RecordKind,
    pub key: Option<String>,
    pub scope: Option<String>,
    pub loc: Location,
    /// The terms a projection gives a keyed record of a kind it projects,
    /// which replace its key's last ones; `None` leaves them as they are.
    pub terms: Option<Vec<String>>,
    /// The numbers a projection adds up for its kind (`Sums`), which replace
    /// its key's last ones in the totals; `None` leaves them as they are.
    pub sums: Option<Sums>,
    /// The record's tags (`pages::tags_of`), each kept with its position.
    pub tags: Vec<String>,
}

/// One key's projection, as a build after serving puts it: (kind, key,
/// latest position, terms, numbers).
pub type Projected = (RecordKind, String, u64, Vec<String>, Option<Sums>);

/// Numbers a projection adds up for a kind (theseus-lv2): each keyed
/// record's replace its key's last ones in the kind's totals, so a reader of
/// the totals (health's sessions, turns, tokens, and cost) reads one row, not
/// every record. As u128s, so a fixed-point sum is exact.
pub type Sums = [u128; 8];

fn sums_bytes(s: &Sums) -> Vec<u8> {
    s.iter().flat_map(|n| n.to_be_bytes()).collect()
}
fn sums_from(b: &[u8]) -> Sums {
    let mut s = [0u128; 8];
    let (words, _) = b.as_chunks::<16>();
    for (n, w) in s.iter_mut().zip(words) {
        *n = u128::from_be_bytes(*w);
    }
    s
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
fn term_key(kind: RecordKind, term: &str, key: &str) -> Vec<u8> {
    let mut v = Vec::with_capacity(3 + term.len() + key.len());
    v.extend_from_slice(&kind.to_be_bytes());
    v.extend_from_slice(term.as_bytes());
    v.push(0);
    v.extend_from_slice(key.as_bytes());
    v
}
/// The bounds of every `term_key` whose term is in `lo..hi`: a term never
/// holds a 0x00, so a term `t` with `lo <= t < hi` sorts its keys there.
fn term_bounds(kind: RecordKind, lo: &str, hi: &str) -> (Vec<u8>, Vec<u8>) {
    let at = |t: &str| {
        let mut v = kind.to_be_bytes().to_vec();
        v.extend_from_slice(t.as_bytes());
        v
    };
    (at(lo), at(hi))
}
/// A key's terms as `termsof` keeps them: each ended by 0x00.
fn terms_bytes(terms: &[String]) -> Vec<u8> {
    let mut v = Vec::with_capacity(terms.iter().map(|t| t.len() + 1).sum());
    for t in terms {
        v.extend_from_slice(t.as_bytes());
        v.push(0);
    }
    v
}
fn terms_from(b: &[u8]) -> Vec<String> {
    b.split(|c| *c == 0)
        .filter(|t| !t.is_empty())
        .map(|t| String::from_utf8_lossy(t).into_owned())
        .collect()
}
/// The term and the key in a `term_key`.
fn term_and_key(b: &[u8]) -> Option<(String, String)> {
    let rest = b.get(2..)?;
    let nul = rest.iter().position(|c| *c == 0)?;
    Some((
        String::from_utf8_lossy(&rest[..nul]).into_owned(),
        String::from_utf8_lossy(&rest[nul + 1..]).into_owned(),
    ))
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
const TERMS: TableDefinition<&[u8], u64> = TableDefinition::new("terms");
const TERMSOF: TableDefinition<&[u8], &[u8]> = TableDefinition::new("termsof");
const SUMS: TableDefinition<u16, &[u8]> = TableDefinition::new("sums");
const SUMSOF: TableDefinition<&[u8], &[u8]> = TableDefinition::new("sumsof");
pub(crate) const META: TableDefinition<&str, u64> = TableDefinition::new("meta");
pub(crate) const COUNTS: TableDefinition<u16, u64> = TableDefinition::new("counts");
const KEYCOUNTS: TableDefinition<u16, u64> = TableDefinition::new("keycounts");
const SCOPECOUNTS: TableDefinition<&[u8], u64> = TableDefinition::new("scopecounts");
const CLOCK: TableDefinition<u16, u64> = TableDefinition::new("clock");
const BYTIME: TableDefinition<&[u8], u64> = TableDefinition::new("bytime");
pub(crate) const TAGGED: TableDefinition<&[u8], ()> = TableDefinition::new("tagged");
const TERMCOUNTS: TableDefinition<&[u8], u64> = TableDefinition::new("termcounts");
const BORN: TableDefinition<&[u8], u64> = TableDefinition::new("born");
const BYBIRTH: TableDefinition<&[u8], &[u8]> = TableDefinition::new("bybirth");

/// The index's shape (theseus-vm3n.5): `meta` keeps under this name the
/// checkpoint at which the counts, the clock, `bytime`, and `tagged` were
/// whole. Every checkpoint of this build sets it; an older build, which keeps
/// none of them, moves the checkpoint alone. An open that finds it anywhere
/// but at the checkpoint empties the index, and the store's open then
/// rebuilds it from the WAL, as for an index with no checkpoint (a restore).
/// A change to what those tables hold renames it.
pub const SHAPE: &str = "index.shape.3";

/// A minute, in ms: `bytime`'s grain.
pub const MINUTE_MS: u64 = 60_000;

fn bytime(kind: RecordKind, minute: u64) -> [u8; 10] {
    bykind(kind, minute)
}
fn tagged(kind: RecordKind, tag: &str, pos: u64) -> Vec<u8> {
    let mut v = Vec::with_capacity(11 + tag.len());
    v.extend_from_slice(&kind.to_be_bytes());
    v.extend_from_slice(tag.as_bytes());
    v.push(0);
    v.extend_from_slice(&pos.to_be_bytes());
    v
}

pub struct RedbIndex {
    db: Db,
    repaired: bool,
    /// Whether the counts, the clocks, `bytime`, `tagged`, and `termcounts`
    /// are whole (`SHAPE`): false from an open that found another shape's
    /// index until `recount` (theseus-vm3n.5); its readers walk until then.
    shaped: std::sync::atomic::AtomicBool,
}

/// The database, open until its index drops, which times its close
/// (theseus-26r): redb's close commits its allocator state and writes its
/// shutdown header, the last syncs of a clean stop.
struct Db(Option<Database>);

impl std::ops::Deref for Db {
    type Target = Database;
    fn deref(&self) -> &Database {
        self.0
            .as_ref()
            .expect("the index's database is open until it drops")
    }
}

impl Drop for RedbIndex {
    fn drop(&mut self) {
        if let Some(db) = self.db.0.take() {
            let t0 = std::time::Instant::now();
            drop(db);
            tracing::debug!(
                ms = (t0.elapsed().as_secs_f64() * 1000.0 * 100.0).round() / 100.0,
                "store: index closed"
            );
        }
    }
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
        let shaped = shape_or_empty(&txn)?;
        {
            txn.open_table(LOC)?;
            txn.open_table(BYKEY)?;
            txn.open_table(BYKIND)?;
            txn.open_table(BYSCOPE)?;
            txn.open_table(TERMS)?;
            txn.open_table(TERMSOF)?;
            txn.open_table(SUMS)?;
            txn.open_table(SUMSOF)?;
            txn.open_table(META)?;
            txn.open_table(COUNTS)?;
            txn.open_table(KEYCOUNTS)?;
            txn.open_table(SCOPECOUNTS)?;
            txn.open_table(CLOCK)?;
            txn.open_table(BYTIME)?;
            txn.open_table(TAGGED)?;
            txn.open_table(TERMCOUNTS)?;
            txn.open_table(BORN)?;
            txn.open_table(BYBIRTH)?;
        }
        txn.commit()?;
        if !shaped {
            tracing::info!(
                shape = SHAPE,
                "store: the index was written by a build that keeps another shape; its counts, \
                 clocks, and tags are built again after serving"
            );
        }
        Ok(Self {
            db: Db(Some(db)),
            repaired,
            shaped: std::sync::atomic::AtomicBool::new(shaped),
        })
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
        Self::why_not_a_database(e).is_some()
    }

    /// Why the file is not a redb database, as the startup log and the
    /// `store.index_replaced` row say it, with redb's own words after;
    /// `None` when `not_a_database` is false.
    pub fn why_not_a_database(e: &anyhow::Error) -> Option<String> {
        let Some(redb::DatabaseError::Storage(redb::StorageError::Io(io))) =
            e.downcast_ref::<redb::DatabaseError>()
        else {
            return None;
        };
        match io.kind() {
            std::io::ErrorKind::UnexpectedEof => {
                Some(format!("the file ends inside redb's header (redb: {io})"))
            }
            std::io::ErrorKind::InvalidData => Some(format!(
                "the file does not start with redb's magic number (redb: {io})"
            )),
            _ => None,
        }
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
            let mut births = Vec::new();
            let mut fresh = Vec::with_capacity(entries.len());
            for e in entries {
                loc.insert(e.position, loc_bytes(e.loc).as_slice())?;
                if let Some(k) = &e.key {
                    let kb = bykey(e.kind, k);
                    if byk.insert(kb.as_slice(), e.position)?.is_none() {
                        births.push((e.kind, e.position, kb));
                    }
                }
                fresh.push(
                    bkd.insert(bykind(e.kind, e.position).as_slice(), ())?
                        .is_none(),
                );
                if let Some(sc) = &e.scope {
                    bsc.insert(byscope(sc, e.position).as_slice(), ())?;
                }
            }
            put_shaped(&txn, entries, &fresh, &births, false)?;
            if entries.iter().any(|e| e.key.is_some() && e.terms.is_some()) {
                let mut terms = txn.open_table(TERMS)?;
                let mut termsof = txn.open_table(TERMSOF)?;
                let mut counts = txn.open_table(TERMCOUNTS)?;
                for e in entries {
                    if let (Some(k), Some(t)) = (&e.key, &e.terms) {
                        let tables = (&mut terms, &mut termsof, &mut counts);
                        put_terms(tables, e.kind, k, t, e.position)?;
                    }
                }
            }
            if entries.iter().any(|e| e.key.is_some() && e.sums.is_some()) {
                let mut sums = txn.open_table(SUMS)?;
                let mut sumsof = txn.open_table(SUMSOF)?;
                for e in entries {
                    if let (Some(k), Some(s)) = (&e.key, &e.sums) {
                        put_sums(&mut sums, &mut sumsof, e.kind, k, s)?;
                    }
                }
            }
        }
        txn.commit()?;
        Ok(())
    }

    /// A kind's totals: every key's numbers, added up (theseus-lv2).
    pub fn totals(&self, kind: RecordKind) -> Result<Sums> {
        let txn = self.db.begin_read()?;
        let t = txn.open_table(SUMS)?;
        Ok(t.get(kind)?
            .map(|v| sums_from(v.value()))
            .unwrap_or_default())
    }

    /// `apply`, for a replay of many records at once (theseus-byu: a
    /// restore, an open with no checkpoint): each table's rows sorted by key
    /// and inserted in order, a table at a time, in one non-durable
    /// transaction. A B-tree fed in key order fills its pages one after
    /// another, where the replay's position order scatters `bykey`,
    /// `byscope`, and `terms` across the tree. The rows are `apply`'s: a key
    /// that several entries name keeps the last one's.
    pub fn apply_bulk(&self, entries: &[IndexEntry]) -> Result<()> {
        let mut txn = self.db.begin_write()?;
        txn.set_durability(Durability::None)?;
        {
            // A replay's positions only grow: `loc` is in key order, and so
            // is each kind's stretch of `bykind`.
            let mut loc = txn.open_table(LOC)?;
            for e in entries {
                loc.insert(e.position, loc_bytes(e.loc).as_slice())?;
            }
            let mut by_kind: Vec<([u8; 10], usize)> = entries
                .iter()
                .enumerate()
                .map(|(i, e)| (bykind(e.kind, e.position), i))
                .collect();
            by_kind.sort_unstable();
            let mut bkd = txn.open_table(BYKIND)?;
            let mut fresh = vec![false; entries.len()];
            for (k, i) in &by_kind {
                fresh[*i] = bkd.insert(k.as_slice(), ())?.is_none();
            }
            // Each key's latest position: its last entry's.
            // (key, latest position, first position).
            let mut keys: Vec<(Vec<u8>, u64, u64)> = entries
                .iter()
                .filter_map(|e| Some((bykey(e.kind, e.key.as_deref()?), e.position, e.position)))
                .collect();
            keys.sort_unstable();
            keys.dedup_by(|later, earlier| {
                if later.0 == earlier.0 {
                    earlier.1 = later.1;
                    true
                } else {
                    false
                }
            });
            let mut byk = txn.open_table(BYKEY)?;
            let mut births = Vec::new();
            for (k, p, first) in keys {
                if byk.insert(k.as_slice(), p)?.is_none() {
                    births.push((RecordKind::from_be_bytes([k[0], k[1]]), first, k));
                }
            }
            put_shaped(&txn, entries, &fresh, &births, true)?;
            let mut scopes: Vec<Vec<u8>> = entries
                .iter()
                .filter_map(|e| Some(byscope(e.scope.as_deref()?, e.position)))
                .collect();
            scopes.sort_unstable();
            let mut bsc = txn.open_table(BYSCOPE)?;
            for k in &scopes {
                bsc.insert(k.as_slice(), ())?;
            }
        }
        // Each key's last terms and numbers, in key order.
        let mut projected: Vec<&IndexEntry> = entries
            .iter()
            .filter(|e| e.key.is_some() && (e.terms.is_some() || e.sums.is_some()))
            .collect();
        projected.sort_by(|a, b| (a.kind, &a.key, a.position).cmp(&(b.kind, &b.key, b.position)));
        projected.dedup_by(|later, earlier| {
            let same = (later.kind, &later.key) == (earlier.kind, &earlier.key);
            if same {
                std::mem::swap(later, earlier);
            }
            same
        });
        if !projected.is_empty() {
            let mut terms = txn.open_table(TERMS)?;
            let mut termsof = txn.open_table(TERMSOF)?;
            let mut counts = txn.open_table(TERMCOUNTS)?;
            let mut sums = txn.open_table(SUMS)?;
            let mut sumsof = txn.open_table(SUMSOF)?;
            for e in projected {
                let Some(k) = &e.key else { continue };
                if let Some(t) = &e.terms {
                    let tables = (&mut terms, &mut termsof, &mut counts);
                    put_terms(tables, e.kind, k, t, e.position)?;
                }
                if let Some(s) = &e.sums {
                    put_sums(&mut sums, &mut sumsof, e.kind, k, s)?;
                }
            }
        }
        txn.commit()?;
        Ok(())
    }

    /// The checkpoint a projection's terms were whole at (`set_checkpoint`
    /// sets it), if any.
    pub fn terms_mark(&self, name: &str) -> Result<Option<u64>> {
        let txn = self.db.begin_read()?;
        let t = txn.open_table(META)?;
        Ok(t.get(name)?.map(|v| v.value()))
    }

    /// (key, latest position) of up to `limit` keys of `kind` after `after`
    /// (`None`: from the first), in key order: one stretch of a walk over a
    /// kind's keys.
    pub fn keys_of_kind_after(
        &self,
        kind: RecordKind,
        after: Option<&str>,
        limit: usize,
    ) -> Result<Vec<(String, u64)>> {
        let txn = self.db.begin_read()?;
        let t = txn.open_table(BYKEY)?;
        let lo = match after {
            // The smallest key after `after`'s.
            Some(k) => {
                let mut v = bykey(kind, k);
                v.push(0);
                v
            }
            None => kind.to_be_bytes().to_vec(),
        };
        let hi = (kind + 1).to_be_bytes().to_vec();
        let mut out = Vec::new();
        for row in t.range(lo.as_slice()..hi.as_slice())?.take(limit) {
            let (k, v) = row?;
            out.push((
                String::from_utf8_lossy(&k.value()[2..]).into_owned(),
                v.value(),
            ));
        }
        Ok(out)
    }

    /// Put each row's (kind, key, position, terms, numbers) whose key's
    /// latest position is still `position`, in one non-durable transaction:
    /// an append since the row was read put its own (theseus-lv2). Returns
    /// how many it put.
    pub fn put_terms_if_latest(&self, rows: &[Projected]) -> Result<u64> {
        let mut txn = self.db.begin_write()?;
        txn.set_durability(Durability::None)?;
        let mut put = 0;
        {
            let byk = txn.open_table(BYKEY)?;
            let mut terms = txn.open_table(TERMS)?;
            let mut termsof = txn.open_table(TERMSOF)?;
            let mut counts = txn.open_table(TERMCOUNTS)?;
            let mut sums = txn.open_table(SUMS)?;
            let mut sumsof = txn.open_table(SUMSOF)?;
            for (kind, key, position, t, s) in rows {
                let latest = byk.get(bykey(*kind, key).as_slice())?.map(|v| v.value());
                if latest == Some(*position) {
                    let tables = (&mut terms, &mut termsof, &mut counts);
                    put_terms(tables, *kind, key, t, *position)?;
                    if let Some(s) = s {
                        put_sums(&mut sums, &mut sumsof, *kind, key, s)?;
                    }
                    put += 1;
                }
            }
        }
        txn.commit()?;
        Ok(put)
    }

    /// (key, latest position) of every key of `kind` with a term in
    /// `lo..hi`, each key once, in key order.
    pub fn keys_by_terms(
        &self,
        kind: RecordKind,
        lo: &str,
        hi: &str,
    ) -> Result<Vec<(String, u64)>> {
        let txn = self.db.begin_read()?;
        let t = txn.open_table(TERMS)?;
        let (from, to) = term_bounds(kind, lo, hi);
        let mut out = std::collections::BTreeMap::new();
        for row in t.range(from.as_slice()..to.as_slice())? {
            let (k, v) = row?;
            if let Some((_, key)) = term_and_key(k.value()) {
                out.insert(key, v.value());
            }
        }
        Ok(out.into_iter().collect())
    }

    /// How many (term, key) pairs of `kind` have a term in `lo..hi`: for a
    /// range of one term, how many keys have it. Read from `termcounts`, a
    /// row per term (theseus-vm3n.5), so it costs the terms in the range,
    /// not their keys.
    pub fn count_by_terms(&self, kind: RecordKind, lo: &str, hi: &str) -> Result<u64> {
        if !self.shaped() {
            return self.count_by_terms_walked(kind, lo, hi);
        }
        let txn = self.db.begin_read()?;
        let t = txn.open_table(TERMCOUNTS)?;
        let (from, to) = term_bounds(kind, lo, hi);
        let mut n = 0;
        for row in t.range(from.as_slice()..to.as_slice())? {
            n += row?.1.value();
        }
        Ok(n)
    }

    /// `count_by_terms`, walked from `terms`, for a test to hold it to.
    pub fn count_by_terms_walked(&self, kind: RecordKind, lo: &str, hi: &str) -> Result<u64> {
        let txn = self.db.begin_read()?;
        let t = txn.open_table(TERMS)?;
        let (from, to) = term_bounds(kind, lo, hi);
        Ok(t.range(from.as_slice()..to.as_slice())?.count() as u64)
    }

    /// The terms a key's latest record has, as the projection gave them.
    pub fn terms_of(&self, kind: RecordKind, key: &str) -> Result<Vec<String>> {
        let txn = self.db.begin_read()?;
        let t = txn.open_table(TERMSOF)?;
        Ok(t.get(bykey(kind, key).as_slice())?
            .map(|v| terms_from(v.value()))
            .unwrap_or_default())
    }

    /// (key, latest position) of every key of `kind` that starts with
    /// `prefix`, in key order.
    pub fn keys_with_prefix(&self, kind: RecordKind, prefix: &str) -> Result<Vec<(String, u64)>> {
        let txn = self.db.begin_read()?;
        let t = txn.open_table(BYKEY)?;
        let lo = bykey(kind, prefix);
        let mut out = Vec::new();
        for row in t.range(lo.as_slice()..)? {
            let (k, v) = row?;
            let kb = k.value();
            if !kb.starts_with(&lo) {
                break;
            }
            out.push((String::from_utf8_lossy(&kb[2..]).into_owned(), v.value()));
        }
        Ok(out)
    }

    /// How many keys `kind` has: one row, kept with every append
    /// (theseus-vm3n.5).
    pub fn count_keys(&self, kind: RecordKind) -> Result<u64> {
        if !self.shaped() {
            return self.count_keys_walked(kind);
        }
        let txn = self.db.begin_read()?;
        let t = txn.open_table(KEYCOUNTS)?;
        Ok(t.get(kind)?.map_or(0, |v| v.value()))
    }

    /// How many keys `kind` has, walked from the key table: what
    /// `count_keys` keeps, for a test to hold it to.
    pub fn count_keys_walked(&self, kind: RecordKind) -> Result<u64> {
        let txn = self.db.begin_read()?;
        let t = txn.open_table(BYKEY)?;
        let lo = kind.to_be_bytes().to_vec();
        let hi = (kind + 1).to_be_bytes().to_vec();
        Ok(t.range(lo.as_slice()..hi.as_slice())?.count() as u64)
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

    /// Positions of a kind with position > `after`, oldest first, at most
    /// `limit`: one page of a walk over a kind (theseus-xo0m).
    pub fn positions_of_kind_after(
        &self,
        kind: RecordKind,
        after: u64,
        limit: usize,
    ) -> Result<Vec<u64>> {
        let txn = self.db.begin_read()?;
        let t = txn.open_table(BYKIND)?;
        let lo = bykind(kind, after.saturating_add(1));
        let hi = bykind(kind, u64::MAX);
        let mut out = Vec::new();
        for row in t.range(lo.as_slice()..=hi.as_slice())?.take(limit) {
            let (k, _) = row?;
            if let Some(p) = u64_from(&k.value()[2..]) {
                out.push(p);
            }
        }
        Ok(out)
    }

    /// How many records `kind` has: one row, kept with every append
    /// (theseus-vm3n.5).
    pub fn count_of_kind(&self, kind: RecordKind) -> Result<u64> {
        if !self.shaped() {
            return self.count_of_kind_walked(kind);
        }
        let txn = self.db.begin_read()?;
        let t = txn.open_table(COUNTS)?;
        Ok(t.get(kind)?.map_or(0, |v| v.value()))
    }

    /// `count_of_kind`, walked from `bykind`, for a test to hold it to.
    pub fn count_of_kind_walked(&self, kind: RecordKind) -> Result<u64> {
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

    /// How many records a scope has: one row, kept with every append
    /// (theseus-vm3n.5).
    pub fn count_in_scope(&self, scope: &str) -> Result<u64> {
        if !self.shaped() {
            return self.count_in_scope_walked(scope);
        }
        let txn = self.db.begin_read()?;
        let t = txn.open_table(SCOPECOUNTS)?;
        Ok(t.get(scope.as_bytes())?.map_or(0, |v| v.value()))
    }

    /// `count_in_scope`, walked from `byscope`, for a test to hold it to.
    pub fn count_in_scope_walked(&self, scope: &str) -> Result<u64> {
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

    /// Make everything durable and record the checkpoint position, and, when
    /// a projection named `terms` kept its terms with every write, that they
    /// are whole there too (theseus-lv2). A writer with no projection, or an
    /// older build, moves the checkpoint alone, and the next projected open
    /// then builds the terms again.
    pub fn set_checkpoint(&self, position: u64, terms: Option<&str>) -> Result<()> {
        self.set_checkpoint_with(position, terms, &[], true)
    }

    /// `set_checkpoint`, with `meta`'s values in the same commit; with no
    /// sync of its own unless `durable`: then the next durable commit, or
    /// redb's close, makes it durable (theseus-02k).
    pub fn set_checkpoint_with(
        &self,
        position: u64,
        terms: Option<&str>,
        meta: &[(&str, u64)],
        durable: bool,
    ) -> Result<()> {
        let mut txn = self.db.begin_write()?;
        txn.set_durability(if durable {
            Durability::Immediate
        } else {
            Durability::None
        })?;
        {
            let mut t = txn.open_table(META)?;
            t.insert("checkpoint", position)?;
            if self.shaped() {
                t.insert(SHAPE, position)?;
            }
            if let Some(name) = terms {
                t.insert(name, position)?;
            }
            for (k, v) in meta {
                t.insert(*k, *v)?;
            }
        }
        txn.commit()?;
        Ok(())
    }

    /// The values `meta` keeps under each of `keys`, in order.
    pub fn meta(&self, keys: &[&str]) -> Result<Vec<Option<u64>>> {
        let txn = self.db.begin_read()?;
        let t = txn.open_table(META)?;
        keys.iter()
            .map(|k| Ok(t.get(*k)?.map(|v| v.value())))
            .collect()
    }
}

impl RedbIndex {
    /// Whether the shape's tables are whole (`SHAPE`, theseus-vm3n.5).
    pub fn shaped(&self) -> bool {
        self.shaped.load(std::sync::atomic::Ordering::Acquire)
    }

    /// (position, location) of up to `limit` records with `after <
    /// position <= upto`, in position order: one stretch of a build's walk.
    pub fn locations_after(
        &self,
        after: u64,
        upto: u64,
        limit: usize,
    ) -> Result<Vec<(u64, Location)>> {
        let mut out = Vec::new();
        if after >= upto {
            return Ok(out);
        }
        let txn = self.db.begin_read()?;
        let t = txn.open_table(LOC)?;
        for row in t.range(after + 1..=upto)?.take(limit) {
            let (k, v) = row?;
            out.extend(loc_from(v.value()).map(|l| (k.value(), l)));
        }
        Ok(out)
    }

    /// One stretch of a build after serving (theseus-vm3n.5): each record's
    /// tags, its key's birth where it has none yet, and its kind's clock into `bytime`, from `clocks`, the build's
    /// own clock per kind, which it carries on. In one non-durable
    /// transaction; what an append put meanwhile stays (`put_minute`).
    pub fn put_built(
        &self,
        records: &[Built],
        clocks: &mut BTreeMap<RecordKind, u64>,
    ) -> Result<()> {
        let mut txn = self.db.begin_write()?;
        txn.set_durability(Durability::None)?;
        {
            let mut bt = txn.open_table(BYTIME)?;
            let mut tg = txn.open_table(TAGGED)?;
            let mut born = txn.open_table(BORN)?;
            let mut bybirth = txn.open_table(BYBIRTH)?;
            for (position, kind, at, tags, key) in records {
                if let Some(k) = key {
                    put_born(&mut born, &mut bybirth, *position, &bykey(*kind, k))?;
                }
                let was = clocks.get(kind).copied();
                let at = was.map_or(*at, |c| c.max(*at));
                if was.is_none_or(|c| at / MINUTE_MS > c / MINUTE_MS) {
                    put_minute(&mut bt, *kind, at / MINUTE_MS, *position)?;
                }
                clocks.insert(*kind, at);
                for tag in tags {
                    tg.insert(tagged(*kind, tag, *position).as_slice(), ())?;
                }
            }
        }
        txn.commit()?;
        Ok(())
    }

    /// The build's last step (theseus-vm3n.5): every count, walked once from
    /// the table it counts, all in one transaction, so no append lands
    /// between a walk and its count; from then on each append keeps them,
    /// and the index is whole. A kind's clock is put where an append has
    /// not put one. It holds the writer for one walk of the index, once.
    pub fn recount(&self, clocks: &BTreeMap<RecordKind, u64>) -> Result<()> {
        fn kind_of(k: &[u8]) -> RecordKind {
            RecordKind::from_be_bytes([k[0], k[1]])
        }
        let mut txn = self.db.begin_write()?;
        txn.set_durability(Durability::None)?;
        {
            let mut by_kind: BTreeMap<RecordKind, u64> = BTreeMap::new();
            for row in txn.open_table(BYKIND)?.iter()? {
                *by_kind.entry(kind_of(row?.0.value())).or_default() += 1;
            }
            let mut keys: BTreeMap<RecordKind, u64> = BTreeMap::new();
            for row in txn.open_table(BYKEY)?.iter()? {
                *keys.entry(kind_of(row?.0.value())).or_default() += 1;
            }
            let mut scopes: BTreeMap<Vec<u8>, u64> = BTreeMap::new();
            for row in txn.open_table(BYSCOPE)?.iter()? {
                let (k, _) = row?;
                let kb = k.value();
                *scopes.entry(kb[..kb.len() - 9].to_vec()).or_default() += 1;
            }
            let mut terms: BTreeMap<Vec<u8>, u64> = BTreeMap::new();
            for row in txn.open_table(TERMS)?.iter()? {
                let (k, _) = row?;
                let kb = k.value();
                if let Some(nul) = kb[2..].iter().position(|c| *c == 0) {
                    *terms.entry(kb[..2 + nul].to_vec()).or_default() += 1;
                }
            }
            txn.delete_table(COUNTS)?;
            txn.delete_table(KEYCOUNTS)?;
            txn.delete_table(SCOPECOUNTS)?;
            txn.delete_table(TERMCOUNTS)?;
            let mut t = txn.open_table(COUNTS)?;
            for (k, n) in &by_kind {
                t.insert(*k, *n)?;
            }
            let mut t = txn.open_table(KEYCOUNTS)?;
            for (k, n) in &keys {
                t.insert(*k, *n)?;
            }
            let mut t = txn.open_table(SCOPECOUNTS)?;
            for (k, n) in &scopes {
                t.insert(k.as_slice(), *n)?;
            }
            let mut t = txn.open_table(TERMCOUNTS)?;
            for (k, n) in &terms {
                t.insert(k.as_slice(), *n)?;
            }
            let mut t = txn.open_table(CLOCK)?;
            for (k, at) in clocks {
                if t.get(*k)?.is_none() {
                    t.insert(*k, *at)?;
                }
            }
        }
        txn.commit()?;
        self.shaped
            .store(true, std::sync::atomic::Ordering::Release);
        Ok(())
    }

    /// The newest `limit` keys of `kind` by birth, born before `before`
    /// when given: (birth, key, latest position), newest first, and whether
    /// older ones remain. One read transaction (theseus-vm3n.5).
    pub fn keys_by_birth(
        &self,
        kind: RecordKind,
        before: Option<u64>,
        limit: usize,
    ) -> Result<Births> {
        let txn = self.db.begin_read()?;
        let bb = txn.open_table(BYBIRTH)?;
        let byk = txn.open_table(BYKEY)?;
        let (lo, hi) = (bykind(kind, 0), bykind(kind, before.unwrap_or(u64::MAX)));
        let mut out = Vec::new();
        let mut more = false;
        for row in bb.range(lo.as_slice()..hi.as_slice())?.rev() {
            if out.len() == limit {
                more = true;
                break;
            }
            let (k, v) = row?;
            let key = String::from_utf8_lossy(v.value()).into_owned();
            let Some(born) = u64_from(&k.value()[2..]) else {
                continue;
            };
            if let Some(latest) = byk.get(bykey(kind, &key).as_slice())? {
                out.push((born, key, latest.value()));
            }
        }
        Ok((out, more))
    }

    /// The first `bytime` entry of `kind` at `minute` or after: (its minute,
    /// the first position whose clock is in it). `None` when the kind's
    /// clock has not reached `minute`.
    pub fn first_from_minute(&self, kind: RecordKind, minute: u64) -> Result<Option<(u64, u64)>> {
        let txn = self.db.begin_read()?;
        let t = txn.open_table(BYTIME)?;
        let lo = bytime(kind, minute);
        let hi = bytime(kind, u64::MAX);
        let Some(row) = t.range(lo.as_slice()..=hi.as_slice())?.next() else {
            return Ok(None);
        };
        let (k, v) = row?;
        Ok(u64_from(&k.value()[2..]).map(|m| (m, v.value())))
    }

    /// One page of `kind`'s positions in `lo..hi`, with any of `tags` (every
    /// record of the kind when there are none): the newest `limit` of them
    /// when `newest`, else the oldest, in position order; whether more lie
    /// past the page in its direction; and the kind's count. One read
    /// transaction, so the page and the count are one snapshot (theseus-tphr).
    /// It costs about the page: each tag's postings are read from the bound
    /// in, `limit + 1` at most.
    pub fn page(
        &self,
        kind: RecordKind,
        tags: &[String],
        (lo, hi): (u64, u64),
        newest: bool,
        limit: usize,
    ) -> Result<(Vec<u64>, bool, u64)> {
        let txn = self.db.begin_read()?;
        let count = if self.shaped() {
            txn.open_table(COUNTS)?.get(kind)?.map_or(0, |v| v.value())
        } else {
            let t = txn.open_table(BYKIND)?;
            let (a, b) = (bykind(kind, 0), bykind(kind, u64::MAX));
            t.range(a.as_slice()..=b.as_slice())?.count() as u64
        };
        if hi <= lo || limit == 0 {
            return Ok((Vec::new(), hi > lo && limit == 0, count));
        }
        let want = limit.saturating_add(1);
        let mut found: Vec<u64> = Vec::new();
        if tags.is_empty() {
            let t = txn.open_table(BYKIND)?;
            let (a, b) = (bykind(kind, lo), bykind(kind, hi));
            let range = t.range(a.as_slice()..b.as_slice())?;
            let keys: Vec<_> = if newest {
                range.rev().take(want).collect()
            } else {
                range.take(want).collect()
            };
            for row in keys {
                let (k, _) = row?;
                found.extend(u64_from(&k.value()[2..]));
            }
        } else {
            let t = txn.open_table(TAGGED)?;
            for tag in tags {
                let (a, b) = (tagged(kind, tag, lo), tagged(kind, tag, hi));
                let range = t.range(a.as_slice()..b.as_slice())?;
                let keys: Vec<_> = if newest {
                    range.rev().take(want).collect()
                } else {
                    range.take(want).collect()
                };
                for row in keys {
                    let (k, _) = row?;
                    let kb = k.value();
                    found.extend(u64_from(&kb[kb.len() - 8..]));
                }
            }
            found.sort_unstable();
            found.dedup();
            if newest {
                found.reverse();
            }
            found.truncate(want);
        }
        let more = found.len() > limit;
        found.truncate(limit);
        found.sort_unstable();
        Ok((found, more, count))
    }
}

/// The newest keys by birth: each one's (birth, key, latest position),
/// newest first, and whether older ones remain.
pub type Births = (Vec<(u64, String, u64)>, bool);

/// A record as a build of the shape reads it: position, kind, frame time,
/// tags, key.
pub type Built = (u64, RecordKind, u64, Vec<String>, Option<String>);

/// Whether the index's shape (`SHAPE`) is whole at its checkpoint, making it
/// so where that is free (theseus-vm3n.5). An index with no checkpoint is
/// emptied, all but `meta`'s history-check marks: the store's open replays
/// the whole WAL into it, its rule for one, and what it held past no
/// checkpoint may be partial. A checkpointed index whose mark is elsewhere
/// (an older build wrote it last, which keeps none of these tables) keeps
/// its own tables and loses only the shape's, which `WalStore::build_shape`
/// builds again after serving: the open still reads only the WAL's tail.
fn shape_or_empty(txn: &redb::WriteTransaction) -> Result<bool> {
    let (cp, keys) = {
        let meta = txn.open_table(META)?;
        let cp = meta.get("checkpoint")?.map(|v| v.value());
        if cp.is_some() && cp == meta.get(SHAPE)?.map(|v| v.value()) {
            return Ok(true);
        }
        let mut keys = Vec::new();
        for row in meta.iter()? {
            let (k, _) = row?;
            let k = k.value().to_string();
            if !k.starts_with("verified.") {
                keys.push(k);
            }
        }
        (cp, keys)
    };
    txn.delete_table(COUNTS)?;
    txn.delete_table(KEYCOUNTS)?;
    txn.delete_table(SCOPECOUNTS)?;
    txn.delete_table(CLOCK)?;
    txn.delete_table(BYTIME)?;
    txn.delete_table(TAGGED)?;
    txn.delete_table(TERMCOUNTS)?;
    txn.delete_table(BORN)?;
    txn.delete_table(BYBIRTH)?;
    if cp.is_some() {
        return Ok(false);
    }
    txn.delete_table(LOC)?;
    txn.delete_table(BYKEY)?;
    txn.delete_table(BYKIND)?;
    txn.delete_table(BYSCOPE)?;
    txn.delete_table(TERMS)?;
    txn.delete_table(TERMSOF)?;
    txn.delete_table(SUMS)?;
    txn.delete_table(SUMSOF)?;
    let mut meta = txn.open_table(META)?;
    for k in &keys {
        meta.remove(k.as_str())?;
    }
    Ok(true)
}

/// Mark the key `kb` (its `bykey` bytes) born at `first`, unless it is
/// already: a build after serving meets each key it walks at its first
/// position, after appends born the new ones (theseus-vm3n.5).
fn put_born(
    born: &mut redb::Table<&'static [u8], u64>,
    bybirth: &mut redb::Table<&'static [u8], &'static [u8]>,
    first: u64,
    kb: &[u8],
) -> Result<()> {
    if born.get(kb)?.is_some() {
        return Ok(());
    }
    born.insert(kb, first)?;
    let kind = RecordKind::from_be_bytes([kb[0], kb[1]]);
    bybirth.insert(bykind(kind, first).as_slice(), &kb[2..])?;
    Ok(())
}

/// Point `bytime`'s (kind, minute) at `position` unless it already points
/// at an earlier one: a build after serving fills in older rows after an
/// append put a newer one (theseus-vm3n.5).
fn put_minute(
    bt: &mut redb::Table<&'static [u8], u64>,
    kind: RecordKind,
    minute: u64,
    position: u64,
) -> Result<()> {
    let k = bytime(kind, minute);
    let was = bt.get(k.as_slice())?.map(|v| v.value());
    if was.is_none_or(|w| w > position) {
        bt.insert(k.as_slice(), position)?;
    }
    Ok(())
}

/// The counts, the clocks, `bytime`, and `tagged` for `entries`, in
/// position order, in the transaction that indexes them (theseus-vm3n.5).
/// Only the entries `fresh` marks count: those `bykind` did not hold yet. An
/// open replays the WAL past the checkpoint, and an index closed cleanly
/// already holds some of it, so a replayed record must not count twice.
/// `births` are the keys new to `bykey`, each with its first position and
/// its `bykey` bytes: they are counted, and born (`born`, `bybirth`). A kind's clock is the newest
/// frame time its records have had: a frame whose time stepped back takes
/// the clock's, so the clock, and `bytime` with it, only grows with
/// position. `sort_tags` puts the postings in key order first, for a replay.
fn put_shaped(
    txn: &redb::WriteTransaction,
    entries: &[IndexEntry],
    fresh: &[bool],
    births: &[(RecordKind, u64, Vec<u8>)],
    sort_tags: bool,
) -> Result<()> {
    if !fresh.contains(&true) && births.is_empty() {
        return Ok(());
    }
    let mut counts: BTreeMap<RecordKind, u64> = BTreeMap::new();
    let mut scopes: BTreeMap<&str, u64> = BTreeMap::new();
    let mut clocks: BTreeMap<RecordKind, Option<u64>> = BTreeMap::new();
    let mut posts: Vec<Vec<u8>> = Vec::new();
    {
        let clock = txn.open_table(CLOCK)?;
        let mut bt = txn.open_table(BYTIME)?;
        for e in entries
            .iter()
            .zip(fresh)
            .filter(|(_, f)| **f)
            .map(|(e, _)| e)
        {
            *counts.entry(e.kind).or_default() += 1;
            if let Some(sc) = &e.scope {
                *scopes.entry(sc.as_str()).or_default() += 1;
            }
            let was = match clocks.get(&e.kind) {
                Some(c) => *c,
                None => clock.get(e.kind)?.map(|v| v.value()),
            };
            let at = was.map_or(e.at_unix_ms, |c| c.max(e.at_unix_ms));
            if was.is_none_or(|c| at / MINUTE_MS > c / MINUTE_MS) {
                put_minute(&mut bt, e.kind, at / MINUTE_MS, e.position)?;
            }
            clocks.insert(e.kind, Some(at));
            for tag in &e.tags {
                posts.push(tagged(e.kind, tag, e.position));
            }
        }
    }
    let mut clock = txn.open_table(CLOCK)?;
    for (kind, at) in &clocks {
        if let Some(at) = at {
            clock.insert(*kind, *at)?;
        }
    }
    let mut ct = txn.open_table(COUNTS)?;
    for (kind, n) in &counts {
        let was = ct.get(*kind)?.map_or(0, |v| v.value());
        ct.insert(*kind, was + n)?;
    }
    let mut kc = txn.open_table(KEYCOUNTS)?;
    let mut new_keys: BTreeMap<RecordKind, u64> = BTreeMap::new();
    {
        let mut born = txn.open_table(BORN)?;
        let mut bybirth = txn.open_table(BYBIRTH)?;
        for (kind, first, kb) in births {
            *new_keys.entry(*kind).or_default() += 1;
            put_born(&mut born, &mut bybirth, *first, kb)?;
        }
    }
    for (kind, n) in &new_keys {
        let was = kc.get(*kind)?.map_or(0, |v| v.value());
        kc.insert(*kind, was + n)?;
    }
    let mut sc = txn.open_table(SCOPECOUNTS)?;
    for (scope, n) in &scopes {
        let was = sc.get(scope.as_bytes())?.map_or(0, |v| v.value());
        sc.insert(scope.as_bytes(), was + n)?;
    }
    if !posts.is_empty() {
        if sort_tags {
            posts.sort_unstable();
        }
        let mut t = txn.open_table(TAGGED)?;
        for k in &posts {
            t.insert(k.as_slice(), ())?;
        }
    }
    Ok(())
}

/// Replace `key`'s numbers in its kind's totals with `new`: the totals lose
/// the key's last ones and gain these, so they always add up the latest
/// record of every key.
fn put_sums(
    sums: &mut redb::Table<u16, &[u8]>,
    sumsof: &mut redb::Table<&[u8], &[u8]>,
    kind: RecordKind,
    key: &str,
    new: &Sums,
) -> Result<()> {
    let k = bykey(kind, key);
    let old = sumsof
        .get(k.as_slice())?
        .map(|v| sums_from(v.value()))
        .unwrap_or_default();
    if old == *new {
        return Ok(());
    }
    let mut total = sums
        .get(kind)?
        .map(|v| sums_from(v.value()))
        .unwrap_or_default();
    for ((t, o), n) in total.iter_mut().zip(old).zip(new) {
        *t = t.wrapping_sub(o).wrapping_add(*n);
    }
    sums.insert(kind, sums_bytes(&total).as_slice())?;
    sumsof.insert(k.as_slice(), sums_bytes(new).as_slice())?;
    Ok(())
}

/// `terms`, `termsof`, and `termcounts`, open in one write transaction.
type Tables<'a, 't> = (
    &'a mut redb::Table<'t, &'static [u8], u64>,
    &'a mut redb::Table<'t, &'static [u8], &'static [u8]>,
    &'a mut redb::Table<'t, &'static [u8], u64>,
);

/// Add `by` to how many keys of `kind` have `term` (theseus-vm3n.5).
fn bump(
    counts: &mut redb::Table<&'static [u8], u64>,
    kind: RecordKind,
    term: &str,
    by: i64,
) -> Result<()> {
    let k = term_bounds(kind, term, term).0;
    let was = counts.get(k.as_slice())?.map_or(0, |v| v.value());
    let now = was.saturating_add_signed(by);
    if now == 0 {
        counts.remove(k.as_slice())?;
    } else {
        counts.insert(k.as_slice(), now)?;
    }
    Ok(())
}

/// Replace `key`'s terms with `new`, each pointing at `position`.
fn put_terms(
    (terms, termsof, counts): Tables<'_, '_>,
    kind: RecordKind,
    key: &str,
    new: &[String],
    position: u64,
) -> Result<()> {
    let k = bykey(kind, key);
    let old = termsof
        .get(k.as_slice())?
        .map(|v| terms_from(v.value()))
        .unwrap_or_default();
    for t in old.iter().filter(|t| !new.contains(t)) {
        if terms.remove(term_key(kind, t, key).as_slice())?.is_some() {
            bump(counts, kind, t, -1)?;
        }
    }
    for t in new {
        debug_assert!(!t.is_empty() && !t.contains('\0'), "a term: {t:?}");
        if terms
            .insert(term_key(kind, t, key).as_slice(), position)?
            .is_none()
        {
            bump(counts, kind, t, 1)?;
        }
    }
    if new.is_empty() {
        termsof.remove(k.as_slice())?;
    } else if old != new {
        termsof.insert(k.as_slice(), terms_bytes(new).as_slice())?;
    }
    Ok(())
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
            terms: None,
            sums: None,
            at_unix_ms: 0,
            tags: Vec::new(),
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
        idx.set_checkpoint(5, None).unwrap();
        assert_eq!(idx.checkpoint().unwrap(), Some(5));
        drop(idx);
        let idx = RedbIndex::open(&path).unwrap();
        assert_eq!(idx.checkpoint().unwrap(), Some(5));
        assert_eq!(idx.latest_position(1, "s1").unwrap(), Some(4));
        assert_eq!(idx.count_keys(1).unwrap(), 2);
        assert_eq!(idx.keys_with_prefix(1, "s").unwrap().len(), 2);
        assert_eq!(
            idx.keys_with_prefix(1, "s2").unwrap(),
            vec![("s2".to_string(), 3)]
        );
        assert!(idx.keys_with_prefix(1, "t").unwrap().is_empty());
    }

    /// A key's terms follow its latest record: a new record's terms replace
    /// the last ones, a term's keys come back in key order, each once, and a
    /// checkpoint marks them whole only for the projection that wrote them
    /// (theseus-lv2).
    #[test]
    fn terms_follow_each_keys_latest_record() {
        let dir = tempfile::tempdir().unwrap();
        let idx = RedbIndex::open(&dir.path().join("index.redb")).unwrap();
        let e = |p: u64, key: &str, terms: &[&str]| IndexEntry {
            position: p,
            kind: 4,
            key: Some(key.into()),
            scope: None,
            loc: Location {
                segment: 1,
                offset: p * 10,
                len: 10,
            },
            terms: Some(terms.iter().map(|t| t.to_string()).collect()),
            sums: None,
            at_unix_ms: 0,
            tags: Vec::new(),
        };
        let keys = |lo: &str, hi: &str| -> Vec<String> {
            idx.keys_by_terms(4, lo, hi)
                .unwrap()
                .into_iter()
                .map(|(k, _)| k)
                .collect()
        };
        idx.apply(
            &[
                e(1, "b", &["s:waiting"]),
                e(2, "a", &["s:waiting", "due"]),
                e(3, "c", &["s:running", "l:05"]),
            ],
            false,
        )
        .unwrap();
        assert_eq!(keys("s:waiting", "s:waiting\u{1}"), ["a", "b"]);
        assert_eq!(keys("s:", "s;"), ["a", "b", "c"], "a key once");
        assert_eq!(idx.count_by_terms(4, "s:", "s;").unwrap(), 3);
        assert_eq!(keys("due", "due\u{1}"), ["a"]);
        // The next record of "a" replaces its terms; of "c", drops them.
        idx.apply(&[e(4, "a", &["s:queued"]), e(5, "c", &[])], false)
            .unwrap();
        assert_eq!(keys("s:waiting", "s:waiting\u{1}"), ["b"]);
        assert!(keys("due", "due\u{1}").is_empty());
        assert!(keys("l:", "l;").is_empty());
        assert_eq!(
            idx.keys_by_terms(4, "s:queued", "s:queued\u{1}").unwrap(),
            [("a".to_string(), 4)]
        );
        assert_eq!(idx.terms_of(4, "a").unwrap(), ["s:queued"]);
        assert!(idx.terms_of(4, "c").unwrap().is_empty());
        // A record with no terms (`None`) leaves its key's as they were.
        let mut plain = e(6, "b", &[]);
        plain.terms = None;
        idx.apply(&[plain], false).unwrap();
        assert_eq!(idx.terms_of(4, "b").unwrap(), ["s:waiting"]);
        // Whole at a checkpoint only under the name that kept them.
        idx.set_checkpoint(6, Some("terms.test.1")).unwrap();
        assert_eq!(idx.terms_mark("terms.test.1").unwrap(), Some(6));
        idx.set_checkpoint(7, None).unwrap();
        assert_eq!(idx.terms_mark("terms.test.1").unwrap(), Some(6));
        assert_eq!(idx.terms_mark("terms.test.2").unwrap(), None);
        // A build after serving walks the keys a stretch at a time, and puts
        // a key's terms only while its latest record is the one it read.
        let all = idx.keys_of_kind_after(4, None, 10).unwrap();
        assert_eq!(all, [("a".into(), 4), ("b".into(), 6), ("c".into(), 5)]);
        assert_eq!(
            idx.keys_of_kind_after(4, Some("a"), 1).unwrap(),
            [("b".to_string(), 6)]
        );
        assert!(idx.keys_of_kind_after(4, Some("c"), 10).unwrap().is_empty());
        let put = idx
            .put_terms_if_latest(&[
                (4, "b".into(), 6, vec!["s:blocked".into()], None),
                // Read at 5, and "c" is still at 5: put.
                (4, "c".into(), 5, vec!["s:waiting".into()], None),
                // Read at 1: "a" has moved on to 4, which put its own.
                (4, "a".into(), 1, vec!["s:waiting".into()], None),
            ])
            .unwrap();
        assert_eq!(put, 2);
        assert_eq!(keys("s:blocked", "s:blocked\u{1}"), ["b"]);
        assert_eq!(keys("s:waiting", "s:waiting\u{1}"), ["c"]);
        assert_eq!(idx.terms_of(4, "a").unwrap(), ["s:queued"]);
    }

    /// A kind's totals add up each key's latest numbers: a key's next record
    /// replaces its last ones, and a record with none leaves them
    /// (theseus-lv2).
    #[test]
    fn totals_add_up_each_keys_latest_numbers() {
        let dir = tempfile::tempdir().unwrap();
        let idx = RedbIndex::open(&dir.path().join("index.redb")).unwrap();
        let e = |p: u64, key: &str, sums: Option<Sums>| IndexEntry {
            position: p,
            kind: 1,
            key: Some(key.into()),
            scope: None,
            loc: Location {
                segment: 1,
                offset: p * 10,
                len: 10,
            },
            terms: None,
            sums,
            at_unix_ms: 0,
            tags: Vec::new(),
        };
        let n = |a: u128, b: u128| Some([1, a, b, 0, 0, 0, 0, 0]);
        assert_eq!(idx.totals(1).unwrap(), [0; 8]);
        idx.apply(&[e(1, "s1", n(1, 10)), e(2, "s2", n(2, 20))], false)
            .unwrap();
        assert_eq!(idx.totals(1).unwrap(), [2, 3, 30, 0, 0, 0, 0, 0]);
        // s1 again: its numbers replace its last ones; a plain record of s2
        // leaves s2's.
        idx.apply(&[e(3, "s1", n(4, 15)), e(4, "s2", None)], false)
            .unwrap();
        assert_eq!(idx.totals(1).unwrap(), [2, 6, 35, 0, 0, 0, 0, 0]);
        assert_eq!(idx.totals(2).unwrap(), [0; 8], "another kind's own");
        // The bulk build and the build after serving add up the same.
        let bulk = tempfile::tempdir().unwrap();
        let other = RedbIndex::open(&bulk.path().join("index.redb")).unwrap();
        other
            .apply_bulk(&[
                e(1, "s1", n(1, 10)),
                e(2, "s2", n(2, 20)),
                e(3, "s1", n(4, 15)),
                e(4, "s2", None),
            ])
            .unwrap();
        assert_eq!(other.totals(1).unwrap(), [2, 6, 35, 0, 0, 0, 0, 0]);
        other
            .put_terms_if_latest(&[(1, "s2".into(), 4, vec![], n(5, 50))])
            .unwrap();
        assert_eq!(other.totals(1).unwrap(), [2, 9, 65, 0, 0, 0, 0, 0]);
    }
}
