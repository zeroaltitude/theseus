//! What the vector table holds, holder by holder (theseus-xxac7): the index
//! tender's resident set on a real store is this table, and a number with no
//! holder's name on it cannot be shrunk. `heap.rs` measures the heap a chunk
//! costs in all; this says where it went.
//!
//! Each figure is what a holder has **asked the allocator for** — its
//! capacity, not its length — so the sum is the ask and not the resident set:
//! the allocator's own chunk headers and rounding are outside it, as are the
//! 768-d vectors (on disk), tantivy's own memory, and the embedder's weights.
//! `the_breakdown_accounts_for_the_heap_it_measures` holds the sum against
//! glibc's own count so these lines cannot drift into fiction.
//!
//! To read it on a real index, with the tender stopped (a copy of the index
//! is enough; it is opened read-only and nothing is written):
//!
//! ```text
//! THESEUS_HELD_INDEX=<state>/index cargo nextest run -p theseus-index \
//!   --run-ignored only -E 'test(what_an_index_holds)' --no-capture
//! ```

use std::fmt;
use std::mem::size_of;
use std::sync::Arc;

use super::{Cache, Holders, Interner, Row, Slot, Table};

/// What `capacity` entries of `entry` bytes cost in a `std` hash map or set:
/// `hashbrown` allocates a power-of-two number of buckets, seven eighths of
/// which it will fill, and one control byte each.
fn map_bytes(capacity: usize, entry: usize) -> u64 {
    if capacity == 0 {
        return 0;
    }
    let mut buckets = 8usize;
    while buckets - buckets / 8 < capacity {
        buckets *= 2;
    }
    (buckets * (entry + 1)) as u64
}

/// What `len` elements of `elem` bytes cost in a `BTreeSet`: an **estimate**,
/// the one figure here that is not read from a capacity. A B-tree's nodes hold
/// 11 elements and stand between half and wholly full; this takes them two
/// thirds full, so the figure is within about a third either way. Nothing in
/// the tree reports the true one, which is why the cross-check below measures
/// a table whose backlog is empty.
fn tree_bytes(len: usize, elem: usize) -> u64 {
    if len == 0 {
        return 0;
    }
    ((len / 7 + 1) * (11 * elem + 16)) as u64
}

/// An interned name table: its names, their bytes, and its index.
fn interner_bytes(i: &Interner) -> u64 {
    let names = (i.names.capacity() * size_of::<String>()) as u64;
    let text: u64 = i.names.iter().map(|s| s.capacity() as u64).sum();
    // The index holds a second copy of every name.
    names + text * 2 + map_bytes(i.ids.capacity(), size_of::<String>() + size_of::<u32>())
}

/// The heap the vector table holds, by holder, in bytes (see the module's
/// doc). `chunks` and `records` are counts, not bytes.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Held {
    /// Rows: one per chunk the index holds.
    pub chunks: u64,
    /// Records: one per distinct chunk text with a vector.
    pub records: u64,
    /// `Cache::q`: each record's int8 cut of the embedding.
    pub cuts: u64,
    /// `Table::rows`: one [`Row`] a chunk.
    pub rows: u64,
    /// `Table::by_hash`: the alive rows that hold each text.
    pub rows_by_hash: u64,
    /// `Cache::hashes`: each record's text hash.
    pub hashes: u64,
    /// `Table::by_node`: each node's row numbers, and the node ids its rows
    /// share.
    pub by_node: u64,
    /// `Cache::by_hash`: a file's records by hash (`Entries`).
    pub records_by_hash: u64,
    /// `Cache::scale` and `Cache::dead`: a record's scale, and whether a row
    /// still holds its text.
    pub scales: u64,
    /// `Table::backlog`: the texts waiting for a vector (estimated).
    pub backlog: u64,
    /// The sessions' and kinds' names, the other spaces' files, and what a
    /// reconcile must not add back.
    pub rest: u64,
}

impl Held {
    /// Each holder and its bytes, largest first, ties by name.
    pub fn lines(&self) -> Vec<(&'static str, u64)> {
        let mut v = vec![
            ("the int8 cuts", self.cuts),
            ("the rows", self.rows),
            ("the rows by text", self.rows_by_hash),
            ("the record hashes", self.hashes),
            ("the rows by node", self.by_node),
            ("the records by hash", self.records_by_hash),
            ("the scales and the dead flags", self.scales),
            ("the backlog", self.backlog),
            ("the names and the rest", self.rest),
        ];
        v.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(b.0)));
        v
    }

    /// Every holder's bytes.
    pub fn total(&self) -> u64 {
        self.lines().iter().map(|(_, b)| b).sum()
    }

    /// Bytes a chunk, or 0 with no chunk.
    pub fn per_chunk(&self) -> f64 {
        if self.chunks == 0 {
            return 0.0;
        }
        self.total() as f64 / self.chunks as f64
    }
}

impl fmt::Display for Held {
    /// The table a reader prints: the total, then each holder with its share
    /// and its bytes a chunk.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let total = self.total();
        writeln!(
            f,
            "{:.1} MiB over {} chunks and {} records: {:.0} bytes a chunk",
            total as f64 / 1_048_576.0,
            self.chunks,
            self.records,
            self.per_chunk()
        )?;
        let chunks = self.chunks.max(1) as f64;
        for (name, bytes) in self.lines() {
            let share = if total == 0 {
                0.0
            } else {
                100.0 * bytes as f64 / total as f64
            };
            writeln!(
                f,
                "  {name:30} {:>9.1} MiB  {share:>4.1}%  {:>6.1} bytes a chunk",
                bytes as f64 / 1_048_576.0,
                bytes as f64 / chunks
            )?;
        }
        Ok(())
    }
}

impl Table {
    /// What this table holds, holder by holder (theseus-xxac7). It walks every
    /// row's maps, so it is a measure and not a turn's path.
    pub fn held(&self) -> Held {
        let mut h = Held {
            chunks: self.rows.len() as u64,
            rows: (self.rows.capacity() * size_of::<Row>()) as u64,
            rows_by_hash: map_bytes(
                self.by_hash.capacity(),
                size_of::<u128>() + size_of::<Holders>(),
            ) + self
                .by_hash
                .values()
                .map(|v| match v {
                    Holders::One(_) => 0,
                    Holders::Many(v) => (v.capacity() * size_of::<u32>()) as u64,
                })
                .sum::<u64>(),
            by_node: map_bytes(
                self.by_node.capacity(),
                size_of::<Arc<str>>() + size_of::<Vec<u32>>(),
            ) + self
                .by_node
                .iter()
                // An `Arc<str>`'s own block: two counts and the bytes.
                .map(|(k, v)| (2 * size_of::<usize>() + k.len() + v.capacity() * 4) as u64)
                .sum::<u64>(),
            backlog: map_bytes(
                self.backlog.items.capacity(),
                size_of::<u128>() + size_of::<(u32, Slot)>(),
            ) + tree_bytes(self.backlog.order.len(), size_of::<Slot>())
                + self
                    .backlog
                    .buckets
                    .iter()
                    .map(|b| tree_bytes(b.len(), size_of::<Slot>()))
                    .sum::<u64>(),
            rest: interner_bytes(&self.sessions)
                + interner_bytes(&self.kinds)
                + (self.others.capacity()
                    * size_of::<(super::Stamp, super::PathBuf, super::PathBuf)>())
                    as u64
                + map_bytes(self.forgot_nodes.capacity(), size_of::<String>())
                + map_bytes(self.forgot_chunks.capacity(), size_of::<(String, u32)>()),
            ..Held::default()
        };
        for c in &self.caches {
            h.records += c.len() as u64;
            h.cuts += c.q.capacity() as u64;
            h.hashes += (c.hashes.capacity() * size_of::<u128>()) as u64;
            h.scales += (c.scale.capacity() * size_of::<f32>() + c.dead.capacity()) as u64;
            h.records_by_hash += (c.by_hash.slots() * size_of::<u32>()) as u64;
        }
        h
    }
}

impl Cache {
    /// What this one file's memory holds, holder by holder: the same lines a
    /// whole table's `held` folds its files into.
    #[cfg(test)]
    pub(super) fn held(&self) -> Held {
        Held {
            records: self.len() as u64,
            cuts: self.q.capacity() as u64,
            hashes: (self.hashes.capacity() * size_of::<u128>()) as u64,
            scales: (self.scale.capacity() * size_of::<f32>() + self.dead.capacity()) as u64,
            records_by_hash: (self.by_hash.slots() * size_of::<u32>()) as u64,
            ..Held::default()
        }
    }
}

/// The row's own measure on a real index (theseus-xxac7), as `heap.rs`'s
/// `fill` module is a tool beside its tests: the tender stopped, the index (or
/// a copy of it) opened read-only, the table built as a start builds it, and
/// the breakdown printed. It reads node ids, sessions, kinds and text hashes,
/// never a chunk's text, and prints only counts and bytes.
#[cfg(test)]
mod tool {
    use super::super::*;
    use crate::engine::{open_or_create, Engine};

    #[test]
    #[ignore = "a tool: reads the index THESEUS_HELD_INDEX names"]
    fn what_an_index_holds() {
        let Ok(dir) = std::env::var("THESEUS_HELD_INDEX") else {
            eprintln!("set THESEUS_HELD_INDEX=<state>/index (a stopped tender's, or a copy)");
            return;
        };
        let dir = PathBuf::from(dir);
        let (index, fields) = open_or_create(&crate::state::Paths::new(&dir).bm25()).unwrap();
        let engine = Engine::new(index, fields).unwrap();
        let nodes = engine.all_nodes().unwrap();
        let mut t = Table::default();
        let vdir = dir.join("vectors");
        if let Ok(entries) = fs::read_dir(&vdir) {
            for e in entries.flatten() {
                let path = e.path();
                if path.extension().and_then(|x| x.to_str()) != Some("json") {
                    continue;
                }
                if let Some(stamp) = crate::state::load::<Stamp>(&path) {
                    t.caches.push(Cache::open(&vdir, &stamp).unwrap());
                }
            }
        }
        t.opened = true;
        for n in &nodes {
            t.add_node(n);
        }
        eprintln!("{} nodes in {}\n{}", nodes.len(), dir.display(), t.held());
    }
}

#[cfg(test)]
mod tests {
    use super::{map_bytes, tree_bytes};
    use std::collections::{BTreeSet, HashMap};

    /// `map_bytes` against a real map's own capacity: the buckets a `std` map
    /// reports room for are the ones this counts.
    #[test]
    fn map_bytes_follows_the_maps_own_capacity() {
        for n in [1usize, 7, 8, 9, 100, 3000] {
            let mut m: HashMap<u64, u64> = HashMap::with_capacity(n);
            for i in 0..n as u64 {
                m.insert(i, i);
            }
            let b = map_bytes(m.capacity(), 16);
            let least = (n * 17) as u64;
            assert!(
                b >= least && b <= 4 * least.max(136),
                "{n} entries: {b} bytes against {least} of payload"
            );
        }
        assert_eq!(map_bytes(0, 16), 0);
    }

    /// The estimate for a B-tree is within a third of what its elements hold,
    /// and zero for none.
    #[test]
    fn the_btree_estimate_stays_near_its_elements() {
        assert_eq!(tree_bytes(0, 32), 0);
        let s: BTreeSet<u64> = (0..10_000u64).collect();
        let b = tree_bytes(s.len(), 8);
        let payload = (s.len() * 8) as u64;
        assert!(b > payload && b < 4 * payload, "{b} against {payload}");
    }
}
