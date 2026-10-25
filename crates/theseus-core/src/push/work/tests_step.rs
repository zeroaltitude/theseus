//! A plan step as the board builds it from its record (theseus-tx1vj): the
//! step view reads `paused` from a suspended record, and attention follows.
//! `tests.rs` pins the two pure helpers; this builds the view itself, so the
//! line in `step_view` that joins them is held too.

use std::sync::Arc;

use theseus_kernel::{Authority, Budget, ExecState, Execution, SessionKind, Wake};
use theseus_protocol::work::{attention_of, WorkKind, WorkState};
use theseus_protocol::Level;

use super::*;
use crate::task_graph::{NewTask, TaskOrigin, TaskState};

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

/// A conversation's execution at rest on input: the owner of the steps below.
fn execution() -> Execution {
    Execution {
        id: "exe_0000a1b2c3".into(),
        schema: 2,
        session_id: "ses_0000d4e5f6".into(),
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
        wakes: vec![],
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

/// A plan step the conversation made, in `state`, as the task graph holds it.
fn step(id: &str, session: &str, state: TaskState) -> theseus_protocol::tasks::TaskRecord {
    NewTask {
        id: id.into(),
        title: "a step",
        objective: String::new(),
        acceptance: vec![],
        parent: None,
        deps: vec![],
        session: None,
        origin: TaskOrigin {
            session: session.into(),
            principal: "agent".into(),
            by_model: true,
            ..Default::default()
        },
        state,
    }
    .build(1_000)
}

/// A step built from a suspended record (a cancelled or out-of-budget
/// execution, recoverable) is held: its view is `paused`, waiting, and reads
/// "paused" at rest. Every other record's step is not paused, and a step
/// that is no longer suspended stops being so when its record changes.
#[test]
fn a_step_built_from_a_suspended_record_is_paused_and_reads_paused() {
    let dir = tempfile::tempdir().unwrap();
    let core = core(&dir);
    let e = execution();
    let x = crate::push::view(&e, vec![], None, 1, 1_000);
    let mut w = WorkBoard::default();
    let mut d = Dirty::default();
    w.add_exec(&core, 1, &e, x, None, &mut d);

    let mut at = 10;
    let mut view_of = |w: &mut WorkBoard, state: TaskState| {
        at += 1;
        let t = step("tsk_step01", &e.session_id, state);
        w.take_task(&core, &(at, t), &mut d);
        w.step_view("tsk_step01")
            .unwrap_or_else(|| panic!("no step view for {state:?}"))
    };

    let v = view_of(&mut w, TaskState::Suspended);
    assert_eq!(v.kind, WorkKind::Step);
    assert!(v.paused, "a suspended record's step is held");
    assert_eq!(v.state, WorkState::Waiting);
    let a = attention_of(&v);
    assert_eq!((a.level, a.label.as_str()), (Level::Idle, "paused"));

    for state in [
        TaskState::Proposed,
        TaskState::Accepted,
        TaskState::InProgress,
        TaskState::Blocked,
        TaskState::WaitingHuman,
        TaskState::Done,
        TaskState::Failed,
        TaskState::Abandoned,
    ] {
        let v = view_of(&mut w, state);
        assert!(!v.paused, "{state:?} is not a held step");
        assert_ne!(
            attention_of(&v).label,
            "paused",
            "{state:?} does not read paused"
        );
    }
    // And back: the same record, suspended again, is held again.
    assert!(view_of(&mut w, TaskState::Suspended).paused);
}
