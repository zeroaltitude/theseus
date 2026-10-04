//! `ledger.tail` through the store's index (theseus-vm3n.5): each filtered
//! read answers what the scan it replaced answered.

use serde_json::json;
use theseus_protocol::{LedgerTailParams, LedgerTailResult};

use super::tests::test_core;
use crate::ledger::LedgerRow;

const KINDS: [&str; 5] = [
    "turn.started",
    "loop.started",
    "action.declined",
    // The old name of `action.declined`, as an older build stored it.
    "action.denied",
    "provider.error",
];
const SESSIONS: [&str; 3] = ["ses_x", "ses_y", "ses_z"];

/// A small deterministic generator: the test needs no crate for it.
struct Lcg(u64);

impl Lcg {
    fn below(&mut self, n: u64) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        (self.0 >> 33) % n
    }
}

fn positions(r: &LedgerTailResult) -> Vec<u64> {
    r.rows.iter().map(|x| x.position).collect()
}

/// A randomized check on one store: `ledger.tail` with any of a kind (a
/// renamed one's two names included), a session, `after`, `before`, and `n`
/// gives the rows the old read gave: every row, filtered by `is_kind` and the
/// session, then the first `n` after `after`, or the newest `n`. Its cursors
/// say whether more follow, and `total` is every row.
#[tokio::test]
async fn ledger_tail_through_the_index_answers_as_the_scan_did() {
    let core = test_core("ok");
    let mut g = Lcg(5);
    for i in 0..600u64 {
        let kind = KINDS[g.below(5) as usize];
        let row = match g.below(4) {
            0 => json!({"at_unix_ms": i, "kind": kind, "data": {"i": i}}),
            s => {
                json!({"at_unix_ms": i, "kind": kind, "session_id": SESSIONS[s as usize - 1], "data": {"i": i}})
            }
        };
        core.store.append_ledger(&row).unwrap();
    }
    let all: Vec<(u64, LedgerRow)> = core.store.ledger_after(0, usize::MAX).unwrap();
    let last = all.last().unwrap().0;
    for q in 0..300 {
        let kind = (g.below(3) > 0).then(|| KINDS[g.below(5) as usize]);
        let session = (g.below(2) > 0).then(|| SESSIONS[g.below(3) as usize]);
        let n = g.below(40) as usize;
        let (after, before) = match g.below(3) {
            0 => (Some(g.below(last + 1)), None),
            1 => (None, Some(1 + g.below(last + 1))),
            _ => (None, None),
        };
        let got = core
            .ledger_tail(LedgerTailParams {
                n: Some(n),
                kind: kind.map(str::to_string),
                session_id: session.map(str::to_string),
                after,
                before,
                ..Default::default()
            })
            .unwrap();
        let mut want: Vec<u64> = all
            .iter()
            .filter(|(p, r)| {
                kind.is_none_or(|k| r.is_kind(k))
                    && session.is_none_or(|s| r.session_id.as_deref() == Some(s))
                    && after.is_none_or(|a| *p > a)
                    && before.is_none_or(|b| *p < b)
            })
            .map(|(p, _)| *p)
            .collect();
        let more = want.len() > n;
        if after.is_some() {
            want.truncate(n);
        } else {
            want.drain(..want.len().saturating_sub(n));
        }
        let ctx =
            format!("query {q}: {kind:?} {session:?} after {after:?} before {before:?} n {n}");
        assert_eq!(positions(&got), want, "{ctx}");
        assert_eq!(got.total, all.len() as u64, "{ctx}");
        assert_eq!(
            got.next,
            after.and(more.then(|| want.last().copied()).flatten()),
            "{ctx}"
        );
        assert_eq!(
            got.older,
            before.and(more.then(|| want.first().copied()).flatten()),
            "{ctx}"
        );
    }
}

/// `before` pages back from the newest by `older` until none is left, each
/// row once; and a window of time bounds what a read sees, by the store's
/// clock (the rows here were all written in the last moments).
#[tokio::test]
async fn ledger_tail_pages_back_by_before_and_keeps_to_a_window() {
    let core = test_core("ok");
    for i in 0..90u64 {
        let kind = if i % 3 == 0 {
            "turn.started"
        } else {
            "loop.started"
        };
        let row =
            json!({"at_unix_ms": i, "kind": kind, "session_id": "ses_back", "data": {"i": i}});
        core.store.append_ledger(&row).unwrap();
    }
    let read = |p: LedgerTailParams| core.ledger_tail(p).unwrap();
    let newest = read(LedgerTailParams {
        n: Some(4),
        kind: Some("turn.started".into()),
        ..Default::default()
    });
    assert_eq!(newest.older, None, "a tail read has no cursor");
    let mut back: Vec<u64> = positions(&newest);
    let mut before = back.first().copied();
    while let Some(b) = before {
        let r = read(LedgerTailParams {
            n: Some(4),
            kind: Some("turn.started".into()),
            before: Some(b),
            ..Default::default()
        });
        assert!(r.rows.len() <= 4);
        let mut page = positions(&r);
        page.extend(back);
        back = page;
        before = r.older;
    }
    let all = read(LedgerTailParams {
        n: Some(1000),
        kind: Some("turn.started".into()),
        ..Default::default()
    });
    assert_eq!(back, positions(&all), "each row once, in order");
    assert_eq!(back.len(), 30);
    let now = theseus_protocol::now_unix_ms();
    let window = |since: Option<u64>, until: Option<u64>| {
        read(LedgerTailParams {
            n: Some(1000),
            since_ms: since,
            until_ms: until,
            session_id: Some("ses_back".into()),
            ..Default::default()
        })
        .rows
        .len()
    };
    assert_eq!(window(Some(now - 600_000), None), 90);
    assert_eq!(window(None, Some(now + 600_000)), 90);
    assert_eq!(window(Some(now + 600_000), None), 0);
    assert_eq!(window(None, Some(now - 600_000)), 0);
}

/// A page and its `total` are one snapshot (theseus-tphr): read while a
/// writer appends rows, every answer that holds the whole ledger counts
/// exactly the rows it shows. Two reads, the page then the count, once let
/// a row land between them, and a caller that compared the two (the daemon's
/// history-check test, against health's refused count) saw one too many.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_pages_total_is_counted_in_the_same_snapshot_as_its_rows() {
    let core = test_core("ok");
    let writer = {
        let core = core.clone();
        std::thread::spawn(move || {
            for i in 0..800u64 {
                let row = json!({"at_unix_ms": i, "kind": "loop.started", "data": {"i": i}});
                core.store.append_ledger(&row).unwrap();
            }
        })
    };
    let mut reads = 0;
    while !writer.is_finished() {
        let r = core
            .ledger_tail(LedgerTailParams {
                n: Some(1000),
                ..Default::default()
            })
            .unwrap();
        assert_eq!(
            r.total,
            r.rows.len() as u64,
            "read {reads}: the count and the rows of one snapshot"
        );
        reads += 1;
    }
    writer.join().unwrap();
    assert!(reads > 10, "{reads} reads raced the writer");
}
