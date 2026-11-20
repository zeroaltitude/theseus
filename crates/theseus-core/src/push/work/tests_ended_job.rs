//! A failed job asks for the owner unless a turn has its result: the turn
//! that ran it read it as its call's answer, or took it as a late result
//! (`result_read`, `take_reads`). The word `failed` stays on the view; only
//! its level changes, so every count that reads the level agrees.

use std::sync::Arc;

use theseus_kernel::{ActionState, Authority, Budget, ExecState, Execution, SessionKind, Wake};
use theseus_protocol::work::{attention_of, WorkKind, WorkState, WorkView};
use theseus_protocol::Level;

use super::tests::job_action;
use super::*;

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

/// The execution that ran the job, in `state`, with `queued` results waiting.
fn execution(state: ExecState, queued: &[&str]) -> Execution {
    Execution {
        id: "exe_a".into(),
        schema: 2,
        session_id: "ses_a".into(),
        kind: SessionKind::Conversation,
        state,
        authority: Authority::default(),
        budget: Budget::new(2_000_000),
        wake: Some(Wake::Input),
        outstanding: vec![],
        queued_results: queued.iter().map(|c| c.to_string()).collect(),
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

fn frame_of(execs: Vec<(u64, Execution)>) -> Taken {
    Taken {
        execs,
        actions: Vec::new(),
        rows: Vec::new(),
        fired: Vec::new(),
        tasks: Vec::new(),
        sessions: Vec::new(),
        routines: Vec::new(),
    }
}

/// A board that holds the execution, and the job failed in the frame that
/// wrote `e` (the settle's own: action record, then the execution's).
fn failed_job(core: &Core, e: &Execution, corr: &str) -> WorkBoard {
    let mut w = WorkBoard::default();
    let mut d = Dirty::default();
    let x = crate::push::view(&running(e), vec![], None, 1, 1_000);
    w.add_exec(core, 1, &running(e), x, None, &mut d);
    let open = job_action(corr, ActionState::Dispatched, 10_000, None);
    w.take_action(&Board::default(), 10, &open, &mut d);
    settle(&mut w, e, corr, 12);
    w
}

/// What the execution looked like before: not ended, so the board can hold it.
fn running(e: &Execution) -> Execution {
    Execution {
        state: ExecState::Waiting,
        queued_results: vec![],
        ..e.clone()
    }
}

/// The job fails in a frame that carries `e` as the execution's record.
fn settle(w: &mut WorkBoard, e: &Execution, corr: &str, pos: u64) {
    let mut d = Dirty::default();
    let failed = job_action(corr, ActionState::Failed, 10_000, Some(10_019));
    w.take_action(&Board::default(), pos, &failed, &mut d);
    w.take_reads(&frame_of(vec![(pos, e.clone())]), &mut d);
}

fn job_view(w: &WorkBoard, corr: &str) -> WorkView {
    let v = w.act_view(corr).expect("the job has a view");
    assert_eq!((v.kind, v.state), (WorkKind::Job, WorkState::Failed));
    v
}

/// A result the job's own turn settled is not queued: that turn read it, and
/// its failure is that turn's. The view says `failed`, at the level of a thing
/// over, and a count of what needs the owner does not hold it.
#[test]
fn a_failed_job_its_turn_read_does_not_ask_for_the_owner() {
    let dir = tempfile::tempdir().unwrap();
    let core = core(&dir);
    let w = failed_job(&core, &execution(ExecState::Running, &[]), "act_read");
    let mut v = job_view(&w, "act_read");
    assert!(v.read);
    v.attention = attention_of(&v);
    assert_eq!(
        (v.attention.level, v.attention.label.as_str()),
        (Level::Idle, "failed")
    );
}

/// Nothing read it: its result waits in the queue of an execution that took
/// no turn since, or the execution ended. It asks for the owner, as before.
#[test]
fn a_failed_job_nothing_read_still_asks_for_the_owner() {
    let dir = tempfile::tempdir().unwrap();
    let core = core(&dir);
    for (what, e) in [
        (
            "queued for a later turn",
            execution(ExecState::Waiting, &["act_n"]),
        ),
        ("its execution ended", execution(ExecState::Complete, &[])),
    ] {
        let w = failed_job(&core, &e, "act_n");
        let v = job_view(&w, "act_n");
        assert!(!v.read, "{what}");
        let a = attention_of(&v);
        assert_eq!(
            (a.level, a.label.as_str()),
            (Level::NeedsYou, "failed"),
            "{what}"
        );
    }
}

/// A late result a turn takes is read at that moment: the frame that empties
/// the queue turns the job's level, and nothing later (the execution's own end)
/// turns it back.
#[test]
fn a_late_result_taken_by_a_turn_is_read_and_stays_read() {
    let dir = tempfile::tempdir().unwrap();
    let core = core(&dir);
    let mut w = failed_job(
        &core,
        &execution(ExecState::Waiting, &["act_late"]),
        "act_late",
    );
    let level = |w: &WorkBoard| attention_of(&job_view(w, "act_late")).level;
    assert_eq!(level(&w), Level::NeedsYou, "queued: no turn has it");
    let mut d = Dirty::default();
    w.take_reads(
        &frame_of(vec![(20, execution(ExecState::Running, &[]))]),
        &mut d,
    );
    assert!(
        d.acts.contains("act_late"),
        "the view is rebuilt when it turns"
    );
    assert_eq!(level(&w), Level::Idle, "a turn took it");
    w.take_reads(
        &frame_of(vec![(30, execution(ExecState::Complete, &[]))]),
        &mut Dirty::default(),
    );
    assert_eq!(
        level(&w),
        Level::Idle,
        "the execution's end does not undo it"
    );
}

/// The count agrees with the level: the tree's open views hold a failed job
/// nothing read and a failed task, and not a failed job a turn read.
#[test]
fn the_open_views_under_a_task_hold_what_asks_for_the_owner_alone() {
    let dir = tempfile::tempdir().unwrap();
    let core = core(&dir);
    let mut w = failed_job(&core, &execution(ExecState::Running, &[]), "act_read");
    let orphan = job_action("act_orphan", ActionState::Dispatched, 10_000, None);
    let mut d = Dirty::default();
    w.take_action(&Board::default(), 14, &orphan, &mut d);
    settle(
        &mut w,
        &execution(ExecState::Waiting, &["act_orphan"]),
        "act_orphan",
        16,
    );
    for corr in ["act_read", "act_orphan"] {
        let mut v = job_view(&w, corr);
        v.attention = attention_of(&v);
        w.views.insert(corr.to_string(), v);
        w.kids
            .entry("exe_a".into())
            .or_default()
            .insert(corr.to_string());
    }
    let mut open = Vec::new();
    w.open_below("exe_a", &mut open);
    let ids: Vec<&str> = open.iter().map(|v| v.id.as_str()).collect();
    assert_eq!(ids, ["act_orphan"], "the read job is over; the other asks");
}

/// The owner's dismissal of a failed job nothing read: the action carries the
/// kernel's mark (`dismissed_by_owner_at_ms`), and its view is `Idle` with
/// "failed · dismissed", the word kept, and out of every count that reads the
/// level, as a dismissed task's is.
#[test]
fn a_failed_job_the_owner_dismissed_is_idle_and_counted_by_none() {
    let dir = tempfile::tempdir().unwrap();
    let core = core(&dir);
    let corr = "act_dis";
    let mut w = failed_job(&core, &execution(ExecState::Waiting, &[corr]), corr);
    let level = |w: &WorkBoard| {
        let a = attention_of(&job_view(w, corr));
        (a.level, a.label)
    };
    assert_eq!(level(&w), (Level::NeedsYou, "failed".to_string()));
    let mut dismissed = job_action(corr, ActionState::Failed, 10_000, Some(10_019));
    dismissed.detail = Some(serde_json::json!({ theseus_kernel::retry::JOB_DISMISSED: 20_000u64 }));
    let mut d = Dirty::default();
    w.take_action(&Board::default(), 30, &dismissed, &mut d);
    assert!(d.acts.contains(corr), "the view is rebuilt");
    assert_eq!(level(&w), (Level::Idle, "failed · dismissed".to_string()));
    let mut v = job_view(&w, corr);
    v.attention = attention_of(&v);
    w.views.insert(corr.to_string(), v);
    w.kids
        .entry("exe_a".into())
        .or_default()
        .insert(corr.to_string());
    let mut open = Vec::new();
    w.open_below("exe_a", &mut open);
    assert!(open.is_empty(), "no tree counts it");
    let mut r = theseus_protocol::work::Rollup::default();
    super::settle::bump(&mut r, &job_view(&w, corr));
    assert_eq!((r.needs_you, r.failed), (0, 0), "nor the rollup");
}
