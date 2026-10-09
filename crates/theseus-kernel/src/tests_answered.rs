//! A question answered before the turn that asked it ends (theseus-klo2): the
//! answer's wake finds the execution running and does nothing, so the turn's
//! end must not park it on a question that has its answer.

use crate::kernel::*;
use crate::tests::*;
use crate::types::*;

/// A turn asks, the answer lands while it still runs, and its end parks on
/// the question: the execution is queued for the answer's turn instead.
#[test]
fn an_answer_that_lands_before_its_turn_ends_wakes_the_execution() {
    for approve in [true, false] {
        let w = world();
        let (_, e, g) = running(&w);
        let ask = proposal("fs.write");
        let a = w
            .kernel
            .plan_confirm_with(&g, &ask, RetryClass::NonRepeatable, None, |_| Ok(vec![]))
            .unwrap();
        if approve {
            w.kernel
                .bind_confirm(&a.correlation_id, "zeroaltitude", &ask)
                .unwrap();
        } else {
            w.kernel
                .decline_action(&a.correlation_id, "zeroaltitude", "not now")
                .unwrap();
        }
        // The answer's wake, as the core sends it: the turn holds the
        // execution, so it changes nothing.
        let woke = w.kernel.wake(&e.id, "confirmed").unwrap();
        assert_eq!(woke.state, ExecState::Running);
        let ended = w
            .kernel
            .end_turn(
                g,
                TurnEnd::Wait {
                    wake: Wake::Confirm {
                        confirm_id: a.correlation_id.clone(),
                    },
                },
            )
            .unwrap();
        assert_eq!(ended.state, ExecState::Queued, "approve {approve}");
        assert!(ended.resume_pending && ended.wake.is_none());
        let queued = rows(&w, &e.session_id, "execution.queued");
        assert_eq!(queued.last().unwrap()["why"], "answered");
    }
}

/// A question still unanswered at the turn's end parks it, as before.
#[test]
fn an_unanswered_question_still_parks_its_execution() {
    let w = world();
    let (_, e, g) = running(&w);
    let a = w
        .kernel
        .plan_confirm_with(
            &g,
            &proposal("fs.write"),
            RetryClass::NonRepeatable,
            None,
            |_| Ok(vec![]),
        )
        .unwrap();
    let wake = Wake::Confirm {
        confirm_id: a.correlation_id,
    };
    let ended = w
        .kernel
        .end_turn(g, TurnEnd::Wait { wake: wake.clone() })
        .unwrap();
    assert_eq!((ended.state, ended.wake), (ExecState::Waiting, Some(wake)));
    assert!(!ended.resume_pending);
    drop(e);
}
