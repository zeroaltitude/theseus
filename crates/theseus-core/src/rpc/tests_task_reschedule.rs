//! `task.reschedule` (theseus-jid7t.2): a deferred task's start moves, on a
//! clock the test drives. The due scan wakes it at the new time and not the
//! old, `previous` is the undo, each of the three time forms works, and a
//! task that is not waiting to start, or a time that is not later, is refused.

use std::sync::Arc;

use theseus_kernel::{ExecState, NoEvidence, VirtualClock};
use theseus_protocol::ops::{TaskRescheduleParams, TaskRescheduleResult};
use theseus_protocol::task_start::TaskStartParams;

use crate::task_start::Deferred;
use crate::test_support::{owner_cli as cli, stranger};
use crate::tests_judge::kinds;
use crate::tests_route::{rig_on_clock, Rig};

const T0: u64 = 1_790_000_000_000;
const MIN: u64 = 60_000;

fn rig() -> (Rig, Arc<VirtualClock>) {
    let clock = VirtualClock::new(T0);
    (rig_on_clock(None, 0, |_| {}, clock.clone()), clock)
}

/// A task started for `after_ms` from now: its session id.
fn deferred(r: &Rig, after_ms: u64) -> String {
    r.core
        .task_start(
            &TaskStartParams {
                brief: "Count the boats at dawn".into(),
                start_after_ms: Some(after_ms),
                budget_usd: Some(0.25),
                ..Default::default()
            },
            cli(),
        )
        .unwrap()
        .session_id
}

fn at(task: &str, ms: u64) -> TaskRescheduleParams {
    TaskRescheduleParams {
        task: task.into(),
        start_at_ms: Some(ms),
        start_after_ms: None,
        start_when: None,
        author: None,
        discord: None,
    }
}

fn move_to(r: &Rig, p: &TaskRescheduleParams) -> TaskRescheduleResult {
    r.core.task_reschedule(p, &cli()).unwrap()
}

fn woke(r: &Rig) -> Vec<String> {
    r.core.kernel.reconcile(&NoEvidence).unwrap().woke_due
}

fn exec_of(r: &Rig, task: &str) -> theseus_kernel::Execution {
    r.core
        .kernel
        .execution(&task.replacen("ses_", "exe_", 1))
        .unwrap()
        .unwrap()
}

fn refused(r: &Rig, p: &TaskRescheduleParams) -> String {
    format!("{:#}", r.core.task_reschedule(p, &cli()).unwrap_err())
}

#[tokio::test]
async fn the_due_scan_wakes_it_at_the_new_time_and_not_the_old() {
    let (r, clock) = rig();
    let task = deferred(&r, 2 * MIN);
    let moved = move_to(&r, &at(&task, T0 + 10 * MIN));
    assert_eq!(
        (moved.starts_at_ms, moved.previous),
        (T0 + 10 * MIN, T0 + 2 * MIN)
    );
    clock.advance(3 * MIN);
    assert!(woke(&r).is_empty(), "the old time passed and woke nothing");
    assert_eq!(exec_of(&r, &task).state, ExecState::Waiting);
    clock.advance(7 * MIN);
    assert_eq!(woke(&r), vec![exec_of(&r, &task).id]);
    assert_eq!(exec_of(&r, &task).state, ExecState::Queued);
}

#[tokio::test]
async fn previous_and_a_second_call_restore_it() {
    let (r, clock) = rig();
    let task = deferred(&r, 2 * MIN);
    let first = move_to(&r, &at(&task, T0 + 30 * MIN));
    let undone = move_to(&r, &at(&task, first.previous));
    assert_eq!(undone.starts_at_ms, T0 + 2 * MIN);
    assert_eq!(undone.previous, T0 + 30 * MIN);
    clock.advance(2 * MIN);
    assert_eq!(woke(&r).len(), 1, "it starts at its first time");
}

#[tokio::test]
async fn each_of_the_three_time_forms_moves_it() {
    let (r, _) = rig();
    let task = deferred(&r, 2 * MIN);
    let now = r.core.kernel.now_ms();
    assert_eq!(now, T0);
    assert_eq!(
        move_to(&r, &at(&task, T0 + 5 * MIN)).starts_at_ms,
        T0 + 5 * MIN
    );
    let after = TaskRescheduleParams {
        start_at_ms: None,
        start_after_ms: Some(7 * MIN),
        ..at(&task, 0)
    };
    assert_eq!(move_to(&r, &after).starts_at_ms, T0 + 7 * MIN);
    let when = TaskRescheduleParams {
        start_at_ms: None,
        start_when: Some("in 2h".into()),
        ..at(&task, 0)
    };
    let got = move_to(&r, &when).starts_at_ms;
    assert!(
        (T0 + 2 * 3_600_000 - 10_000..=T0 + 2 * 3_600_000 + 10_000).contains(&got),
        "{}",
        got - T0
    );
    // The times the first turn reads moved with it, and when it was set did not.
    let times: Deferred = r
        .core
        .store
        .get_meta(&crate::task_start::key(&task))
        .unwrap()
        .unwrap();
    assert_eq!((times.due_at_ms, times.set_at_ms), (got, T0));
}

#[tokio::test]
async fn a_started_task_a_task_with_no_deferred_start_and_a_held_one_are_refused() {
    let (r, clock) = rig();
    let early = deferred(&r, 2 * MIN);
    // A task with no deferred start.
    let now = r
        .core
        .task_start(
            &TaskStartParams {
                brief: "Count the boats now".into(),
                budget_usd: Some(0.25),
                ..Default::default()
            },
            cli(),
        )
        .unwrap()
        .session_id;
    assert!(refused(&r, &at(&now, T0 + 9 * MIN)).contains("not waiting to start"));
    // A held one waits on a due time that never comes.
    let held = deferred(&r, 5 * MIN);
    r.core.task_hold(&held, true, &cli()).unwrap();
    assert!(refused(&r, &at(&held, T0 + 9 * MIN)).contains("paused"));
    // A task whose time came and went has started.
    clock.advance(3 * MIN);
    assert_eq!(woke(&r).len(), 1);
    let said = refused(&r, &at(&early, T0 + 20 * MIN));
    assert!(said.contains("not waiting to start"), "{said}");
    assert_eq!(
        exec_of(&r, &early).state,
        ExecState::Queued,
        "nothing moved it"
    );
    assert!(kinds(&r.core.store, "task.rescheduled").is_empty());
}

#[tokio::test]
async fn a_time_in_the_past_two_forms_or_none_are_refused_and_nothing_is_written() {
    let (r, _) = rig();
    let task = deferred(&r, 2 * MIN);
    for (p, why) in [
        (at(&task, T0 - 1), "ago"),
        (at(&task, T0 + 100), "at least"),
        (
            TaskRescheduleParams {
                start_after_ms: Some(5 * MIN),
                ..at(&task, T0 + 9 * MIN)
            },
            "not several",
        ),
        (
            TaskRescheduleParams {
                start_at_ms: None,
                ..at(&task, 0)
            },
            "give one of",
        ),
    ] {
        let said = refused(&r, &p);
        assert!(said.contains(why), "{why}: {said}");
    }
    assert_eq!(
        exec_of(&r, &task).wake,
        Some(theseus_kernel::Wake::DueAt {
            at_ms: T0 + 2 * MIN
        })
    );
    assert!(kinds(&r.core.store, "task.rescheduled").is_empty());
}

#[tokio::test]
async fn the_ledger_row_names_the_task_the_move_and_who_and_nothing_is_posted() {
    let (r, _) = rig();
    let task = deferred(&r, 2 * MIN);
    let posts = r.core.kernel.outbox_actions().unwrap().len();
    move_to(&r, &at(&task, T0 + 9 * MIN));
    let rows = kinds(&r.core.store, "task.rescheduled");
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].session_id.as_deref(), Some(task.as_str()));
    assert_eq!(rows[0].data["session_id"], task.as_str());
    assert_eq!(rows[0].data["task"], exec_of(&r, &task).id.as_str());
    assert_eq!(
        (rows[0].data["from"].as_u64(), rows[0].data["to"].as_u64()),
        (Some(T0 + 2 * MIN), Some(T0 + 9 * MIN))
    );
    assert_eq!(rows[0].data["by"], "the CLI");
    assert_eq!(
        r.core.kernel.outbox_actions().unwrap().len(),
        posts,
        "no post to a place"
    );
}

#[tokio::test]
async fn only_the_owner_from_a_private_place_moves_a_start() {
    let (r, _) = rig();
    let task = deferred(&r, 2 * MIN);
    let e = r
        .core
        .task_reschedule(&at(&task, T0 + 9 * MIN), &stranger())
        .unwrap_err();
    let said = format!("{e:#}");
    assert!(said.contains("owner") || said.contains("private"), "{said}");
    assert!(kinds(&r.core.store, "task.rescheduled").is_empty());
}
