//! The day ceiling's notify mode and the burn-rate brake (theseus-9e02; the
//! owner, 2026-10-11: tokens are less cautious, so the ceiling warns and only
//! a runaway stops). Under notify the ceiling refuses nothing and warns once
//! a level a day; the runaway line stops the day, sticky until midnight; the
//! stop mode is the money row's, tested in `tests_day_ceiling.rs`; a session
//! whose settled spend in ten minutes reaches its line is stopped, and an
//! approved question starts its window empty.

use crate::day_ceiling::{level_of, BURN_WINDOW_MS};
use crate::kernel::*;
use crate::tests::*;
use crate::types::*;

/// A world whose day warns at `ceiling` micros and stops at `runaway`, with a
/// warning every `step`, in a zone three hours east of UTC.
fn notify_world(ceiling: Micros, runaway: Micros, step: Micros) -> World {
    world_with(KernelConfig {
        daily_ceiling_micros: ceiling,
        daily_notify: true,
        daily_runaway_micros: runaway,
        daily_step_micros: step,
        zone: crate::repeat::TimeZone::posix("XST-3").unwrap(),
        ..KernelConfig::default()
    })
}

fn plan(w: &World, g: &TurnGuard, reserve: Micros) -> anyhow::Result<Action> {
    w.kernel.plan_action(
        g,
        &proposal(PROVIDER_TOOL),
        RetryClass::SafeToRepeat,
        None,
        reserve,
    )
}

fn settle(w: &World, a: &Action, cost: Micros) {
    w.kernel
        .accept_completion(&completion(
            &a.correlation_id,
            Outcome::Succeeded,
            Some(cost),
        ))
        .unwrap();
}

fn refusal(r: &anyhow::Result<Action>) -> Option<crate::day_ceiling::Reached> {
    match r.as_ref().err()?.downcast_ref::<KernelError>()? {
        KernelError::DayCeiling(r) => Some((**r).clone()),
        _ => None,
    }
}

fn braked(r: &anyhow::Result<Action>) -> Option<crate::day_ceiling::BurnStop> {
    match r.as_ref().err()?.downcast_ref::<KernelError>()? {
        KernelError::BurnRate(b) => Some((**b).clone()),
        _ => None,
    }
}

/// Under notify the ceiling refuses no call: a call that passes it is made
/// and settled, and the spend counts.
#[test]
fn under_notify_a_call_past_the_ceiling_is_made() {
    let w = notify_world(1_000, 100_000, 1_000);
    let (_, _, g) = running(&w);
    let a = dispatched(&w, &g, PROVIDER_TOOL, 900);
    settle(&w, &a, 900);
    let b = dispatched(&w, &g, PROVIDER_TOOL, 900);
    settle(&w, &b, 900);
    let c = w.kernel.day_ceiling();
    let t = c.today(c.now());
    assert_eq!(t.spent, 1_800);
    assert!(
        t.past() && !t.reached(),
        "past the ceiling, and nothing refused"
    );
}

/// The warning comes at the ceiling and at each step after it, once each a
/// day, the highest unannounced level at a check; none below the ceiling, none
/// under stop, and a restart's read of today's levels keeps it from saying
/// one twice.
#[test]
fn the_warning_comes_at_the_ceiling_and_each_step_once() {
    let w = notify_world(1_000, 100_000, 1_000);
    let c = w.kernel.day_ceiling();
    let now = c.now();
    assert!(c.take_warning(now).is_none(), "below the ceiling");
    c.book(now, 999);
    assert!(c.take_warning(now).is_none());
    c.book(now, 1);
    let first = c.take_warning(now).expect("at the ceiling");
    assert_eq!((first.level, first.spent, first.limit), (1, 1_000, 1_000));
    assert!(c.take_warning(now).is_none(), "once");
    c.book(now, 999);
    assert!(c.take_warning(now).is_none(), "between steps");
    c.book(now, 1);
    assert_eq!(c.take_warning(now).expect("a step after").level, 2);
    // One jump past three steps says the highest, once.
    c.book(now, 3_000);
    assert_eq!(c.take_warning(now).expect("the highest").level, 5);
    assert!(c.take_warning(now).is_none());
    // The next local day starts again.
    let tomorrow = now + 24 * 3_600_000;
    c.book(tomorrow, 1_000);
    assert_eq!(c.take_warning(tomorrow).expect("a new day").level, 1);
    // A restart's read keeps today's levels.
    let w2 = notify_world(1_000, 100_000, 1_000);
    let c2 = w2.kernel.day_ceiling();
    c2.seed(
        c2.now(),
        &crate::day_ceiling::Seed {
            spent: 2_500,
            warned: 2,
            ..Default::default()
        },
    );
    assert!(c2.take_warning(c2.now()).is_none(), "level 2 was said");
    c2.book(c2.now(), 500);
    assert_eq!(c2.take_warning(c2.now()).expect("level 3").level, 3);
    // Under stop, no warning: its refusal has its own notice.
    let w3 = world_with(KernelConfig {
        daily_ceiling_micros: 1_000,
        ..KernelConfig::default()
    });
    let c3 = w3.kernel.day_ceiling();
    c3.book(c3.now(), 5_000);
    assert!(c3.take_warning(c3.now()).is_none());
    assert_eq!(level_of(999, 1_000, 1_000), 0);
    assert_eq!(level_of(1_000, 1_000, 0), 1);
    assert_eq!(level_of(9_999, 1_000, 0), 1);
}

/// The runaway line stops the day under notify: a call whose reservation
/// would pass it is refused with the runaway's words, nothing written, and the
/// stop is sticky: a smaller call is refused too, until local midnight; the
/// ceiling itself is never sticky.
#[test]
fn the_runaway_line_stops_the_day_and_holds_until_midnight() {
    let w = notify_world(1_000, 5_000, 1_000);
    // 2026-10-08 23:00:00 at UTC+3.
    let before = 1_791_489_600_000;
    w.clock.set(before);
    let (_, _, g) = running(&w);
    let a = dispatched(&w, &g, PROVIDER_TOOL, 4_000);
    settle(&w, &a, 4_000);
    let frames = w.kernel.store().stats().unwrap().frames_appended;
    let r = plan(&w, &g, 1_001);
    let reached = refusal(&r).expect("the runaway line refuses");
    assert!(reached.runaway && reached.limit == 5_000, "{reached:?}");
    assert_eq!(
        w.kernel.store().stats().unwrap().frames_appended,
        frames,
        "a refusal writes nothing"
    );
    let words = r.unwrap_err().to_string();
    assert!(
        words.starts_with("today's spend reached the $0.005 daily runaway line")
            && words.contains("`[kernel] daily_runaway_usd` applies at the daemon's next start"),
        "{words}"
    );
    // Sticky: a call that would fit is refused too.
    let small = plan(&w, &g, 10);
    let again = refusal(&small).expect("sticky");
    assert!(again.runaway && again.since.is_some());
    assert!(small
        .unwrap_err()
        .to_string()
        .contains("daily runaway line stopped model calls"));
    let c = w.kernel.day_ceiling();
    assert!(c.today(c.now()).reached());
    // Local midnight lifts it (2026-10-09 00:01 at UTC+3).
    w.clock.set(before + 3_600_000 + 60_000);
    plan(&w, &g, 10).expect("the day turned");
    // A runaway line of 0 never stops the day.
    let w0 = notify_world(1_000, 0, 1_000);
    let (_, _, g0) = running(&w0);
    let b = dispatched(&w0, &g0, PROVIDER_TOOL, 90_000);
    settle(&w0, &b, 90_000);
    plan(&w0, &g0, 10).expect("still nothing");
}

/// A session whose settled spend in the last ten minutes reaches the line is
/// stopped at its next call, nothing written; the same spend over eleven
/// minutes is not; another session is not; and an approved resume starts the
/// window empty, its spend kept.
#[test]
fn a_session_that_burns_its_line_in_ten_minutes_is_stopped_and_resumes_on_the_answer() {
    let w = world_with(KernelConfig {
        burn_line_micros: 1_000,
        ..KernelConfig::default()
    });
    let (_, e, g) = running(&w);
    let a = dispatched(&w, &g, PROVIDER_TOOL, 600);
    settle(&w, &a, 600);
    w.clock.advance(5 * 60_000);
    let b = dispatched(&w, &g, PROVIDER_TOOL, 600);
    settle(&w, &b, 399);
    plan(&w, &g, 1).expect("$999 of 1000 in the window");
    // 1 more micro reaches it.
    let c = w.kernel.day_ceiling();
    c.burn_record(c.now(), &e.id, 1);
    let frames = w.kernel.store().stats().unwrap().frames_appended;
    let r = plan(&w, &g, 1);
    let stop = braked(&r).expect("the burn-rate line stops the call");
    assert_eq!((stop.spent, stop.line), (1_000, 1_000));
    assert_eq!(w.kernel.store().stats().unwrap().frames_appended, frames);
    assert!(r.unwrap_err().to_string().contains("last 10 minutes"));
    // The first settle leaves the window 10 minutes after it was made: the
    // same $1,000 spread over eleven minutes is not a stop.
    w.clock.advance(5 * 60_000 + 1);
    assert!(
        c.burn_stop(c.now(), &e.id).is_none(),
        "only $400 is left in the window"
    );
    plan(&w, &g, 1).expect("$400 in the window: the line is not reached");
    assert_eq!(BURN_WINDOW_MS, 600_000);
}

/// The answer to a burn-rate question resumes the session: its spend stays
/// and its window starts empty; the question is a budget question, which the
/// confirm path answers.
#[test]
fn an_approved_burn_question_empties_the_window_and_keeps_the_spend() {
    let w = world_with(KernelConfig {
        burn_line_micros: 1_000,
        ..KernelConfig::default()
    });
    let (_, e, g) = running(&w);
    let a = dispatched(&w, &g, PROVIDER_TOOL, 1_000);
    settle(&w, &a, 1_000);
    let b = braked(&plan(&w, &g, 1)).expect("stopped");
    let call =
        serde_json::json!({"brake": {"spent_usd": 0.001, "line_usd": 0.001, "window_minutes": 10}});
    let q = w.kernel.ask_budget_for(&g, 0, call).unwrap();
    let (after, before) = w.kernel.reset_budget(&q.correlation_id, "owner").unwrap();
    assert_eq!(before, 1_000);
    assert_eq!(after.budget.spent_micros, 1_000, "a resume keeps the spend");
    assert_eq!(after.budget.resets, 0, "and is no reset");
    let c = w.kernel.day_ceiling();
    assert!(
        c.burn_stop(c.now(), &e.id).is_none(),
        "the window starts empty {b:?}"
    );
    let q = w.kernel.action(&q.correlation_id).unwrap().unwrap();
    assert!(
        q.resolution.as_deref().unwrap().contains("resumed"),
        "{:?}",
        q.resolution
    );
}

/// AWS stays cautious (theseus-9e02): with the session limit and the day
/// ceiling both notifying, a model call past the limit is made, but an AWS
/// hand's reservation past it is still refused, `OverBudget`, nothing
/// written, and no AWS reservation is held on the day or counted toward a
/// burn line.
#[test]
fn an_aws_reservation_past_the_limit_is_still_refused_under_the_soft_settings() {
    let w = world_with(KernelConfig {
        spend_limit_notify: true,
        daily_notify: true,
        burn_line_micros: 1_000,
        spend_limit_micros: 100_000,
        ..KernelConfig::default()
    });
    let (_, _, g) = crate::tests::following(&w);
    let frames = w.kernel.store().stats().unwrap().frames_appended;
    let r = w.kernel.plan_action(
        &g,
        &proposal("aws.hand"),
        RetryClass::SafeToRepeat,
        None,
        200_000,
    );
    assert!(
        matches!(
            r.as_ref()
                .err()
                .and_then(|e| e.downcast_ref::<KernelError>()),
            Some(KernelError::OverBudget { .. })
        ),
        "{r:?}"
    );
    assert_eq!(w.kernel.store().stats().unwrap().frames_appended, frames);
    let c = w.kernel.day_ceiling();
    assert_eq!(
        c.today(c.now()).held,
        0,
        "an AWS reservation never holds the day"
    );
    // A model call over the same limit is made: only tokens went soft.
    plan(&w, &g, 200_000).expect("a model call past the session limit notifies");
}

// A fan-out's spend counts in its parent's burn window (theseus-o1h3f.5).

/// A world whose burn line is 1,000 micros, a parent running a turn, and `n`
/// tasks of it, each admitted with a turn of its own.
fn family(n: usize) -> (World, Execution, TurnGuard, Vec<(Execution, TurnGuard)>) {
    let w = world_with(KernelConfig {
        burn_line_micros: 1_000,
        ..KernelConfig::default()
    });
    let (_, parent, g) = running(&w);
    let kids = (0..n)
        .map(|_| {
            let t = w
                .kernel
                .open_task(&g, &new_id("act"), 30_000, None, false, |_| Ok(vec![]))
                .unwrap()
                .task;
            let tg = w.kernel.admit(&t.id).unwrap();
            (t, tg)
        })
        .collect();
    (w, parent, g, kids)
}

fn window(w: &World, id: &str) -> Option<Micros> {
    let c = w.kernel.day_ceiling();
    // The line is 1,000, so a stop reads the spend exactly when reached.
    c.burn_stop(c.now(), id).map(|b| b.spent)
}

/// Two helpers, each just over half the line, trip their parent's line
/// together though neither reaches its own; the parent's next provider call
/// is refused as a burn-rate stop.
#[test]
fn two_children_trip_their_parents_burn_line() {
    let (w, parent, g, kids) = family(2);
    for (_, tg) in &kids {
        let a = dispatched(&w, tg, PROVIDER_TOOL, 600);
        settle(&w, &a, 600);
    }
    for (t, _) in &kids {
        assert_eq!(
            window(&w, &t.id),
            None,
            "600 of 1,000 in a child's own window"
        );
    }
    assert_eq!(
        window(&w, &parent.id),
        Some(1_200),
        "the parent's window holds both"
    );
    let stop = braked(&plan(&w, &g, 1)).expect("the parent's next call is refused");
    assert_eq!((stop.spent, stop.line), (1_200, 1_000));
}

/// The ancestors are walked all the way up, not one step: a record made for
/// an execution reaches every parent above it (a task's depth is one today,
/// so the chain is stored by hand).
#[test]
fn a_settled_cost_reaches_every_ancestor_to_the_top() {
    let (w, parent, _g, kids) = family(1);
    let (task, _) = &kids[0];
    // A helper under the task, written as the store would hold it.
    let mut helper = task.clone();
    helper.id = new_id("exe");
    helper.session_id = new_id("ses");
    helper.parent = Some(task.id.clone());
    helper.state = ExecState::Running;
    let rec = crate::kernel::exec_record(&helper).unwrap();
    w.kernel.commit(&[rec]).unwrap();
    w.kernel.apply_day([crate::day_ceiling::Effect::Settle {
        id: "res-x".into(),
        execution_id: helper.id.clone(),
        reserved: 700,
        actual: Some(700),
    }]);
    let read = |id: &str| w.kernel.day_ceiling().burn_len(id);
    assert_eq!(read(&helper.id), 1, "its own");
    assert_eq!(read(&task.id), 1, "its parent task's");
    assert_eq!(read(&parent.id), 1, "the conversation's");
}

/// The day's total counts a cost once: the same two settles leave
/// `Today`'s spend at 1,200, and nothing held.
#[test]
fn the_day_counts_a_childs_cost_once() {
    let (w, _parent, _g, kids) = family(2);
    for (_, tg) in &kids {
        let a = dispatched(&w, tg, PROVIDER_TOOL, 600);
        settle(&w, &a, 600);
    }
    let c = w.kernel.day_ceiling();
    let t = c.today(c.now());
    assert_eq!((t.spent, t.held), (1_200, 0));
}

/// A settle inside a transaction that fails books nothing, for the child
/// or for its parent.
#[test]
fn a_failed_frame_books_nothing_in_any_window() {
    let (w, parent, _g, kids) = family(1);
    let (task, tg) = &kids[0];
    let a = dispatched(&w, tg, PROVIDER_TOOL, 600);
    let done = completion(&a.correlation_id, Outcome::Succeeded, Some(600));
    let r: anyhow::Result<()> = w.kernel.frame(&[&task.id, &parent.id], |k| {
        k.accept_completion(&done)?;
        anyhow::bail!("the frame fails after the settle");
    });
    assert!(r.is_err());
    let c = w.kernel.day_ceiling();
    assert_eq!(c.burn_len(&task.id), 0, "nothing for the child");
    assert_eq!(c.burn_len(&parent.id), 0, "nothing for the parent");
    assert_eq!(c.today(c.now()).spent, 0);
}

/// An execution with no parent writes exactly one ring, and a task writes
/// its own and its parent's, no more.
#[test]
fn a_plain_execution_writes_one_ring() {
    let (w, parent, g, kids) = family(1);
    let a = dispatched(&w, &g, PROVIDER_TOOL, 400);
    settle(&w, &a, 400);
    let c = w.kernel.day_ceiling();
    assert_eq!(c.burn_rings(), 1, "a conversation: its own");
    assert_eq!(c.burn_len(&parent.id), 1);
    let b = dispatched(&w, &kids[0].1, PROVIDER_TOOL, 300);
    settle(&w, &b, 300);
    assert_eq!(c.burn_rings(), 2, "a task: its own and its parent's");
    assert_eq!(c.burn_len(&parent.id), 2);
}

/// Resume is the resumed execution's own window and no other: resuming a
/// parent empties the parent's, so the helpers' earlier spend does not stop
/// it again, while a helper's window stays its own (it has its own line) and
/// resuming a helper never empties its parent's, which would loosen the
/// aggregate line. Spend after the resume counts in the parent from empty.
#[test]
fn a_resume_clears_only_the_resumed_executions_window() {
    let (w, parent, g, kids) = family(2);
    for (_, tg) in &kids {
        let a = dispatched(&w, tg, PROVIDER_TOOL, 600);
        settle(&w, &a, 600);
    }
    let c = w.kernel.day_ceiling();
    c.burn_clear(&kids[0].0.id);
    assert_eq!(
        window(&w, &parent.id),
        Some(1_200),
        "resuming a child must not loosen its parent's line"
    );
    c.burn_clear(&parent.id);
    assert_eq!(
        window(&w, &parent.id),
        None,
        "the parent resumed with an empty window"
    );
    assert_eq!(c.burn_len(&kids[1].0.id), 1, "a child's window is its own");
    plan(&w, &g, 1).expect("the earlier spend does not stop the resumed parent");
    let a = dispatched(&w, &kids[1].1, PROVIDER_TOOL, 300);
    settle(&w, &a, 300);
    assert_eq!(c.burn_len(&parent.id), 1, "later spend counts from empty");
}

/// An execution stored by hand under `parent`, as the anchor test's helper is.
fn stored_under(w: &World, template: &Execution, parent: Option<String>) -> Execution {
    let mut e = template.clone();
    e.id = new_id("exe");
    e.session_id = new_id("ses");
    e.parent = parent;
    e.state = ExecState::Running;
    w.kernel
        .commit(&[crate::kernel::exec_record(&e).unwrap()])
        .unwrap();
    e
}

fn settle_for(w: &World, execution: &Execution) {
    w.kernel.apply_day([crate::day_ceiling::Effect::Settle {
        id: "res-x".into(),
        execution_id: execution.id.clone(),
        reserved: 700,
        actual: Some(700),
    }]);
}

/// The walk stops at `BURN_ANCESTORS` (8) steps above the origin: in a chain
/// of ten, the origin's 8th ancestor holds the settle and its 9th, the top of
/// the chain, does not. Both sides of the line, so neither a lower nor a
/// higher bound passes.
#[test]
fn a_settled_cost_stops_at_the_ancestor_bound() {
    let (w, parent, _g, _kids) = family(1);
    // chain[0] is the deepest, chain[9] the top.
    let mut chain: Vec<Execution> = Vec::new();
    let mut above = None;
    for _ in 0..10 {
        let e = stored_under(&w, &parent, above);
        above = Some(e.id.clone());
        chain.push(e);
    }
    chain.reverse();
    settle_for(&w, &chain[0]);
    let c = w.kernel.day_ceiling();
    assert_eq!(c.burn_len(&chain[0].id), 1, "its own");
    assert_eq!(
        c.burn_len(&chain[8].id),
        1,
        "the 8th ancestor, the last reached"
    );
    assert_eq!(
        c.burn_len(&chain[9].id),
        0,
        "the 9th ancestor, past the bound"
    );
    assert_eq!(c.burn_rings(), 9, "the origin and eight ancestors, no more");
}

/// A parent cycle (a corrupt store's) ends the walk at the first repeat:
/// each execution of a 2-cycle holds the settle once, not once per lap. An
/// unguarded walk still ends at the depth bound, so the failure is a count,
/// not a hang.
#[test]
fn a_parent_cycle_ends_the_walk_at_the_first_repeat() {
    let (w, parent, _g, _kids) = family(1);
    let a = stored_under(&w, &parent, None);
    let b = stored_under(&w, &parent, Some(a.id.clone()));
    let mut a_cycled = a.clone();
    a_cycled.parent = Some(b.id.clone());
    w.kernel
        .commit(&[crate::kernel::exec_record(&a_cycled).unwrap()])
        .unwrap();
    settle_for(&w, &a);
    let c = w.kernel.day_ceiling();
    assert_eq!(c.burn_len(&a.id), 1, "a, once");
    assert_eq!(c.burn_len(&b.id), 1, "b, once");
    assert_eq!(c.burn_rings(), 2);
}
