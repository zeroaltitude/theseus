//! A start's rows take the index's nodes as they come (issue 21 of 1.0.7): a reconcile that was
//! handed a list of every node alive left that list's strings scattered through the heap, and
//! the pages they sat in could never be given back. These tests hold the stream: the rows grow
//! while the index is still being read, the lock is not held across the read, and a commit or a
//! forget that lands in the middle is honoured.

use std::collections::HashSet;

use super::*;

const NODES: u64 = 20_000;

fn id(i: u64) -> String {
    format!("nd_{i:08}")
}

fn node(i: u64, salt: u128) -> NodeChunks {
    NodeChunks {
        node_id: id(i),
        position: i,
        session: format!("ses_{}", i % 50),
        kind: "user_message".into(),
        external: false,
        chunks: (0..3)
            .map(|c| ChunkKey {
                chunk: c,
                hash: (u128::from(i) << 8 | u128::from(c)) + salt,
                tokens: 10,
            })
            .collect(),
    }
}

/// An index that hands its nodes one at a time and never as a list: `all_nodes` is a failure.
/// `at` is told what the table holds when node `look` is about to be handed, and may act on it.
struct Stream<'a> {
    v: &'a Vectors,
    look: u64,
    at: &'a dyn Fn(&Vectors),
}

impl Texts for Stream<'_> {
    fn chunk_text(&self, _: &str, _: u32) -> Option<String> {
        None
    }

    fn all_nodes(&self) -> anyhow::Result<Vec<NodeChunks>> {
        panic!("a start asked for every node as one list");
    }

    fn each_node(
        &self,
        size: &mut dyn FnMut(usize, usize),
        each: &mut dyn FnMut(NodeChunks),
    ) -> anyhow::Result<()> {
        size(NODES as usize, 3 * NODES as usize);
        for i in 0..NODES {
            if i == self.look {
                (self.at)(self.v);
            }
            each(node(i, 0));
        }
        Ok(())
    }
}

fn vectors(dir: &std::path::Path) -> Vectors {
    Vectors::new(VectorConfig::new(Some(dir.to_path_buf())), dir)
}

fn hashes_of(v: &Vectors, node_id: &str) -> Vec<u128> {
    let t = v.table.read().unwrap();
    t.by_node
        .get(node_id)
        .map(|ids| ids.iter().map(|&i| t.rows[i as usize].hash).collect())
        .unwrap_or_default()
}

/// The rows are there as the index is read, not after it: partway through, most of what came
/// has a row, and the table's lock is free for a query.
#[test]
fn the_rows_grow_while_the_index_is_read() {
    let tmp = tempfile::tempdir().unwrap();
    let v = vectors(tmp.path());
    let seen = std::cell::Cell::new((0usize, false));
    let look = |v: &Vectors| {
        let t = v.table.try_read();
        seen.set((t.as_ref().map_or(0, |t| t.rows.len()), t.is_ok()));
    };
    v.reconcile(&Stream {
        v: &v,
        look: 15_000,
        at: &look,
    })
    .unwrap();
    let (rows, free) = seen.get();
    assert!(free, "the lock was held across the read");
    assert!(
        rows >= 3 * 15_000 - 4_096,
        "{rows} rows when 45,000 chunks had been read (one batch is 4,096 chunks)"
    );
    let t = v.table.read().unwrap();
    assert_eq!(t.rows.len(), 3 * NODES as usize);
    assert_eq!(t.rows.capacity(), 3 * NODES as usize, "measured once");
    assert!(t.reconciled);
}

/// A node committed while the index is read keeps the commit's chunks: the read's copy of it,
/// older, is left out.
#[test]
fn a_commit_during_the_read_is_kept() {
    let tmp = tempfile::tempdir().unwrap();
    let v = vectors(tmp.path());
    let commit = |v: &Vectors| v.on_commit(&[node(17_000, 1 << 100)]);
    v.reconcile(&Stream {
        v: &v,
        look: 5_000,
        at: &commit,
    })
    .unwrap();
    let want: Vec<u128> = node(17_000, 1 << 100)
        .chunks
        .iter()
        .map(|c| c.hash)
        .collect();
    assert_eq!(hashes_of(&v, &id(17_000)), want);
    assert_eq!(v.table.read().unwrap().rows.len(), 3 * NODES as usize);
}

/// A node forgotten while the index is read stays forgotten, whether the read has handed it over
/// yet or not: one already in the rows goes, one still to come is not added.
#[test]
fn a_forget_during_the_read_holds() {
    let tmp = tempfile::tempdir().unwrap();
    let v = vectors(tmp.path());
    let forget = |v: &Vectors| {
        let gone = [id(100), id(18_000)];
        v.forget(&gone, &[(id(19_000), 1)], &HashSet::new())
            .unwrap();
    };
    v.reconcile(&Stream {
        v: &v,
        look: 10_000,
        at: &forget,
    })
    .unwrap();
    assert!(hashes_of(&v, &id(100)).is_empty(), "one already read");
    assert!(hashes_of(&v, &id(18_000)).is_empty(), "one still to come");
    assert_eq!(
        hashes_of(&v, &id(19_000)).len(),
        2,
        "its second chunk alone"
    );
    assert_eq!(hashes_of(&v, &id(17_999)).len(), 3);
}
