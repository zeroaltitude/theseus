//! Wakes (DD8, theseus-cff): a conversation's own due times, beside its one
//! `wake`. Each fires once, only at its time and only while its execution is
//! free; input before it leaves it pending; a crash neither loses it nor runs
//! it twice; one due while the daemon was down runs late and says so.

use theseus_store::NewRecord;

use crate::kernel::*;
use crate::tests::*;
use crate::types::*;
use crate::wakes::{wake_id, MAX_PENDING};

fn exec(w: &World, id: &str) -> Execution {
    w.kernel.execution(id).unwrap().unwrap()
}

/// A conversation with a turn that set one wake due `in_ms` from now, and
/// then parked on input: the execution and the wake's id.
fn with_wake(w: &World, in_ms: u64, note: &str) -> (Execution, String) {
    let (_, e, g) = running(w);
    let call = new_id("act");
    let due = w.kernel.now_ms() + in_ms;
    let set = w
        .kernel
        .set_wake(&g, &call, due, note, Some("discord:dm:1".into()), None)
        .unwrap();
    assert_eq!(set.wake.target.as_deref(), Some("discord:dm:1"));
    assert!(set.set);
    assert_eq!(set.wake.id, wake_id(&call));
    w.kernel
        .end_turn(g, TurnEnd::Wait { wake: Wake::Input })
        .unwrap();
    (exec(w, &e.id), set.wake.id)
}

/// A turn on `id`, which must be queued, that takes its due wakes.
fn take(w: &World, id: &str) -> (TurnGuard, Vec<crate::wakes::FiredWake>) {
    let g = w.kernel.admit(id).unwrap();
    let fired = w.kernel.take_wakes(&g, |_| Ok(vec![])).unwrap();
    (g, fired)
}

/// The wake waits beside the input the conversation parked on, fires only
/// once its time has come, queues the execution for the driver, and the next
/// turn takes it in one frame; a second take writes nothing.
#[test]
fn a_wake_fires_only_at_its_time_and_is_taken_once() {
    let w = world();
    let (e, wid) = with_wake(&w, 2_000, "check the build");
    assert_eq!(e.state, ExecState::Waiting);
    assert_eq!(e.wake, Some(Wake::Input), "it still waits on input too");
    assert_eq!(e.wakes.len(), 1);
    assert_eq!(e.wakes[0].note, "check the build");
    let set = &rows(&w, &e.session_id, "wake.set")[0];
    assert_eq!(set["wake_id"], wid);
    assert_eq!(set["in_ms"], 2_000);

    w.clock.advance(1_999);
    let rep = w.kernel.reconcile(&NoEvidence).unwrap();
    assert!(rep.woke_due.is_empty(), "not before its time");
    assert_eq!(exec(&w, &e.id).state, ExecState::Waiting);

    w.clock.advance(1);
    let rep = w.kernel.reconcile(&NoEvidence).unwrap();
    assert_eq!(rep.woke_due, vec![e.id.clone()]);
    let q = exec(&w, &e.id);
    assert_eq!(q.state, ExecState::Queued);
    assert!(q.resume_pending, "the driver takes the turn");
    assert_eq!(q.wakes.len(), 1, "queued, not yet taken");
    let queued = rows(&w, &e.session_id, "execution.queued");
    assert_eq!(queued.last().unwrap()["why"], "wake");
    assert_eq!(queued.last().unwrap()["wakes"][0], wid);

    let (g, fired) = take(&w, &e.id);
    assert_eq!(fired.len(), 1);
    assert_eq!(fired[0].wake.id, wid);
    assert_eq!(fired[0].late_ms, 0);
    assert!(!fired[0].while_down);
    assert!(exec(&w, &e.id).wakes.is_empty());
    let row = &rows(&w, &e.session_id, "wake.fired")[0];
    assert_eq!(
        (row["late_ms"].as_u64(), row["while_down"].as_bool()),
        (Some(0), Some(false))
    );
    let before = w.kernel.store().last_position();
    assert!(w.kernel.take_wakes(&g, |_| Ok(vec![])).unwrap().is_empty());
    assert_eq!(
        w.kernel.store().last_position(),
        before,
        "nothing due, nothing written"
    );
    let e = w
        .kernel
        .end_turn(g, TurnEnd::Wait { wake: Wake::Input })
        .unwrap();
    assert_eq!((e.state, e.wake), (ExecState::Waiting, Some(Wake::Input)));
}

/// The records a take builds ride in its own frame.
#[test]
fn a_take_writes_its_nodes_in_the_frame_that_removes_the_wakes() {
    let w = world();
    let (e, _) = with_wake(&w, 1_000, "n");
    w.clock.advance(1_000);
    w.kernel.fire_due(&e.id).unwrap().unwrap();
    let g = w.kernel.admit(&e.id).unwrap();
    let before = w.kernel.store().last_position();
    let fired = w
        .kernel
        .take_wakes(&g, |f| {
            Ok(vec![NewRecord::json(
                theseus_store::kinds::META,
                Some("probe.wake"),
                &serde_json::json!({"wakes": f.len()}),
            )?])
        })
        .unwrap();
    assert_eq!(fired.len(), 1);
    let frames = w.kernel.store().scan(before + 1, None, 100).unwrap();
    assert!(frames
        .iter()
        .any(|r| r.key.as_deref() == Some("probe.wake")));
    assert!(exec(&w, &e.id).wakes.is_empty());
    drop(g);
}

/// Input before the due time runs a turn that takes nothing, and the wake
/// stays pending and fires later.
#[test]
fn input_before_the_due_time_leaves_the_wake_pending() {
    let w = world();
    let (e, wid) = with_wake(&w, 60_000, "later");
    w.clock.advance(10_000);
    w.kernel.wake_input(&e.id).unwrap();
    let (g, fired) = take(&w, &e.id);
    assert!(fired.is_empty());
    let e = w
        .kernel
        .end_turn(g, TurnEnd::Wait { wake: Wake::Input })
        .unwrap();
    assert_eq!(e.state, ExecState::Waiting);
    assert_eq!(e.wakes[0].id, wid, "still pending");
    w.clock.advance(50_000);
    assert_eq!(
        w.kernel.reconcile(&NoEvidence).unwrap().woke_due,
        vec![e.id.clone()]
    );
    let (_g, fired) = take(&w, &e.id);
    assert_eq!(fired[0].wake.id, wid);
}

/// A wake that falls due while a turn runs waits for it, and the frame that
/// ends the turn queues the execution again at once.
#[test]
fn a_busy_execution_runs_its_wake_when_its_turn_ends() {
    let w = world();
    let (e, _) = with_wake(&w, 1_000, "soon");
    w.kernel.wake_input(&e.id).unwrap();
    let g = w.kernel.admit(&e.id).unwrap();
    w.clock.advance(5_000);
    assert!(
        w.kernel.reconcile(&NoEvidence).unwrap().woke_due.is_empty(),
        "busy"
    );
    assert!(w.kernel.fire_due(&e.id).unwrap().is_none());
    let e = w
        .kernel
        .end_turn(g, TurnEnd::Wait { wake: Wake::Input })
        .unwrap();
    assert_eq!(
        e.state,
        ExecState::Queued,
        "queued in the frame that ended the turn"
    );
    assert!(e.resume_pending);
    assert_eq!(
        rows(&w, &e.session_id, "execution.queued").last().unwrap()["why"],
        "wake"
    );
    let (_g, fired) = take(&w, &e.id);
    assert_eq!(fired[0].late_ms, 4_000);
    assert!(!fired[0].while_down);
}

/// A conversation waiting on its job is free for a wake, as it is for new
/// input ("check the build in ten minutes" while the build runs); one
/// waiting on the operator's approval or budget answer is not.
#[test]
fn a_wake_fires_while_a_job_runs_and_waits_for_an_approval() {
    let w = world();
    let (_, e, g) = running(&w);
    let job = dispatched(&w, &g, "proc.run", 0);
    let due = w.kernel.now_ms() + 1_000;
    w.kernel
        .set_wake(&g, &new_id("act"), due, "check the build", None, None)
        .unwrap();
    w.kernel
        .end_turn(
            g,
            TurnEnd::Wait {
                wake: Wake::Actions {
                    correlation_ids: vec![job.correlation_id.clone()],
                },
            },
        )
        .unwrap();
    w.clock.advance(1_000);
    assert_eq!(
        w.kernel.reconcile(&NoEvidence).unwrap().woke_due,
        vec![e.id.clone()]
    );
    let (g, fired) = take(&w, &e.id);
    assert_eq!(fired.len(), 1);
    assert_eq!(
        exec(&w, &e.id).outstanding,
        vec![job.correlation_id],
        "the job runs on"
    );
    drop(g);

    let mut waiting = exec(&w, &e.id);
    waiting.state = ExecState::Waiting;
    waiting.wake = Some(Wake::Confirm {
        confirm_id: "act_x".into(),
    });
    assert!(!crate::wakes::free(&waiting));
    waiting.wake = Some(Wake::Budget {
        correlation_id: "act_y".into(),
    });
    assert!(!crate::wakes::free(&waiting));
    waiting.wake = Some(Wake::Input);
    assert!(crate::wakes::free(&waiting));
}

/// A restart before the due time keeps the wake, and it fires once; a
/// restart after its turn took it finds nothing to run.
#[test]
fn a_wake_survives_a_restart_and_fires_once() {
    let w = world();
    let (e, wid) = with_wake(&w, 10_000, "after the restart");
    let (w, rep) = crash(w, KernelConfig::default());
    assert!(rep.reconcile.woke_due.is_empty(), "not due yet");
    assert_eq!(exec(&w, &e.id).wakes[0].id, wid);
    w.clock.advance(10_000);
    assert_eq!(
        w.kernel.reconcile(&NoEvidence).unwrap().woke_due,
        vec![e.id.clone()]
    );
    let (g, fired) = take(&w, &e.id);
    assert_eq!(fired.len(), 1);
    assert!(!fired[0].while_down, "it fell due while this process ran");
    // The daemon dies before the turn ends: the turn is requeued, and the
    // wake it took is not taken again.
    std::mem::forget(g);
    let (w, rep) = crash(w, KernelConfig::default());
    assert_eq!(rep.requeued_interrupted, vec![e.id.clone()]);
    let (_g, fired) = take(&w, &e.id);
    assert!(fired.is_empty(), "never twice");
    assert_eq!(rows(&w, &e.session_id, "wake.fired").len(), 1);
}

/// A wake that fell due while the daemon was down runs after startup: not in
/// startup's own pass, which writes nothing for it before the socket serves,
/// but in the first due scan after it. It runs late, by how much, and says
/// the daemon was not running.
#[test]
fn a_wake_due_while_the_daemon_was_down_runs_after_startup_marked_late() {
    let w = world();
    let (e, wid) = with_wake(&w, 30_000, "while down");
    // The daemon is down for a minute and a half, across the due time: the
    // old kernel does nothing while the clock moves, and the next one starts
    // after it.
    w.clock.advance(90_000);
    let before = w.kernel.store().last_position();
    let (w, rep) = crash(w, KernelConfig::default());
    assert!(rep.reconcile.woke_due.is_empty(), "startup leaves it");
    assert_eq!(exec(&w, &e.id).state, ExecState::Waiting);
    let startup_records = w.kernel.store().last_position() - before;
    assert_eq!(
        w.kernel.reconcile(&NoEvidence).unwrap().woke_due,
        vec![e.id.clone()],
        "the first due scan queues it"
    );
    assert!(
        startup_records <= 5,
        "a clean start's records only: {startup_records}"
    );
    let (_g, fired) = take(&w, &e.id);
    assert_eq!(fired[0].wake.id, wid);
    assert_eq!(fired[0].late_ms, 60_000);
    assert!(fired[0].while_down);
    let row = &rows(&w, &e.session_id, "wake.fired")[0];
    assert_eq!(
        (row["late_ms"].as_u64(), row["while_down"].as_bool()),
        (Some(60_000), Some(true))
    );
}

/// At most `MAX_PENDING` wakes: the next is refused and writes nothing. The
/// same call sets its wake once.
#[test]
fn the_cap_is_enforced_and_a_call_sets_its_wake_once() {
    let w = world();
    let (_, e, g) = running(&w);
    let now = w.kernel.now_ms();
    let first = new_id("act");
    for i in 0..MAX_PENDING as u64 {
        let call = if i == 0 { first.clone() } else { new_id("act") };
        w.kernel
            .set_wake(&g, &call, now + 60_000 - i * 1_000, "n", None, None)
            .unwrap();
    }
    let before = w.kernel.store().last_position();
    let err = w
        .kernel
        .set_wake(&g, &new_id("act"), now + 1_000, "one too many", None, None)
        .unwrap_err();
    assert!(matches!(
        err.downcast_ref::<KernelError>(),
        Some(KernelError::TooManyWakes { max }) if *max == MAX_PENDING
    ));
    assert!(err.to_string().contains("5 pending wakes"), "{err}");
    assert_eq!(w.kernel.store().last_position(), before);
    let again = w
        .kernel
        .set_wake(&g, &first, now + 1, "again", None, None)
        .unwrap();
    assert!(!again.set, "the same call's wake");
    assert_eq!(again.wake.due_at_ms, now + 60_000);
    assert_eq!(w.kernel.store().last_position(), before);
    let e = exec(&w, &e.id);
    let dues: Vec<u64> = e.wakes.iter().map(|x| x.due_at_ms).collect();
    let mut sorted = dues.clone();
    sorted.sort();
    assert_eq!(dues, sorted, "soonest first");
    drop(g);
}

/// A cancel removes the wake and nothing fires; a second cancel writes
/// nothing. Cancelling the execution drops its wakes with a row each.
#[test]
fn a_cancel_clears_the_wake_and_nothing_fires() {
    let w = world();
    let (e, wid) = with_wake(&w, 5_000, "never");
    let (after, gone) = w
        .kernel
        .cancel_wake(&e.id, &wid, "the CLI")
        .unwrap()
        .unwrap();
    assert_eq!(gone.id, wid);
    assert!(after.wakes.is_empty());
    assert_eq!(
        rows(&w, &e.session_id, "wake.cancelled")[0]["by"],
        "the CLI"
    );
    let before = w.kernel.store().last_position();
    assert!(w
        .kernel
        .cancel_wake(&e.id, &wid, "the CLI")
        .unwrap()
        .is_none());
    assert_eq!(w.kernel.store().last_position(), before);
    w.clock.advance(10_000);
    assert!(w.kernel.reconcile(&NoEvidence).unwrap().woke_due.is_empty());
    assert_eq!(exec(&w, &e.id).state, ExecState::Waiting);

    let (e2, wid2) = with_wake(&w, 5_000, "stopped");
    w.kernel.cancel_execution(&e2.id, "eddie").unwrap();
    let e2 = exec(&w, &e2.id);
    assert!(e2.wakes.is_empty());
    let row = &rows(&w, &e2.session_id, "wake.cancelled")[0];
    assert_eq!(
        (row["wake_id"].as_str(), row["by"].as_str()),
        (Some(wid2.as_str()), Some("eddie"))
    );
    w.clock.advance(10_000);
    assert!(w.kernel.reconcile(&NoEvidence).unwrap().woke_due.is_empty());
    assert!(w.kernel.pending_wakes().unwrap().is_empty());
}
