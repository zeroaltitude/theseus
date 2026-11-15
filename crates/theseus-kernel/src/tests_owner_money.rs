//! The owner's two words on money (issue 6; theseus-x3m9, theseus-cv3v): reopening an execution that
//! ended at its dollar limit, and settling or forgiving a call whose spend is held as unknown.

use crate::kernel::*;
use crate::tests::{dispatched, rows, running, world, World};
use crate::types::*;

/// An execution ended at its dollar limit: `spent` against `limit`, as the kernel leaves one.
fn exhausted(w: &World, limit: Micros, spent: Micros) -> Execution {
    let (_, mut e, g) = running(w);
    w.kernel
        .end_turn(
            g,
            TurnEnd::Complete {
                reason: "done".into(),
            },
        )
        .unwrap();
    e = w.kernel.execution(&e.id).unwrap().unwrap();
    e.state = ExecState::BudgetExhausted;
    e.wake = None;
    e.ended_reason = Some("the budget is exhausted".into());
    e.budget.limit_micros = limit;
    e.budget.spent_micros = spent;
    w.kernel
        .store()
        .append(&[exec_record(&e).unwrap()])
        .unwrap();
    w.kernel.execution(&e.id).unwrap().unwrap()
}

/// KEY (x3m9): an execution at or over its limit reopens with a new limit above its spend: it waits on
/// input, its limit is its own (pinned), one row names who and both limits; and the refusals say why.
#[test]
fn an_execution_at_its_limit_reopens_with_a_new_limit() {
    let w = world();
    let e = exhausted(&w, 1_000, 1_000);
    assert_eq!(e.state, ExecState::BudgetExhausted);
    // Not above what it spent: refused, and nothing changed.
    let err = w
        .kernel
        .reopen_exhausted(&e.id, 1_000, "the owner")
        .unwrap_err();
    assert!(err.to_string().contains("must be above what"), "{err:#}");
    assert_eq!(
        w.kernel.execution(&e.id).unwrap().unwrap().state,
        ExecState::BudgetExhausted
    );
    let r = w
        .kernel
        .reopen_exhausted(&e.id, 5_000, "the owner")
        .unwrap();
    assert_eq!(
        (r.state, r.wake.clone()),
        (ExecState::Waiting, Some(Wake::Input))
    );
    assert_eq!(
        (r.budget.limit_micros, r.budget.spent_micros),
        (5_000, 1_000)
    );
    assert!(r.budget.pinned, "the config's limit does not take it back");
    assert!(r.ended_reason.is_none());
    let row = rows(&w, &r.session_id, "budget.reopened");
    assert_eq!(row.len(), 1, "{row:?}");
    assert_eq!(row[0]["by"], "the owner");
    assert_eq!(row[0]["was_limit_usd"], micros_to_usd(1_000));
    assert_eq!(row[0]["limit_usd"], micros_to_usd(5_000));
    // Only an ended one: a second reopen finds it waiting.
    let err = w
        .kernel
        .reopen_exhausted(&e.id, 9_000, "the owner")
        .unwrap_err();
    assert!(
        err.to_string()
            .contains("only an execution that ended at its dollar limit"),
        "{err:#}"
    );
}

/// A call held as unknown spend is settled at its cost: the held amount is released into it, the row says
/// it was the owner's word, the outcome stays unknown, and a second settle finds it counted.
#[test]
fn a_call_held_as_unknown_spend_is_settled_at_its_cost() {
    let w = world();
    let (_, e, g) = running(&w);
    let a = dispatched(&w, &g, "proc.run", 200);
    w.kernel
        .end_turn(
            g,
            TurnEnd::Wait {
                wake: Wake::Actions {
                    correlation_ids: vec![a.correlation_id.clone()],
                },
            },
        )
        .unwrap();
    w.clock.advance(61_000);
    w.kernel.reconcile(&NoEvidence).unwrap();
    let held = w.kernel.execution(&e.id).unwrap().unwrap();
    assert_eq!(
        (held.budget.held_unknown_micros, held.budget.spent_micros),
        (200, 0),
        "held as unknown"
    );
    let (done, was) = w
        .kernel
        .settle_held_spend(&a.correlation_id, 120, "the owner")
        .unwrap();
    assert_eq!(was, 200);
    assert_eq!(
        done.state,
        ActionState::OutcomeUnknown,
        "its outcome is still unknown"
    );
    let after = w.kernel.execution(&e.id).unwrap().unwrap();
    assert_eq!(
        (after.budget.held_unknown_micros, after.budget.spent_micros),
        (0, 120),
        "released into what it cost"
    );
    let row = rows(&w, &e.session_id, "action.resolved");
    assert_eq!(row.len(), 1, "{row:?}");
    assert_eq!(
        (&row[0]["owner"], &row[0]["by"], &row[0]["forgiven"]),
        (&true.into(), &"the owner".into(), &false.into())
    );
    let err = w
        .kernel
        .settle_held_spend(&a.correlation_id, 1, "the owner")
        .unwrap_err();
    assert!(err.to_string().contains("nothing to settle"), "{err:#}");
    // A result that arrives later adds only what it costs above the settled figure.
    w.kernel
        .accept_completion(&crate::tests::completion(
            &a.correlation_id,
            Outcome::Succeeded,
            Some(150),
        ))
        .unwrap();
    let late = w.kernel.execution(&e.id).unwrap().unwrap();
    assert_eq!(late.budget.spent_micros, 150, "{:?}", late.budget);
}

/// Forgiving counts the call as nothing, and says so in its row, so the books are never silent; a call
/// that is not unknown, or whose spend is not held, is refused with words.
#[test]
fn forgiving_counts_it_as_nothing_and_says_so() {
    let w = world();
    let (_, e, g) = running(&w);
    let a = dispatched(&w, &g, "proc.run", 300);
    let err = w
        .kernel
        .settle_held_spend(&a.correlation_id, 0, "the owner")
        .unwrap_err();
    assert!(
        err.to_string().contains("expected outcome_unknown"),
        "{err:#}"
    );
    w.kernel
        .end_turn(
            g,
            TurnEnd::Wait {
                wake: Wake::Actions {
                    correlation_ids: vec![a.correlation_id.clone()],
                },
            },
        )
        .unwrap();
    w.clock.advance(61_000);
    w.kernel.reconcile(&NoEvidence).unwrap();
    w.kernel
        .settle_held_spend(&a.correlation_id, 0, "the owner")
        .unwrap();
    let after = w.kernel.execution(&e.id).unwrap().unwrap();
    assert_eq!(
        (after.budget.held_unknown_micros, after.budget.spent_micros),
        (0, 0)
    );
    let row = rows(&w, &e.session_id, "action.resolved");
    assert_eq!(row[0]["forgiven"], true);
    assert_eq!(row[0]["held_usd"], micros_to_usd(300));
}
