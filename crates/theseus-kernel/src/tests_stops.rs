//! `/stop` halts a conversation's work and keeps the conversation (W1,
//! theseus-lji): the running turn plans nothing more and parks on input, its
//! jobs are told to stop, what waits on the operator is declined, and the
//! same execution takes the next input.

use crate::kernel::*;
use crate::tests::*;
use crate::types::*;

/// The cancel's last step for a job its tree stop verified gone (18a).
fn verified(w: &World, job: &str) {
    let tree = Verdict::verified_as(VerifiedBy::Tree, Some(1));
    w.kernel.cancel_verified(job, Some(&tree)).unwrap();
}

/// The cancel's last step for a job whose wrapper did not answer (18a).
fn uncertain(w: &World, job: &str) {
    let why = Verdict::uncertain(VerifiedBy::Tree, "its wrapper did not answer");
    w.kernel.cancel_uncertain(job, &why).unwrap();
}

fn exec(w: &World, id: &str) -> Execution {
    w.kernel.execution(id).unwrap().unwrap()
}

/// A stop while a turn runs: its job is told to stop and its model call is
/// left to finish, its next plan is refused, and its end parks the same
/// execution on input, which the next input wakes as always.
#[test]
fn a_stop_during_a_turn_refuses_its_next_step_and_parks_it_on_input() {
    let w = world();
    let (sid, e, g) = running(&w);
    let job = dispatched(&w, &g, "proc.run", 0);
    let call = dispatched(&w, &g, "provider.messages", 5_000);
    let s = w
        .kernel
        .stop_execution(&e.id, "discord:eddie")
        .unwrap()
        .expect("it was running");
    assert!(s.turn_running);
    assert_eq!(
        s.to_kill,
        vec![job.correlation_id.clone()],
        "not the model call"
    );
    let x = exec(&w, &e.id);
    assert_eq!(x.state, ExecState::Running, "the turn still holds it");
    assert_eq!(
        x.stopped.as_ref().map(|s| s.by.as_str()),
        Some("discord:eddie")
    );
    assert_eq!(
        w.kernel
            .action(&job.correlation_id)
            .unwrap()
            .unwrap()
            .cancel,
        Some(CancelState::Requested)
    );
    assert_eq!(
        w.kernel
            .action(&call.correlation_id)
            .unwrap()
            .unwrap()
            .cancel,
        None
    );
    // The turn's next step is refused, and so is a wake or a task.
    let err = w
        .kernel
        .plan_action(&g, &proposal("fs.read"), RetryClass::SafeToRepeat, None, 0)
        .unwrap_err();
    assert!(
        matches!(
            err.downcast_ref::<KernelError>(),
            Some(KernelError::Stopped { .. })
        ),
        "{err:#}"
    );
    assert!(
        format!("{err}").contains("stopped by discord:eddie"),
        "{err}"
    );
    assert!(w.kernel.ask_budget(&g, 1).is_err());
    // The model call settles with its real cost; the job's cancel settles it.
    w.kernel
        .accept_completion(&completion(
            &call.correlation_id,
            Outcome::Succeeded,
            Some(2_000),
        ))
        .unwrap();
    verified(&w, &job.correlation_id);
    // The turn's end parks it on input, whatever the turn asked for.
    let ended = w
        .kernel
        .end_turn(
            g,
            TurnEnd::Wait {
                wake: Wake::Actions {
                    correlation_ids: vec![job.correlation_id.clone()],
                },
            },
        )
        .unwrap();
    assert_eq!(
        (ended.state, ended.wake.clone(), ended.resume_pending),
        (ExecState::Waiting, Some(Wake::Input), false)
    );
    assert!(ended.stopped.is_none());
    assert_eq!(
        ended.budget.spent_micros, 2_000,
        "the call's cost is booked"
    );
    assert_eq!(ended.budget.reserved_micros, 0);
    // The stopped job is the next turn's late result, though nothing queued
    // the execution for it.
    assert!(ended.queued_results.contains(&job.correlation_id));
    let row = &rows(&w, &sid, "execution.stopped")[0];
    assert_eq!(row["turn_running"], true);
    assert_eq!(
        rows(&w, &sid, "execution.waiting").last().unwrap()["why"],
        "stopped"
    );
    // The next input continues the same execution.
    let q = w.kernel.wake_input(&e.id).unwrap();
    assert_eq!(q.state, ExecState::Queued);
    let g = w.kernel.admit(&e.id).unwrap();
    let results = w.kernel.take_results(&g).unwrap();
    assert!(results
        .iter()
        .any(|a| a.correlation_id == job.correlation_id && a.state == ActionState::Cancelled));
    w.kernel
        .plan_action(&g, &proposal("fs.read"), RetryClass::SafeToRepeat, None, 0)
        .expect("a new turn plans again");
}

/// A stop with no turn running: the execution waits on a job and an approval.
/// The job is told to stop, the approval and a budget question are declined,
/// and the execution waits on input at once, in one frame.
#[test]
fn a_stop_between_turns_stops_the_job_declines_what_waits_and_waits_on_input() {
    let w = world();
    let (sid, e, g) = running(&w);
    let job = dispatched(&w, &g, "proc.run", 0);
    let ask = w
        .kernel
        .plan_confirm_with(
            &g,
            &proposal("fs.write"),
            RetryClass::NonRepeatable,
            None,
            |_| Ok(vec![]),
        )
        .unwrap();
    assert!(ask.awaits_confirm());
    w.kernel
        .end_turn(
            g,
            TurnEnd::Wait {
                wake: Wake::Confirm {
                    confirm_id: ask.correlation_id.clone(),
                },
            },
        )
        .unwrap();
    let frames = w.kernel.store().stats().unwrap().frames_appended;
    let s = w.kernel.stop_execution(&e.id, "the CLI").unwrap().unwrap();
    assert_eq!(
        w.kernel.store().stats().unwrap().frames_appended,
        frames + 1
    );
    assert!(!s.turn_running);
    assert_eq!(s.to_kill, vec![job.correlation_id.clone()]);
    assert_eq!(s.declined.len(), 1);
    assert_eq!(s.declined[0].correlation_id, ask.correlation_id);
    let a = w.kernel.action(&ask.correlation_id).unwrap().unwrap();
    assert_eq!(a.state, ActionState::Cancelled);
    assert_eq!(a.resolution.as_deref(), Some("stopped by the CLI"));
    assert!(!a.awaits_confirm());
    assert!(w.kernel.pending_confirms().unwrap().is_empty());
    let x = exec(&w, &e.id);
    assert_eq!(
        (x.state, x.wake, x.stopped),
        (ExecState::Waiting, Some(Wake::Input), None)
    );
    assert_eq!(rows(&w, &sid, "action.declined")[0]["reason"], "stopped");
    // A job whose outcome the kill could not verify still reaches the next
    // turn, and nothing queues the execution for it.
    uncertain(&w, &job.correlation_id);
    let x = exec(&w, &e.id);
    assert_eq!(x.state, ExecState::Waiting);
    assert_eq!(x.queued_results, vec![job.correlation_id]);
    assert!(!crate::wakes::due_now(&x, w.kernel.now_ms()));
    // A second stop finds nothing more to stop.
    let again = w.kernel.stop_execution(&e.id, "the CLI").unwrap().unwrap();
    assert!(again.to_kill.is_empty() && again.declined.is_empty());
}

/// A stop keeps the conversation's own pending wakes: `/cancel <id>` is what
/// cancels one. A due wake fires once the stopped turn has ended, as it would
/// for any free execution.
#[test]
fn a_stop_keeps_the_sessions_wakes() {
    let w = world();
    let (_, e, g) = running(&w);
    let now = w.kernel.now_ms();
    w.kernel
        .set_wake(
            &g,
            &new_id("act"),
            now + 60_000,
            "check the build",
            None,
            None,
        )
        .unwrap();
    w.kernel
        .stop_execution(&e.id, "discord:eddie")
        .unwrap()
        .unwrap();
    let err = w
        .kernel
        .set_wake(&g, &new_id("act"), now + 60_000, "another", None, None)
        .unwrap_err();
    assert!(matches!(
        err.downcast_ref::<KernelError>(),
        Some(KernelError::Stopped { .. })
    ));
    let x = w
        .kernel
        .end_turn(g, TurnEnd::Wait { wake: Wake::Input })
        .unwrap();
    assert_eq!(x.wakes.len(), 1, "the wake stays");
    w.clock.advance(61_000);
    let queued = w.kernel.fire_due(&e.id).unwrap().expect("due, and free");
    assert_eq!(queued.state, ExecState::Queued);
}

/// A crash while a stopped turn still held its execution: startup does not
/// resume the turn, and the execution waits on input.
#[test]
fn a_stopped_turn_cut_by_a_crash_is_not_resumed() {
    let w = world();
    let (sid, e, g) = running(&w);
    w.kernel
        .stop_execution(&e.id, "discord:eddie")
        .unwrap()
        .unwrap();
    drop(g);
    let (w, rep) = crash(w, KernelConfig::default());
    assert!(!rep.requeued_interrupted.contains(&e.id));
    let x = exec(&w, &e.id);
    assert_eq!(
        (x.state, x.wake.clone(), x.resume_pending, x.stopped.clone()),
        (ExecState::Waiting, Some(Wake::Input), false, None)
    );
    assert_eq!(x.interrupted, 1);
    assert_eq!(
        rows(&w, &sid, "execution.interrupted")[0]["stopped_by"],
        "discord:eddie"
    );
}

/// An ended execution has nothing to stop, and writes nothing; a cancel
/// still ends a stopped one; a task is stopped by its cancel instead.
#[test]
fn a_stop_writes_nothing_for_an_ended_execution_and_refuses_a_task() {
    let w = world();
    let (_, e, g) = running(&w);
    w.kernel.stop_execution(&e.id, "a").unwrap().unwrap();
    w.kernel.cancel_execution(&e.id, "b").unwrap();
    let x = exec(&w, &e.id);
    assert_eq!(x.state, ExecState::Cancelled);
    assert!(x.stopped.is_none());
    drop(g);
    let before = w.kernel.store().last_position();
    assert!(w.kernel.stop_execution(&e.id, "a").unwrap().is_none());
    assert_eq!(w.kernel.store().last_position(), before);
    // A task.
    let (_, _p, pg) = running(&w);
    let t = w
        .kernel
        .open_task(&pg, &new_id("act"), 10_000, None, false, |_| Ok(vec![]))
        .unwrap()
        .task;
    let err = w.kernel.stop_execution(&t.id, "a").unwrap_err();
    assert!(matches!(
        err.downcast_ref::<KernelError>(),
        Some(KernelError::StopTask { .. })
    ));
    drop(pg);
}
