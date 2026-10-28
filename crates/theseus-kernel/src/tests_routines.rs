//! A routine's firing (theseus-pvm5c): a series that comes due opens a task
//! under the routine's execution in one frame and puts the next occurrence back;
//! a skip writes `wake.skipped` and re-arms; a paused routine is never due, and
//! run now runs once and leaves the series alone, paused or not; the month's
//! reset is a `budget.reset { why: "month" }` row in the fire's frame; and a
//! restart across occurrences passes the missed ones over, counted, for one
//! catch-up run to say.

use jiff::tz::TimeZone;

use crate::kernel::*;
use crate::repeat::{Every, Repeat};
use crate::routines::{RoutineDue, RoutinePlan, RunPlan};
use crate::tests::*;
use crate::types::*;
use crate::wake_kinds::{NewWake, Purpose, TaskRun, WakeRun};

const MIN: u64 = 60_000;
/// A month's limit, and a run's carve.
const MONTH: u64 = 1_000_000;
const RUN: u64 = 100_000;

fn cfg() -> KernelConfig {
    KernelConfig {
        min_repeat_ms: MIN,
        zone: TimeZone::UTC,
        ..KernelConfig::default()
    }
}

fn exec(w: &World, id: &str) -> Execution {
    w.kernel.execution(id).unwrap().unwrap()
}

/// A routine: a conversation whose limit is the month, with a minute series
/// whose first run is due a minute from now. Its execution and wake id.
fn routine(w: &World) -> (Execution, String) {
    let e = w
        .kernel
        .open_execution(
            &new_id("ses"),
            SessionKind::Conversation,
            auth(),
            Some(MONTH),
            None,
        )
        .unwrap();
    let first = w.kernel.now_ms() + MIN;
    let set = w
        .kernel
        .create_wake(
            &e.id,
            None,
            NewWake {
                due_at_ms: first,
                note: "Morning digest".into(),
                repeat: Some(Repeat {
                    every: Every::parse("1m").unwrap(),
                    first_ms: first,
                    days: vec![],
                    until_ms: None,
                    cron: None,
                }),
                purpose: Purpose::Routine,
                run: WakeRun::Task(TaskRun {
                    brief: "summarize".into(),
                    limit_micros: RUN,
                    tools: vec![],
                }),
                ..Default::default()
            },
            "the CLI",
        )
        .unwrap();
    (exec(w, &e.id), set.wake.id)
}

fn run(reset_month: bool) -> RoutinePlan {
    RoutinePlan::Run(RunPlan {
        want_micros: RUN,
        reset_month,
        reports_to: None,
        records: vec![],
        extra: vec![],
    })
}

fn fire(
    w: &World,
    id: &str,
    plan: impl Fn(&RoutineDue) -> RoutinePlan,
) -> Vec<crate::RoutineFired> {
    w.kernel.fire_routines(id, |_, due| Ok(plan(due))).unwrap()
}

#[test]
fn a_minute_series_opens_three_tasks_one_frame_each_and_keeps_its_place() {
    let w = world_with(cfg());
    let (e, _) = routine(&w);
    assert!(Kernel::routine_due(&e, w.kernel.now_ms() + MIN));
    for n in 1..=3u64 {
        w.clock.advance(MIN);
        let before = theseus_store::frames_written_here();
        let out = fire(&w, &e.id, |_| run(false));
        assert_eq!(
            theseus_store::frames_written_here() - before,
            1,
            "run {n} is one frame"
        );
        assert_eq!(out.len(), 1);
        let task = out[0].task.as_ref().unwrap();
        assert_eq!(task.parent.as_deref(), Some(e.id.as_str()));
        assert_eq!(
            (task.budget.limit_micros, task.state),
            (RUN, ExecState::Queued)
        );
        let now = exec(&w, &e.id);
        assert_eq!(now.wakes.len(), 1, "the series is back on the list");
        assert_eq!(now.wakes[0].occurrence, 1 + n as u32);
        assert_eq!(
            now.budget.reserved_micros,
            n * RUN,
            "each run is carved from the month"
        );
        assert_eq!(now.budget.available(), MONTH - n * RUN);
    }
    assert_eq!(w.kernel.tasks(Some(&e.id)).unwrap().len(), 3);
    assert_eq!(rows(&w, &e.session_id, "wake.fired").len(), 3);
    // Nothing is due until the next minute: the scan finds nothing to fire.
    assert!(fire(&w, &e.id, |_| run(false)).is_empty());
}

#[test]
fn a_skip_writes_wake_skipped_and_puts_the_series_back() {
    let w = world_with(cfg());
    let (e, wake) = routine(&w);
    w.clock.advance(MIN);
    let out = fire(&w, &e.id, |_| RoutinePlan::Skip {
        why: "overlap",
        post: None,
        extra: vec![],
    });
    assert_eq!(out[0].skipped, Some("overlap"));
    assert!(out[0].task.is_none());
    assert!(w.kernel.tasks(Some(&e.id)).unwrap().is_empty());
    let now = exec(&w, &e.id);
    assert_eq!(now.wakes[0].id, wake);
    assert_eq!(now.wakes[0].due_at_ms, w.kernel.now_ms() + MIN);
    assert_eq!(now.budget.reserved_micros, 0, "nothing was carved");
    let skipped = rows(&w, &e.session_id, "wake.skipped");
    assert_eq!(skipped.len(), 1);
    assert_eq!(skipped[0]["why"], "overlap");
    assert!(rows(&w, &e.session_id, "wake.fired").is_empty());
}

#[test]
fn a_paused_routine_is_never_due_and_run_now_runs_once_leaving_the_series() {
    let w = world_with(cfg());
    let (e, wake) = routine(&w);
    w.kernel
        .pause_wake(&e.id, &wake, None, "the CLI")
        .unwrap()
        .unwrap();
    w.clock.advance(5 * MIN);
    let held = exec(&w, &e.id);
    assert!(
        !Kernel::routine_due(&held, w.kernel.now_ms()),
        "paused is never due"
    );
    assert!(
        w.kernel.notice_executions().unwrap().is_empty(),
        "the scan's term leaves a paused one out"
    );
    assert!(fire(&w, &e.id, |_| run(false)).is_empty());
    // Run now, while paused: one task; the wake is as it was.
    let one = w
        .kernel
        .run_routine_now(&e.id, &wake, |_, due| {
            assert!(due.manual);
            Ok(run(false))
        })
        .unwrap();
    assert!(one.manual && one.task.is_some());
    let after = exec(&w, &e.id);
    assert_eq!(after.wakes[0].paused_at_ms, held.wakes[0].paused_at_ms);
    assert_eq!(after.wakes[0].due_at_ms, held.wakes[0].due_at_ms);
    assert_eq!(after.wakes[0].occurrence, held.wakes[0].occurrence);
    assert_eq!(w.kernel.tasks(Some(&e.id)).unwrap().len(), 1);
    let fired = rows(&w, &e.session_id, "wake.fired");
    assert_eq!((fired.len(), fired[0]["manual"].as_bool()), (1, Some(true)));
}

#[test]
fn a_months_reset_is_a_budget_reset_row_in_the_fires_frame() {
    let w = world_with(cfg());
    let (mut e, _) = routine(&w);
    // The month is spent: only a run's room is reserved against nothing.
    e.budget.spent_micros = MONTH;
    w.kernel
        .store()
        .append(&[
            theseus_store::NewRecord::json(theseus_store::kinds::EXECUTION, Some(&e.id), &e)
                .unwrap(),
        ])
        .unwrap();
    w.clock.advance(MIN);
    let seen = std::cell::RefCell::new(None);
    let out = w
        .kernel
        .fire_routines(&e.id, |_, due| {
            *seen.borrow_mut() = Some((due.available_micros, due.available_after_reset_micros));
            Ok(run(true))
        })
        .unwrap();
    assert_eq!(
        *seen.borrow(),
        Some((0, MONTH)),
        "the kernel says what a reset would leave"
    );
    assert!(out[0].reset_month && out[0].task.is_some());
    let now = exec(&w, &e.id);
    assert_eq!(now.budget.spent_micros, 0);
    assert_eq!(now.budget.resets, 1);
    assert_eq!(now.budget.reserved_micros, RUN);
    let resets = rows(&w, &e.session_id, "budget.reset");
    assert_eq!(resets.len(), 1);
    assert_eq!(resets[0]["why"], "month");
    assert_eq!(
        resets[0]["spent_before_usd"].as_f64(),
        Some(micros_to_usd(MONTH))
    );
}

#[test]
fn a_restart_across_occurrences_passes_two_over_for_one_catch_up_to_say() {
    let w = world_with(cfg());
    let (e, wake) = routine(&w);
    // The daemon is down for three minutes, across three occurrences.
    w.clock.advance(3 * MIN);
    let (w, _) = crash(w, cfg());
    let seen = std::cell::RefCell::new(None);
    let out = w
        .kernel
        .fire_routines(&e.id, |_, due| {
            *seen.borrow_mut() = Some((due.fired.missed, due.fired.while_down));
            Ok(run(false))
        })
        .unwrap();
    assert_eq!(
        *seen.borrow(),
        Some((2, true)),
        "two missed while the daemon was down"
    );
    assert_eq!(out.len(), 1, "one catch-up, never a burst");
    assert_eq!(w.kernel.tasks(Some(&e.id)).unwrap().len(), 1);
    let now = exec(&w, &e.id);
    assert_eq!(now.wakes[0].id, wake);
    assert_eq!(
        now.wakes[0].occurrence, 4,
        "numbered past the two it passed over"
    );
    assert_eq!(now.wakes[0].due_at_ms, w.kernel.now_ms() + MIN);
    let row = &rows(&w, &e.session_id, "wake.fired")[0];
    assert_eq!(
        (row["missed"].as_u64(), row["while_down"].as_bool()),
        (Some(2), Some(true))
    );
}

/// theseus-rrqmr: a routine's fire goes through the task depth bound. A
/// routine's series can be set on any execution that has not ended
/// (`create_wake` asks nothing about its kind), a task included; firing it
/// there opens nothing and refuses with `TaskDepth`, and the fire writes
/// nothing at all. Before the fix `routines.rs` called `carve_task` with no
/// depth argument, so the fire carved a task under a task — a chain the
/// depth bound could say nothing about.
#[test]
fn a_routine_fired_in_a_task_refuses_at_the_depth_bound() {
    let w = world_with(cfg());
    let (_, _parent, g) = running(&w);
    let task = w
        .kernel
        .open_task(&g, &new_id("act"), 10 * RUN, None, false, |_| Ok(vec![]))
        .unwrap()
        .task;
    let first = w.kernel.now_ms() + MIN;
    w.kernel
        .create_wake(
            &task.id,
            None,
            NewWake {
                due_at_ms: first,
                note: "a routine where no routine belongs".into(),
                repeat: Some(Repeat {
                    every: Every::parse("1m").unwrap(),
                    first_ms: first,
                    days: vec![],
                    until_ms: None,
                    cron: None,
                }),
                purpose: Purpose::Routine,
                run: WakeRun::Task(TaskRun {
                    brief: "summarize".into(),
                    limit_micros: RUN,
                    tools: vec![],
                }),
                ..Default::default()
            },
            "the CLI",
        )
        .unwrap();
    w.clock.advance(MIN);
    let at = w.kernel.store().last_position();
    let err = w
        .kernel
        .fire_routines(&task.id, |_, _| Ok(run(false)))
        .unwrap_err();
    assert!(
        matches!(
            err.downcast_ref::<KernelError>(),
            Some(KernelError::TaskDepth { .. })
        ),
        "{err:#}"
    );
    assert_eq!(
        w.kernel.store().last_position(),
        at,
        "a refused fire writes nothing"
    );
    assert!(
        w.kernel.tasks(Some(&task.id)).unwrap().is_empty(),
        "and opens nothing"
    );
    // `run_routine_now` is the same bound: a person asking changes nothing.
    let wake = exec(&w, &task.id).wakes[0].id.clone();
    let err = w
        .kernel
        .run_routine_now(&task.id, &wake, |_, _| Ok(run(false)))
        .unwrap_err();
    assert!(
        matches!(
            err.downcast_ref::<KernelError>(),
            Some(KernelError::TaskDepth { .. })
        ),
        "{err:#}"
    );
    assert_eq!(w.kernel.store().last_position(), at);
    drop(g);
}
