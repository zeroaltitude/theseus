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

use super::spend::{Stored, META_KEY};
use crate::provider::FakeProvider;
use crate::rpc::{Core, Parts};
use crate::store::Store;
use crate::tests_judge::{board, config, kinds, rig_with, texts, turn};

const LIMIT: f64 = 0.0002;
/// A reservation that fits the day's limit (200 micros) once, never twice.
const NEED: u64 = 150;

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
    assert!(svc.reserve("2026-10-01", NEED), "the first fits");
    assert!(said(&r.core, "Shadow judging paused").is_empty());
    for _ in 0..3 {
        assert!(!svc.reserve("2026-10-01", NEED), "refused at the limit");
    }
    let rows = kinds(&r.core.store, "judge.paused");
    assert_eq!(rows.len(), 1, "{rows:?}");
    assert_eq!(rows[0].data["limit_micros"], 200);
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
    assert!(svc.reserve("2026-10-01", NEED));
    assert!(!svc.reserve("2026-10-01", NEED));
    assert!(
        said(&core, "Shadow judging resumed").is_empty(),
        "not before"
    );
    // The first judgment settles, as a call that came back does; what is
    // still in flight at midnight carries into the next day.
    svc.budget.settle("2026-10-01", NEED, 0, true, false);
    assert!(svc.reserve("2026-10-02", NEED), "the new day has room");
    let rows = kinds(&core.store, "judge.resumed");
    assert_eq!(rows.len(), 1, "{rows:?}");
    assert_eq!(rows[0].data["day"], "2026-10-02");
    assert_eq!(rows[0].data["paused_day"], "2026-10-01");
    let expect = "Shadow judging resumed: a new day (2026-10-02) after 2026-10-01's pause.";
    assert_eq!(said(&core, "Shadow judging resumed"), [expect]);
    // The day's other reservations say nothing more.
    assert!(!svc.reserve("2026-10-02", NEED));
    assert_eq!(said(&core, "Shadow judging resumed"), [expect], "once");
    assert_eq!(kinds(&core.store, "judge.resumed").len(), 1);
    drop(core);

    // A restart forgets the pause: the next day's first reservation in the
    // fresh process writes no resume row and says no line.
    let again = core_on(dir.path(), &jev);
    assert!(again.runner.judge.reserve("2026-10-03", NEED));
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
    assert!(svc.reserve(day, 100));
    let rows = kinds(&r.core.store, "judge.block_booked");
    assert_eq!(rows.len(), 1, "{rows:?}");
    assert_eq!(rows[0].data["reserved_micros"], 10_000);
    assert_eq!(rows[0].data["booked_micros"], 8_000);
    assert_eq!(
        said(&r.core, "The judge booked"),
        ["The judge booked $0.01 of today's reserved block as spent: a stop lost what it settled."]
    );
    assert!(svc.reserve(day, 100));
    assert_eq!(kinds(&r.core.store, "judge.block_booked").len(), 1);
    assert_eq!(said(&r.core, "The judge booked").len(), 1, "once");
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
