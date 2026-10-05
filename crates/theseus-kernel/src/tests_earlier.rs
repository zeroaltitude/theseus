//! An earlier process's in-process calls (theseus-m9iy): a provider call in
//! flight when the process died is unknown at the first tick after the
//! restart, not at its deadline, and a job is left to its evidence. Its
//! reservation is booked as spent, which a reset clears, where every other
//! unknown mark holds it (theseus-f3wr).

use serde_json::json;

use crate::earlier::EARLIER_PROCESS;
use crate::kernel::*;
use crate::tests::{crash, dispatched, rows, running, world, World};
use crate::types::*;

fn frames(w: &World) -> u64 {
    w.kernel.store().stats().unwrap().frames_appended
}

/// A budget's spent, reserved, and held.
fn money(b: &Budget) -> (Micros, Micros, Micros) {
    (b.spent_micros, b.reserved_micros, b.held_unknown_micros)
}

/// The call's one `action.outcome_unknown` row, from the earlier process's
/// mark, says its cost is its reservation (100), as an estimate, and so does
/// the action's `detail`.
fn booked_at_its_reservation(w: &World, session: &str, call: &Action) {
    let unknown = rows(w, session, "action.outcome_unknown");
    assert_eq!(unknown.len(), 1);
    assert_eq!(
        unknown[0]["producer"],
        format!("reconciler:{EARLIER_PROCESS}")
    );
    assert_eq!(unknown[0]["outcome"], "unknown");
    assert_eq!(
        (&unknown[0]["cost_basis"], &unknown[0]["cost_usd"]),
        (&json!("reservation"), &json!(0.0001)),
        "the row says its cost is the reservation, as an estimate"
    );
    let marked = w.kernel.action(&call.correlation_id).unwrap().unwrap();
    assert_eq!(marked.state, ActionState::OutcomeUnknown);
    assert_eq!(marked.detail, Some(json!({"cost_basis": "reservation"})));
}

/// A provider call and a job, both in flight when the process dies, and a
/// third session parked on a provider call of its own. The start writes what
/// it wrote before (the interrupted turn, then its step rows) and marks
/// nothing; the driver's first tick marks both calls unknown in one frame,
/// with their own reason, each reservation booked as spent; the job stays
/// dispatched; the parked session wakes; and a second tick writes nothing,
/// and books nothing more.
#[test]
fn an_earlier_processs_provider_call_is_unknown_at_the_first_tick_and_a_job_is_not() {
    let w = world();
    let (s1, e1, g1) = running(&w);
    let call = dispatched(&w, &g1, PROVIDER_TOOL, 100);
    let (_, e2, g2) = running(&w);
    let job = dispatched(&w, &g2, "proc.run", 0);
    let parked = Wake::Actions {
        correlation_ids: vec![job.correlation_id.clone()],
    };
    w.kernel
        .end_turn(g2, TurnEnd::Wait { wake: parked })
        .unwrap();
    let (_, e3, g3) = running(&w);
    let waited = dispatched(&w, &g3, PROVIDER_TOOL, 0);
    let on_call = Wake::Actions {
        correlation_ids: vec![waited.correlation_id.clone()],
    };
    w.kernel
        .end_turn(g3, TurnEnd::Wait { wake: on_call })
        .unwrap();
    std::mem::forget(g1); // the process dies holding the first turn

    let (w, rep) = crash(w, KernelConfig::default());
    assert_eq!(rep.requeued_interrupted, vec![e1.id.clone()]);
    assert!(rep.reconcile.marked_unknown.is_empty());
    assert_eq!(
        frames(&w),
        2,
        "the interrupted turn and the step rows, as before: nothing before serving"
    );
    let state = |c: &str| w.kernel.action(c).unwrap().unwrap().state;
    assert_eq!(state(&call.correlation_id), ActionState::Dispatched);

    // The driver's first tick, once the socket serves.
    let f0 = frames(&w);
    let mut marked = w.kernel.mark_earlier_calls_unknown().unwrap();
    marked.sort();
    let mut want = vec![call.correlation_id.clone(), waited.correlation_id.clone()];
    want.sort();
    assert_eq!(marked, want);
    assert_eq!(frames(&w), f0 + 1, "one frame for all");
    assert_eq!(state(&call.correlation_id), ActionState::OutcomeUnknown);
    assert_eq!(state(&waited.correlation_id), ActionState::OutcomeUnknown);
    assert_eq!(state(&job.correlation_id), ActionState::Dispatched);
    booked_at_its_reservation(&w, &s1, &call);
    let e1 = w.kernel.execution(&e1.id).unwrap().unwrap();
    assert_eq!(e1.state, ExecState::Queued);
    assert!(e1.outstanding.is_empty());
    assert_eq!(e1.queued_results, vec![call.correlation_id]);
    assert_eq!(
        money(&e1.budget),
        (100, 0, 0),
        "its reservation booked as spent, nothing held"
    );
    assert!(e1.budget.reservations.is_empty());
    let e3 = w.kernel.execution(&e3.id).unwrap().unwrap();
    assert_eq!(e3.state, ExecState::Queued, "what waited on the call wakes");
    let e2 = w.kernel.execution(&e2.id).unwrap().unwrap();
    assert_eq!(e2.state, ExecState::Waiting, "the job's session waits on");

    assert!(w.kernel.mark_earlier_calls_unknown().unwrap().is_empty());
    assert_eq!(frames(&w), f0 + 1, "a second tick writes nothing");
    assert!(w
        .kernel
        .reconcile(&NoEvidence)
        .unwrap()
        .marked_unknown
        .is_empty());
    let e1 = w.kernel.execution(&e1.id).unwrap().unwrap();
    assert_eq!(money(&e1.budget), (100, 0, 0), "and books nothing more");
}

/// After the mark, a reset frees what the call cost: the spend is back to $0
/// and the whole limit is room again. Held as unknown, a reset could not free
/// it (`a_reset_that_cannot_free_what_is_held_unknown_asks_once_and_does_not_loop`).
#[test]
fn a_reset_frees_what_an_earlier_processs_call_was_booked() {
    let w = world();
    let (_, e, g) = running(&w);
    let call = dispatched(&w, &g, PROVIDER_TOOL, 40_000);
    std::mem::forget(g);
    let (w, _) = crash(w, KernelConfig::default());
    assert_eq!(
        w.kernel.mark_earlier_calls_unknown().unwrap(),
        vec![call.correlation_id]
    );
    let booked = w.kernel.execution(&e.id).unwrap().unwrap();
    assert_eq!(
        (
            booked.budget.spent_micros,
            booked.budget.held_unknown_micros
        ),
        (40_000, 0)
    );
    assert_eq!(booked.budget.available(), 60_000);
    assert_eq!(
        booked.budget.available_after_reset(),
        100_000,
        "a reset can free all of it"
    );

    let g = w.kernel.admit(&e.id).unwrap();
    assert_eq!(w.kernel.take_results(&g).unwrap().len(), 1);
    let q = w.kernel.ask_budget(&g, 90_000).unwrap();
    let (after, before) = w.kernel.reset_budget(&q.correlation_id, "op").unwrap();
    assert_eq!(before, 40_000, "the booked call was the spend");
    assert_eq!(
        (
            after.budget.spent_micros,
            after.budget.reserved_micros,
            after.budget.held_unknown_micros
        ),
        (0, 0, 0)
    );
    assert_eq!(after.budget.available(), 100_000, "its room is whole");
    drop(g);
}

/// Every other unknown mark holds its reservation as before: a provider call
/// of this process, overdue with no evidence, may still run, or its cost may
/// still arrive.
#[test]
fn an_overdue_mark_still_holds_its_reservation_as_unknown() {
    let w = world();
    let (s, e, g) = running(&w);
    let call = dispatched(&w, &g, PROVIDER_TOOL, 300);
    w.clock.advance(61_000);
    let rep = w.kernel.reconcile(&NoEvidence).unwrap();
    assert_eq!(rep.marked_unknown, vec![call.correlation_id.clone()]);
    let e = w.kernel.execution(&e.id).unwrap().unwrap();
    assert_eq!(
        (
            e.budget.spent_micros,
            e.budget.reserved_micros,
            e.budget.held_unknown_micros
        ),
        (0, 0, 300),
        "held as unknown, not booked"
    );
    let a = w.kernel.action(&call.correlation_id).unwrap().unwrap();
    assert_eq!((a.state, a.detail), (ActionState::OutcomeUnknown, None));
    let row = &rows(&w, &s, "action.outcome_unknown")[0];
    assert_eq!(row["producer"], "reconciler:overdue_no_evidence");
    assert!(row.get("cost_basis").is_none(), "{row}");
    assert!(row["cost_usd"].is_null());
    drop(g);
}

/// A booked call's outcome learned later (no producer can bring one for a
/// process that is gone, but a resolution must not count it twice): the
/// booked estimate stands, nothing held for another call is taken, and only
/// a cost above the reservation is booked more.
#[test]
fn a_booked_calls_late_resolution_counts_it_once() {
    let w = world();
    let (_, e, g) = running(&w);
    let under = dispatched(&w, &g, PROVIDER_TOOL, 500);
    let over = dispatched(&w, &g, PROVIDER_TOOL, 300);
    std::mem::forget(g);
    let (w, _) = crash(w, KernelConfig::default());
    assert_eq!(w.kernel.mark_earlier_calls_unknown().unwrap().len(), 2);
    // A call of this process, overdue: held.
    let g = w.kernel.admit(&e.id).unwrap();
    let held = dispatched(&w, &g, PROVIDER_TOOL, 700);
    w.clock.advance(61_000);
    let rep = w.kernel.reconcile(&NoEvidence).unwrap();
    assert_eq!(rep.marked_unknown, vec![held.correlation_id.clone()]);
    let now = |w: &World| money(&w.kernel.execution(&e.id).unwrap().unwrap().budget);
    assert_eq!(now(&w), (800, 0, 700));
    let resolve = |c: &Action, cost| {
        w.kernel
            .accept_completion(&crate::tests::completion(
                &c.correlation_id,
                Outcome::Succeeded,
                cost,
            ))
            .unwrap()
    };
    assert!(matches!(
        resolve(&under, Some(200)),
        Accepted::ResolvedUnknown { .. }
    ));
    assert_eq!(now(&w), (800, 0, 700), "the estimate stands");
    resolve(&over, Some(450));
    assert_eq!(now(&w), (950, 0, 700), "the cost above it is booked");
    resolve(&held, Some(100));
    assert_eq!(now(&w), (1_050, 0, 0), "a held one resolves as before");
    drop(g);
}

/// The heartbeat's reconcile marks them too, when the driver has not.
#[test]
fn the_heartbeat_marks_an_earlier_processs_calls_when_the_driver_has_not() {
    let w = world();
    let (_, _, g) = running(&w);
    let call = dispatched(&w, &g, PROVIDER_TOOL, 0);
    std::mem::forget(g);
    let (w, _) = crash(w, KernelConfig::default());
    let rep = w.kernel.reconcile(&NoEvidence).unwrap();
    assert_eq!(rep.marked_unknown, vec![call.correlation_id]);
    assert!(w.kernel.mark_earlier_calls_unknown().unwrap().is_empty());
}

/// A provider call this process dispatched is not an earlier one's: only
/// startup's scan notes calls.
#[test]
fn a_call_this_process_dispatched_is_left_as_it_is() {
    let w = world();
    let (_, _, g) = running(&w);
    let call = dispatched(&w, &g, PROVIDER_TOOL, 0);
    assert!(w.kernel.mark_earlier_calls_unknown().unwrap().is_empty());
    assert!(w
        .kernel
        .reconcile(&NoEvidence)
        .unwrap()
        .marked_unknown
        .is_empty());
    let a = w.kernel.action(&call.correlation_id).unwrap().unwrap();
    assert_eq!(a.state, ActionState::Dispatched);
    drop(g);
}
