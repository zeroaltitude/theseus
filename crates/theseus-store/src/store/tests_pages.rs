//! The index's pages, counts, and shape (theseus-vm3n.5): each read through
//! the index answers what a walk of every record answers.

use rand::{Rng as _, SeedableRng as _};

use super::*;
use crate::pages::{ledger_kind, ledger_kind_session, ledger_session};
use crate::record::kinds;
use crate::wal::test_clock;

fn open(dir: &Path) -> WalStore {
    WalStore::open(dir, WalConfig::default())
        .unwrap()
        .with_checkpoint_every(0)
}

/// A ledger row of `kind`, in `session` when there is one.
fn row(kind: &str, session: Option<&str>, i: u64) -> NewRecord {
    let v = match session {
        Some(s) => {
            serde_json::json!({"at_unix_ms": i, "kind": kind, "session_id": s, "data": {"i": i}})
        }
        None => serde_json::json!({"at_unix_ms": i, "kind": kind, "data": {"i": i}}),
    };
    NewRecord::json(kinds::LEDGER, None, &v).unwrap()
}

/// Every ledger row the store holds, as (position, kind, session), by a
/// read of every record: what a page must agree with.
fn walked(s: &WalStore) -> Vec<(u64, String, Option<String>, u64)> {
    s.scan(1, None, usize::MAX)
        .unwrap()
        .into_iter()
        .filter(|r| r.kind == kinds::LEDGER)
        .map(|r| {
            let v: serde_json::Value = r.decode().unwrap();
            (
                r.position,
                v["kind"].as_str().unwrap().to_string(),
                v["session_id"].as_str().map(str::to_string),
                r.at_unix_ms,
            )
        })
        .collect()
}

/// Each count the index keeps equals the walk it replaces.
fn counts_agree(s: &WalStore, scopes: &[&str]) {
    let idx = &s.inner.index;
    for kind in [kinds::LEDGER, kinds::SESSION, kinds::NODE, kinds::META] {
        assert_eq!(
            idx.count_of_kind(kind).unwrap(),
            idx.count_of_kind_walked(kind).unwrap(),
            "records of kind {kind}"
        );
        assert_eq!(
            idx.count_keys(kind).unwrap(),
            idx.count_keys_walked(kind).unwrap(),
            "keys of kind {kind}"
        );
    }
    for sc in scopes {
        assert_eq!(
            idx.count_in_scope(sc).unwrap(),
            idx.count_in_scope_walked(sc).unwrap(),
            "records in {sc}"
        );
    }
}

/// The counts (theseus-vm3n.5) are kept with every append, in batches that
/// mix kinds, keys, and scopes, and equal a walk's after every batch, after
/// a reopen that replays the tail, and after an index rebuilt whole.
#[test]
fn each_count_is_kept_with_every_append_and_equals_a_walk() {
    let dir = tempfile::tempdir().unwrap();
    let scopes = ["ses_0", "ses_1", "ses_2"];
    let mut rng = rand::rngs::StdRng::seed_from_u64(7);
    let s = open(dir.path());
    for i in 0..300u64 {
        let mut batch = Vec::new();
        for _ in 0..rng.random_range(1..5) {
            let r = match rng.random_range(0..4) {
                0 => row("turn.started", Some(scopes[rng.random_range(0..3)]), i),
                1 => NewRecord::json(
                    kinds::SESSION,
                    Some(&format!("ses_{}", rng.random_range(0..12))),
                    &i,
                )
                .unwrap(),
                2 => NewRecord::json(kinds::NODE, Some(&format!("n{i}")), &i)
                    .unwrap()
                    .scoped(scopes[rng.random_range(0..3)]),
                _ => NewRecord::json(kinds::META, Some("m"), &i).unwrap(),
            };
            batch.push(r);
        }
        s.append(&batch).unwrap();
        if i % 37 == 0 {
            counts_agree(&s, &scopes);
        }
        if i == 150 {
            s.checkpoint().unwrap();
        }
    }
    counts_agree(&s, &scopes);
    let ledger = s.count_of_kind(kinds::LEDGER).unwrap();
    assert!(ledger > 50, "{ledger}");
    drop(s);
    // The tail past the checkpoint is replayed at the open.
    let s = open(dir.path());
    assert!(s.stats().unwrap().replayed_into_index > 0);
    counts_agree(&s, &scopes);
    assert_eq!(s.count_of_kind(kinds::LEDGER).unwrap(), ledger);
    drop(s);
    // A whole rebuild, in bulk.
    std::fs::remove_file(dir.path().join("index.redb")).unwrap();
    let s = open(dir.path());
    counts_agree(&s, &scopes);
    assert_eq!(s.count_of_kind(kinds::LEDGER).unwrap(), ledger);
}

const KINDS: [&str; 4] = [
    "turn.started",
    "turn.ended",
    "provider.error",
    "store.corrupt",
];
const SESSIONS: [&str; 3] = ["ses_a", "ses_b", "ses_c"];

/// The tags a filter on `kind` and `session` reads.
fn tags(kind: Option<&str>, session: Option<&str>) -> Vec<String> {
    match (kind, session) {
        (Some(k), Some(s)) => vec![ledger_kind_session(k, s)],
        (Some(k), None) => vec![ledger_kind(k)],
        (None, Some(s)) => vec![ledger_session(s)],
        (None, None) => Vec::new(),
    }
}

/// A randomized check on one store: each page through the tags and the
/// cursors (theseus-vm3n.5) is exactly the page a scan of every row, then a
/// filter, gives: the same positions, the same `more`, and the kind's count.
#[test]
fn a_filtered_page_equals_the_scans_answer() {
    let dir = tempfile::tempdir().unwrap();
    let mut rng = rand::rngs::StdRng::seed_from_u64(11);
    let s = open(dir.path());
    for i in 0..1500u64 {
        let mut batch = Vec::new();
        for _ in 0..rng.random_range(1..4) {
            let k = KINDS[rng.random_range(0..4)];
            let sid = (rng.random_range(0..4) > 0).then(|| SESSIONS[rng.random_range(0..3)]);
            batch.push(row(k, sid, i));
        }
        if i % 7 == 0 {
            batch.push(NewRecord::json(kinds::SESSION, Some("ses_a"), &i).unwrap());
        }
        s.append(&batch).unwrap();
        if i == 700 {
            s.checkpoint().unwrap();
        }
    }
    let all = walked(&s);
    let last = s.last_position();
    for q in 0..400 {
        let kind = (rng.random_range(0..3) > 0).then(|| KINDS[rng.random_range(0..4)]);
        let session = (rng.random_range(0..2) > 0).then(|| SESSIONS[rng.random_range(0..3)]);
        let limit = rng.random_range(0..60usize);
        let (after, before) = match rng.random_range(0..4) {
            0 => (Some(rng.random_range(0..=last)), None),
            1 => (None, Some(rng.random_range(1..=last + 1))),
            2 => {
                let a = rng.random_range(0..=last);
                (Some(a), Some(rng.random_range(a..=last + 1)))
            }
            _ => (None, None),
        };
        let page = Page {
            kind: kinds::LEDGER,
            tags: tags(kind, session),
            after,
            before,
            limit,
            ..Page::default()
        };
        let got = s.page(&page).unwrap().unwrap();
        let mut want: Vec<u64> = all
            .iter()
            .filter(|(p, k, sid, _)| {
                kind.is_none_or(|x| x == k)
                    && session.is_none_or(|x| sid.as_deref() == Some(x))
                    && after.is_none_or(|a| *p > a)
                    && before.is_none_or(|b| *p < b)
            })
            .map(|(p, ..)| *p)
            .collect();
        let more = want.len() > limit;
        if after.is_some() {
            want.truncate(limit);
        } else {
            want.drain(..want.len().saturating_sub(limit));
        }
        let positions: Vec<u64> = got.records.iter().map(|r| r.position).collect();
        assert_eq!(positions, want, "query {q}: {page:?}");
        assert_eq!(got.more, more, "query {q}: {page:?}");
        assert_eq!(got.count, all.len() as u64, "query {q}");
        assert_eq!(
            (got.first, got.last),
            (want.first().copied(), want.last().copied())
        );
    }
    // Two tags read as either: a renamed kind's two names.
    let got = s
        .page(&Page {
            kind: kinds::LEDGER,
            tags: vec![ledger_kind("turn.ended"), ledger_kind("provider.error")],
            limit: 25,
            ..Page::default()
        })
        .unwrap()
        .unwrap();
    let mut want: Vec<u64> = all
        .iter()
        .filter(|(_, k, ..)| k == "turn.ended" || k == "provider.error")
        .map(|(p, ..)| *p)
        .collect();
    want.drain(..want.len() - 25);
    let positions: Vec<u64> = got.records.iter().map(|r| r.position).collect();
    assert_eq!(positions, want);
}

/// A window of time (theseus-vm3n.5): its bounds are inclusive, to the ms,
/// across minutes; and a clock that steps back does not split it. The rows
/// written while the host's clock was behind count at the kind's clock (the
/// newest time its rows have had), so a window is one stretch of positions,
/// and each row still says its own time.
#[test]
fn a_window_keeps_its_bounds_and_a_clock_that_steps_back_does_not_split_it() {
    let dir = tempfile::tempdir().unwrap();
    let s = open(dir.path());
    let wal = dir.path().join("wal");
    let t0 = 1_800_000_000_000u64; // a minute's start
                                   // Rows every 10 s for 5 minutes: positions 1..=30 at t0 + 10 s × (p - 1).
    for i in 0..30u64 {
        test_clock::set(&wal, t0 + i * 10_000);
        s.append(&[row("turn.started", Some("ses_a"), i)]).unwrap();
    }
    let window = |since: Option<u64>, until: Option<u64>| -> Vec<u64> {
        s.page(&Page {
            kind: kinds::LEDGER,
            since_ms: since,
            until_ms: until,
            limit: 1000,
            ..Page::default()
        })
        .unwrap()
        .unwrap()
        .records
        .iter()
        .map(|r| r.position)
        .collect()
    };
    let at = |p: u64| t0 + (p - 1) * 10_000;
    assert_eq!(
        window(Some(at(7)), Some(at(13))),
        (7..=13).collect::<Vec<_>>()
    );
    assert_eq!(
        window(Some(at(7) + 1), Some(at(13) - 1)),
        (8..=12).collect::<Vec<_>>()
    );
    assert_eq!(window(Some(at(25)), None), (25..=30).collect::<Vec<_>>());
    assert_eq!(window(None, Some(at(3))), vec![1, 2, 3]);
    assert!(window(Some(at(30) + 1), None).is_empty());
    assert_eq!(window(Some(0), None).len(), 30);
    assert!(window(Some(at(9)), Some(at(8))).is_empty());
    // The host's clock steps back four minutes, writes three rows, and
    // then catches up past where it was.
    let stepped = at(30) - 240_000;
    for i in 0..3u64 {
        test_clock::set(&wal, stepped + i * 1_000);
        s.append(&[row("turn.started", Some("ses_a"), 100 + i)])
            .unwrap();
    }
    test_clock::set(&wal, at(30) + 30_000);
    s.append(&[row("turn.ended", Some("ses_a"), 200)]).unwrap();
    // 31..=33 count at at(30), the clock they found; 34 at its own time.
    assert_eq!(window(Some(at(30)), None), vec![30, 31, 32, 33, 34]);
    assert_eq!(window(Some(at(30) + 1), None), vec![34]);
    // A window at the time they were written finds the row first written
    // then (6, at t0 + 50 s), and none of them.
    assert_eq!(window(Some(stepped), Some(stepped + 5_000)), vec![6]);
    assert_eq!(window(Some(at(29)), Some(at(30))), vec![29, 30, 31, 32, 33]);
    // They say their own time.
    let r = s.get(32).unwrap().unwrap();
    assert_eq!(r.at_unix_ms, stepped + 1_000);
    // A window with a filter and a cursor.
    let got = s
        .page(&Page {
            kind: kinds::LEDGER,
            tags: vec![ledger_kind("turn.started")],
            since_ms: Some(at(20)),
            before: Some(33),
            limit: 4,
            ..Page::default()
        })
        .unwrap()
        .unwrap();
    let positions: Vec<u64> = got.records.iter().map(|r| r.position).collect();
    assert_eq!(positions, vec![29, 30, 31, 32]);
    assert!(got.more);
    // The same answers after a rebuild from the WAL: the clock is replayed.
    drop(s);
    std::fs::remove_file(dir.path().join("index.redb")).unwrap();
    let s = open(dir.path());
    let positions: Vec<u64> = s
        .page(&Page {
            kind: kinds::LEDGER,
            since_ms: Some(at(30)),
            limit: 1000,
            ..Page::default()
        })
        .unwrap()
        .unwrap()
        .records
        .iter()
        .map(|r| r.position)
        .collect();
    assert_eq!(positions, vec![30, 31, 32, 33, 34]);
}

/// Cursor pages read while a writer appends (theseus-vm3n.5): paging back
/// from the newest by `before`, and forward by `after`, each row is read
/// exactly once, with no gap, whatever lands meanwhile.
#[test]
fn cursor_pages_under_concurrent_writes_have_no_duplicate_or_gap() {
    let dir = tempfile::tempdir().unwrap();
    let s = Arc::new(open(dir.path()));
    for i in 0..400u64 {
        s.append(&[row(
            KINDS[(i % 3) as usize],
            Some(SESSIONS[(i % 2) as usize]),
            i,
        )])
        .unwrap();
    }
    let stop = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let writer = {
        let (s, stop) = (s.clone(), stop.clone());
        std::thread::spawn(move || {
            let mut i = 1000u64;
            while !stop.load(Ordering::Relaxed) {
                s.append(&[row(
                    KINDS[(i % 3) as usize],
                    Some(SESSIONS[(i % 2) as usize]),
                    i,
                )])
                .unwrap();
                i += 1;
            }
        })
    };
    let tags = vec![ledger_kind_session("turn.started", "ses_a")];
    // Back from the newest, seven at a time.
    let mut back: Vec<u64> = Vec::new();
    let mut before = None;
    loop {
        let p = s
            .page(&Page {
                kind: kinds::LEDGER,
                tags: tags.clone(),
                before,
                limit: 7,
                ..Page::default()
            })
            .unwrap()
            .unwrap();
        back.extend(p.records.iter().rev().map(|r| r.position));
        if !p.more {
            break;
        }
        before = p.first;
    }
    // Forward from the start, five at a time, until a page comes back short.
    let mut forward: Vec<u64> = Vec::new();
    let mut after = 0;
    let newest = back[0];
    while after < newest {
        let p = s
            .page(&Page {
                kind: kinds::LEDGER,
                tags: tags.clone(),
                after: Some(after),
                limit: 5,
                ..Page::default()
            })
            .unwrap()
            .unwrap();
        forward.extend(p.records.iter().map(|r| r.position));
        let Some(last) = p.last else { break };
        after = last;
    }
    stop.store(true, Ordering::Relaxed);
    writer.join().unwrap();
    back.reverse();
    let want: Vec<u64> = walked(&s)
        .into_iter()
        .filter(|(p, k, sid, _)| {
            *p <= newest && k == "turn.started" && sid.as_deref() == Some("ses_a")
        })
        .map(|(p, ..)| p)
        .collect();
    assert_eq!(back, want, "paged back: no duplicate, no gap");
    let forward: Vec<u64> = forward.into_iter().filter(|p| *p <= newest).collect();
    assert_eq!(forward, want, "paged forward: no duplicate, no gap");
    assert!(want.len() > 60, "{}", want.len());
}

/// An index another shape wrote (an older build keeps no counts, clocks, or
/// tags, and moves the checkpoint alone) keeps its tables at the open, which
/// still reads only the WAL's tail; its counts walk and a page by tag is
/// `None` until `build_shape`, after serving, has built the rest. Appends
/// that land between the build's stretches count once, and every count and
/// page then equals a walk; the next checkpoint marks the shape, and the
/// history check's mark survives it all.
#[test]
fn an_index_of_another_shape_is_built_after_serving_and_then_answers_whole() {
    let dir = tempfile::tempdir().unwrap();
    let s = open(dir.path());
    for i in 0..60u64 {
        s.append(&[
            row(KINDS[(i % 4) as usize], Some(SESSIONS[(i % 3) as usize]), i),
            NewRecord::json(kinds::SESSION, Some(&format!("ses_{}", i % 5)), &i).unwrap(),
        ])
        .unwrap();
    }
    s.checkpoint().unwrap();
    drop(s);
    // As an older build leaves it: the tables it does not know gone, and
    // no shape mark at its checkpoint.
    {
        let db = redb::Database::create(dir.path().join("index.redb")).unwrap();
        let txn = db.begin_write().unwrap();
        txn.delete_table(crate::index::COUNTS).unwrap();
        txn.delete_table(crate::index::TAGGED).unwrap();
        {
            let mut meta = txn.open_table(crate::index::META).unwrap();
            meta.insert("verified.position", 9).unwrap();
            meta.remove(crate::index::SHAPE).unwrap();
        }
        txn.commit().unwrap();
    }
    let s = open(dir.path());
    let st = s.stats().unwrap();
    assert_eq!(st.replayed_into_index, 0, "the open reads only the tail");
    assert!(st.shape_pending && !s.shaped());
    let by_tag = |s: &WalStore| {
        s.page(&Page {
            kind: kinds::LEDGER,
            tags: vec![ledger_kind("turn.ended")],
            limit: 1000,
            ..Page::default()
        })
        .unwrap()
        .map(|p| p.records.iter().map(|r| r.position).collect::<Vec<u64>>())
    };
    assert_eq!(by_tag(&s), None, "a page by tag waits for the build");
    counts_agree(&s, &["ses_a"]);
    assert_eq!(
        s.count_of_kind(kinds::LEDGER).unwrap(),
        60,
        "a count walks meanwhile"
    );
    // The build, seven records a stretch, with appends between.
    let mut at = None;
    let mut i = 100u64;
    loop {
        at = s.build_shape(at, 7).unwrap();
        s.append(&[row("turn.ended", Some("ses_a"), i)]).unwrap();
        i += 1;
        if at.is_none() {
            break;
        }
    }
    assert!(s.shaped() && !s.stats().unwrap().shape_pending);
    counts_agree(&s, &["ses_a", "ses_b"]);
    let want: Vec<u64> = walked(&s)
        .into_iter()
        .filter(|(_, k, ..)| k == "turn.ended")
        .map(|(p, ..)| p)
        .collect();
    assert_eq!(by_tag(&s), Some(want.clone()));
    assert_eq!(s.count_of_kind(kinds::LEDGER).unwrap(), 60 + (i - 100));
    assert_eq!(s.count_keys(kinds::SESSION).unwrap(), 5);
    assert_eq!(
        s.inner.index.meta(&["verified.position"]).unwrap(),
        vec![Some(9)],
        "the history check's mark is kept"
    );
    s.checkpoint().unwrap();
    drop(s);
    // Marked whole at the checkpoint: the next open has nothing to build.
    let s = open(dir.path());
    assert!(s.shaped());
    assert_eq!(by_tag(&s), Some(want));
    counts_agree(&s, &["ses_a"]);
}

fn state_terms(_: RecordKind, payload: &[u8]) -> Vec<String> {
    let n: u64 = serde_json::from_slice(payload).unwrap_or(0);
    let mut t = vec![format!(
        "s:{}",
        ["queued", "running", "settled"][(n % 3) as usize]
    )];
    if n.is_multiple_of(5) {
        t.push(format!("x:e{}", n % 4));
    }
    t
}

fn no_sums(_: RecordKind, _: &[u8]) -> Option<Sums> {
    None
}

static STATES: Projection = Projection {
    name: "states.test.1",
    kinds: &[kinds::EXECUTION],
    terms: state_terms,
    sums: no_sums,
};

/// A term's count (`termcounts`, theseus-vm3n.5) follows each key's latest
/// terms, as a term leaves a key and another arrives, and equals a walk of
/// the term's keys: one term, a prefix of them, and every one; after a
/// reopen and after a rebuild in bulk too.
#[test]
fn each_terms_count_follows_its_keys_and_equals_a_walk() {
    let dir = tempfile::tempdir().unwrap();
    let opened = || {
        WalStore::open_projected(dir.path(), WalConfig::default(), &STATES)
            .unwrap()
            .with_checkpoint_every(0)
    };
    let agree = |s: &WalStore| {
        for (lo, hi) in [
            ("s:queued", "s:queued\u{1}"),
            ("s:settled", "s:settled\u{1}"),
            ("s:", "s;"),
            ("x:", "x;"),
            ("", "\u{7f}"),
        ] {
            assert_eq!(
                s.inner
                    .index
                    .count_by_terms(kinds::EXECUTION, lo, hi)
                    .unwrap(),
                s.inner
                    .index
                    .count_by_terms_walked(kinds::EXECUTION, lo, hi)
                    .unwrap(),
                "{lo}..{hi}"
            );
        }
    };
    let mut rng = rand::rngs::StdRng::seed_from_u64(3);
    let s = opened();
    for i in 0..400u64 {
        let key = format!("e{}", rng.random_range(0..40));
        s.append(&[
            NewRecord::json(kinds::EXECUTION, Some(&key), &rng.random_range(0..100u64)).unwrap(),
        ])
        .unwrap();
        if i % 50 == 0 {
            agree(&s);
        }
        if i == 200 {
            s.checkpoint().unwrap();
        }
    }
    agree(&s);
    let settled = s
        .count_by_terms(kinds::EXECUTION, "s:settled", "s:settled\u{1}")
        .unwrap()
        .unwrap();
    assert!(settled > 0);
    drop(s);
    let s = opened();
    agree(&s);
    drop(s);
    std::fs::remove_file(dir.path().join("index.redb")).unwrap();
    let s = opened();
    agree(&s);
    assert_eq!(
        s.count_by_terms(kinds::EXECUTION, "s:settled", "s:settled\u{1}")
            .unwrap(),
        Some(settled)
    );
}
