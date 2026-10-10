//! A turn held before its admission is written (theseus-2uby): the hold
//! writes nothing and keeps every other turn off the execution; the
//! admission, written later in the caller's frame, is `admit_input`'s; a
//! cancel since the hold refuses it; a stop since the hold marks the turn.

use std::sync::{Arc, Mutex};

use theseus_protocol::LedgerKind;
use theseus_store::kinds;

use crate::kernel::*;
use crate::tests::*;
use crate::types::*;

fn frames(w: &World) -> Arc<Mutex<Vec<Vec<u16>>>> {
    let seen: Arc<Mutex<Vec<Vec<u16>>>> = Arc::default();
    let into = seen.clone();
    assert!(w.kernel.observe(Arc::new(move |c: Committed<'_>| {
        into.lock()
            .unwrap()
            .push(c.records.iter().map(|r| r.kind).collect());
    })));
    seen
}

fn waiting(w: &World) -> Execution {
    w.kernel
        .open_execution(
            &new_id("ses"),
            SessionKind::Conversation,
            auth(),
            Some(100_000),
            None,
        )
        .unwrap()
}

/// The hold writes nothing, and a second turn on the execution is refused
/// as a running one's is; the admission, in a frame with the caller's own
/// record after it, writes what `admit_input` writes, in one frame; a second
/// admission writes nothing.
#[test]
fn a_held_turn_writes_nothing_until_its_admission_rides_the_callers_frame() {
    let w = world();
    let e = waiting(&w);
    let seen = frames(&w);
    let before = w.kernel.store().last_position();
    let g = w.kernel.hold_turn(&e.id).unwrap();
    assert_eq!(g.turn, 1);
    assert_eq!(w.kernel.store().last_position(), before, "the hold wrote");
    assert!(w.kernel.holds_turn(&e.id));
    assert!(!w.kernel.admitted(&g).unwrap());
    assert_eq!(
        w.kernel.execution(&e.id).unwrap().unwrap().state,
        ExecState::Waiting
    );
    let held = w.kernel.hold_turn(&e.id).unwrap_err();
    assert!(
        matches!(
            held.downcast_ref::<KernelError>(),
            Some(KernelError::TurnHeld { .. })
        ),
        "{held:#}"
    );
    let other = w.kernel.admit_input(&e.id).unwrap_err();
    assert!(
        matches!(
            other.downcast_ref::<KernelError>(),
            Some(KernelError::TurnHeld { .. })
        ),
        "{other:#}"
    );
    seen.lock().unwrap().clear();
    let own = w
        .kernel
        .ledger(
            LedgerKind::ExecutionRunning,
            Some(&e.session_id),
            serde_json::json!({}),
        )
        .unwrap();
    let wrote = w
        .kernel
        .frame(&[&e.id], |k| {
            let wrote = k.admit_held(&g)?;
            k.stage(std::slice::from_ref(&own))?;
            Ok(wrote)
        })
        .unwrap();
    assert!(wrote);
    assert_eq!(
        *seen.lock().unwrap(),
        vec![vec![kinds::EXECUTION, kinds::LEDGER, kinds::LEDGER]],
        "the execution once, running's row, then the caller's: the refused \
         `admit_input` above wrote the wake alone, as it does when admission waits"
    );
    let now = w.kernel.execution(&e.id).unwrap().unwrap();
    assert_eq!((now.state, now.turns), (ExecState::Running, 1));
    assert!(w.kernel.admitted(&g).unwrap());
    assert!(!w.kernel.admit_held(&g).unwrap(), "admitted once");
    assert_eq!(seen.lock().unwrap().len(), 1);
    w.kernel
        .end_turn(g, TurnEnd::Wait { wake: Wake::Input })
        .unwrap();
}

/// A cancel that lands between the hold and the admission is read under the
/// admission's lock: refused, nothing written, and the turn's end writes
/// nothing either. The cancel saw the held turn as running (its sweep waits
/// for the turn's end).
#[test]
fn a_cancel_after_the_hold_refuses_the_admission() {
    let w = world();
    let e = waiting(&w);
    let g = w.kernel.hold_turn(&e.id).unwrap();
    w.kernel.cancel_execution(&e.id, "operator").unwrap();
    let before = w.kernel.store().last_position();
    let refused = w.kernel.admit_held(&g).unwrap_err();
    assert!(
        matches!(
            refused.downcast_ref::<KernelError>(),
            Some(KernelError::NotRunnable {
                state: "cancelled",
                ..
            })
        ),
        "{refused:#}"
    );
    assert_eq!(w.kernel.store().last_position(), before, "a refusal wrote");
    let e = w.kernel.execution(&e.id).unwrap().unwrap();
    assert_eq!((e.state, e.turns), (ExecState::Cancelled, 0));
    drop(g);
    assert!(!w.kernel.holds_turn(&e.id));
}

/// A stop that lands between the hold and the admission marks the held turn
/// as it marks a running one, and leaves the execution where it was: the
/// admission keeps the mark, so the turn's first plan is refused. Parking it
/// on input instead would be undone by the admission's wake (the lost update
/// theseus-id9 was filed for, in a new place).
#[test]
fn a_stop_after_the_hold_marks_the_turn_its_admission_keeps() {
    let w = world();
    let e = waiting(&w);
    let g = w.kernel.hold_turn(&e.id).unwrap();
    let stop = w.kernel.stop_execution(&e.id, "operator").unwrap().unwrap();
    assert!(stop.turn_running, "{stop:?}");
    assert_eq!(stop.execution.state, ExecState::Waiting);
    assert!(w.kernel.admit_held(&g).unwrap());
    let now = w.kernel.execution(&e.id).unwrap().unwrap();
    assert_eq!(now.state, ExecState::Running);
    assert_eq!(
        now.stopped.as_ref().map(|s| s.by.as_str()),
        Some("operator")
    );
    let planned = w.kernel.plan_and_dispatch(
        &g,
        &proposal(PROVIDER_TOOL),
        RetryClass::SafeToRepeat,
        None,
        0,
        |_| Ok(vec![]),
    );
    let refused = planned.unwrap_err();
    assert!(
        matches!(
            refused.downcast_ref::<KernelError>(),
            Some(KernelError::Stopped { .. })
        ),
        "{refused:#}"
    );
    let ended = w
        .kernel
        .end_turn(g, TurnEnd::Wait { wake: Wake::Input })
        .unwrap();
    assert_eq!(ended.state, ExecState::Waiting);
}

/// From a waiting execution, the admission writes the input's wake and
/// `running` in one frame, as `admit_input` does: the wake's row, the
/// execution once, running's row.
#[test]
fn a_held_turns_admission_wakes_a_waiting_execution_in_its_frame() {
    let w = world();
    let e = waiting(&w);
    let g = w.kernel.hold_turn(&e.id).unwrap();
    let seen = frames(&w);
    assert!(w.kernel.admit_held(&g).unwrap());
    assert_eq!(
        *seen.lock().unwrap(),
        vec![vec![kinds::LEDGER, kinds::EXECUTION, kinds::LEDGER]]
    );
    w.kernel
        .end_turn(g, TurnEnd::Wait { wake: Wake::Input })
        .unwrap();
}
