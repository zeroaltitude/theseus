//! A failed index write after a good sync is tried again (theseus-lu5n): the
//! frames are durable, so the batch is answered Ok once the index has them,
//! and fails only after `INDEX_TRIES` failures. Safe because `apply` over
//! what a failed one left equals one `apply` (`tests/index_reapply.rs`).

use super::*;
use crate::record::kinds;

fn open(dir: &Path) -> WalStore {
    WalStore::open(dir, WalConfig::default())
        .unwrap()
        .with_checkpoint_every(0)
}

fn meta(key: &str, v: u64) -> NewRecord {
    NewRecord::json(kinds::META, Some(key), &v).unwrap()
}

/// Keys written, ended inside one batch, ended again, and written afresh:
/// the batches whose index write is several transactions.
fn script(s: &WalStore) {
    s.append(&[meta("a.1", 1), meta("a.2", 2), meta("b.1", 3)])
        .unwrap();
    s.append(&[
        meta("a.2", 20),
        NewRecord::delete_meta("a.1"),
        meta("c.1", 4),
        NewRecord::delete_meta("b.1"),
        meta("a.1", 5),
    ])
    .unwrap();
    s.append(&[NewRecord::delete_meta("a.1"), NewRecord::delete_meta("zz")])
        .unwrap();
    s.append(&[meta("a.1", 100)]).unwrap();
}

type Reads = (Vec<(String, u64)>, u64, u64, u64, Vec<String>);

fn reads(s: &WalStore) -> Reads {
    let latest = s
        .latest_of_kind(kinds::META)
        .unwrap()
        .into_iter()
        .map(|r| (r.key.clone().unwrap(), r.decode::<u64>().unwrap()))
        .collect();
    let (keys, walked) = (
        s.count_keys(kinds::META).unwrap(),
        s.inner.index.count_keys_walked(kinds::META).unwrap(),
    );
    assert_eq!(keys, walked, "count_keys maintained != walked");
    let (born, _) = s
        .inner
        .index
        .keys_by_birth(kinds::META, None, usize::MAX)
        .unwrap();
    (
        latest,
        keys,
        s.count_of_kind(kinds::META).unwrap(),
        s.count_of_kind(kinds::TOMBSTONE).unwrap(),
        born.into_iter().map(|(_, k, _)| k).collect(),
    )
}

#[test]
fn a_failed_index_write_is_retried_and_the_batch_is_answered_ok() {
    let dir = tempfile::tempdir().unwrap();
    let s = open(dir.path());
    s.inner.index_fail.store(1, Ordering::SeqCst);
    let placed = s.append(&[meta("a.1", 1)]).expect("the retry indexes it");
    assert_eq!(placed.len(), 1);
    assert_eq!(s.inner.index_fail.load(Ordering::SeqCst), 0);
    assert_eq!(
        s.latest_by_key(kinds::META, "a.1")
            .unwrap()
            .unwrap()
            .decode::<u64>()
            .unwrap(),
        1
    );
    assert_eq!(s.count_keys(kinds::META).unwrap(), 1);
}

/// Covers a retry whose first try failed before it touched the index (the
/// hook's `index_fail`), so no retry here runs over leftovers; see
/// `a_batch_whose_index_write_landed_but_failed_is_retried_to_the_same_index`.
#[test]
fn a_tombstone_batch_retried_equals_the_same_batch_untouched() {
    let (d1, d2) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let (plain, retried) = (open(d1.path()), open(d2.path()));
    script(&plain);
    // The second append's index write fails once: the first try is spent.
    retried
        .append(&[meta("a.1", 1), meta("a.2", 2), meta("b.1", 3)])
        .unwrap();
    retried.inner.index_fail.store(2, Ordering::SeqCst);
    retried
        .append(&[
            meta("a.2", 20),
            NewRecord::delete_meta("a.1"),
            meta("c.1", 4),
            NewRecord::delete_meta("b.1"),
            meta("a.1", 5),
        ])
        .unwrap();
    assert_eq!(retried.inner.index_fail.load(Ordering::SeqCst), 0);
    retried
        .append(&[NewRecord::delete_meta("a.1"), NewRecord::delete_meta("zz")])
        .unwrap();
    retried.append(&[meta("a.1", 100)]).unwrap();
    let want = reads(&plain);
    assert!(want.1 >= 3 && want.3 >= 3, "{want:?}");
    assert_eq!(reads(&retried), want);
}

#[test]
fn an_index_that_keeps_failing_still_fails_the_batch() {
    let dir = tempfile::tempdir().unwrap();
    let s = open(dir.path());
    s.inner.index_fail.store(100, Ordering::SeqCst);
    let err = s.append(&[meta("a.1", 1)]).unwrap_err();
    assert!(format!("{err:#}").contains("index"), "{err:#}");
    assert_eq!(s.inner.index_fail.load(Ordering::SeqCst), 100 - 3);
}

#[test]
fn a_failed_batch_comes_back_at_the_open_though_a_checkpoint_ran() {
    let dir = tempfile::tempdir().unwrap();
    {
        let s = open(dir.path());
        s.inner.index_fail.store(INDEX_TRIES, Ordering::SeqCst);
        assert!(s.append(&[meta("a.1", 1)]).is_err());
        s.append(&[meta("b.1", 2)]).unwrap();
        // A checkpoint may not claim the failed frame's position: the open
        // replays only what follows the index's checkpoint.
        assert!(s.checkpoint().is_err(), "refused while the index has a gap");
    }
    let s = open(dir.path());
    let a = s.latest_by_key(kinds::META, "a.1").unwrap().unwrap();
    assert_eq!(a.decode::<u64>().unwrap(), 1, "the failed frame came back");
    let (keys, walked) = (
        s.count_keys(kinds::META).unwrap(),
        s.inner.index.count_keys_walked(kinds::META).unwrap(),
    );
    assert_eq!((keys, walked), (2, 2));
}

#[test]
fn a_failed_batch_comes_back_at_the_open_without_a_checkpoint() {
    let dir = tempfile::tempdir().unwrap();
    {
        let s = open(dir.path());
        s.inner.index_fail.store(INDEX_TRIES, Ordering::SeqCst);
        assert!(s.append(&[meta("a.1", 1)]).is_err());
        s.append(&[meta("b.1", 2)]).unwrap();
    }
    let s = open(dir.path());
    let a = s.latest_by_key(kinds::META, "a.1").unwrap().unwrap();
    assert_eq!(a.decode::<u64>().unwrap(), 1);
    assert_eq!(s.count_keys(kinds::META).unwrap(), 2);
}

#[test]
fn a_batch_whose_index_write_landed_but_failed_is_retried_to_the_same_index() {
    let (d1, d2) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let (plain, retried) = (open(d1.path()), open(d2.path()));
    script(&plain);
    retried
        .append(&[meta("a.1", 1), meta("a.2", 2), meta("b.1", 3)])
        .unwrap();
    // The tombstone batch's first try writes the whole batch and then fails:
    // the retry runs over an index that already holds all of it.
    retried.inner.index_fail_after.store(1, Ordering::SeqCst);
    retried
        .append(&[
            meta("a.2", 20),
            NewRecord::delete_meta("a.1"),
            meta("c.1", 4),
            NewRecord::delete_meta("b.1"),
            meta("a.1", 5),
        ])
        .unwrap();
    assert_eq!(retried.inner.index_fail_after.load(Ordering::SeqCst), 0);
    retried
        .append(&[NewRecord::delete_meta("a.1"), NewRecord::delete_meta("zz")])
        .unwrap();
    retried.append(&[meta("a.1", 100)]).unwrap();
    let want = reads(&plain);
    assert!(want.1 >= 3 && want.3 >= 3, "{want:?}");
    assert_eq!(reads(&retried), want);
}

#[test]
fn an_index_gap_is_said_once_and_the_periodic_checkpoint_stays_quiet() {
    let dir = tempfile::tempdir().unwrap();
    let s = WalStore::open(dir.path(), WalConfig::default())
        .unwrap()
        .with_checkpoint_every(1);
    assert_eq!(s.index_gap(), None);
    s.inner.index_fail.store(INDEX_TRIES, Ordering::SeqCst);
    assert!(s.append(&[meta("a.1", 1)]).is_err());
    s.append(&[meta("b.1", 2)]).unwrap();
    s.inner.index_fail.store(INDEX_TRIES, Ordering::SeqCst);
    assert!(s.append(&[meta("c.1", 3)]).is_err());
    s.append(&[meta("d.1", 4)]).unwrap();
    let at = s.index_gap().expect("the gap is open");
    assert!((1..=s.last_position()).contains(&at), "{at}");
    // Dropping the store joins the writer, so every periodic checkpoint ran.
    let inner = s.inner.clone();
    drop(s);
    assert_eq!(
        inner.warned.load(Ordering::Relaxed),
        1,
        "one warning for the gap, none from the periodic checkpoint"
    );
    drop(inner);
    let s = open(dir.path());
    assert_eq!(s.index_gap(), None);
    assert_eq!(s.count_keys(kinds::META).unwrap(), 4);
}
