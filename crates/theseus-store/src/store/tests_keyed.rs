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
