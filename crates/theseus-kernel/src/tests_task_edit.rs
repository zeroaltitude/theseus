//! `edit_task` (theseus-jid7t.2): a deferred task's start moves, and its report
//! goes elsewhere, in one frame each, with the carve left where it was.

use crate::kernel::*;
use crate::tests::*;
use crate::tests_deferred::{asleep, exec};
use crate::types::*;

fn move_start(w: &World, id: &str, to: u64) -> Execution {
    w.kernel
        .edit_task(id, |e| {
            e.wake = Some(Wake::DueAt { at_ms: to });
            Ok(((), vec![]))
        })
        .unwrap()
        .0
}

/// The due scan wakes the task at the new time and not at the old one.
#[test]
fn a_moved_start_wakes_at_the_new_time_not_the_old() {
    let w = world();
    let (_, task, at) = asleep(&w, 30_000, 120_000);
    let later = at + 60_000;
    move_start(&w, &task.id, later);
    w.clock.advance(120_000);
    assert!(
        w.kernel.reconcile(&NoEvidence).unwrap().woke_due.is_empty(),
        "the old time passed"
    );
    assert_eq!(exec(&w, &task.id).state, ExecState::Waiting);
    w.clock.advance(60_000);
    let rep = w.kernel.reconcile(&NoEvidence).unwrap();
    assert_eq!(rep.woke_due, vec![task.id.clone()]);
}

/// A moved start can be moved back: the old time is the one that wakes it.
#[test]
fn a_start_moved_back_wakes_at_the_first_time() {
    let w = world();
    let (_, task, at) = asleep(&w, 30_000, 120_000);
    move_start(&w, &task.id, at + 600_000);
    move_start(&w, &task.id, at);
    w.clock.advance(120_000);
    let rep = w.kernel.reconcile(&NoEvidence).unwrap();
    assert_eq!(rep.woke_due, vec![task.id.clone()]);
}

/// The report's target changes and the carve does not; an error from the edit
/// writes nothing.
#[test]
fn a_new_report_target_leaves_the_carve_and_a_refusal_writes_nothing() {
    let w = world();
    let (parent, task, _) = asleep(&w, 30_000, 120_000);
    let (e, ()) = w
        .kernel
        .edit_task(&task.id, |e| {
            e.reports_to = Some("discord:dm:2".into());
            Ok(((), vec![]))
        })
        .unwrap();
    assert_eq!(e.reports_to.as_deref(), Some("discord:dm:2"));
    assert_eq!(exec(&w, &task.id).reports_to, e.reports_to, "as stored");
    assert_eq!(e.parent, task.parent, "its parent never changes");
    assert_eq!(exec(&w, &parent.id).budget.reserved_micros, 30_000);
    let refused = w.kernel.edit_task::<()>(&task.id, |e| {
        e.reports_to = None;
        anyhow::bail!("no")
    });
    assert!(refused.is_err() && refused.unwrap_err().to_string() == "no");
    assert_eq!(exec(&w, &task.id).reports_to, e.reports_to);
}

/// The records the caller adds ride the execution's frame.
#[test]
fn the_callers_records_ride_the_same_frame() {
    let w = world();
    let (_, task, _) = asleep(&w, 30_000, 120_000);
    let before = rows(&w, &task.session_id, "task.rehomed").len();
    let (_, ()) = w
        .kernel
        .edit_task(&task.id, |_| {
            let row = w.kernel.ledger(
                theseus_protocol::LedgerKind::TaskRehomed,
                Some(&task.session_id),
                serde_json::json!({"task": task.id}),
            )?;
            Ok(((), vec![row]))
        })
        .unwrap();
    assert_eq!(rows(&w, &task.session_id, "task.rehomed").len(), before + 1);
}
