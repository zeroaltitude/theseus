//! The start's read of today's spend does not grow with the day's rows
//! (theseus-vqcgq): a stored total, read past, equals the full read; the
//! rows a start reads follow the rows written since the last total, never
//! the day's. The cost is counted in records read from the log
//! (`theseus_store::records_read_here`), never in time.

use serde_json::json;
use theseus_store::{records_read_here, NewRecord};

use crate::day_ceiling::{read_today, read_today_total, spend_of, TOTAL_KEY};
use crate::store::Store;
use crate::tests_day_ceiling::rig;

const LIMIT: u64 = 200_000_000;

fn row_at(kind: &str, at: u64, data: serde_json::Value) -> NewRecord {
    let r = crate::ledger::LedgerRow {
        at_unix_ms: at,
        kind: kind.into(),
        session_id: Some("ses_seed".into()),
        turn_id: None,
        data,
    };
    NewRecord::json(theseus_store::kinds::LEDGER, None, &r).unwrap()
}

fn cost_rows(n: usize, at: u64) -> Vec<NewRecord> {
    (0..n)
        .map(|i| {
            row_at(
                "action.succeeded",
                at,
                json!({"producer": "provider:anthropic", "cost_usd": 0.001, "correlation_id": format!("act_{i}")}),
            )
        })
        .collect()
}

fn append_in_frames(store: &Store, rows: Vec<NewRecord>) {
    for chunk in rows.chunks(500) {
        store.append(chunk).unwrap();
    }
}

/// Records read from the log by `f`.
fn reads<R>(f: impl FnOnce() -> R) -> (R, u64) {
    let before = records_read_here();
    let out = f();
    (out, records_read_here() - before)
}

/// A start reads the rows written since the stored total and nothing of the
/// rest: with 10 rows or 10,000 before it, the read decodes the same rows and
/// reads the same few records.
#[test]
fn the_starts_read_is_bounded_whatever_the_days_rows() {
    let now = theseus_protocol::now_unix_ms();
    let since = now - 3_600_000;
    let mut seen = Vec::new();
    for n in [10usize, 10_000] {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(&dir.path().join("store")).unwrap();
        append_in_frames(&store, cost_rows(n, now));
        // The first start has no total: the one full read.
        let first = read_today_total(&store, since, "today", LIMIT)
            .unwrap()
            .unwrap();
        assert_eq!((first.rows, first.resumed), (n as u64, false));
        // A restart with nothing written since reads no day row.
        let (again, cold) = reads(|| read_today_total(&store, since, "today", LIMIT));
        let again = again.unwrap().unwrap();
        assert_eq!((again.rows, again.resumed), (0, true), "n {n}");
        assert_eq!(again.seed, first.seed, "n {n}");
        // Three rows written since: the read decodes those three.
        append_in_frames(&store, cost_rows(3, now));
        let (more, warm) = reads(|| read_today_total(&store, since, "today", LIMIT));
        let more = more.unwrap().unwrap();
        assert_eq!(more.rows, 3, "n {n}");
        assert_eq!(more.seed.spent, first.seed.spent + 3_000, "n {n}");
        seen.push((cold, warm));
    }
    // Same reads at 10 rows and at 10,000, and few: the total's record, the
    // judge's budget, and the three rows.
    assert_eq!(seen[0], seen[1], "{seen:?}");
    assert!(seen[0].0 <= 3 && seen[0].1 <= 6, "{seen:?}");
}

/// The same bound through the daemon's own start read (`seed_day_ceiling`),
/// and the minute's advance of the total (`advance_day_total`) is what
/// makes a restart after a busy run cheap.
#[test]
fn the_runners_start_read_and_advance_follow_the_rows_since_the_total() {
    let mut cold = Vec::new();
    for n in [10usize, 10_000] {
        let r = rig(vec![], |_| {});
        let core = &r.core;
        let now = core.runner.kernel.day_ceiling().now();
        append_in_frames(&core.store, cost_rows(n, now));
        // The run's own advance reads the rows once and stores the total.
        assert_eq!(core.runner.advance_day_total(), Some(n as u64));
        let spent = core.runner.seed_day_ceiling();
        assert_eq!((spent.rows, spent.resumed), (0, true), "n {n}");
        assert_eq!(spent.seed.spent, n as u64 * 1_000, "n {n}");
        // Rows written after it: the restart's read is those.
        append_in_frames(&core.store, cost_rows(4, now));
        let (s, reads) = reads(|| core.runner.seed_day_ceiling());
        assert_eq!(s.rows, 4, "n {n}");
        assert_eq!(s.seed.spent, (n as u64 + 4) * 1_000, "n {n}");
        cold.push(reads);
    }
    assert_eq!(cold[0], cold[1], "{cold:?}");
    assert!(cold[0] <= 8, "{cold:?}");
}

/// The stored total and the full read agree at every step: across rows of
/// every kind that counts, a day's turning, a restart (the store reopened),
/// two ceilings written under two limits, a warning, and a raised limit.
#[test]
fn the_total_equals_the_full_read_across_days_and_restarts() {
    let now = theseus_protocol::now_unix_ms();
    let (s1, s2) = (now - 3_600_000, now - 1_800_000);
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("store");
    let check = |store: &Store, since: u64, day: &str, limit: u64, what: &str| {
        let full = read_today(store, since, day, limit).unwrap().unwrap();
        let total = read_today_total(store, since, day, limit).unwrap().unwrap();
        assert_eq!(total.seed, full.seed, "{what}");
        // An oracle that shares nothing with the fold: every ledger row.
        let rows: Vec<(u64, crate::ledger::LedgerRow)> = store.ledger_after(0, 100_000).unwrap();
        let spent: u64 = rows
            .iter()
            .filter(|(_, r)| r.at_unix_ms >= since)
            .filter_map(|(_, r)| spend_of(r))
            .sum();
        assert_eq!(total.seed.spent, spent, "{what}");
        // And for the day's marks: the highest warning, and the earliest
        // ceiling row written under a limit no lower than this start's.
        let mine = |r: &crate::ledger::LedgerRow| r.at_unix_ms >= since && r.data["day"] == day;
        let warned = rows
            .iter()
            .map(|(_, r)| r)
            .filter(|r| r.kind == "spend.day_warning" && mine(r))
            .map(|r| r.data["level"].as_u64().unwrap_or(1))
            .max()
            .unwrap_or(0);
        assert_eq!(total.seed.warned, warned, "{what}");
        let reached = rows
            .iter()
            .map(|(_, r)| r)
            .filter(|r| r.kind == "spend.ceiling" && mine(r))
            .filter(|r| {
                r.data["limit_usd"]
                    .as_f64()
                    .is_none_or(|l| theseus_kernel::usd_to_micros(l) >= limit)
            })
            .map(|r| r.at_unix_ms)
            .min();
        assert_eq!(total.seed.reached_at_ms, reached, "{what}");
    };
    let mut store = Store::open(&path).unwrap();
    // Day one.
    store
        .append(&[
            row_at(
                "action.succeeded",
                now - 2_700_000,
                json!({"producer": "provider:anthropic", "cost_usd": 0.5}),
            ),
            row_at("speech.synthesized", now, json!({"cost_usd": 0.0625})),
            row_at(
                "spend.ceiling",
                now - 2_000_000,
                json!({"day": "d1", "limit_usd": 100.0}),
            ),
        ])
        .unwrap();
    check(&store, s1, "d1", LIMIT, "d1 first");
    store
        .append(&[
            row_at(
                "spend.ceiling",
                now - 1_900_000,
                json!({"day": "d1", "limit_usd": 300.0}),
            ),
            row_at(
                "spend.ceiling",
                now - 1_000_000,
                json!({"day": "d1", "limit_usd": 100.0}),
            ),
            // A second row under the same limit, later: the earliest is the stop's time.
            row_at(
                "spend.ceiling",
                now - 500_000,
                json!({"day": "d1", "limit_usd": 300.0}),
            ),
            row_at("spend.day_warning", now, json!({"day": "d1", "level": 2})),
            row_at("judge.replay", now, json!({"cost_usd": 0.015625})),
        ])
        .unwrap();
    check(&store, s1, "d1", LIMIT, "d1 more");
    // A raised limit lifts the low ceiling's stop but not the high one's.
    check(&store, s1, "d1", 250_000_000, "d1 limit 250");
    check(&store, s1, "d1", 400_000_000, "d1 limit 400");
    // A restart.
    drop(store);
    store = Store::open(&path).unwrap();
    store
        .append(&cost_rows(5, now))
        .expect("rows after the restart");
    check(&store, s1, "d1", LIMIT, "d1 after restart");
    // The day turns: the same log, the next day's window and name. The rows
    // before its midnight are not its, and its ceilings are its own.
    check(&store, s2, "d2", LIMIT, "d2 first");
    store
        .append(&[
            row_at("spend.ceiling", now, json!({"day": "d2"})),
            row_at("spend.day_warning", now, json!({"day": "d2", "level": 1})),
        ])
        .unwrap();
    check(&store, s2, "d2", LIMIT, "d2 ceilings");
    drop(store);
    store = Store::open(&path).unwrap();
    store.append(&cost_rows(2, now)).unwrap();
    check(&store, s2, "d2", LIMIT, "d2 after restart");
    // Back to day one's name: not d2's total.
    check(&store, s1, "d1", LIMIT, "back to d1");
    // A day of another name in the same window starts a total of its own:
    // d1's ceilings and warning are not its.
    check(&store, s1, "d1b", LIMIT, "d1b");
}

/// A store an older build wrote has no total: the first start reads the day
/// once and stores it, and the next reads none. A total the build cannot
/// trust (another rule or list of kinds, or ahead of the log) is not used.
#[test]
fn a_missing_or_untrusted_total_is_one_full_read_then_stored() {
    let now = theseus_protocol::now_unix_ms();
    let since = now - 3_600_000;
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open(&dir.path().join("store")).unwrap();
    append_in_frames(&store, cost_rows(40, now));
    assert!(store
        .get_meta::<serde_json::Value>(TOTAL_KEY)
        .unwrap()
        .is_none());
    let first = read_today_total(&store, since, "today", LIMIT)
        .unwrap()
        .unwrap();
    assert_eq!(
        (first.rows, first.resumed, first.seed.spent),
        (40, false, 40_000)
    );
    let stored = store
        .get_meta::<serde_json::Value>(TOTAL_KEY)
        .unwrap()
        .expect("the total is stored");
    let second = read_today_total(&store, since, "today", LIMIT)
        .unwrap()
        .unwrap();
    assert_eq!((second.rows, second.resumed), (0, true));
    // A stored total under another rule, kinds, or ahead of the log is read
    // over, with the whole day counted again and never double counted.
    for (field, value) in [
        ("rule", json!(0)),
        ("kinds", json!("action.succeeded")),
        ("through", json!(u64::MAX / 2)),
        ("since_ms", json!(since + 1)),
    ] {
        let mut bad = stored.clone();
        bad[field] = value;
        bad["spent"] = json!(999_999_999_u64);
        store.put_meta(TOTAL_KEY, &bad).unwrap();
        let s = read_today_total(&store, since, "today", LIMIT)
            .unwrap()
            .unwrap();
        assert_eq!(
            (s.rows, s.resumed, s.seed.spent),
            (40, false, 40_000),
            "{field}"
        );
    }
}

/// The issue's figures: the start's read at 66,666 spend rows today, the full
/// read (what every start paid) against the stored total's. A measurement:
/// `--run-ignored only --no-capture`, in a release profile.
#[test]
#[ignore = "a measurement"]
fn the_starts_read_at_66666_rows() {
    let now = theseus_protocol::now_unix_ms();
    let since = now - 3_600_000;
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open(&dir.path().join("store")).unwrap();
    append_in_frames(&store, cost_rows(66_666, now));
    let t = std::time::Instant::now();
    let full = read_today(&store, since, "today", LIMIT).unwrap().unwrap();
    println!("full read: {} rows in {:?}", full.rows, t.elapsed());
    let t = std::time::Instant::now();
    let first = read_today_total(&store, since, "today", LIMIT)
        .unwrap()
        .unwrap();
    println!(
        "first start (no total): {} rows in {:?}",
        first.rows,
        t.elapsed()
    );
    for _ in 0..3 {
        let t = std::time::Instant::now();
        let again = read_today_total(&store, since, "today", LIMIT)
            .unwrap()
            .unwrap();
        println!("restart: {} rows in {:?}", again.rows, t.elapsed());
        assert_eq!(again.seed, full.seed);
    }
}
