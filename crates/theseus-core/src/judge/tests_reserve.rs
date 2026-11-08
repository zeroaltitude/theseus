//! A reservation's sentences (M5 23b; theseus-02vo): the budget's facts are
//! said once their frame is written, by `write_budget`, and the sink's by
//! its own `write`. Each is read from the narrative's tail, as a client
//! reads it, on a core whose narrative is on.
//!
//! No whole-core turn crosses a local midnight (every point reserves on the
//! wall clock's day), so these call `reserve` with the days they name.

use std::sync::Arc;
use std::time::{Duration, Instant};

use theseus_judge::fake::{FakeJev, FakeMode};
use theseus_store::NewRecord;

use super::spend::{Class, Stored, META_KEY};
use crate::provider::FakeProvider;
use crate::rpc::{Core, Parts};
use crate::store::Store;
use crate::tests_judge::{board, config, kinds, rig_with, texts, turn};

const LIMIT: f64 = 0.0002;
/// A reservation that fits shadow's line (70% of the day's 200 micros, so
/// 140) once, never twice.
const NEED: u64 = 100;

fn said(core: &Arc<Core>, starts: &str) -> Vec<String> {
    core.narrator
        .tail()
        .into_iter()
        .map(|l| l.text)
        .filter(|t| t.starts_with(starts))
        .collect()
}

/// A core on `dir`'s store, the narrative on, as a start builds one.
fn core_on(dir: &std::path::Path, jev: &FakeJev) -> Arc<Core> {
    let mut cfg = config(dir, Some(jev));
    cfg.narrative = true;
    cfg.judge.shadow_limit_usd_per_day = LIMIT;
    let store = Store::open(&dir.join("store")).unwrap();
    let mut p = Parts::for_tests(cfg, Arc::new(FakeProvider::scripted(texts(1))), store);
    p.secrets = board();
    Core::build(p).unwrap()
}

/// Reservations of `NEED` from shadow until the day refuses one: how many it
/// let through (judging goes on past the line, to ten times it).
fn fill(svc: &super::JudgeService, day: &str) -> u32 {
    let mut n = 0;
    while svc.reserve(day, NEED, Class::Shadow) {
        n += 1;
        assert!(n < 1000, "never refused");
    }
    n
}

/// A day's limit reached: one `judge.paused` row and one line, however many
/// reservations the day refuses, and both once the row's frame is written.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_days_limit_says_its_pause_once() {
    let jev = FakeJev::start().unwrap();
    let r = rig_with(vec![], Some(&jev), |c| {
        c.narrative = true;
        c.judge.shadow_limit_usd_per_day = LIMIT;
    });
    let svc = &r.core.runner.judge;
    assert!(said(&r.core, "Shadow judging paused").is_empty());
    assert_eq!(
        fill(svc, "2026-10-01"),
        14,
        "judging goes on to ten times shadow's line (1,400)"
    );
    for _ in 0..3 {
        assert!(
            !svc.reserve("2026-10-01", NEED, Class::Shadow),
            "refused past ten times shadow's line"
        );
    }
    let rows = kinds(&r.core.store, "judge.paused");
    assert_eq!(rows.len(), 1, "{rows:?}");
    assert_eq!(
        rows[0].data["limit_micros"], 1400,
        "ten times shadow's line"
    );
    assert_eq!(rows[0].data["what"], "shadow");
    assert_eq!(
        said(&r.core, "Shadow judging paused"),
        ["Shadow judging paused: today's $0.00 is spent. It resumes at local midnight."],
        "one line for the one row"
    );
}

/// The next day's first reservation, in the process that said the pause,
/// says it resumed, naming both days; a fresh process after a paused day
/// says nothing.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_next_days_first_reservation_says_it_resumed_and_a_restart_does_not() {
    let jev = FakeJev::start().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let core = core_on(dir.path(), &jev);
    let svc = &core.runner.judge;
    assert_eq!(fill(svc, "2026-10-01"), 14);
    assert!(!svc.reserve("2026-10-01", NEED, Class::Shadow));
    assert!(
        said(&core, "Shadow judging resumed").is_empty(),
        "not before"
    );
    // The first judgment settles, as a call that came back does; what is
    // still in flight at midnight carries into the next day.
    for _ in 0..14 {
        svc.budget.settle("2026-10-01", NEED, 0, true, false);
    }
    assert!(
        svc.reserve("2026-10-02", NEED, Class::Shadow),
        "the new day has room"
    );
    let rows = kinds(&core.store, "judge.resumed");
    assert_eq!(rows.len(), 1, "{rows:?}");
    assert_eq!(rows[0].data["day"], "2026-10-02");
    assert_eq!(rows[0].data["paused_day"], "2026-10-01");
    let expect = "Shadow judging resumed: a new day (2026-10-02) after 2026-10-01's pause.";
    assert_eq!(said(&core, "Shadow judging resumed"), [expect]);
    // The day's other reservations say nothing more.
    assert_eq!(fill(svc, "2026-10-02"), 13, "the rest of the new day");
    assert_eq!(said(&core, "Shadow judging resumed"), [expect], "once");
    assert_eq!(kinds(&core.store, "judge.resumed").len(), 1);
    drop(core);

    // A restart forgets the pause: the next day's first reservation in the
    // fresh process writes no resume row and says no line.
    let again = core_on(dir.path(), &jev);
    assert!(again
        .runner
        .judge
        .reserve("2026-10-03", NEED, Class::Shadow));
    assert_eq!(kinds(&again.store, "judge.resumed").len(), 1, "no new row");
    assert!(said(&again, "Shadow judging resumed").is_empty());
}

/// A store whose record holds today's block reserved past its settled
/// spend: the first reservation books the rest, with its row and its line,
/// and the second says nothing more.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_blocks_unsettled_rest_is_booked_at_the_first_reservation_and_said() {
    let jev = FakeJev::start().unwrap();
    let r = rig_with(vec![], Some(&jev), |c| c.narrative = true);
    let day = "2026-10-01";
    let stored = Stored {
        day: day.into(),
        reserved_micros: 10_000,
        spent_micros: 2_000,
    };
    let rec = NewRecord::json(theseus_store::kinds::META, Some(META_KEY), &stored).unwrap();
    r.core.store.append(&[rec]).unwrap();
    let svc = &r.core.runner.judge;
    assert!(
        said(&r.core, "The judge booked").is_empty(),
        "nothing before"
    );
    assert!(svc.reserve(day, 100, Class::Live));
    let rows = kinds(&r.core.store, "judge.block_booked");
    assert_eq!(rows.len(), 1, "{rows:?}");
    assert_eq!(rows[0].data["reserved_micros"], 10_000);
    assert_eq!(rows[0].data["booked_micros"], 8_000);
    assert_eq!(
        said(&r.core, "The judge booked"),
        ["The judge booked $0.01 of today's reserved block as spent: a stop lost what it settled."]
    );
    assert!(svc.reserve(day, 100, Class::Live));
    assert_eq!(kinds(&r.core.store, "judge.block_booked").len(), 1);
    assert_eq!(said(&r.core, "The judge booked").len(), 1, "once");
}

/// One judgment of `ASK` micros asked of `class`, and what it cost, settled
/// as a call that came back: whether the day let it through.
const ASK: u64 = 10;

fn spend_one(svc: &super::JudgeService, day: &str, class: Class) -> bool {
    let granted = svc.reserve(day, ASK, class);
    if granted {
        svc.budget.settle(day, ASK, ASK, true, false);
    }
    granted
}

/// How many of `class`'s judgments the day lets through before it refuses
/// one: counted, so that a run that let none through cannot pass.
fn made(svc: &super::JudgeService, day: &str, class: Class) -> u32 {
    let mut n = 0;
    while spend_one(svc, day, class) {
        n += 1;
        assert!(n < 1000, "never refused");
    }
    n
}

/// The day's budget has two lines (theseus-0j2.16): shadow's is 70% of the
/// limit, live's the whole of it, and each class stops at ten times its own
/// line, so shadow's stop (1,400) is always below live's (2,000). With the
/// day's 200 micros, shadow makes 140 judgments of 10 and is refused; live
/// then makes the 60 that are left. Both counts are asserted, and each refusal
/// says itself once.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn shadow_stops_at_seventy_percent_and_live_runs_on() {
    let jev = FakeJev::start().unwrap();
    let r = rig_with(vec![], Some(&jev), |c| {
        c.narrative = true;
        c.judge.shadow_limit_usd_per_day = LIMIT;
    });
    let svc = &r.core.runner.judge;
    let day = "2026-10-01";
    assert_eq!(svc.budget.cap(Class::Shadow), 140);
    assert_eq!(svc.budget.cap(Class::Live), 200);

    assert_eq!(made(svc, day, Class::Shadow), 140, "shadow, to 10x 140");
    assert!(!spend_one(svc, day, Class::Shadow), "shadow stays stopped");
    assert!(svc.budget.shadow_paused(day), "shadow is paused");
    assert!(!svc.budget.paused(day), "the day's limit is not reached");
    let rows = kinds(&r.core.store, "judge.paused");
    assert_eq!(rows.len(), 1, "{rows:?}");
    assert_eq!(rows[0].data["what"], "shadow");
    assert_eq!(rows[0].data["limit_micros"], 1400);

    assert_eq!(made(svc, day, Class::Live), 60, "live, on to 10x 200");
    assert!(svc.budget.paused(day), "now the limit is reached");
    assert!(!spend_one(svc, day, Class::Shadow));
    let rows = kinds(&r.core.store, "judge.paused");
    assert_eq!(rows.len(), 2, "one row for each line: {rows:?}");
    assert_eq!(rows[1].data["what"], "live");
    assert_eq!(rows[1].data["limit_micros"], 2000);
    assert_eq!(said(&r.core, "Shadow judging paused").len(), 1);
    assert_eq!(said(&r.core, "Judging paused").len(), 1);
    assert_eq!(svc.budget.today(&r.core.store, day).spent_micros, 2000);
}

/// Live spend past shadow's line leaves shadow no room at all, while live
/// goes on: the shadow count is zero only because live took the day, and
/// live's own count is positive.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn live_spend_past_the_shadow_line_leaves_shadow_nothing() {
    let jev = FakeJev::start().unwrap();
    let r = rig_with(vec![], Some(&jev), |c| {
        c.judge.shadow_limit_usd_per_day = LIMIT;
    });
    let svc = &r.core.runner.judge;
    let day = "2026-10-01";
    let live = made(svc, day, Class::Live);
    assert_eq!(live, 200, "live may take ten times the limit");
    assert_eq!(made(svc, day, Class::Shadow), 0, "shadow had no room");
    // A new day starts both lines afresh.
    let next = "2026-10-02";
    assert_eq!(made(svc, next, Class::Shadow), 140);
    assert_eq!(made(svc, next, Class::Live), 60);
}

/// What the ask's mode makes of a judgment: a canary's arm and a live ask
/// act, so they spend live; shadow alone is shadow.
#[test]
fn a_judgments_mode_gives_its_class() {
    use theseus_judge::Mode;
    assert_eq!(Class::of(Mode::Shadow), Class::Shadow);
    assert_eq!(Class::of(Mode::Canary), Class::Live);
    assert_eq!(Class::of(Mode::Live), Class::Live);
}

/// Wait (on the runtime's timer) for `ok`.
async fn until(what: &str, ok: impl Fn() -> bool) {
    let t0 = Instant::now();
    while !ok() {
        assert!(t0.elapsed() < Duration::from_secs(20), "never: {what}");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

/// A breaker that opens says its line once its row's frame is written: the
/// fake Jev is down, and five failed judgments in a row trip it.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_breaker_that_opens_says_its_line_once() {
    let jev = FakeJev::start().unwrap();
    jev.set_mode(FakeMode::Down);
    let r = rig_with(texts(8), Some(&jev), |c| c.narrative = true);
    for i in 0..8 {
        turn(&r.core, None, &format!("turn {i}")).await;
        if !kinds(&r.core.store, "judge.circuit").is_empty() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    until("the breaker's row", || {
        !kinds(&r.core.store, "judge.circuit").is_empty()
    })
    .await;
    until("the breaker's line", || {
        !said(&r.core, "Jev's breaker opened").is_empty()
    })
    .await;
    let line = said(&r.core, "Jev's breaker opened");
    assert_eq!(line.len(), 1, "{:?}", r.core.narrator.tail());
    assert!(
        line[0].starts_with("Jev's breaker opened after 5 failures in a row: judgments skip for "),
        "{line:?}"
    );
    let rows = kinds(&r.core.store, "judge.circuit");
    assert_eq!(rows.len(), 1, "one move, one row: {rows:?}");
}

/// A shed report says its own line, once its row is written: with one
/// in-flight permit held by a slow call, the next judgments are shed.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_shed_report_says_its_line() {
    let jev = FakeJev::start().unwrap();
    jev.set_mode(FakeMode::Slow(Duration::from_secs(60)));
    let r = rig_with(texts(2), Some(&jev), |c| {
        c.narrative = true;
        c.judge.max_in_flight = 1;
        // The slow call holds its permit for the whole test.
        c.judge.total_secs = 25;
    });
    turn(&r.core, None, "one").await;
    until("the slow call's permit", || jev.connections() >= 1).await;
    turn(&r.core, None, "two").await;
    until("the shed row", || {
        !kinds(&r.core.store, "judge.shed").is_empty()
    })
    .await;
    until("the shed line", || !said(&r.core, "Jev shed").is_empty()).await;
    let rows = kinds(&r.core.store, "judge.shed");
    let n = rows[0].data["shed"].as_u64().unwrap();
    assert!(n >= 1, "{rows:?}");
    let want = match n {
        1 => "Jev shed 1 shadow judgment: every in-flight permit was taken.".to_string(),
        n => format!("Jev shed {n} shadow judgments: every in-flight permit was taken."),
    };
    assert_eq!(said(&r.core, "Jev shed"), [want]);
}

/// The ladder (the owner: "overage is notify until 10x, like others, then
/// skip"): past its line a class is told at the line and at each multiple of
/// it and judges on; only past ten times the line is a judgment skipped. The
/// unit is the class's own line: shadow's 140 here (70% of the day's 200).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn past_its_line_a_class_is_told_at_each_multiple_and_skipped_only_past_ten() {
    let jev = FakeJev::start().unwrap();
    let r = rig_with(vec![], Some(&jev), |c| {
        c.narrative = true;
        c.judge.shadow_limit_usd_per_day = LIMIT;
    });
    let svc = &r.core.runner.judge;
    let day = "2026-10-01";
    let mut granted = 0;
    for _ in 0..20 {
        if svc.reserve(day, NEED, Class::Shadow) {
            granted += 1;
        }
    }
    // 100 a call: the 14th takes the held spend to 1,400, ten times the line.
    assert_eq!(granted, 14, "judging goes on to ten times the line");
    let over = kinds(&r.core.store, "judge.over_line");
    let multiples: Vec<u64> = over
        .iter()
        .map(|o| o.data["multiple"].as_u64().unwrap())
        .collect();
    assert_eq!(multiples, (1..=10).collect::<Vec<u64>>(), "{over:?}");
    assert!(over.iter().all(|o| o.data["line_micros"] == 140));
    let paused = kinds(&r.core.store, "judge.paused");
    assert_eq!(paused.len(), 1, "{paused:?}");
    assert_eq!(paused[0].data["limit_micros"], 1400, "ten times the line");
    assert_eq!(said(&r.core, "Shadow judging is at ").len(), 10);
    assert_eq!(said(&r.core, "Shadow judging paused").len(), 1);
}
