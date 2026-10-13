//! The ledger's hash chain (theseus-bmkxk): what the writer writes, and what
//! the walk finds. A tampered store is forged by writing frames straight to
//! the log, around the writer, so each test says what an editor with write
//! access to the log could do.

use super::*;
use crate::chain::{self, Opened};
use crate::chain_walk::{verify, Anchor, Fault, Report};
use crate::record::kinds;

fn open(dir: &Path) -> WalStore {
    WalStore::open(dir, WalConfig::default())
        .unwrap()
        .with_checkpoint_every(0)
}

/// A row as a caller builds it: an object of the ledger's shape.
fn row(n: u64) -> Vec<u8> {
    format!(
        "{{\"at_unix_ms\":{},\"kind\":\"turn.started\",\"session_id\":\"ses_a\",\"data\":{{\"n\":{n}}}}}",
        1_000 + n
    )
    .into_bytes()
}

fn put(s: &WalStore, n: u64) -> u64 {
    s.append(&[NewRecord::bytes(kinds::LEDGER, None, row(n))])
        .unwrap()[0]
}

fn walk(s: &WalStore) -> Report {
    verify(s, None, None).unwrap()
}

/// A chain forged by hand: its start row, then `n` rows, each sealed as the
/// writer seals them. `payloads[k]` is seq `k`.
fn forged(n: u64) -> Vec<Vec<u8>> {
    let mut out = Vec::new();
    let started = chain::started_row(1_000, 0, 0);
    let (p, mut prev) = chain::seal(&started, &chain::GENESIS, 0).unwrap();
    out.push(p);
    for i in 1..=n {
        let (p, l) = chain::seal(&row(i), &prev.digest, i).unwrap();
        prev = l;
        out.push(p);
    }
    out
}

/// A store whose log holds exactly these ledger payloads, one frame each.
fn written(payloads: &[Vec<u8>]) -> (tempfile::TempDir, WalStore) {
    written_after(&[], payloads)
}

/// The same, with `before` ledger payloads ahead of them.
fn written_after(before: &[Vec<u8>], payloads: &[Vec<u8>]) -> (tempfile::TempDir, WalStore) {
    let dir = tempfile::tempdir().unwrap();
    {
        let wal = Wal::open(&dir.path().join("wal"), WalConfig::default()).unwrap();
        for p in before.iter().chain(payloads) {
            wal.append(&[NewRecord::bytes(kinds::LEDGER, None, p.clone())])
                .unwrap();
        }
    }
    let s = open(dir.path());
    (dir, s)
}

fn broken(r: &Report) -> &crate::chain_walk::Break {
    r.broken.as_ref().expect("the walk found a break")
}

#[test]
fn a_fresh_store_starts_its_chain_and_verifies() {
    let dir = tempfile::tempdir().unwrap();
    let s = open(dir.path());
    assert!(s.ledger_head().unwrap().is_none(), "no chain before a row");
    let first = put(&s, 1);
    for n in 2..=5 {
        put(&s, n);
    }
    let r = walk(&s);
    assert!(r.broken.is_none(), "{r:?}");
    assert_eq!(r.rows, 6, "the start row and five rows");
    assert_eq!(r.unchained, 0);
    let head = r.head.unwrap();
    assert_eq!(head.link.seq, 5);
    assert_eq!(s.ledger_head().unwrap(), Some(head));
    assert_eq!(head.position, s.last_position());
    // The start row sits in the caller's own frame, before the caller's row,
    // and says no history came before it.
    let start = s.get(first - 1).unwrap().unwrap();
    assert!(start.payload.starts_with(b"{\"at_unix_ms\":"));
    let text = String::from_utf8(start.payload).unwrap();
    assert!(text.contains("\"kind\":\"ledger.chain_started\""), "{text}");
    assert!(text.contains("\"unchained_rows\":0"), "{text}");
}

#[test]
fn a_stored_row_is_the_callers_bytes_with_one_field_added() {
    let dir = tempfile::tempdir().unwrap();
    let s = open(dir.path());
    let pos = put(&s, 7);
    let stored = s.get(pos).unwrap().unwrap().payload;
    let Opened::Chained { canonical, link } = chain::open(&stored) else {
        panic!("not chained: {}", String::from_utf8_lossy(&stored));
    };
    assert_eq!(
        canonical,
        row(7),
        "the canonical bytes are what the caller wrote"
    );
    assert_eq!(link.seq, 1);
    // The chain field is the row's last, `<seq>.<64 hex>`: 8 + seq + 1 + 64 + ... bytes.
    let added = stored.len() - canonical.len();
    assert_eq!(added, ",\"chain\":\"1.\"".len() + 64, "bytes added per row");
    // And the stored row still decodes as the object it was.
    let v: serde_json::Value = serde_json::from_slice(&stored).unwrap();
    assert_eq!(v["data"]["n"], 7);
    assert_eq!(v["chain"].as_str().unwrap().len(), 66);
}

#[test]
fn a_restart_continues_the_chain_from_the_stored_head() {
    let dir = tempfile::tempdir().unwrap();
    let before = {
        let s = open(dir.path());
        for n in 1..=3 {
            put(&s, n);
        }
        s.ledger_head().unwrap().unwrap()
    };
    let s = open(dir.path());
    assert_eq!(
        s.ledger_head().unwrap(),
        Some(before),
        "read back at the open"
    );
    put(&s, 4);
    let r = walk(&s);
    assert!(r.broken.is_none(), "{r:?}");
    assert_eq!(r.rows, 5, "one start row, ever: {r:?}");
    assert_eq!(r.head.unwrap().link.seq, 4);
}

#[test]
fn a_store_from_before_the_chain_starts_one_that_pins_its_history() {
    let old: Vec<Vec<u8>> = (1..=3).map(row).collect();
    let dir = tempfile::tempdir().unwrap();
    {
        let wal = Wal::open(&dir.path().join("wal"), WalConfig::default()).unwrap();
        for p in &old {
            wal.append(&[NewRecord::bytes(kinds::LEDGER, None, p.clone())])
                .unwrap();
        }
    }
    let s = open(dir.path());
    let r = walk(&s);
    assert!(
        r.broken.is_none() && r.head.is_none(),
        "no chain until a write: {r:?}"
    );
    assert_eq!(r.unchained, 3);
    let pos = put(&s, 9);
    let start = String::from_utf8(s.get(pos - 1).unwrap().unwrap().payload).unwrap();
    assert!(start.contains("\"unchained_rows\":3"), "{start}");
    assert!(start.contains("\"through_position\":3"), "{start}");
    let r = walk(&s);
    assert!(r.broken.is_none(), "{r:?}");
    assert_eq!(r.rows, 2);
    assert_eq!(r.unchained, 3);
}

#[test]
fn concurrent_appenders_make_one_valid_chain() {
    let dir = tempfile::tempdir().unwrap();
    let s = Arc::new(open(dir.path()));
    let threads: Vec<_> = (0..8u64)
        .map(|t| {
            let s = s.clone();
            std::thread::spawn(move || {
                for i in 0..25 {
                    s.append(&[
                        NewRecord::bytes(kinds::META, Some("m"), b"x".to_vec()),
                        NewRecord::bytes(kinds::LEDGER, None, row(t * 100 + i)),
                    ])
                    .unwrap();
                }
            })
        })
        .collect();
    for t in threads {
        t.join().unwrap();
    }
    let r = walk(&s);
    assert!(r.broken.is_none(), "{r:?}");
    assert_eq!(r.rows, 201, "200 rows and the start row");
    assert_eq!(r.head.unwrap().link.seq, 200);
}

#[test]
fn a_frame_with_several_ledger_rows_chains_them_in_order() {
    let dir = tempfile::tempdir().unwrap();
    let s = open(dir.path());
    let batch: Vec<NewRecord> = (1..=4)
        .map(|n| NewRecord::bytes(kinds::LEDGER, None, row(n)))
        .collect();
    let positions = s.append(&batch).unwrap();
    assert_eq!(positions.len(), 4, "the start row is no one's answer");
    let r = walk(&s);
    assert!(r.broken.is_none() && r.rows == 5, "{r:?}");
}

#[test]
fn a_row_that_is_not_an_object_is_left_alone() {
    let dir = tempfile::tempdir().unwrap();
    let s = open(dir.path());
    let p = s
        .append(&[NewRecord::json(kinds::LEDGER, None, &"plain").unwrap()])
        .unwrap()[0];
    assert_eq!(s.get(p).unwrap().unwrap().payload, b"\"plain\"");
    assert!(s.ledger_head().unwrap().is_none());
}

#[test]
fn an_untampered_forged_chain_verifies() {
    let (_d, s) = written(&forged(10));
    let r = walk(&s);
    assert!(r.broken.is_none(), "{r:?}");
    assert_eq!(r.rows, 11);
}

#[test]
fn an_edited_row_is_found_at_its_position_as_edited() {
    let mut rows = forged(10);
    // Seq 7's data, edited, its link left as it was.
    let text = String::from_utf8(rows[7].clone())
        .unwrap()
        .replace("\"n\":7}", "\"n\":8}");
    rows[7] = text.into_bytes();
    let (_d, s) = written(&rows);
    let r = walk(&s);
    let b = broken(&r);
    assert_eq!(b.fault, Fault::Edited);
    assert_eq!(b.position, 8, "seq 7 is the log's eighth record");
    assert_eq!(b.seq, Some(7));
    assert_eq!(b.kind.as_deref(), Some("turn.started"));
    assert!(
        b.detail.contains("changed after it was written"),
        "{}",
        b.detail
    );
    assert_eq!(r.rows, 7, "the seven before it verified");
}

#[test]
fn a_removed_row_is_found_at_the_row_after_it_as_removed() {
    let mut rows = forged(10);
    rows.remove(7);
    let (_d, s) = written(&rows);
    let b = broken(&walk(&s)).clone();
    assert_eq!(b.fault, Fault::Removed { missing: 1 });
    assert_eq!(b.seq, Some(8));
    assert_eq!(b.position, 8, "the row that now stands where seq 7 stood");
}

#[test]
fn two_rows_swapped_are_found_as_reordered() {
    let mut rows = forged(10);
    rows.swap(7, 8);
    let (_d, s) = written(&rows);
    let b = broken(&walk(&s)).clone();
    assert_eq!(b.fault, Fault::Reordered);
    assert_eq!(b.position, 8);
    assert!(b.detail.contains("positions 8 and 9"), "{}", b.detail);
}

#[test]
fn a_truncated_tail_is_found_against_a_head_recorded_earlier() {
    let full = forged(10);
    let Opened::Chained { link, .. } = chain::open(&full[10]) else {
        panic!()
    };
    let anchor = Anchor {
        seq: link.seq,
        digest: chain::hex(&link.digest),
    };
    // Nothing is out of order in what is left: without the head, the chain verifies.
    let (_d, s) = written(&full[..9]);
    assert!(walk(&s).broken.is_none());
    let r = verify(&s, None, Some(anchor.clone())).unwrap();
    let b = broken(&r);
    assert_eq!(b.fault, Fault::Truncated);
    assert_eq!(b.position, 9, "the last row that is there");
    assert!(
        b.detail.contains("2 row(s) are missing from the tail"),
        "{}",
        b.detail
    );
    // The whole chain is fine against the same head.
    let (_d2, s2) = written(&full);
    assert!(verify(&s2, None, Some(anchor)).unwrap().broken.is_none());
}

#[test]
fn a_chain_rewritten_from_a_row_onward_is_found_against_a_head_recorded_earlier() {
    let honest = forged(10);
    let Opened::Chained { link, .. } = chain::open(&honest[10]) else {
        panic!()
    };
    let anchor = Anchor {
        seq: link.seq,
        digest: chain::hex(&link.digest),
    };
    // A forger edits row 4 and re-seals it and every row after it.
    let mut rows = honest[..4].to_vec();
    let Opened::Chained { link: mut prev, .. } = chain::open(&honest[3]) else {
        panic!()
    };
    for i in 4..=10u64 {
        let body = if i == 4 { row(99) } else { row(i) };
        let (p, l) = chain::seal(&body, &prev.digest, i).unwrap();
        prev = l;
        rows.push(p);
    }
    let (_d, s) = written(&rows);
    assert!(walk(&s).broken.is_none(), "it verifies on its own");
    let r = verify(&s, None, Some(anchor)).unwrap();
    let b = broken(&r);
    assert_eq!(b.fault, Fault::Rewritten);
    assert_eq!(b.seq, Some(10));
}

#[test]
fn a_row_with_its_link_stripped_is_found_as_unchained() {
    let mut rows = forged(10);
    rows[6] = row(6);
    let (_d, s) = written(&rows);
    let b = broken(&walk(&s)).clone();
    assert_eq!(b.fault, Fault::Unchained);
    assert_eq!(b.position, 7);
}

#[test]
fn a_first_row_that_is_missing_is_found() {
    let mut rows = forged(5);
    rows.remove(0);
    let (_d, s) = written(&rows);
    let b = broken(&walk(&s)).clone();
    assert_eq!(b.fault, Fault::Removed { missing: 1 });
    assert_eq!(b.position, 1);
}

#[test]
fn since_starts_at_a_position_and_checks_what_follows() {
    let mut rows = forged(10);
    let (_d, s) = written(&rows);
    let r = verify(&s, Some(6), None).unwrap();
    assert!(r.broken.is_none(), "{r:?}");
    assert_eq!(r.rows, 6, "positions 6 through 11");
    rows[8] = String::from_utf8(rows[8].clone())
        .unwrap()
        .replace("\"n\":8}", "\"n\":0}")
        .into_bytes();
    let (_d2, s2) = written(&rows);
    let r = verify(&s2, Some(6), None).unwrap();
    assert_eq!(broken(&r).position, 9);
    // Before the start of the walk, nothing is checked.
    rows[2] = row(0);
    let (_d3, s3) = written(&forged(10));
    assert!(verify(&s3, Some(6), None).unwrap().broken.is_none());
}

#[test]
fn the_history_a_chain_pinned_is_checked_and_an_edit_of_it_found() {
    let old: Vec<Vec<u8>> = (100..105).map(row).collect();
    let mut h = chain::History::default();
    for (i, p) in old.iter().enumerate() {
        h.push(i as u64 + 1, p);
    }
    let start = chain::started_row(1_000, 5, 5);
    let (p0, l0) = chain::seal(&start, &chain::GENESIS, 0).unwrap();
    let pin = format!(
        "{{\"at_unix_ms\":2000,\"kind\":\"{}\",\"data\":{{\"rows\":5,\"through_position\":5,\"digest\":\"{}\"}}}}",
        chain::PINNED_KIND,
        chain::hex(&h.digest())
    );
    let (p1, _) = chain::seal(pin.as_bytes(), &l0.digest, 1).unwrap();
    let chained = vec![p0, p1];
    let (_d, s) = written_after(&old, &chained);
    let r = walk(&s);
    assert!(r.broken.is_none(), "{r:?}");
    assert!(r.history_checked);
    assert_eq!(r.unchained, 5);
    // One old row edited.
    let mut edited = old.clone();
    edited[2] = row(0);
    let (_d2, s2) = written_after(&edited, &chained);
    let r = walk(&s2);
    let b = broken(&r);
    assert_eq!(b.fault, Fault::History);
    assert_eq!(b.position, 7, "the pin row");
    // One old row removed: the start row's count says so, before the pin.
    let (_d3, s3) = written_after(&old[..4], &chained);
    let b = broken(&walk(&s3)).clone();
    assert_eq!(b.fault, Fault::History);
    assert_eq!(b.position, 5, "the start row");
}

#[test]
fn an_edit_that_broke_the_rows_json_still_says_its_kind() {
    let mut rows = forged(10);
    let text = String::from_utf8(rows[4].clone())
        .unwrap()
        .replace("\"data\":{", "\"data\":Z{");
    rows[4] = text.into_bytes();
    let (_d, s) = written(&rows);
    let r = walk(&s);
    let b = broken(&r);
    assert_eq!((b.fault.clone(), b.position), (Fault::Edited, 5));
    assert_eq!(b.kind.as_deref(), Some("turn.started"));
}
