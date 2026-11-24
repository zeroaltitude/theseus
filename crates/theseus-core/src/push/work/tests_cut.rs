//! The board's lines for a wake, a wake that ended, a step and a failed
//! step's report, built by the board from their records: each stops before
//! a known value that runs across its cut. All values invented.

use std::sync::Arc;

use serde_json::json;
use theseus_kernel::{Authority, Budget, ExecState, Execution, SessionKind, Wake};
use theseus_protocol::tasks::TaskEvidence;

use super::*;
use crate::task_graph::{NewTask, TaskOrigin};

const V: &str = "kqzv-0417-wjxp-9932-hmfb";

/// No four characters in a row of V outside a whole copy of it.
fn whole_or_absent(what: &str, text: &str) {
    let rest = text.replace(V, "");
    let chars: Vec<char> = V.chars().collect();
    for w in chars.windows(4) {
        let w: String = w.iter().collect();
        assert!(
            !rest.contains(&w),
            "{what} holds {w:?} of the value: {text:?}"
        );
    }
}

/// 110 characters, then V across the line's cut at 119.
fn across() -> String {
    format!("{}{V} and after", "x".repeat(110))
}

fn stopped() -> String {
    format!("{}…", "x".repeat(110))
}

fn core(dir: &tempfile::TempDir) -> Arc<Core> {
    let mut cfg = crate::Config::example();
    cfg.server.state_dir = dir.path().to_string_lossy().into_owned();
    let store = crate::store::Store::open(&dir.path().join("store")).unwrap();
    Core::build(crate::rpc::Parts::for_tests(
        cfg,
        Arc::new(crate::provider::FakeProvider::scripted(vec![])),
        store,
    ))
    .unwrap()
}

/// A board whose scrubber knows V.
fn board() -> WorkBoard {
    WorkBoard {
        scrubber: Arc::new(crate::scrub::Scrubber::with_values(vec![(
            V.into(),
            "cut_test".into(),
        )])),
        ..WorkBoard::default()
    }
}

/// A conversation's execution at rest on input, holding `wakes`.
fn execution(wakes: Vec<PendingWake>) -> Execution {
    Execution {
        id: "exe_0000c0ffee".into(),
        schema: 2,
        session_id: "ses_0000c0ffee".into(),
        kind: SessionKind::Conversation,
        state: ExecState::Waiting,
        authority: Authority::default(),
        budget: Budget::new(2_000_000),
        wake: Some(Wake::Input),
        outstanding: vec![],
        queued_results: vec![],
        parent: None,
        reports_to: None,
        reports: vec![],
        wakes,
        wake_parent: false,
        report_wakes: vec![],
        helper: None,
        stopped: None,
        turns: 1,
        interrupted: 0,
        resume_pending: false,
        cancel: None,
        ended_reason: None,
        dismissed: None,
        retry_of: None,
        created_at_ms: 1,
        updated_at_ms: 2,
    }
}

/// The board with one execution, whose wake's note is `across()`.
fn with_wake(core: &Core) -> (WorkBoard, Dirty, Execution) {
    let e = execution(vec![PendingWake {
        id: "wak_01cut".into(),
        due_at_ms: 5_000,
        note: across(),
        set_at_ms: 1,
        by: "act_x".into(),
        ..Default::default()
    }]);
    let x = crate::push::view(&e, vec![], None, 1, 1_000);
    let (mut w, mut d) = (board(), Dirty::default());
    w.add_exec(core, 1, &e, x, None, &mut d);
    (w, d, e)
}

#[test]
fn a_wakes_board_line_stops_before_a_known_value() {
    let dir = tempfile::tempdir().unwrap();
    let core = core(&dir);
    let (w, _, _) = with_wake(&core);
    let v = w.wake_view("wak_01cut").expect("the wake's view");
    assert_eq!(v.title, stopped());
    whole_or_absent("the wake's line", &v.title);
}

#[test]
fn an_ended_wakes_board_line_stops_before_a_known_value() {
    let dir = tempfile::tempdir().unwrap();
    let core = core(&dir);
    let (mut w, mut d, e) = with_wake(&core);
    let fired = LedgerRow {
        at_unix_ms: 6_000,
        kind: "wake.fired".into(),
        session_id: Some(e.session_id.clone()),
        turn_id: None,
        data: json!({"execution_id": e.id, "wake_id": "wak_01cut"}),
    };
    w.take_fired(&fired, &mut d);
    let v = w.wake_view("wak_01cut").expect("the ended wake's view");
    assert_eq!(v.ended_at_ms, Some(6_000), "the view is the ended one");
    assert_eq!(v.title, stopped());
    whole_or_absent("the ended wake's line", &v.title);
}

/// A plan step of the execution's session, titled `title`, in `state`.
fn step(title: &str, state: TaskState) -> TaskRecord {
    NewTask {
        id: "tsk_cut01".into(),
        title,
        objective: String::new(),
        acceptance: vec![],
        parent: None,
        deps: vec![],
        session: None,
        origin: TaskOrigin {
            session: "ses_0000c0ffee".into(),
            principal: "agent".into(),
            by_model: true,
            ..Default::default()
        },
        state,
    }
    .build(1_000)
}

#[test]
fn a_steps_board_line_stops_before_a_known_value() {
    let dir = tempfile::tempdir().unwrap();
    let core = core(&dir);
    let (mut w, mut d, _) = with_wake(&core);
    w.take_task(&core, &(11, step(&across(), TaskState::InProgress)), &mut d);
    let v = w.step_view("tsk_cut01").expect("the step's view");
    assert_eq!(v.title, stopped());
    whole_or_absent("the step's line", &v.title);
}

#[test]
fn a_failed_steps_report_line_stops_before_a_known_value() {
    let dir = tempfile::tempdir().unwrap();
    let core = core(&dir);
    let (mut w, mut d, _) = with_wake(&core);
    let mut t = step("a step", TaskState::Failed);
    t.evidence.push(TaskEvidence {
        node: None,
        identity: "the step's own words".into(),
        note: Some(across()),
        by: "ses_0000c0ffee".into(),
        at_ms: 1_500,
    });
    w.take_task(&core, &(12, t), &mut d);
    let v = w.step_view("tsk_cut01").expect("the step's view");
    let line = v.report.and_then(|r| r.first_line).expect("a report line");
    assert_eq!(line, stopped());
    whole_or_absent("the failed step's report line", &line);
}
