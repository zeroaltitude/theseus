//! A routine's start, its stop, and what one run carries to the next
//! (theseus-97b8c). The owner's words: "a recurring scheduled task with
//! flexible parameters: start, optional stop, interval, etc, that acts like a
//! super smart cron but internally treats it as a long term task".
//!
//! The model decides what and when; from the tool call onward the schedule, the
//! runs, the carried state and the stop are the daemon's, and none of them is
//! re-decided by a model at each run:
//!
//! - **A run count stops the series as a fact.** The daemon counts the runs it
//!   opened; the occurrence past the count is skipped `stopped` and the series
//!   is held, whether or not anything is awake to notice.
//! - **A date stop is the kernel's own `until`**: the series ends there, so
//!   nothing is due past it and no skip is needed.
//! - **A start moves the first occurrence**, so "every morning from Monday" is
//!   one routine and not a reminder to make one.
//! - **A run is given the whole of the last run's report**, so run N reports
//!   what changed instead of repeating the state.

use serde_json::json;
use theseus_kernel::Purpose;

use crate::test_support::until_within as until;
use crate::tests_routines::{due_soon, made, reports, rig, rpc, runs};

/// The routine's pending wake, from the kernel.
fn routine_wake(r: &crate::tests_routines::Rig, id: &str) -> theseus_kernel::PendingWake {
    let book = &r.core.runner.tools.routines;
    let spec = book.find(&r.core.store, id).unwrap();
    r.core
        .kernel
        .execution(&spec.execution_id)
        .unwrap()
        .unwrap()
        .wakes
        .into_iter()
        .find(|w| w.purpose == Purpose::Routine)
        .expect("the routine's wake")
}

/// A stop of so many runs is a fact the daemon keeps: it opens that many and no
/// more, the occurrence past it is skipped `stopped`, and the series is held
/// there rather than skipping for ever.
#[tokio::test(flavor = "multi_thread")]
async fn a_run_count_stop_opens_that_many_runs_and_then_holds_the_series() {
    let r = rig();
    let info = made(&r.core, json!({"stop": "after 2 runs"})).await;
    let id = info["id"].as_str().unwrap().to_string();
    assert_eq!(info["stop"], "after 2 runs", "{info}");
    assert!(info["stopped"].is_null(), "nothing has stopped yet");
    for n in 1..=2 {
        due_soon(&r.core).await;
        until(&format!("run {n}'s report"), 30, || {
            reports(&r.core).len() == n
        })
        .await;
    }
    assert_eq!(runs(&r.core, &id).await.len(), 2);
    // The occurrence past the count runs nothing and says why.
    due_soon(&r.core).await;
    until("the stop", 30, || {
        crate::test_support::rows(&r.core, "wake.skipped")
            .iter()
            .any(|s| s["why"] == "stopped")
    })
    .await;
    assert_eq!(runs(&r.core, &id).await.len(), 2, "no third run");
    until("the series held", 30, || {
        routine_wake(&r, &id).paused_at_ms.is_some()
    })
    .await;
    let listed = rpc(&r.core, "routine.list", json!({})).await.unwrap();
    let one = &listed["routines"][0];
    assert_eq!(one["runs"], 2);
    assert_eq!(one["stopped"], "after 2 runs", "{one}");
    assert_eq!(
        one["paused"], true,
        "a stopped series is held, not skipped for ever: {one}"
    );
}

/// A stop that is a time is the kernel's own `until` on the series: the wake
/// ends there, so nothing is due past it.
#[tokio::test(flavor = "multi_thread")]
async fn a_date_stop_is_the_series_until_and_a_stop_before_the_first_run_is_refused() {
    let r = rig();
    let info = made(&r.core, json!({"stop": "in 3h"})).await;
    let id = info["id"].as_str().unwrap().to_string();
    let stop_ms = info["stop_ms"].as_u64().expect("the stop as a time");
    let now = r.core.kernel.now_ms();
    assert!(
        (stop_ms as i64 - (now + 3 * 3_600_000) as i64).abs() < 60_000,
        "stop_ms {stop_ms} is about three hours from {now}"
    );
    let w = routine_wake(&r, &id);
    assert_eq!(
        w.repeat.as_ref().and_then(|p| p.until_ms),
        Some(stop_ms),
        "the series ends at the stop"
    );
    assert!(w.due_at_ms < stop_ms);
    // A stop before the first occurrence would run nothing: refused in words.
    let err = rpc(
        &r.core,
        "routine.create",
        json!({"title": "Nothing", "schedule": "every 1h", "brief": "x",
               "start": "in 5h", "stop": "in 10m"}),
    )
    .await
    .unwrap_err();
    assert!(
        err.contains("before the first run"),
        "a stop with nothing in it says so: {err}"
    );
}

/// A start moves the first occurrence: "every morning, from Monday" is one
/// routine, not a reminder to make one later.
#[tokio::test(flavor = "multi_thread")]
async fn a_start_moves_the_first_occurrence_and_the_series_follows_it() {
    let r = rig();
    let now = r.core.kernel.now_ms();
    let info = made(&r.core, json!({"start": "in 2h"})).await;
    let next = info["next_ms"].as_u64().expect("a first time");
    assert!(
        next >= now + 2 * 3_600_000,
        "the first run is at or after the start: {next} vs {now}"
    );
    assert!(next < now + 3 * 3_600_000, "and not far past it: {next}");
    let start = info["start_ms"].as_u64().expect("the start as asked for");
    assert!(next >= start, "{info}");
    assert!(start >= now + 2 * 3_600_000 - 60_000, "{info}");
}

/// What makes "watch X" different from "run X hourly": run N is given the whole
/// of what run N-1 reported, so it can report the change rather than the state.
/// The carried text is the daemon's: it reads the last run's report, the model
/// does not re-derive it.
#[tokio::test(flavor = "multi_thread")]
async fn a_run_is_given_the_whole_of_the_last_runs_report_not_only_its_first_line() {
    let r = rig();
    *r.model.text.lock().unwrap() =
        "Tide at 06:12.\nSwell is 2 ft from the north.\nWind is light.".into();
    made(&r.core, json!({})).await;
    for n in 1..=2 {
        due_soon(&r.core).await;
        until(&format!("run {n}'s report"), 30, || {
            reports(&r.core).len() == n
        })
        .await;
    }
    let texts: Vec<String> = r
        .model
        .requests
        .lock()
        .unwrap()
        .iter()
        .map(|q| serde_json::to_string(&q.messages).unwrap())
        .collect();
    let second = texts
        .iter()
        .find(|t| t.contains("Run 2 of Tide check"))
        .expect("the second run's brief");
    assert!(
        second.contains("Swell is 2 ft from the north."),
        "the whole of the last report is carried: {second}"
    );
    assert!(
        second.contains("report what has changed"),
        "and the run is told to report the change: {second}"
    );
    let first = texts
        .iter()
        .find(|t| t.contains("Run 1 of Tide check"))
        .expect("the first run's brief");
    assert!(
        !first.contains("report what has changed"),
        "the first run has nothing to compare with: {first}"
    );
}

/// A routine that carries nothing keeps the one-line clause and no more: the
/// carry is a parameter of the tool call, not a judgment made at each run.
#[tokio::test(flavor = "multi_thread")]
async fn carry_none_leaves_the_one_line_clause_and_carries_no_body() {
    let r = rig();
    *r.model.text.lock().unwrap() = "Tide at 06:12.\nSwell is 2 ft from the north.".into();
    let info = made(&r.core, json!({"carry": "none"})).await;
    assert_eq!(info["carry"], "none", "{info}");
    for n in 1..=2 {
        due_soon(&r.core).await;
        until(&format!("run {n}'s report"), 30, || {
            reports(&r.core).len() == n
        })
        .await;
    }
    let texts: Vec<String> = r
        .model
        .requests
        .lock()
        .unwrap()
        .iter()
        .map(|q| serde_json::to_string(&q.messages).unwrap())
        .collect();
    let second = texts
        .iter()
        .find(|t| t.contains("Run 2 of Tide check"))
        .expect("the second run's brief");
    assert!(second.contains("the last run reported: Tide at 06:12."));
    assert!(
        !second.contains("Swell is 2 ft from the north."),
        "nothing is carried: {second}"
    );
}

/// The card the owner answers names the stop and the start, because the stop is
/// what the yes is really about: a routine with none runs until it is paused.
#[test]
fn the_creation_card_names_the_start_and_the_stop() {
    let cfg = crate::Config::example();
    let tool = crate::routine_tool::RoutineCreate {
        zone: theseus_kernel::TimeZone::UTC,
        cfg: cfg.routines.clone(),
        floor_ms: 60_000,
        max_ahead_ms: 400 * 86_400_000,
    };
    let ctx = theseus_tools::ToolCtx::default();
    let plan = theseus_tools::Tool::plan(
        &tool,
        &json!({"title": "Tide check", "schedule": "every 1h", "brief": "look",
                "stop": "after 3 runs"}),
        &ctx,
    )
    .expect("a plan");
    assert!(
        plan.summary.contains("stops after 3 runs"),
        "{}",
        plan.summary
    );
    let plan = theseus_tools::Tool::plan(
        &tool,
        &json!({"title": "Tide check", "schedule": "every 1h", "brief": "look"}),
        &ctx,
    )
    .expect("a plan");
    assert!(
        plan.summary.contains("until it is paused"),
        "a routine with no stop says so: {}",
        plan.summary
    );
}
