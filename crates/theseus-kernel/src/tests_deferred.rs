//! A deferred task (theseus-rcgph): opened asleep, `Waiting{DueAt}`, its carve
//! held from the open, queued by the due scan when its time comes, kept across
//! a restart, and cancelled like any task before it starts.

use crate::kernel::*;
use crate::tasks::Opening;
use crate::tests::*;
use crate::types::*;

pub(crate) fn exec(w: &World, id: &str) -> Execution {
    w.kernel.execution(id).unwrap().unwrap()
}

/// A parent with a turn, and a task its call opened to start `in_ms` from now.
pub(crate) fn asleep(w: &World, want: Micros, in_ms: u64) -> (Execution, Execution, u64) {
    let (_, parent, g) = running(w);
    let at = w.kernel.now_ms() + in_ms;
    let how = Opening {
        reports_to: Some("discord:dm:1".into()),
        wake_parent: false,
        start_at_ms: Some(at),
        helper: None,
    };
    let t = w
        .kernel
        .open_task_at(&g, &new_id("act"), want, how, |_| Ok(vec![]))
        .unwrap();
    assert!(t.opened);
    w.kernel
        .end_turn(g, TurnEnd::Wait { wake: Wake::Input })
        .unwrap();
    (exec(w, &parent.id), t.task, at)
}

/// The task is written asleep in the open's one frame, asks the driver for
/// nothing, and holds its carve: the parent's remaining shows it at once.
#[test]
fn a_deferred_task_opens_asleep_with_its_carve_held() {
    let w = world();
    let (parent, task, at) = asleep(&w, 30_000, 120_000);
    assert_eq!(task.state, ExecState::Waiting);
    assert_eq!(task.wake, Some(Wake::DueAt { at_ms: at }));
    assert!(!task.resume_pending, "no turn until it is due");
    assert_eq!(task.turns, 0);
    assert_eq!(exec(&w, &task.id), task, "as stored");
    assert_eq!(parent.budget.reserved_micros, 30_000);
    assert_eq!(parent.budget.available(), 70_000);
    let opened = &rows(&w, &task.session_id, "execution.opened")[0];
    assert_eq!(opened["starts_at_ms"], at);
    assert!(rows(&w, &task.session_id, "execution.queued").is_empty());
    assert_eq!(
        rows(&w, &task.session_id, "execution.waiting")[0]["why"],
        "deferred"
    );
}

/// Nothing queues it before its time; at its time the due scan queues it once,
/// and its first turn is the ordinary one.
#[test]
fn a_deferred_task_starts_when_it_is_due_and_not_before() {
    let w = world();
    let (_, task, _) = asleep(&w, 30_000, 120_000);
    w.clock.advance(119_999);
    assert!(w.kernel.reconcile(&NoEvidence).unwrap().woke_due.is_empty());
    assert_eq!(exec(&w, &task.id).state, ExecState::Waiting);
    w.clock.advance(1);
    let rep = w.kernel.reconcile(&NoEvidence).unwrap();
    assert_eq!(rep.woke_due, vec![task.id.clone()]);
    let t = exec(&w, &task.id);
    assert_eq!((t.state, t.resume_pending), (ExecState::Queued, true));
    assert!(w.kernel.reconcile(&NoEvidence).unwrap().woke_due.is_empty());
    let g = w.kernel.admit(&task.id).unwrap();
    assert_eq!(g.turn, 1, "its first turn");
}

/// A cancel before the start ends it, reports it, and releases the carve; no
/// turn ever runs, and the due time passing queues nothing.
#[test]
fn a_cancel_before_the_start_releases_the_carve_and_no_turn_runs() {
    let w = world();
    let (parent, task, _) = asleep(&w, 30_000, 120_000);
    w.kernel
        .cancel_execution_with(&task.id, "discord:owner", |_| Ok(vec![]))
        .unwrap();
    let p = exec(&w, &parent.id);
    assert_eq!(p.budget.reserved_micros, 0);
    assert_eq!(p.budget.available(), 100_000);
    assert_eq!(p.reports, vec![task.id.clone()]);
    w.clock.advance(200_000);
    assert!(w.kernel.reconcile(&NoEvidence).unwrap().woke_due.is_empty());
    let t = exec(&w, &task.id);
    assert_eq!((t.state, t.turns), (ExecState::Cancelled, 0));
}

/// A restart before the due time keeps it asleep with its carve, and it starts
/// on time; a restart that passes the due time starts it once, on the first
/// scan after, and the time it had is still its own.
#[test]
fn a_restart_keeps_a_deferred_task_and_starts_it_once() {
    let w = world();
    let (parent, task, at) = asleep(&w, 30_000, 120_000);
    let (w, _) = crash(w, KernelConfig::default());
    let t = exec(&w, &task.id);
    assert_eq!(t.state, ExecState::Waiting);
    assert_eq!(t.wake, Some(Wake::DueAt { at_ms: at }));
    assert_eq!(
        exec(&w, &parent.id).budget.reserved_micros,
        30_000,
        "the carve survives"
    );
    assert!(w.kernel.reconcile(&NoEvidence).unwrap().woke_due.is_empty());
    // The due time passes while no daemon runs: the next process starts after
    // it, and its first scan queues the task once.
    w.clock.advance(300_000);
    let (w, _) = crash(w, KernelConfig::default());
    assert!(
        w.kernel.process_started_ms() > at,
        "the process began after the due time, so the core says the start was late"
    );
    let rep = w.kernel.reconcile(&NoEvidence).unwrap();
    assert_eq!(rep.woke_due, vec![task.id]);
    assert!(w.kernel.reconcile(&NoEvidence).unwrap().woke_due.is_empty());
}
