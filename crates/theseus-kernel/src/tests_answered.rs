//! A question answered while the turn that asked it still runs
//! (theseus-q5af's review): every way that turn can end, and a crash between
//! the answer's frame and the end's. No answer is lost, and the execution is
//! queued once: one continuation, never two.

use crate::gate::Proposal;
use crate::kernel::*;
use crate::tests::*;
use crate::types::*;

fn exec(w: &World, id: &str) -> Execution {
    w.kernel.execution(id).unwrap().unwrap()
}

/// A running turn that asked about `fs.write`: the execution, its guard, the
/// question, and the proposal it was asked about.
fn asked(w: &World) -> (Execution, TurnGuard, Action, Proposal) {
    let (_, e, g) = running(w);
    let ask = proposal("fs.write");
    let q = w
        .kernel
        .plan_confirm_with(&g, &ask, RetryClass::NonRepeatable, None, |_| Ok(vec![]))
        .unwrap();
    (e, g, q, ask)
}

/// The answer as the core writes it: the bind and the wake in one frame.
fn approve(w: &World, e: &Execution, q: &Action, ask: &Proposal) {
    w.kernel
        .frame(&[&e.id], |k| {
            k.bind_confirm(&q.correlation_id, "zeroaltitude", ask)?;
            k.wake(&e.id, "confirmed").map(|_| ())
        })
        .unwrap();
}

fn parks_on(q: &Action) -> TurnEnd {
    TurnEnd::Wait {
        wake: Wake::Confirm {
            confirm_id: q.correlation_id.clone(),
        },
    }
}

/// The `execution.running` rows' `resumed`: one per turn begun.
fn resumed(w: &World, e: &Execution) -> Vec<bool> {
    rows(w, &e.session_id, "execution.running")
        .iter()
        .map(|r| r["resumed"].as_bool().unwrap_or(false))
        .collect()
}

/// Approved while the turn runs, then the turn ends parked on it: queued
/// once, why `answered`; its continuation is the one turn that follows, and
/// after it nothing queues the execution again.
#[test]
fn an_answer_while_the_turn_runs_queues_it_once_and_one_continuation_follows() {
    let w = world();
    let (e, g, q, ask) = asked(&w);
    approve(&w, &e, &q, &ask);
    let queued_before = rows(&w, &e.session_id, "execution.queued").len();
    let end = w.kernel.end_turn(g, parks_on(&q)).unwrap();
    assert_eq!((end.state, end.resume_pending), (ExecState::Queued, true));
    let queued = rows(&w, &e.session_id, "execution.queued");
    assert_eq!(queued.len(), queued_before + 1, "{queued:?}");
    assert_eq!(queued.last().unwrap()["why"], "answered");
    // The continuation: it reads the bound question and dispatches it.
    let g = w.kernel.admit(&e.id).unwrap();
    assert_eq!(resumed(&w, &e), vec![false, true]);
    let a = w.kernel.action(&q.correlation_id).unwrap().unwrap();
    assert!(a.confirm.is_some() && !a.awaits_confirm(), "still bound");
    w.kernel
        .end_turn(g, TurnEnd::Wait { wake: Wake::Input })
        .unwrap();
    let after = exec(&w, &e.id);
    assert_eq!(
        (after.state, after.resume_pending),
        (ExecState::Waiting, false)
    );
    assert_eq!(
        rows(&w, &e.session_id, "execution.queued").len(),
        queued_before + 1,
        "nothing queues it a second time"
    );
    assert!(w.kernel.admit(&e.id).is_err(), "no second continuation");
}

/// The daemon dies after the answer's frame and before the turn's end: the
/// start requeues the interrupted turn, resumed, once; the answer is still
/// bound for the continuation to run.
#[test]
fn a_crash_between_the_answer_and_the_end_resumes_once_with_the_answer() {
    let w = world();
    let (e, g, q, ask) = asked(&w);
    approve(&w, &e, &q, &ask);
    std::mem::forget(g); // the process dies holding the turn
    let (w, rep) = crash(w, KernelConfig::default());
    assert_eq!(rep.requeued_interrupted, vec![e.id.clone()]);
    let now = exec(&w, &e.id);
    assert_eq!((now.state, now.resume_pending), (ExecState::Queued, true));
    let a = w.kernel.action(&q.correlation_id).unwrap().unwrap();
    assert!(a.confirm.is_some() && a.state == ActionState::Planned);
    let g = w.kernel.admit(&e.id).unwrap();
    assert_eq!(resumed(&w, &e), vec![false, true]);
    w.kernel
        .end_turn(g, TurnEnd::Wait { wake: Wake::Input })
        .unwrap();
    assert!(w.kernel.admit(&e.id).is_err(), "no second continuation");
}

/// A `/stop` while the turn runs, after the answer: the stop wins, as it does
/// over a parked question. It declines the bound question with the rest, the
/// turn's end parks on input, and nothing is queued.
#[test]
fn a_stop_after_the_answer_wins_and_nothing_is_queued() {
    let w = world();
    let (e, g, q, ask) = asked(&w);
    approve(&w, &e, &q, &ask);
    w.kernel.stop_execution(&e.id, "zeroaltitude").unwrap();
    let end = w.kernel.end_turn(g, parks_on(&q)).unwrap();
    assert_eq!(
        (end.state, end.wake.clone(), end.resume_pending),
        (ExecState::Waiting, Some(Wake::Input), false)
    );
    let a = w.kernel.action(&q.correlation_id).unwrap().unwrap();
    assert_eq!(a.state, ActionState::Cancelled, "{a:?}");
    assert!(w.kernel.admit(&e.id).is_err());
}

/// A cancel while the turn runs, after the answer: the execution is ended,
/// and the turn's end writes nothing over it.
#[test]
fn a_cancel_after_the_answer_ends_it_and_the_end_changes_nothing() {
    let w = world();
    let (e, g, q, ask) = asked(&w);
    approve(&w, &e, &q, &ask);
    w.kernel
        .cancel_execution_with(&e.id, "zeroaltitude", |_| Ok(vec![]))
        .unwrap();
    let end = w.kernel.end_turn(g, parks_on(&q)).unwrap();
    assert_eq!(end.state, ExecState::Cancelled);
    assert!(rows(&w, &e.session_id, "execution.queued")
        .iter()
        .all(|r| r["why"] != "answered"));
}

/// The budget question has no such window to lose: an approval while the
/// turn runs puts the question in `queued_results`, so the end queues the
/// execution (`result`) and the driver runs it; a decline leaves it waiting
/// on its budget, as a decline after the park does.
#[test]
fn a_budget_question_answered_while_its_turn_runs() {
    for approve in [true, false] {
        let w = world();
        let (_, e, g) = running(&w);
        let q = w.kernel.ask_budget(&g, 1_000).unwrap();
        match approve {
            true => {
                w.kernel
                    .reset_budget(&q.correlation_id, "zeroaltitude")
                    .unwrap();
            }
            false => {
                w.kernel
                    .decline_action(&q.correlation_id, "zeroaltitude", "not now")
                    .unwrap();
            }
        }
        assert_eq!(exec(&w, &e.id).state, ExecState::Running);
        let wake = Wake::Budget {
            correlation_id: q.correlation_id.clone(),
        };
        let end = w
            .kernel
            .end_turn(g, TurnEnd::Wait { wake: wake.clone() })
            .unwrap();
        if approve {
            assert_eq!((end.state, end.wake.clone()), (ExecState::Queued, None));
            assert!(end.queued_results.contains(&q.correlation_id));
            assert_eq!(
                rows(&w, &e.session_id, "execution.queued").last().unwrap()["why"],
                "result"
            );
            assert!(w.kernel.admit(&e.id).is_ok(), "the driver runs it");
        } else {
            assert_eq!((end.state, end.wake), (ExecState::Waiting, Some(wake)));
        }
    }
}
