//! Task executions (DD7, theseus-qn2): the carve, depth one, a task's spend in
//! its parent's, its end reported once, and the races the family's lock
//! (`lock_family`) closes.

use serde_json::json;
use theseus_protocol::LedgerKind;
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

/// A task below a task: a tree deeper than the bound allows today, written
/// directly as `carve_task` would write it (the task's record, and the
/// parent's carve for it), so that the carry up the chain can be held before
/// `theseus-tecj5` raises the bound and makes such trees ordinary. `under` is
/// the new task's parent, and `template` any task's record, for the fields a
/// tree does not vary.
fn deeper_task(w: &World, under: &Execution, template: &Execution, limit: Micros) -> Execution {
    let mut parent = exec(w, &under.id);
    let (id, session_id) = task_ids(&new_id("act"));
    let child = Execution {
        id: id.clone(),
        session_id,
        kind: SessionKind::Task,
        state: ExecState::Queued,
        parent: Some(parent.id.clone()),
        budget: Budget {
            pinned: true,
            ..Budget::new(limit)
        },
        ..template.clone()
    };
    parent.budget.reserved_micros = parent.budget.reserved_micros.saturating_add(limit);
    parent.budget.reservations.insert(carve_key(&id), limit);
    w.kernel
        .commit(&[exec_record(&parent).unwrap(), exec_record(&child).unwrap()])
        .unwrap();
    child
}

/// theseus-deaqt: a **grandchild's** settled spend is the conversation's
/// spend too, not only its own parent task's, and every carve above it
/// shrinks to what the execution under it can still spend. `carry_to_parent`
/// used to stop at one level, so the conversation's `spent_micros` — what its
/// own spend-limit notice reads — never learned a grandchild's cost.
#[test]
fn a_grandchilds_spend_reaches_the_conversation() {
    let w = world();
    let (conv, _g, task, _) = with_task(&w, 30_000);
    let grand = deeper_task(&w, &task, &task, 10_000);
    let frames = w.kernel.store().stats().unwrap().frames_appended;
    w.kernel
        .book_spend(&grand.id, 4_000, LedgerKind::SpeechTranscribed, json!({}))
        .unwrap();
    assert_eq!(
        w.kernel.store().stats().unwrap().frames_appended,
        frames + 1,
        "the whole chain in one frame"
    );
    assert_eq!(exec(&w, &grand.id).budget.spent_micros, 4_000);
    assert_eq!(
        exec(&w, &task.id).budget.spent_micros,
        4_000,
        "the task between them"
    );
    assert_eq!(
        exec(&w, &conv.id).budget.spent_micros,
        4_000,
        "the conversation, whose spend-limit notice reads this"
    );
    assert_eq!(
        carved(&w, &task.id, &grand.id),
        Some(6_000),
        "the task's carve for its own task shrinks"
    );
    assert_eq!(
        carved(&w, &conv.id, &task.id),
        Some(26_000),
        "and so does the conversation's carve for the task"
    );
    let conv_now = exec(&w, &conv.id);
    assert_eq!(conv_now.budget.reserved_micros, 26_000);
    assert_eq!(conv_now.budget.available(), 70_000);
}

/// theseus-deaqt: a tree of three levels below a conversation. What the
/// conversation has spent is the sum of what settled anywhere below it,
/// whichever level settled it, and each carve is what the execution under it
/// can still spend (the invariant kernel-sim checks).
#[test]
fn a_deep_trees_conversation_spend_is_the_sum_of_what_settled_below_it() {
    let w = world();
    let (conv, _g, t1, _) = with_task(&w, 60_000);
    let t2 = deeper_task(&w, &t1, &t1, 40_000);
    let t3 = deeper_task(&w, &t2, &t1, 20_000);
    for (e, cost) in [(&t3, 1_500u64), (&t2, 2_500), (&t1, 500)] {
        w.kernel
            .book_spend(&e.id, cost, LedgerKind::SpeechTranscribed, json!({}))
            .unwrap();
    }
    assert_eq!(exec(&w, &t3.id).budget.spent_micros, 1_500);
    assert_eq!(exec(&w, &t2.id).budget.spent_micros, 1_500 + 2_500);
    assert_eq!(exec(&w, &t1.id).budget.spent_micros, 1_500 + 2_500 + 500);
    assert_eq!(
        exec(&w, &conv.id).budget.spent_micros,
        4_500,
        "the conversation's spend is the whole tree's"
    );
    assert_eq!(carved(&w, &t2.id, &t3.id), Some(20_000 - 1_500));
    assert_eq!(carved(&w, &t1.id, &t2.id), Some(40_000 - 4_000));
    assert_eq!(carved(&w, &conv.id, &t1.id), Some(60_000 - 4_500));
}

/// theseus-deaqt, the other half: a task's END is a one-level carry too. An
/// ended task can still spend only what it has in flight or unknown, and a
/// task of its own that ends takes its carve out of that — so the execution
/// above it must stop holding it carved, and the one above that. Found by the
/// sim's own carve invariant (`kernel-sim --seed 1 --seeds 20 --steps 300`,
/// seed 4 step 152) once the depth bound allows a grandchild: the
/// conversation held a carve for a task that had ended and could spend
/// nothing.
#[test]
fn an_ended_tasks_release_of_its_own_tasks_carve_reaches_the_conversation() {
    let w = world();
    let (conv, _g, t1, _) = with_task(&w, 60_000);
    let t2 = deeper_task(&w, &t1, &t1, 40_000);
    // t1 ends while t2 still holds its carve, so what t1 can still spend is
    // exactly that carve, and the conversation keeps it reserved.
    let g1 = w.kernel.admit(&t1.id).unwrap();
    let ended = w
        .kernel
        .end_turn(
            g1,
            TurnEnd::Complete {
                reason: "reported".into(),
            },
        )
        .unwrap();
    assert_eq!(ended.state, ExecState::Complete);
    assert_eq!(ended.budget.reserved_micros, 40_000, "t2's carve");
    assert_eq!(
        carved(&w, &conv.id, &t1.id),
        Some(40_000),
        "what the ended task can still spend is its own task's carve"
    );
    // Now t2 ends. Its carve leaves t1, so t1 can spend nothing at all, and
    // the conversation must let go of it in the same frame.
    let g2 = w.kernel.admit(&t2.id).unwrap();
    w.kernel
        .end_turn(
            g2,
            TurnEnd::Complete {
                reason: "reported".into(),
            },
        )
        .unwrap();
    assert_eq!(carved(&w, &t1.id, &t2.id), None, "t1 released t2's carve");
    assert_eq!(
        carved(&w, &conv.id, &t1.id),
        None,
        "and the conversation released t1's"
    );
    let c = exec(&w, &conv.id);
    assert_eq!(c.budget.reserved_micros, 0);
    assert_eq!(c.budget.available(), 100_000);
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
        .cancel_execution_with(&task.id, "discord:zeroaltitude", |end| {
            calls += 1;
            assert_eq!(end.execution.state, ExecState::Cancelled);
            Ok(vec![])
        })
        .unwrap();
    w.kernel
        .cancel_execution_with(&task.id, "discord:zeroaltitude", |_| {
            panic!("cancelled once")
        })
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

/// A task's call in flight when the process died is booked at the first
/// tick after the restart (theseus-f3wr): the task's spend, and its parent's
/// through the carry, each once. The parent's carve shrinks by what was
/// booked, so its spend and reservations still add up to what they were, and
/// a second tick books nothing more.
#[test]
fn a_tasks_earlier_call_is_booked_once_in_its_parent_too() {
    let w = world();
    let (parent, g, task, _) = with_task(&w, 30_000);
    let tg = w.kernel.admit(&task.id).unwrap();
    let paid = dispatched(&w, &tg, PROVIDER_TOOL, 5_000);
    w.kernel
        .accept_completion(&completion(
            &paid.correlation_id,
            Outcome::Succeeded,
            Some(4_000),
        ))
        .unwrap();
    let call = dispatched(&w, &tg, PROVIDER_TOOL, 10_000);
    std::mem::forget((g, tg));
    let (w, _) = crash(w, KernelConfig::default());
    let before = exec(&w, &parent.id).budget;
    assert_eq!(
        (before.spent_micros, before.reserved_micros),
        (4_000, 26_000)
    );
    assert_eq!(
        w.kernel.mark_earlier_calls_unknown().unwrap(),
        vec![call.correlation_id]
    );
    let check = |w: &World| {
        let t = exec(w, &task.id).budget;
        assert_eq!(
            (t.spent_micros, t.reserved_micros, t.held_unknown_micros),
            (14_000, 0, 0),
            "the task's own spend"
        );
        let p = exec(w, &parent.id).budget;
        assert_eq!(
            (p.spent_micros, p.held_unknown_micros),
            (14_000, 0),
            "the parent's spend, counted once"
        );
        assert_eq!(carved(w, &parent.id, &task.id), Some(16_000));
        assert_eq!(p.reserved_micros, 16_000);
        assert_eq!(p.available(), before.available(), "nothing counted twice");
    };
    check(&w);
    assert!(w.kernel.mark_earlier_calls_unknown().unwrap().is_empty());
    assert!(w
        .kernel
        .reconcile(&NoEvidence)
        .unwrap()
        .marked_unknown
        .is_empty());
    check(&w);
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
        .cancel_execution(&task.id, "discord:zeroaltitude")
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
        .stop_execution(&parent.id, "discord:zeroaltitude")
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
    // The settle's first read of the parent is `lock_family`'s walk up the
    // family, before the lock (an execution's `parent` never changes, so that
    // read is sound); its second is the carry's, under the lock.
    let r = race(
        &w.kernel,
        &p,
        (kinds::EXECUTION, &parent.id, 2),
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

/// A task parks on its own wake (37b, theseus-7kg), waiting on input beside
/// it. Nothing gives a task input, so one left waiting with no wake is queued
/// in the frame that leaves it so, and its next turn ends it: a cancel of its
/// last wake, and a turn that ends as it parks after that cancel landed.
#[test]
fn a_task_left_waiting_with_no_wake_is_queued_in_that_frame() {
    let w = world();
    let (_, g, t, _) = with_task(&w, 500_000);
    w.kernel
        .end_turn(g, TurnEnd::Wait { wake: Wake::Input })
        .unwrap();
    // Its first turn sets a wake, and parks on it.
    let g = w.kernel.admit(&t.id).unwrap();
    let due = w.kernel.now_ms() + 60_000;
    let set = w
        .kernel
        .set_wake(&g, &new_id("act"), due, "look again", None, None)
        .unwrap();
    let parked = w
        .kernel
        .end_turn(g, TurnEnd::Wait { wake: Wake::Input })
        .unwrap();
    assert_eq!(
        (parked.state, parked.wake.clone(), parked.wakes.len()),
        (ExecState::Waiting, Some(Wake::Input), 1)
    );
    // The operator cancels it: the same frame queues the task.
    let (e, _) = w
        .kernel
        .cancel_wake(&t.id, &set.wake.id, "the CLI")
        .unwrap()
        .unwrap();
    assert_eq!((e.state, e.resume_pending), (ExecState::Queued, true));
    assert_eq!(exec(&w, &t.id).state, ExecState::Queued);
    let queued = rows(&w, &t.session_id, "execution.queued");
    assert_eq!(queued.last().unwrap()["why"], "wake_cancelled");
    assert_eq!(queued.last().unwrap()["wakes"][0], set.wake.id);
    // A turn that would park it with no wake left is queued instead.
    let g = w.kernel.admit(&t.id).unwrap();
    let e = w
        .kernel
        .end_turn(g, TurnEnd::Wait { wake: Wake::Input })
        .unwrap();
    assert_eq!(e.state, ExecState::Queued);
    let queued = rows(&w, &t.session_id, "execution.queued");
    assert_eq!(queued.last().unwrap()["why"], "task_unparked");
    // A conversation is not: it waits on its input.
    let (_, c, g) = running(&w);
    let call = new_id("act");
    let set = w
        .kernel
        .set_wake(&g, &call, due, "look", None, None)
        .unwrap();
    w.kernel
        .end_turn(g, TurnEnd::Wait { wake: Wake::Input })
        .unwrap();
    let (e, _) = w
        .kernel
        .cancel_wake(&c.id, &set.wake.id, "the CLI")
        .unwrap()
        .unwrap();
    assert_eq!(e.state, ExecState::Waiting);
}

/// A task of a task, which `open_task` refuses (depth one) and a later row may allow: written as a record, as a
/// store from such a build would hold it.
fn grandchild(w: &World, of: &Execution, suffix: &str) -> Execution {
    let mut g = of.clone();
    g.id = format!("{}_{suffix}", of.id);
    g.session_id = format!("{}_{suffix}", of.session_id);
    g.parent = Some(of.id.clone());
    g.created_at_ms = of.created_at_ms + 1;
    w.kernel.commit(&[exec_record(&g).unwrap()]).unwrap();
    g
}

/// The tree below a conversation (theseus-cdcwr): children before the task they belong to, siblings oldest first,
/// an ended task walked through, and nothing of another conversation's.
#[test]
fn a_task_tree_lists_each_task_after_its_own_tasks() {
    let w = world();
    let (parent, g, first, _) = with_task(&w, 30_000);
    let second = w
        .kernel
        .open_task(&g, &new_id("act"), 30_000, None, false, |_| Ok(vec![]))
        .unwrap()
        .task;
    let deep = grandchild(&w, &first, "deep");
    let deeper = grandchild(&w, &deep, "deeper");
    let ids = |v: Vec<Execution>| v.into_iter().map(|e| e.id).collect::<Vec<_>>();
    assert_eq!(
        ids(w.kernel.task_tree(&parent.id).unwrap()),
        [
            deeper.id.clone(),
            deep.id.clone(),
            first.id.clone(),
            second.id.clone()
        ]
    );
    // An ended task in the middle is walked through.
    w.kernel
        .cancel_execution_with(&deep.id, "test", |_| Ok(vec![]))
        .unwrap();
    assert_eq!(
        ids(w.kernel.task_tree(&parent.id).unwrap()),
        [deeper.id, deep.id, first.id.clone(), second.id.clone()]
    );
    // A task's own tree is below it alone.
    assert_eq!(ids(w.kernel.task_tree(&first.id).unwrap()).len(), 2);
    assert!(w.kernel.task_tree(&second.id).unwrap().is_empty());
}

// ------------------------------------------------ reports taken by name (theseus-z9mze)

/// A parent with a turn, and `n` tasks its calls opened (the first with
/// `wake_parent` when `wake` is true), none ended yet.
fn with_tasks(w: &World, n: usize, wake: bool) -> (Execution, TurnGuard, Vec<Execution>) {
    let (_, parent, g) = running(w);
    let tasks = (0..n)
        .map(|i| {
            w.kernel
                .open_task(&g, &new_id("act"), 10_000, None, wake && i == 0, |_| {
                    Ok(vec![])
                })
                .unwrap()
                .task
        })
        .collect();
    (parent, g, tasks)
}

fn names(ts: &[&Execution]) -> Vec<String> {
    ts.iter().map(|t| t.id.clone()).collect()
}

/// The parent takes the reports of two of three ended tasks by name: one
/// frame holds its record, the records built from them, and one
/// `task.reports_read` row that says it was a wait. The third stays for the
/// next turn's `take_reports`, which returns it alone.
#[test]
fn take_reports_of_takes_the_named_reports_in_one_frame() {
    let w = world();
    let (parent, g, ts) = with_tasks(&w, 3, false);
    for t in &ts {
        finish(&w, &t.id);
    }
    assert_eq!(
        exec(&w, &parent.id).reports,
        names(&[&ts[0], &ts[1], &ts[2]])
    );
    let frames = w.kernel.store().stats().unwrap().frames_appended;
    let mut built = Vec::new();
    let taken = w
        .kernel
        .take_reports_of(&g, &[ts[2].id.clone(), ts[0].id.clone()], |ids| {
            built.extend(ids.iter().cloned());
            Ok(vec![NewRecord::json(
                kinds::META,
                Some("probe.report"),
                &"written",
            )?])
        })
        .unwrap();
    assert_eq!(
        w.kernel.store().stats().unwrap().frames_appended,
        frames + 1,
        "one frame"
    );
    assert_eq!(taken.ids, names(&[&ts[0], &ts[2]]), "oldest first");
    assert_eq!(built, taken.ids);
    assert!(w
        .kernel
        .store()
        .latest_by_key(kinds::META, "probe.report")
        .unwrap()
        .is_some());
    let read = rows(&w, &parent.session_id, "task.reports_read");
    assert_eq!(read.len(), 1);
    assert_eq!(read[0]["via"], "wait");
    assert_eq!(read[0]["tasks"], json!(taken.ids));
    assert_eq!(exec(&w, &parent.id).reports, vec![ts[1].id.clone()]);
    let next = w.kernel.take_reports(&g, |_| Ok(vec![])).unwrap();
    assert_eq!(next.ids, vec![ts[1].id.clone()], "the rest, once");
    let read = rows(&w, &parent.session_id, "task.reports_read");
    assert!(
        read[1].get("via").is_none(),
        "a turn's own read is as before"
    );
    assert!(w
        .kernel
        .take_reports(&g, |_| panic!("none left"))
        .unwrap()
        .ids
        .is_empty());
}

/// A named task still running is not taken and not an error; when none of the
/// named has ended nothing is written.
#[test]
fn take_reports_of_leaves_a_running_task_and_writes_nothing_when_none_ended() {
    let w = world();
    let (parent, g, ts) = with_tasks(&w, 2, false);
    let all = [ts[0].id.clone(), ts[1].id.clone()];
    let before = w.kernel.store().last_position();
    let none = w
        .kernel
        .take_reports_of(&g, &all, |_| panic!("nothing to build"))
        .unwrap();
    assert!(none.ids.is_empty());
    assert_eq!(
        w.kernel.store().last_position(),
        before,
        "no report: no frame"
    );
    finish(&w, &ts[0].id);
    // The other ended task is not named, and the one named has not ended.
    let before = w.kernel.store().last_position();
    let none = w
        .kernel
        .take_reports_of(&g, &[ts[1].id.clone()], |_| panic!("nothing to build"))
        .unwrap();
    assert!(none.ids.is_empty());
    assert_eq!(w.kernel.store().last_position(), before);
    assert_eq!(exec(&w, &parent.id).reports, vec![ts[0].id.clone()]);
    // Named together, the ended one comes and the running one is missing.
    let some = w.kernel.take_reports_of(&g, &all, |_| Ok(vec![])).unwrap();
    assert_eq!(some.ids, vec![ts[0].id.clone()]);
    assert!(exec(&w, &parent.id).reports.is_empty());
}

/// A wait names its turn by number, so a number that is not the execution's
/// running turn's takes nothing and writes nothing (the stack review's N7): a
/// wait that outlived its turn cannot bind nodes to a finished one.
#[test]
fn take_reports_in_turn_refuses_a_turn_that_is_not_the_running_one() {
    let w = world();
    let (parent, g, ts) = with_tasks(&w, 1, false);
    finish(&w, &ts[0].id);
    let before = w.kernel.store().last_position();
    for stale in [g.turn - 1, g.turn + 1] {
        let refused = w
            .kernel
            .take_reports_in_turn(&parent.id, stale, &[ts[0].id.clone()], |_| {
                panic!("nothing to build")
            });
        assert!(refused.is_err(), "turn {stale} is not the running turn");
    }
    assert_eq!(w.kernel.store().last_position(), before, "nothing written");
    assert_eq!(exec(&w, &parent.id).reports, vec![ts[0].id.clone()]);
    let taken = w
        .kernel
        .take_reports_in_turn(&parent.id, g.turn, &[ts[0].id.clone()], |_| Ok(vec![]))
        .unwrap();
    assert_eq!(taken.ids, vec![ts[0].id.clone()]);
}

/// A taken report's wake goes with it, so the parent is not queued for a turn
/// with nothing to read; the wake of a report not named stays, with its turn.
#[test]
fn take_reports_of_drops_the_wakes_of_what_it_takes_and_keeps_the_rest() {
    let w = world();
    let (parent, g, ts) = with_tasks(&w, 2, true);
    // Only the first asked for a turn; the second is a plain report.
    finish(&w, &ts[0].id);
    finish(&w, &ts[1].id);
    assert_eq!(exec(&w, &parent.id).report_wakes, vec![ts[0].id.clone()]);
    let plain = w
        .kernel
        .take_reports_of(&g, &[ts[1].id.clone()], |_| Ok(vec![]))
        .unwrap();
    assert!(plain.woke.is_empty());
    assert_eq!(
        exec(&w, &parent.id).report_wakes,
        vec![ts[0].id.clone()],
        "the wake of a report not named stays"
    );
    let waker = w
        .kernel
        .take_reports_of(&g, &[ts[0].id.clone()], |_| Ok(vec![]))
        .unwrap();
    assert_eq!(waker.woke, vec![ts[0].id.clone()]);
    let p = exec(&w, &parent.id);
    assert!(p.reports.is_empty() && p.report_wakes.is_empty());
    let read = rows(&w, &parent.session_id, "task.reports_read");
    assert_eq!(read[1]["woke"], json!([ts[0].id.clone()]));
    // Nothing is left to wake the parent: it parks as usual.
    let p = w
        .kernel
        .end_turn(g, TurnEnd::Wait { wake: Wake::Input })
        .unwrap();
    assert_eq!(p.state, ExecState::Waiting);
}

/// The helper mark is stored on the task by the carve, read back after the
/// kernel reopens over the store, and a plain task writes no such key.
#[test]
fn a_helper_mark_is_stored_and_a_plain_task_writes_none() {
    let w = world();
    let (_, parent, g) = running(&w);
    let open = |helper: Option<String>| {
        let how = crate::tasks::Opening {
            reports_to: None,
            wake_parent: false,
            start_at_ms: None,
            helper,
        };
        w.kernel
            .open_task_at(&g, &new_id("act"), 10_000, how, |_| Ok(vec![]))
            .unwrap()
            .task
    };
    let helper = open(Some("act-fan".into()));
    let plain = open(None);
    assert_eq!(helper.helper_of(), Some("act-fan"));
    assert_eq!(plain.helper_of(), None);
    assert_eq!(exec(&w, &helper.id).helper_of(), Some("act-fan"));
    let key = |e: &Execution| {
        serde_json::to_value(e)
            .unwrap()
            .as_object()
            .unwrap()
            .contains_key("helper")
    };
    assert!(key(&exec(&w, &helper.id)));
    assert!(!key(&exec(&w, &plain.id)), "no new key on a plain task");
    assert!(!key(&exec(&w, &parent.id)));
    drop(g);
    let (w, _) = crash(w, KernelConfig::default());
    assert_eq!(exec(&w, &helper.id).helper_of(), Some("act-fan"));
    assert_eq!(exec(&w, &plain.id).helper_of(), None);
    // A record written before the field reads as not a helper.
    let mut old = serde_json::to_value(&plain).unwrap();
    old.as_object_mut().unwrap().remove("helper");
    let old: Execution = serde_json::from_value(old).unwrap();
    assert_eq!(old.helper_of(), None);
}
