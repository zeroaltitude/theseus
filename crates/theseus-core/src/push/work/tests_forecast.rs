//! A task's forecast on the work board, through a whole core (theseus-zz752):
//! the first line of a new task and its view say what tasks like it cost and
//! took; a task past its p90 is told once, never again in the run and not
//! after a restart; a shared place's watcher sees numbers and nothing else.

use serde_json::json;
use theseus_protocol::notices::Why;
use theseus_protocol::work::{WorkKind, WorkView};

use super::super::board::seed_inputs;
use super::super::work as board_work;
use super::*;
use crate::forecast::Row;
use crate::provider::Scripted;
use crate::tests_tasks::{
    answers_a_call, arranged, first_user, parent_session, rig, turn, until, ASK,
};

/// `n` finished tasks of the class a one-piece arrangement makes (`open`),
/// each costing `usd` dollars and taking `min` minutes.
fn history(n: usize, usd: f64, min: f64) -> Vec<Row> {
    (0..n)
        .map(|i| Row {
            task: format!("tsk_hist{i:03}"),
            class: "open".into(),
            terms: crate::forecast::terms_of("migrate the fixtures", ""),
            usd,
            min,
            loops: Some(4),
            started_ms: 1_000 + i as u64,
            ended_ms: 2_000 + i as u64,
        })
        .collect()
}

fn script(req: &crate::provider::ProviderRequest) -> Scripted {
    if first_user(req).contains("CHILD:") {
        return if answers_a_call(req) {
            Scripted::text("Done.")
        } else {
            Scripted::tools("", &[("c1", "proc_run", json!({"argv": ["echo", "hi"]}))])
        };
    }
    if answers_a_call(req) {
        return Scripted::text("Started it.");
    }
    Scripted::tools(
        "Starting it.",
        &[(
            "t_task",
            "task_create",
            json!({"brief": "CHILD: migrate the fixtures", "budget_usd": 1.5,
                   "arrangement": arranged()}),
        )],
    )
}

fn task_view(core: &crate::Core) -> Option<WorkView> {
    let all = core.push.work.lock().unwrap().all();
    all.into_values().find(|v| v.kind == WorkKind::Task)
}

fn past_notices(core: &crate::Core) -> Vec<theseus_protocol::notices::Notice> {
    core.push
        .notices_since(0)
        .0
        .into_iter()
        .filter(|n| n.why == Why::PastForecast)
        .collect()
}

/// The first line of a new task is its forecast, and its view carries the
/// same numbers; with fewer than five like it the line says how many.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_new_task_says_its_forecast_first() {
    let mut r = rig(script, |_| {});
    *r.model.hold.lock().unwrap() = Some("CHILD".into());
    r.core.push.ensure(&r.core).await.unwrap();
    r.core.tools.forecasts.loaded(history(30, 0.60, 8.0));
    let parent = parent_session(&r.core);
    let res = turn(&r.core, &parent, ASK).await;
    r.entered.recv().await.unwrap();
    until("the task's view has its forecast", || {
        task_view(&r.core).is_some_and(|v| v.forecast.is_some())
    })
    .await;
    let f = task_view(&r.core).unwrap().forecast.unwrap();
    assert_eq!(f.n, 25, "the 25 nearest of the 30");
    assert!(
        (f.usd_p50 - 0.60).abs() < 1e-9 && (f.usd_p90 - 0.60).abs() < 1e-9,
        "{f:?}"
    );
    assert!((f.min_p50 - 8.0).abs() < 1e-9, "{f:?}");
    assert!(!f.past);
    let text = first_result(&r.core, &res.session_id, "t_task");
    let first = text.lines().next().unwrap();
    assert!(
        first.starts_with("⏳ task ")
            && first.contains("\"CHILD: migrate the fixtures\"")
            && first.ends_with("forecast 8 min (p90 8) · $0.60 (p90 $0.60) · from 25 like it"),
        "{first}"
    );
}

/// The text of the call's result in a session.
fn first_result(core: &crate::Core, session: &str, id: &str) -> String {
    core.store
        .session_nodes(session)
        .unwrap()
        .into_iter()
        .find_map(|(_, n)| match n.body {
            crate::node::Body::ToolResult {
                tool_use_id,
                content,
                late: false,
                ..
            } if tool_use_id == id => Some(content),
            _ => None,
        })
        .unwrap_or_else(|| panic!("no result for {id}"))
}

/// A history of two like it is no forecast, and the line says so.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn fewer_than_five_like_it_says_no_forecast_yet() {
    let mut r = rig(script, |_| {});
    *r.model.hold.lock().unwrap() = Some("CHILD".into());
    r.core.push.ensure(&r.core).await.unwrap();
    r.core.tools.forecasts.loaded(history(2, 0.60, 8.0));
    let parent = parent_session(&r.core);
    let res = turn(&r.core, &parent, ASK).await;
    r.entered.recv().await.unwrap();
    let text = first_result(&r.core, &res.session_id, "t_task");
    assert!(
        text.lines()
            .next()
            .unwrap()
            .ends_with(" · no forecast yet: 2 like it"),
        "{text}"
    );
    assert!(task_view(&r.core).unwrap().forecast.is_none());
}

/// A task past its p90 is told once: not by the loops after it, not after a
/// restart (a board seeded from the store reads it already past), and the
/// notice is an Inform with Keep going and Stop.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_task_past_its_p90_is_told_once() {
    let mut r = rig(script, |_| {});
    *r.model.hold.lock().unwrap() = Some("CHILD".into());
    r.core.push.ensure(&r.core).await.unwrap();
    // Tasks like it took a millionth of a minute at most: any frame after
    // the open is past it.
    r.core.tools.forecasts.loaded(history(30, 5.0, 0.000_001));
    let parent = parent_session(&r.core);
    turn(&r.core, &parent, ASK).await;
    r.entered.recv().await.unwrap();
    until("the notice", || !past_notices(&r.core).is_empty()).await;
    let n = past_notices(&r.core).remove(0);
    assert_eq!(n.urgency, theseus_protocol::notices::Urgency::Inform);
    let ids: Vec<&str> = n.answers.iter().map(|a| a.id.as_str()).collect();
    assert_eq!(ids, ["keep_going", "stop"]);
    assert!(n.line.contains("is past its forecast"), "{}", n.line);
    assert!(task_view(&r.core).unwrap().forecast.unwrap().past);

    // A restart: a board seeded from the store, its forecasts read, then a
    // frame that touches the task.
    let core = r.core.clone();
    let told = tokio::task::spawn_blocking(move || {
        let (board, execs, actions) = seed_inputs(&core, super::super::hot::RECENT).unwrap();
        let mut w = board_work::seed(&core, &board, &execs, &actions);
        w.forecast_open(&core, theseus_protocol::now_unix_ms());
        let exec = w
            .all()
            .into_values()
            .find(|v| v.kind == WorkKind::Task)
            .expect("the task is on the seeded board");
        assert!(exec.forecast.as_ref().is_some_and(|f| f.past), "{exec:?}");
        let mut d = Dirty::default();
        d.execs.insert(exec.id);
        let f = Frame {
            at_ms: theseus_protocol::now_unix_ms(),
            committed: std::time::Instant::now(),
            position: board.position + 1,
            records: Vec::new(),
            nodes: Vec::new(),
        };
        w.settle(&f, d).notices.len()
    })
    .await
    .unwrap();
    assert_eq!(told, 0, "a restart does not tell it again");

    // Let the task run its loops and end: still the one notice.
    r.model.gate.add_permits(10);
    until("the task ends", || {
        task_view(&r.core).is_some_and(|v| v.state.ended())
    })
    .await;
    assert_eq!(past_notices(&r.core).len(), 1, "once per task");
    // And it joined the table of finished tasks, with its loops counted.
    assert_eq!(r.core.tools.forecasts.len(), 31);
}

/// A shared place's watcher sees a forecast's numbers and nothing of the
/// tasks they come from: the view is the same, and no brief of the history
/// is in it.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_forecast_carries_numbers_and_no_brief() {
    let mut r = rig(script, |_| {});
    *r.model.hold.lock().unwrap() = Some("CHILD".into());
    r.core.push.ensure(&r.core).await.unwrap();
    let mut rows = history(30, 0.60, 8.0);
    for row in &mut rows {
        // The history's briefs are the owner's private words.
        row.terms = crate::forecast::terms_of("quarterly payroll reconciliation", "");
    }
    r.core.tools.forecasts.loaded(rows);
    let parent = parent_session(&r.core);
    turn(&r.core, &parent, ASK).await;
    r.entered.recv().await.unwrap();
    until("the forecast", || {
        task_view(&r.core).is_some_and(|v| v.forecast.is_some())
    })
    .await;
    let v = task_view(&r.core).unwrap();
    let wire = serde_json::to_string(&v.forecast).unwrap();
    for word in ["payroll", "reconciliation", "tsk_hist", "quarterly"] {
        assert!(!wire.contains(word), "{wire}");
    }
    // The forecast is five numbers, a flag and the judge's read: nothing else.
    let keys: Vec<String> = serde_json::to_value(v.forecast.clone().unwrap())
        .unwrap()
        .as_object()
        .unwrap()
        .keys()
        .cloned()
        .collect();
    for k in &keys {
        assert!(
            ["usd_p50", "usd_p90", "min_p50", "min_p90", "n", "past", "read", "read_p"]
                .contains(&k.as_str()),
            "{k}"
        );
    }
    // The same forecast in a scope that reads no private text: the view is
    // let through with its numbers (the place rule scrubs words, not numbers).
    let scope = crate::push::WorkScope {
        roots: Some([v.root.clone()].into()),
        guarded: true,
    };
    let shown = scope.show(&v, &r.core.tools.scrubber);
    let (a, b) = (shown.forecast.unwrap(), v.forecast.unwrap());
    assert_eq!(a.n, b.n);
    assert!((a.usd_p90 - b.usd_p90).abs() < 1e-9 && (a.min_p50 - b.min_p50).abs() < 1e-9);
}
