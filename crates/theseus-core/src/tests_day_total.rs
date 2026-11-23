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

/// The retired thread importer's spend stays today's after an upgrade (issue
/// 44): a priced `thread.imported` row written earlier today, and a stored
/// total an older build wrote (the list of kinds before the importer was
/// retired, with the row in its sum; and the list after, without it). The
/// full read and the stored total's read both keep the row's cost, and the
/// total stored again is resumed with it.
#[test]
fn the_retired_importers_spend_still_counts_after_an_upgrade() {
    let now = theseus_protocol::now_unix_ms();
    let since = now - 3_600_000;
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open(&dir.path().join("store")).unwrap();
    let mut rows = cost_rows(4, now);
    rows.push(row_at(
        "thread.imported",
        now,
        json!({"slug": "tax", "from": "harness", "model": "m", "cost_usd": 0.5}),
    ));
    append_in_frames(&store, rows);
    let full = read_today(&store, since, "today", LIMIT).unwrap().unwrap();
    assert_eq!(
        full.seed.spent, 504_000,
        "the full read keeps the import's cost"
    );
    let before_retired = "action.succeeded,action.failed,action.outcome_unknown,\
        speech.transcribed,speech.synthesized,synthesis.proposed,judge.replay,judge.audit,\
        judge.backfill,judge.proposal,people.extracted,thread.imported,card.proposed,\
        card.observed,keep_warm,initiative.spoke,onto.named";
    let after_retired = before_retired.replace("thread.imported,", "");
    for (kinds, spent) in [
        (before_retired.to_string(), 504_000u64),
        (after_retired, 4_000),
    ] {
        let old = json!({
            "rule": 1, "kinds": kinds, "day": "today", "since_ms": since,
            "through": store.last_position(), "spent": spent, "warned": 0, "ceilings": [],
        });
        store.put_meta(TOTAL_KEY, &old).unwrap();
        let s = read_today_total(&store, since, "today", LIMIT)
            .unwrap()
            .unwrap();
        assert_eq!(s.seed, full.seed, "the stored total's read, from {kinds}");
        let again = read_today_total(&store, since, "today", LIMIT)
            .unwrap()
            .unwrap();
        assert_eq!(
            (again.resumed, again.seed),
            (true, full.seed.clone()),
            "{kinds}"
        );
    }
}

/// Guidance keeps its own day mark even when an answer fails or is refused.
/// Reopening the store must not turn that settled spend back into headroom.
#[test]
fn guidance_spend_survives_restart_once() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("store");
    let store = Store::open(&path).unwrap();
    store
        .put_meta(
            crate::consolidate::guidance::MARK,
            &json!({"day": "2026-11-23", "spent_usd": 0.0625}),
        )
        .unwrap();
    drop(store);
    let store = Store::open(&path).unwrap();
    for _ in 0..2 {
        let total = read_today_total(&store, 0, "2026-11-23", LIMIT)
            .unwrap()
            .unwrap();
        assert_eq!(total.seed.spent, 62_500);
    }
    assert_eq!(
        read_today_total(&store, 0, "2026-11-24", LIMIT)
            .unwrap()
            .unwrap()
            .seed
            .spent,
        0
    );
}

#[test]
fn unreadable_guidance_spend_restores_no_headroom() {
    for mark in [
        json!({"day": "2026-11-23"}),
        json!({"day": "2026-11-23", "spent_usd": -1.0}),
    ] {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("store");
        let store = Store::open(&path).unwrap();
        store
            .put_meta(crate::consolidate::guidance::MARK, &mark)
            .unwrap();
        drop(store);
        let store = Store::open(&path).unwrap();
        assert!(
            read_today_total(&store, 0, "2026-11-23", LIMIT).is_err(),
            "a damaged guidance mark must leave startup admission closed"
        );
    }
}

/// Settlement rows must replay the same model-only amount after the store closes.
#[test]
fn uncertain_and_cancelled_model_spend_survives_restart() {
    let _clock = crate::test_support::ClockPin;
    theseus_protocol::time::move_clock_to(1_800_000_000_000);
    for (kind, cost, expected) in [
        ("unknown", None, 100),
        ("cancel", None, 100),
        ("verified", Some(70), 70),
        ("late", Some(150), 150),
        ("tool", None, 0),
    ] {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("store");
        let now = 1_800_000_000_000;
        {
            let store = Store::open(&path).unwrap();
            let kernel = theseus_kernel::Kernel::new(
                store.shared(),
                theseus_kernel::VirtualClock::new(now),
                theseus_kernel::KernelConfig::default(),
            );
            kernel.startup(None, &theseus_kernel::NoEvidence).unwrap();
            let a = day_action(
                &kernel,
                if kind == "tool" {
                    "proc.run"
                } else {
                    theseus_kernel::PROVIDER_TOOL
                },
            );
            match kind {
                "cancel" | "tool" | "late" => {
                    kernel
                        .cancel_unsupported(&a.correlation_id, "cannot stop")
                        .unwrap();
                }
                "verified" => {
                    kernel
                        .cancel_verified_costing(
                            &a.correlation_id,
                            &theseus_kernel::Verdict::uncertain(
                                theseus_kernel::VerifiedBy::None,
                                "fixture",
                            ),
                            cost.unwrap(),
                        )
                        .unwrap();
                }
                _ => {
                    kernel
                        .mark_unknown(&a.correlation_id, "connection lost")
                        .unwrap();
                }
            }
            if kind == "late" {
                kernel
                    .accept_completion(&day_completion(&a.correlation_id, cost))
                    .unwrap();
            }
            let ceiling = kernel.day_ceiling();
            assert_eq!(ceiling.today(now).spent, expected, "live {kind}");
        }
        let store = Store::open(&path).unwrap();
        let first = read_today_total(&store, now - 1, "fixture-day", LIMIT)
            .unwrap()
            .unwrap();
        assert_eq!(first.seed.spent, expected, "restart {kind}");
        assert_eq!(
            read_today_total(&store, now - 1, "fixture-day", LIMIT)
                .unwrap()
                .unwrap()
                .seed,
            first.seed,
            "read twice {kind}"
        );
    }
}

fn day_action(kernel: &theseus_kernel::Kernel, tool: &str) -> theseus_kernel::Action {
    use theseus_kernel::{Authority, RetryClass};
    let e = kernel
        .open_execution(
            "ses_day_fixture",
            theseus_protocol::SessionKind::Conversation,
            Authority {
                principal: "owner".into(),
                delegated_by: None,
                ceilings: Default::default(),
            },
            Some(10_000),
            None,
        )
        .unwrap();
    kernel.wake_input(&e.id).unwrap();
    let g = kernel.admit(&e.id).unwrap();
    let proposal = theseus_kernel::gate::Proposal {
        tool: tool.into(),
        args: json!({}),
        resource: None,
        policy_context: json!({}),
    };
    let a = kernel
        .plan_action(&g, &proposal, RetryClass::SafeToRepeat, None, 100)
        .unwrap();
    kernel
        .authorize(&a.correlation_id, &proposal, None)
        .unwrap();
    kernel.dispatch(&a.correlation_id, None).unwrap()
}

fn day_completion(id: &str, cost: Option<u64>) -> theseus_kernel::Completion {
    theseus_kernel::Completion {
        correlation_id: id.into(),
        outcome: theseus_kernel::Outcome::Succeeded,
        result_ref: None,
        external_op_id: None,
        started_at_ms: 1_800_000_000_000,
        finished_at_ms: 1_800_000_000_000,
        producer: "provider:fixture".into(),
        signature: None,
        cost_micros: cost,
        detail: None,
    }
}

#[test]
fn owner_settlement_counts_only_day_excess_across_restart() {
    let _clock = crate::test_support::ClockPin;
    theseus_protocol::time::move_clock_to(1_800_000_000_000);
    for (settled, late, expected) in [(150, 170, 170), (50, 70, 100), (50, 170, 170)] {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("store");
        let now = 1_800_000_000_000;
        {
            let store = Store::open(&path).unwrap();
            let kernel = theseus_kernel::Kernel::new(
                store.shared(),
                theseus_kernel::VirtualClock::new(now),
                theseus_kernel::KernelConfig::default(),
            );
            kernel.startup(None, &theseus_kernel::NoEvidence).unwrap();
            let a = day_action(&kernel, theseus_kernel::PROVIDER_TOOL);
            kernel
                .mark_unknown(&a.correlation_id, "connection lost")
                .unwrap();
            kernel
                .settle_held_spend(&a.correlation_id, settled, "owner")
                .unwrap();
            assert_eq!(
                kernel.day_ceiling().today(now).spent,
                settled.max(100),
                "owner's bill"
            );
            kernel
                .accept_completion(&day_completion(&a.correlation_id, Some(late)))
                .unwrap();
            assert_eq!(kernel.day_ceiling().today(now).spent, expected, "late bill");
        }
        let store = Store::open(&path).unwrap();
        assert_eq!(
            read_today_total(&store, now - 1, "fixture-day", LIMIT)
                .unwrap()
                .unwrap()
                .seed
                .spent,
            expected,
            "restart"
        );
    }
}

#[test]
fn a_restart_seed_is_not_added_again_after_live_spend() {
    let _clock = crate::test_support::ClockPin;
    theseus_protocol::time::move_clock_to(1_800_000_000_000);
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("store");
    let now = 1_800_000_000_000;
    {
        let store = Store::open(&path).unwrap();
        store
            .append(&[row_at(
                "speech.synthesized",
                now,
                json!({"cost_usd": 0.0001}),
            )])
            .unwrap();
    }
    let store = Store::open(&path).unwrap();
    let cutoff = store.last_position();
    let k = theseus_kernel::Kernel::new(
        store.shared(),
        theseus_kernel::VirtualClock::new(now),
        theseus_kernel::KernelConfig::default(),
    );
    let c = k.day_ceiling();
    c.set_limit(LIMIT);
    c.require_seed();
    assert!(c.fits(now, 1).unwrap_err().pending);
    assert!(c.hold(now, "blocked", 1).unwrap_err().pending);
    // Startup reconciliation can settle earlier calls before the deferred seed lands.
    c.book(now, 25);
    store
        .append(&[row_at(
            "speech.synthesized",
            now,
            json!({"cost_usd": 0.000025}),
        )])
        .unwrap();
    let seed = crate::day_ceiling::read_today_total_at(
        &store,
        now - 1,
        "fixture-day",
        LIMIT,
        Some(cutoff),
    )
    .unwrap()
    .unwrap()
    .seed;
    assert_eq!(
        seed.spent, 100,
        "the seed stops before this process's bookings"
    );
    c.seed(now, &seed);
    assert!(
        c.fits(now, 1).is_ok(),
        "temporary seed refusal was not a sticky stop"
    );
    let full = read_today_total(&store, now - 1, "fixture-day", LIMIT)
        .unwrap()
        .unwrap()
        .seed;
    assert_eq!(full.spent, 125);
    c.seed(now, &full);
    assert_eq!(
        c.today(now).spent,
        125,
        "one complete startup seed, never twice"
    );
}

#[test]
fn unreadable_judge_spend_is_not_zero_after_restart() {
    let _clock = crate::test_support::ClockPin;
    theseus_protocol::time::move_clock_to(1_800_000_000_000);
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("store");
    {
        let store = Store::open(&path).unwrap();
        store
            .put_meta(
                crate::judge::spend::META_KEY,
                &json!({"day": "fixture-day", "reserved_micros": 0, "spent_micros": "unreadable"}),
            )
            .unwrap();
    }
    let store = Store::open(&path).unwrap();
    assert!(
        read_today_total(&store, 0, "fixture-day", LIMIT).is_err(),
        "unreadable day spend must not restore headroom"
    );
    let now = 1_800_000_000_000;
    let k = theseus_kernel::Kernel::new(
        store.shared(),
        theseus_kernel::VirtualClock::new(now),
        theseus_kernel::KernelConfig::default(),
    );
    let c = k.day_ceiling();
    c.set_limit(LIMIT);
    c.require_seed();
    assert!(c.hold(now, "blocked", 1).unwrap_err().pending);
    store
        .put_meta(
            crate::judge::spend::META_KEY,
            &json!({"day": "fixture-day", "reserved_micros": 0, "spent_micros": 99}),
        )
        .unwrap();
    let seed = read_today_total(&store, 0, "fixture-day", LIMIT)
        .unwrap()
        .unwrap()
        .seed;
    c.seed(now, &seed);
    assert_eq!(c.today(now).spent, 99);
    assert!(c.hold(now, "after-repair", 1).is_ok());
}

#[test]
fn a_failed_restart_read_closes_core_admission_until_a_successful_retry() {
    let _clock = crate::test_support::ClockPin;
    theseus_protocol::time::move_clock_to(1_800_000_000_000);
    use std::sync::Arc;
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("store");
    let now = 1_800_000_000_000;
    let clock = theseus_kernel::VirtualClock::new(now);
    let mut cfg = crate::Config::example();
    cfg.server.state_dir = dir.path().to_string_lossy().into_owned();
    cfg.tools.roots = vec![];
    let day;
    {
        let store = Store::open(&path).unwrap();
        let k = theseus_kernel::Kernel::new(
            store.shared(),
            clock.clone(),
            cfg.kernel.to_kernel_config(),
        );
        day = k.day_ceiling().day_of(now).0;
        store
            .put_meta(
                crate::judge::spend::META_KEY,
                &json!({"day": day, "spent_micros": "unreadable", "reserved_micros": 0}),
            )
            .unwrap();
    }
    let reopened = Store::open(&path).unwrap();
    assert!(read_today_total(&reopened, 0, &day, LIMIT).is_err());
    let parts = crate::rpc::Parts::for_tests(
        cfg,
        Arc::new(crate::provider::FakeProvider::scripted(vec![])),
        reopened,
    );
    let core = crate::Core::build_with_clock(parts, clock).unwrap();
    assert!(core.runner.seed_day_ceiling().later);
    let c = core.kernel.day_ceiling();
    assert!(!c.seed_applied());
    assert!(c.hold(now, "model", 1).unwrap_err().pending);
    assert!(core.runner.day_hold(1, "fixture background call").is_err());
    assert!(
        !c.take_notice(now),
        "unreadiness is not a daily-limit notice"
    );
    core.store
        .put_meta(
            crate::judge::spend::META_KEY,
            &json!({"day": day, "spent_micros": 99, "reserved_micros": 0}),
        )
        .unwrap();
    assert!(!core.runner.seed_day_ceiling().later);
    assert_eq!(c.today(now).spent, 99);
    assert!(core.runner.day_hold(1, "fixture background call").is_ok());
    core.runner.seed_day_ceiling();
    assert_eq!(c.today(now).spent, 99);
}

#[test]
fn damaged_day_marks_stop_today_but_not_tomorrow() {
    let _clock = crate::test_support::ClockPin;
    let now = 1_800_000_000_000;
    theseus_protocol::time::move_clock_to(now);
    for key in [
        crate::judge::spend::META_KEY,
        crate::consolidate::guidance::MARK,
    ] {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("store");
        let clock = theseus_kernel::VirtualClock::new(now);
        let mut cfg = crate::Config::example();
        cfg.server.state_dir = dir.path().to_string_lossy().into_owned();
        cfg.tools.roots = vec![];
        let midnight;
        {
            let store = Store::open(&path).unwrap();
            let k = theseus_kernel::Kernel::new(
                store.shared(),
                clock.clone(),
                cfg.kernel.to_kernel_config(),
            );
            let (today, _, end) = k.day_ceiling().day_of(now);
            midnight = end;
            store
                .put_meta(
                    key,
                    &json!({"day": today, "spent_micros": "garbage", "spent_usd": "garbage"}),
                )
                .unwrap();
        }
        let parts = crate::rpc::Parts::for_tests(
            cfg,
            std::sync::Arc::new(crate::provider::FakeProvider::scripted(vec![])),
            Store::open(&path).unwrap(),
        );
        let core = crate::Core::build_with_clock(parts, clock.clone()).unwrap();
        assert!(core.runner.seed_day_ceiling().later);
        assert!(core.runner.day_hold(1, "test").is_err());
        clock.set(midnight + 1);
        theseus_protocol::time::move_clock_to(midnight + 1);
        let seeded = core.runner.seed_day_ceiling();
        assert!(
            !seeded.later,
            "yesterday's damaged mark must not stop today: {key}"
        );
        assert_eq!(seeded.seed.spent, 0);
        assert!(core.runner.day_hold(1, "test").is_ok());
        theseus_protocol::time::move_clock_to(now);
    }
}

#[test]
fn pending_seed_without_a_bar_refuses_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open(&dir.path().join("store")).unwrap();
    let k = theseus_kernel::Kernel::new(
        store.shared(),
        theseus_kernel::VirtualClock::new(1_800_000_000_000),
        theseus_kernel::KernelConfig::default(),
    );
    let c = k.day_ceiling();
    c.set_limit(0);
    c.require_seed();
    assert!(!c.seed_applied());
    assert!(c.fits(c.now(), 1).is_ok());
    assert!(c.hold_call(c.now(), 1).is_ok());
}

#[test]
fn day_read_failure_warns_on_change_or_minute_and_clears_on_success() {
    let dir = tempfile::tempdir().unwrap();
    let (core, _) = crate::tests_day_ceiling::build(dir.path(), vec![], |_| {});
    let now = core.kernel.day_ceiling().now();
    let mut failure = core.runner.day_read_failure.lock().unwrap();
    let error = "today's judge spending cannot be read".to_string();
    assert!(failure.failed(now - 60_001, error.clone()));
    assert!(!failure.failed(now - 60_000, error.clone()));
    assert!(failure.failed(now - 1, error));
    drop(failure);
    let h = core.health();
    let phase = h.startup.iter().find(|p| p.name == "day_ceiling").unwrap();
    assert!(phase.detail["read_error"]
        .as_str()
        .unwrap()
        .contains("judge spending"));
    let mut failure = core.runner.day_read_failure.lock().unwrap();
    assert!(failure.failed(now, "today's guidance spending cannot be read".into()));
    drop(failure);
    assert!(!core.runner.seed_day_ceiling().later);
    assert!(core
        .runner
        .day_read_failure
        .lock()
        .unwrap()
        .health(now)
        .is_none());
}

#[cfg(target_os = "linux")]
#[test]
fn a_failed_seed_retry_thread_releases_the_core_and_store_on_drop() {
    fn retry_threads() -> Vec<std::path::PathBuf> {
        std::fs::read_dir("/proc/self/task")
            .unwrap()
            .flatten()
            .map(|e| e.path())
            .filter(|p| {
                std::fs::read_to_string(p.join("comm")).is_ok_and(|n| n.trim() == "day-ceiling")
            })
            .collect()
    }
    let dir = tempfile::tempdir().unwrap();
    {
        let store = Store::open(&dir.path().join("store")).unwrap();
        store
            .put_meta(crate::judge::spend::META_KEY, &json!({"day": null}))
            .unwrap();
    }
    let (core, _) = crate::tests_day_ceiling::build(dir.path(), vec![], |_| {});
    assert!(core.runner.seed_day_ceiling().later);
    let weak_core = std::sync::Arc::downgrade(&core);
    let weak_store = std::sync::Arc::downgrade(core.store.inner());
    let until = std::time::Instant::now() + std::time::Duration::from_secs(3);
    while retry_threads().is_empty() {
        assert!(
            std::time::Instant::now() < until,
            "retry thread never started"
        );
        std::thread::yield_now();
    }
    drop(core);
    while !retry_threads().is_empty() || weak_store.upgrade().is_some() {
        assert!(
            std::time::Instant::now() < until,
            "retry thread kept the store open"
        );
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    assert!(weak_core.upgrade().is_none());
    assert!(weak_store.upgrade().is_none());
    Store::open(&dir.path().join("store")).unwrap();
}
