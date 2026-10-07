//! Reads that skip keys by a predicate on the key (theseus-7087): a key the
//! predicate fails is never read from the log, and the answer is the one a
//! read of every record, filtered, gives.

use super::*;
use crate::record::kinds;

fn open(dir: &Path) -> WalStore {
    WalStore::open(dir, WalConfig::default())
        .unwrap()
        .with_checkpoint_every(0)
}

fn session(key: &str, v: u64) -> NewRecord {
    NewRecord::json(kinds::SESSION, Some(key), &serde_json::json!({"v": v})).unwrap()
}

/// Skipped keys start so, as an imported session's do.
fn skipped(k: &str) -> bool {
    k.starts_with("ses_ep")
}

/// A store with live keys born before, between, and after two runs of
/// skipped ones, some keys written twice.
fn store_with_runs(dir: &Path) -> WalStore {
    let s = open(dir);
    let mut live = 0u64;
    let mut add_live = |s: &WalStore, n: u64| {
        for _ in 0..n {
            s.append(&[session(&format!("ses_{live:04x}"), live)])
                .unwrap();
            live += 1;
        }
    };
    add_live(&s, 5);
    for run in 0..2u64 {
        let batch: Vec<NewRecord> = (0..300)
            .map(|i| session(&format!("ses_ep{run}{i:04}"), i))
            .collect();
        for chunk in batch.chunks(50) {
            s.append(chunk).unwrap();
        }
        add_live(&s, 7);
    }
    // A live key and a skipped one written again: their births stay.
    s.append(&[session("ses_0001", 99), session("ses_ep00007", 99)])
        .unwrap();
    s
}

#[test]
fn latest_of_kind_where_reads_only_the_kept_keys_records() {
    let dir = tempfile::tempdir().unwrap();
    let s = store_with_runs(dir.path());
    let want: Vec<(String, u64)> = s
        .latest_of_kind(kinds::SESSION)
        .unwrap()
        .into_iter()
        .filter(|r| !skipped(r.key.as_deref().unwrap()))
        .map(|r| (r.key.unwrap(), r.position))
        .collect();
    let before = records_read_here();
    let got = s
        .latest_of_kind_where(kinds::SESSION, &|k| !skipped(k))
        .unwrap();
    let read = records_read_here() - before;
    let got: Vec<(String, u64)> = got
        .into_iter()
        .map(|r| (r.key.unwrap(), r.position))
        .collect();
    assert_eq!(got, want);
    assert_eq!(want.len(), 19);
    assert_eq!(read, 19, "only the kept keys' records are read");
}

/// Every page of the filtered walk equals the page an unfiltered walk, `n`
/// at a time, gives once it drops the skipped keys: the same keys, births,
/// and cursors. And each page reads its own records alone, however long
/// the skipped run it steps over.
#[test]
fn newest_keys_where_steps_over_a_skipped_run_in_one_walk() {
    let dir = tempfile::tempdir().unwrap();
    let s = store_with_runs(dir.path());
    // The walk the filter replaces: `n` keys at a time, skipping by key.
    let walked = |n: usize, before: Option<u64>| {
        let mut out = Vec::new();
        let mut cursor = before;
        loop {
            let (born, more) = s.newest_keys(kinds::SESSION, cursor, n).unwrap().unwrap();
            for (b, r) in &born {
                if skipped(r.key.as_deref().unwrap()) {
                    continue;
                }
                out.push((*b, r.key.clone().unwrap(), r.position));
                if out.len() == n {
                    let last = born.last().map(|(l, _)| *l) == Some(*b);
                    return (out, (more || !last).then_some(*b));
                }
            }
            match born.last() {
                Some((b, _)) if more => cursor = Some(*b),
                _ => return (out, None),
            }
        }
    };
    for n in [1, 2, 3, 5, 7, 20, 1000] {
        let mut cursor = None;
        let mut pages = 0;
        loop {
            let (want, want_older) = walked(n, cursor);
            let before = records_read_here();
            let (got, more) = s
                .newest_keys_where(kinds::SESSION, cursor, n, &|k| !skipped(k))
                .unwrap()
                .unwrap();
            let read = records_read_here() - before;
            assert_eq!(read, got.len() as u64, "a page reads its own records alone");
            let older = more.then(|| got.last().map(|(b, _)| *b)).flatten();
            let got: Vec<(u64, String, u64)> = got
                .into_iter()
                .map(|(b, r)| (b, r.key.unwrap(), r.position))
                .collect();
            assert_eq!(got, want, "page {pages} of {n}");
            assert_eq!(older, want_older, "page {pages} of {n}'s cursor");
            pages += 1;
            match older {
                Some(b) => cursor = Some(b),
                None => break,
            }
        }
        assert!(pages >= 19usize.div_ceil(n), "{n}: {pages} pages");
    }
}

/// What `index_rows_here` counts (theseus-26jo): each row a key walk or the
/// births walk yields, and each lookup of one key.
#[test]
fn the_index_rows_a_walk_visits_are_counted_on_its_thread() {
    let dir = tempfile::tempdir().unwrap();
    let s = store_with_runs(dir.path());
    let rows = |f: &dyn Fn()| {
        let before = index_rows_here();
        f();
        index_rows_here() - before
    };
    // 19 live keys and 600 skipped ones.
    assert_eq!(
        rows(&|| drop(s.latest_of_kind(kinds::SESSION).unwrap())),
        619
    );
    assert_eq!(
        rows(&|| drop(s.latest_by_key(kinds::SESSION, "ses_0001").unwrap())),
        1
    );
    // The newest page of 3: the 7 newest births are live, so it visits 3
    // birth rows, looks up 3 keys, and meets the fourth row to say more.
    assert_eq!(
        rows(&|| drop(s.newest_keys(kinds::SESSION, None, 3).unwrap())),
        7
    );
}
