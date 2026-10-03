//! Task executions (DD7, theseus-qn2): the carve, depth one, a task's spend in
//! its parent's, its end reported once, and the races the family's lock
//! (`lock_family`) closes.

use theseus_store::{kinds, NewRecord};

use crate::kernel::*;
use crate::tasks::{carve_key, task_ids};
use crate::tests::*;
use crate::types::*;

/// A parent with a turn, and a task opened by one of its calls: the parent's
/// guard, the task, and the call's correlation id.
fn with_task(w: &World, want: Micros) -> (Execution, TurnGuard, Execution, String) {
    let (_, parent, g) = running(w);
    let call = new_id("act");
    let t = w
        .kernel
        .open_task(&g, &call, want, Some("discord:dm:1".into()), false, |_| {
            Ok(vec![])
        })
        .unwrap();
    assert!(t.opened);
    (parent, g, t.task, call)
}

fn exec(w: &World, id: &str) -> Execution {
    w.kernel.execution(id).unwrap().unwrap()
}

fn carved(w: &World, parent: &str, task: &str) -> Option<Micros> {
    exec(w, parent)
        .budget
        .reservations
        .get(&carve_key(task))
        .copied()
}

/// A task is queued for the driver with a pinned limit carved from its
/// parent, which reserves it; its ids come from the call that opened it, and
/// it inherits the parent's authority.
#[test]
fn a_task_opens_queued_with_a_budget_carved_from_its_parent() {
    let w = world();
    let (parent, _g, task, call) = with_task(&w, 30_000);
    let (eid, sid) = task_ids(&call);
    assert_eq!(
        (task.id.as_str(), task.session_id.as_str()),
        (eid.as_str(), sid.as_str())
    );
    assert_eq!(task.kind, SessionKind::Task);
    assert_eq!(task.state, ExecState::Queued);
    assert!(task.resume_pending, "the driver takes its first turn");
    assert_eq!(task.parent.as_deref(), Some(parent.id.as_str()));
    assert_eq!(task.reports_to.as_deref(), Some("discord:dm:1"));
    assert_eq!(task.authority, parent.authority);
    assert_eq!(task.budget.limit_micros, 30_000);
    assert!(task.budget.pinned, "a carve is its own limit");
    assert_eq!(exec(&w, &task.id), task, "as stored");
    let p = exec(&w, &parent.id);
    assert_eq!(carved(&w, &parent.id, &task.id), Some(30_000));
    assert_eq!(p.budget.reserved_micros, 30_000);
    assert_eq!(p.budget.available(), 70_000);
    let row = &rows(&w, &parent.session_id, "budget.carved")[0];
    assert_eq!(row["carved_usd"], 0.03);
    assert_eq!(row["available_after_usd"], 0.07);
    assert_eq!(
        rows(&w, &task.session_id, "execution.opened")[0]["parent"],
        parent.id
    );
    assert_eq!(
        w.kernel.tasks(Some(&parent.id)).unwrap(),
        vec![task.clone()]
    );
    assert_eq!(w.kernel.tasks(None).unwrap(), vec![task]);
}

/// A request for more than the parent has left is capped at what it has
/// left, and a parent with nothing left has nothing to carve.
#[test]
fn a_request_for_more_than_the_parent_has_left_is_capped() {
    let w = world();
    let (parent, g, task, _) = with_task(&w, 60_000);
    assert_eq!(task.budget.limit_micros, 60_000);
    let second = w
        .kernel
        .open_task(&g, &new_id("act"), 500_000, None, false, |_| Ok(vec![]))
        .unwrap();
    assert_eq!(second.available_before, 40_000);
    assert_eq!(second.task.budget.limit_micros, 40_000, "capped");
    assert_eq!(exec(&w, &parent.id).budget.available(), 0);
    let before = w.kernel.store().last_position();
    let err = w
        .kernel
        .open_task(&g, &new_id("act"), 1, None, false, |_| Ok(vec![]))
        .unwrap_err();
    assert!(
        matches!(
            err.downcast_ref::<KernelError>(),
            Some(KernelError::NothingToCarve { available: 0, .. })
        ),
        "{err:#}"
    );
    assert_eq!(w.kernel.store().last_position(), before, "nothing written");
}

/// The same call run again (after a crash between its task's frame and its
/// result) gets its task back and writes nothing: one call, one task.
#[test]
fn the_same_call_opens_its_task_once() {
    let w = world();
    let (parent, g, task, call) = with_task(&w, 30_000);
    let before = w.kernel.store().last_position();
    let again = w
        .kernel
        .open_task(&g, &call, 30_000, None, false, |_| panic!("no records"))
        .unwrap();
    assert!(!again.opened);
    assert_eq!(again.task, task);
    assert_eq!(w.kernel.store().last_position(), before);
    assert_eq!(exec(&w, &parent.id).budget.reserved_micros, 30_000);
}

/// A task cannot open tasks (depth one); nothing is written.
#[test]
fn a_task_cannot_start_tasks() {
    let w = world();
    let (_, _g, task, _) = with_task(&w, 30_000);
    let tg = w.kernel.admit(&task.id).unwrap();
    let before = w.kernel.store().last_position();
    let err = w
        .kernel
        .open_task(&tg, &new_id("act"), 1_000, None, false, |_| Ok(vec![]))
        .unwrap_err();
    assert!(
        matches!(
            err.downcast_ref::<KernelError>(),
            Some(KernelError::TaskDepth { .. })
        ),
        "{err:#}"
    );
    assert!(format!("{err}").contains("depth one"), "{err}");
    assert_eq!(w.kernel.store().last_position(), before);
}

/// A task's costs are its parent's spend too, in the same frame, and the
/// parent's carve shrinks to what the task can still spend. The task stops
/// at its own limit, and asks, though its parent has more left.
#[test]
fn a_tasks_spend_counts_against_its_parent_and_it_stops_at_its_carve() {
    let w = world();
    let (parent, _g, task, _) = with_task(&w, 30_000);
    let tg = w.kernel.admit(&task.id).unwrap();
    let a = dispatched(&w, &tg, "provider.messages", 10_000);
    let p = exec(&w, &parent.id);
    assert_eq!(p.budget.spent_micros, 0);
    assert_eq!(p.budget.reserved_micros, 30_000, "the carve covers it");
    let frames = w.kernel.store().stats().unwrap().frames_appended;
    w.kernel
        .accept_completion(&completion(
            &a.correlation_id,
            Outcome::Succeeded,
            Some(4_000),
        ))
        .unwrap();
    assert_eq!(
        w.kernel.store().stats().unwrap().frames_appended,
        frames + 1,
        "one frame for both"
    );
    let (t, p) = (exec(&w, &task.id), exec(&w, &parent.id));
    assert_eq!(t.budget.spent_micros, 4_000);
    assert_eq!(
        p.budget.spent_micros, 4_000,
        "the parent's spend includes it"
    );
    assert_eq!(carved(&w, &parent.id, &task.id), Some(26_000));
    assert_eq!(p.budget.reserved_micros, 26_000);
    assert_eq!(
        p.budget.available(),
        70_000,
        "what the parent may still promise"
    );
    // Its own limit, not the parent's, stops it.
    let err = w
        .kernel
        .plan_action(
            &tg,
            &proposal("provider.messages"),
            RetryClass::NonRepeatable,
            None,
            26_001,
        )
        .unwrap_err();
    assert!(
        matches!(
            err.downcast_ref::<KernelError>(),
            Some(KernelError::OverBudget {
                limit: 30_000,
                available: 26_000,
                ..
            })
        ),
        "{err:#}"
    );
    let q = w.kernel.ask_budget(&tg, 26_001).unwrap();
    assert_eq!(q.session_id, task.session_id, "the task asks, as usual");
    drop(tg);
}

/// A task that ends releases its carve, but for what it still has in flight,
/// which its settle then spends or releases; it joins the parent's reports,
/// which the parent's next turn takes once, in one frame.
#[test]
fn a_task_that_ends_reports_to_its_parent_once() {
    let w = world();
    let (parent, g, task, _) = with_task(&w, 30_000);
    let tg = w.kernel.admit(&task.id).unwrap();
    let job = dispatched(&w, &tg, "provider.messages", 5_000);
    let t = w
        .kernel
        .end_turn(
            tg,
            TurnEnd::Complete {
                reason: "reported".into(),
            },
        )
        .unwrap();
    assert_eq!(t.state, ExecState::Complete);
    let p = exec(&w, &parent.id);
    assert_eq!(p.reports, vec![task.id.clone()]);
    assert_eq!(carved(&w, &parent.id, &task.id), Some(5_000), "in flight");
    assert_eq!(p.budget.reserved_micros, 5_000);
    let row = &rows(&w, &parent.session_id, "task.ended")[0];
    assert_eq!(row["released_usd"], 0.025);
    // The call in flight settles after the end: spent, and the carve gone.
    w.kernel
        .accept_completion(&completion(
            &job.correlation_id,
            Outcome::Succeeded,
            Some(2_000),
        ))
        .unwrap();
    let p = exec(&w, &parent.id);
    assert_eq!(p.budget.spent_micros, 2_000);
    assert_eq!(carved(&w, &parent.id, &task.id), None);
    assert_eq!(p.budget.reserved_micros, 0);
    // The parent's next turn reads the report, once.
    let mut built = Vec::new();
    let ids = w
        .kernel
        .take_reports(&g, |ids| {
            built.extend(ids.iter().cloned());
            Ok(vec![NewRecord::json(
                kinds::META,
                Some("probe.report"),
                &"written",
            )?])
        })
        .unwrap();
    assert_eq!(ids.ids, vec![task.id]);
    assert!(ids.woke.is_empty(), "it asked for no turn");
    assert_eq!(built, ids.ids);
    assert!(exec(&w, &parent.id).reports.is_empty());
    assert!(
        w.kernel
            .store()
            .latest_by_key(kinds::META, "probe.report")
            .unwrap()
            .is_some(),
        "the records it built were written with it"
    );
    let before = w.kernel.store().last_position();
    assert!(w
        .kernel
        .take_reports(&g, |_| panic!("no reports"))
        .unwrap()
        .ids
        .is_empty());
    assert_eq!(w.kernel.store().last_position(), before, "none: no frame");
}

/// A cancel ends a task as `end_turn` does, and its records (the report that
/// it was cancelled) are written with it, once: a second cancel writes none.
#[test]
fn a_cancelled_task_reports_once_and_releases_its_carve() {
    let w = world();
    let (parent, _g, task, _) = with_task(&w, 30_000);
    let mut calls = 0;
    w.kernel
        .cancel_execution_with(&task.id, "discord:eddie", |end| {
            calls += 1;
            assert_eq!(end.execution.state, ExecState::Cancelled);
            Ok(vec![])
        })
        .unwrap();
    w.kernel
        .cancel_execution_with(&task.id, "discord:eddie", |_| panic!("cancelled once"))
        .unwrap();
    assert_eq!(calls, 1);
    let p = exec(&w, &parent.id);
    assert_eq!(p.reports, vec![task.id]);
    assert_eq!(p.budget.reserved_micros, 0);
    assert_eq!(p.budget.available(), 100_000);
}

/// A call a cancel could not stop holds its reservation as unknown, so the
/// parent's carve keeps it; the call's late completion brings its real cost,
/// which is booked in the task and the parent, and the hold and the carve go.
#[test]
fn a_cancelled_tasks_late_completion_reconciles_what_it_held() {
    let w = world();
    let (parent, _g, task, _) = with_task(&w, 30_000);
    let tg = w.kernel.admit(&task.id).unwrap();
    let call = dispatched(&w, &tg, "provider.messages", 10_000);
    let stop = w.kernel.cancel_execution(&task.id, "operator").unwrap();
    assert_eq!(stop, vec![call.correlation_id.clone()]);
    w.kernel
        .cancel_unsupported(&call.correlation_id, "it runs in process")
        .unwrap();
    assert_eq!(exec(&w, &task.id).budget.held_unknown_micros, 10_000);
    assert_eq!(carved(&w, &parent.id, &task.id), Some(10_000), "still held");
    let late = w
        .kernel
        .accept_completion(&completion(
            &call.correlation_id,
            Outcome::Succeeded,
            Some(3_000),
        ))
        .unwrap();
    assert!(matches!(late, Accepted::LateAfterCancel { .. }));
    let t = exec(&w, &task.id);
    assert_eq!(t.state, ExecState::Cancelled, "not revived");
    assert_eq!(
        (t.budget.held_unknown_micros, t.budget.spent_micros),
        (0, 3_000)
    );
    let p = exec(&w, &parent.id);
    assert_eq!(p.budget.spent_micros, 3_000);
    assert_eq!(carved(&w, &parent.id, &task.id), None);
    assert_eq!(p.budget.reserved_micros, 0);
    // A second copy of it changes nothing.
    w.kernel
        .accept_completion(&completion(
            &call.correlation_id,
            Outcome::Succeeded,
            Some(3_000),
        ))
        .unwrap();
    assert_eq!(exec(&w, &parent.id).budget.spent_micros, 3_000);
    drop(tg);
}

/// A crash keeps everything: the task stays queued for the driver, a task
/// mid-turn is requeued as any execution is, and the carve stays reserved.
#[test]
fn a_task_and_its_carve_survive_a_crash() {
    let w = world();
    let (parent, g, task, _) = with_task(&w, 30_000);
    let tg = w.kernel.admit(&task.id).unwrap();
    drop((g, tg));
    let (w, rep) = crash(w, KernelConfig::default());
    assert!(rep.requeued_interrupted.contains(&task.id));
    let t = exec(&w, &task.id);
    assert_eq!((t.state, t.resume_pending), (ExecState::Queued, true));
    assert_eq!(carved(&w, &parent.id, &task.id), Some(30_000));
}

// ------------------------------------------------------------ the report's wake (W1)

/// A parent waiting on input, with a task opened with `wake_parent`.
fn waiting_with_waking_task(w: &World) -> (Execution, Execution) {
    let (_, parent, g) = running(w);
    let t = w
        .kernel
        .open_task(&g, &new_id("act"), 20_000, None, true, |_| Ok(vec![]))
        .unwrap()
        .task;
    assert!(t.wake_parent);
    w.kernel
        .end_turn(g, TurnEnd::Wait { wake: Wake::Input })
        .unwrap();
    (parent, t)
}

fn finish(w: &World, task: &str) -> Execution {
    let tg = w.kernel.admit(task).unwrap();
    w.kernel
        .end_turn(
            tg,
            TurnEnd::Complete {
                reason: "reported".into(),
            },
        )
        .unwrap()
}

/// A task opened with `wake_parent` that finishes queues its free parent for
/// the driver in the frame that ends it, ledgered as the report's wake. The
/// parent's turn reads the report and clears the ask, and its end parks as
/// usual.
#[test]
fn a_task_with_wake_parent_queues_its_free_parent_in_the_frame_that_ends_it() {
    let w = world();
    let (parent, task) = waiting_with_waking_task(&w);
    let frames = w.kernel.store().stats().unwrap().frames_appended;
    finish(&w, &task.id);
    assert_eq!(
        w.kernel.store().stats().unwrap().frames_appended,
        frames + 2,
        "the admit, then one frame for the end and the wake"
    );
    let p = exec(&w, &parent.id);
    assert_eq!(
        (p.state, p.wake.clone(), p.resume_pending),
        (ExecState::Queued, None, true)
    );
    assert_eq!(p.reports, vec![task.id.clone()]);
    assert_eq!(p.report_wakes, vec![task.id.clone()]);
    let row = &rows(&w, &parent.session_id, "task.report_wake")[0];
    assert_eq!(row["task"], task.id);
    assert_eq!(row["execution_id"], parent.id);
    assert_eq!(row["queued"], true);
    let q = rows(&w, &parent.session_id, "execution.queued");
    assert_eq!(q.last().unwrap()["why"], "report");
    assert_eq!(q.last().unwrap()["tasks"][0], task.id);
    // The parent's turn reads it, and asks for nothing more.
    let g = w.kernel.admit(&parent.id).unwrap();
    let taken = w.kernel.take_reports(&g, |_| Ok(vec![])).unwrap();
    assert_eq!(taken.ids, vec![task.id.clone()]);
    assert_eq!(taken.woke, vec![task.id.clone()]);
    let read = rows(&w, &parent.session_id, "task.reports_read");
    assert_eq!(read[0]["woke"][0], task.id);
    let p = w
        .kernel
        .end_turn(g, TurnEnd::Wait { wake: Wake::Input })
        .unwrap();
    assert_eq!(p.state, ExecState::Waiting);
    assert!(p.report_wakes.is_empty() && p.reports.is_empty());
}

/// Without `wake_parent`, a report starts no turn: the parent waits on input
/// with the report on its list, as before.
#[test]
fn without_wake_parent_a_report_starts_no_turn() {
    let w = world();
    let (_, parent, g) = running(&w);
    let t = w
        .kernel
        .open_task(&g, &new_id("act"), 20_000, None, false, |_| Ok(vec![]))
        .unwrap()
        .task;
    w.kernel
        .end_turn(g, TurnEnd::Wait { wake: Wake::Input })
        .unwrap();
    finish(&w, &t.id);
    let p = exec(&w, &parent.id);
    assert_eq!(
        (p.state, p.wake.clone()),
        (ExecState::Waiting, Some(Wake::Input))
    );
    assert_eq!(p.reports, vec![t.id]);
    assert!(p.report_wakes.is_empty());
    assert!(!crate::wakes::due_now(&p, w.kernel.now_ms()));
    assert!(rows(&w, &parent.session_id, "task.report_wake").is_empty());
}

/// Two reports that land together start one turn, which reads them both.
#[test]
fn two_reports_that_land_together_start_one_turn() {
    let w = world();
    let (_, parent, g) = running(&w);
    let open = |g: &TurnGuard| {
        w.kernel
            .open_task(g, &new_id("act"), 20_000, None, true, |_| Ok(vec![]))
            .unwrap()
            .task
    };
    let (a, b) = (open(&g), open(&g));
    w.kernel
        .end_turn(g, TurnEnd::Wait { wake: Wake::Input })
        .unwrap();
    finish(&w, &a.id);
    finish(&w, &b.id);
    let p = exec(&w, &parent.id);
    assert_eq!(p.state, ExecState::Queued, "queued once, by the first");
    assert_eq!(p.report_wakes, vec![a.id.clone(), b.id.clone()]);
    let queued: Vec<_> = rows(&w, &parent.session_id, "execution.queued")
        .into_iter()
        .filter(|r| r["why"] == "report")
        .collect();
    assert_eq!(queued.len(), 1, "the second found it queued");
    let g = w.kernel.admit(&parent.id).unwrap();
    let taken = w.kernel.take_reports(&g, |_| Ok(vec![])).unwrap();
    assert_eq!(taken.ids, vec![a.id.clone(), b.id.clone()]);
    assert_eq!(taken.woke, vec![a.id, b.id]);
    let p = w
        .kernel
        .end_turn(g, TurnEnd::Wait { wake: Wake::Input })
        .unwrap();
    assert_eq!(p.state, ExecState::Waiting, "one turn read them both");
}

/// A parent that is busy when the report lands keeps the ask, and the frame
/// that ends its turn queues it. A failed task wakes its parent too.
#[test]
fn a_busy_parent_takes_the_reports_turn_when_its_own_turn_ends() {
    let w = world();
    let (_, parent, g) = running(&w);
    let t = w
        .kernel
        .open_task(&g, &new_id("act"), 20_000, None, true, |_| Ok(vec![]))
        .unwrap()
        .task;
    let tg = w.kernel.admit(&t.id).unwrap();
    w.kernel
        .end_turn(
            tg,
            TurnEnd::Fail {
                reason: "provider:overloaded".into(),
            },
        )
        .unwrap();
    let p = exec(&w, &parent.id);
    assert_eq!(p.state, ExecState::Running, "busy: its own turn runs");
    assert_eq!(p.report_wakes, vec![t.id]);
    assert_eq!(
        rows(&w, &parent.session_id, "task.report_wake")[0]["queued"],
        false
    );
    let p = w
        .kernel
        .end_turn(g, TurnEnd::Wait { wake: Wake::Input })
        .unwrap();
    assert_eq!((p.state, p.resume_pending), (ExecState::Queued, true));
    let ended = rows(&w, &parent.session_id, "execution.queued");
    assert_eq!(ended.last().unwrap()["why"], "report");
}

/// A cancelled task wakes nothing: whoever cancelled it is there, and its
/// report, which the parent's next turn still reads, says who did.
#[test]
fn a_cancelled_task_wakes_nothing() {
    let w = world();
    let (parent, task) = waiting_with_waking_task(&w);
    w.kernel
        .cancel_execution(&task.id, "discord:eddie")
        .unwrap();
    let p = exec(&w, &parent.id);
    assert_eq!(
        (p.state, p.wake.clone()),
        (ExecState::Waiting, Some(Wake::Input))
    );
    assert_eq!(p.reports, vec![task.id]);
    assert!(p.report_wakes.is_empty());
    assert!(rows(&w, &parent.session_id, "task.report_wake").is_empty());
}

/// A report that asked for a turn while its parent's turn was being stopped
/// is not lost: the stopped turn parks on input, and the due scan queues the
/// parent at its next tick, `why: report`.
#[test]
fn a_reports_wake_outlives_a_stop_of_its_parents_turn() {
    let w = world();
    let (_, parent, g) = running(&w);
    let t = w
        .kernel
        .open_task(&g, &new_id("act"), 20_000, None, true, |_| Ok(vec![]))
        .unwrap()
        .task;
    w.kernel
        .stop_execution(&parent.id, "discord:eddie")
        .unwrap()
        .unwrap();
    finish(&w, &t.id);
    let p = w
        .kernel
        .end_turn(g, TurnEnd::Wait { wake: Wake::Input })
        .unwrap();
    assert_eq!(p.state, ExecState::Waiting, "a stop parks it");
    assert!(crate::wakes::due_now(&p, w.kernel.now_ms()));
    let q = w.kernel.fire_due(&parent.id).unwrap().expect("queued");
    assert_eq!((q.state, q.resume_pending), (ExecState::Queued, true));
    let row = rows(&w, &parent.session_id, "execution.queued");
    assert_eq!(row.last().unwrap()["why"], "report");
}

// ------------------------------------------------------------ races

/// A task's settle and its parent's own reservation, each read before the
/// other's frame: with `lock_family`, the parent's reservation waits for the
/// task's frame, and neither update is lost.
#[test]
fn a_tasks_settle_and_its_parents_own_reservation_never_lose_each_other() {
    let (w, p) = pausing_world();
    let (parent, g, task, _) = with_task(&w, 30_000);
    let tg = w.kernel.admit(&task.id).unwrap();
    let a = dispatched(&w, &tg, "provider.messages", 10_000);
    let r = race(
        &w.kernel,
        &p,
        (kinds::EXECUTION, &parent.id, 1),
        |k| {
            k.accept_completion(&completion(
                &a.correlation_id,
                Outcome::Succeeded,
                Some(4_000),
            ))
        },
        |k| {
            k.plan_and_dispatch(
                &g,
                &proposal("provider.messages"),
                RetryClass::NonRepeatable,
                None,
                7_000,
                |_| Ok(vec![]),
            )
        },
    );
    r.first.unwrap();
    let mine = r.second.unwrap();
    let x = exec(&w, &parent.id);
    assert_eq!(x.budget.spent_micros, 4_000, "the task's spend was kept");
    assert!(
        x.budget
            .reservations
            .contains_key(mine.reservation_id.as_deref().unwrap()),
        "the parent's reservation was kept"
    );
    assert_eq!(x.budget.reserved_micros, 26_000 + 7_000);
    assert!(
        r.second_waited,
        "the parent's plan waited for the task's frame"
    );
    drop((g, tg));
}

/// The same race the other way: the parent's reservation read first, and the
/// task's settle waits for its frame.
#[test]
fn a_parents_reservation_and_its_tasks_settle_never_lose_each_other() {
    let (w, p) = pausing_world();
    let (parent, g, task, _) = with_task(&w, 30_000);
    let tg = w.kernel.admit(&task.id).unwrap();
    let a = dispatched(&w, &tg, "provider.messages", 10_000);
    // The parent's plan is a transaction: its first read finds the family,
    // before the lock, and its second is the plan's own, under it.
    let r = race(
        &w.kernel,
        &p,
        (kinds::EXECUTION, &parent.id, 2),
        |k| {
            k.plan_and_dispatch(
                &g,
                &proposal("provider.messages"),
                RetryClass::NonRepeatable,
                None,
                7_000,
                |_| Ok(vec![]),
            )
        },
        |k| {
            k.accept_completion(&completion(
                &a.correlation_id,
                Outcome::Succeeded,
                Some(4_000),
            ))
        },
    );
    let mine = r.first.unwrap();
    r.second.unwrap();
    let x = exec(&w, &parent.id);
    assert_eq!(x.budget.spent_micros, 4_000, "the task's spend was kept");
    assert!(
        x.budget
            .reservations
            .contains_key(mine.reservation_id.as_deref().unwrap()),
        "the parent's reservation was kept"
    );
    assert_eq!(x.budget.reserved_micros, 26_000 + 7_000);
    assert!(
        r.second_waited,
        "the task's settle waited for the parent's frame"
    );
    drop((g, tg));
}

/// Opening a task, read before the parent's own reservation was indexed: the
/// reservation waits, and the parent keeps both it and the carve.
#[test]
fn opening_a_task_and_the_parents_reservation_never_lose_each_other() {
    let (w, p) = pausing_world();
    let (_, parent, g) = running(&w);
    let call = new_id("act");
    let r = race(
        &w.kernel,
        &p,
        (kinds::EXECUTION, &parent.id, 1),
        |k| k.open_task(&g, &call, 30_000, None, false, |_| Ok(vec![])),
        |k| {
            k.plan_and_dispatch(
                &g,
                &proposal("provider.messages"),
                RetryClass::NonRepeatable,
                None,
                7_000,
                |_| Ok(vec![]),
            )
        },
    );
    assert!(r.second_waited);
    let t = r.first.unwrap().task;
    let mine = r.second.unwrap();
    let x = exec(&w, &parent.id);
    assert_eq!(x.budget.reservations.get(&carve_key(&t.id)), Some(&30_000));
    assert!(x
        .budget
        .reservations
        .contains_key(mine.reservation_id.as_deref().unwrap()));
    assert_eq!(x.budget.reserved_micros, 37_000);
    drop(g);
}
