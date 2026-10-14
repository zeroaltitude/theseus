//! The owner's verbs on wakes and a notice's firing (theseus-8ux6x): a
//! reminder fires in one frame with no turn, even while its conversation's
//! turn runs; snooze, pause and resume are one frame each, whatever the
//! conversation is doing; a series that is paused keeps its place; a task or a
//! probe is refused with words; the caps and the horizon hold.

use jiff::tz::TimeZone;
use serde_json::json;

use crate::kernel::*;
use crate::outbox::Post;
use crate::repeat::{Every, Repeat};
use crate::tests::*;
use crate::types::*;
use crate::wake_kinds::{NewWake, Purpose, TaskRun, WakeLimits, WakeProbe, WakeRun};
use crate::wake_owner::{reminder, NoticePost};

const MIN: u64 = 60_000;
const DAY: u64 = 24 * 60 * MIN;

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

/// What the core would stage for a fired reminder: its note, to a place.
fn post_of(e: &Execution, f: &crate::wakes::FiredWake) -> anyhow::Result<NoticePost> {
    Ok(NoticePost {
        post: Some(Post {
            session_id: e.session_id.clone(),
            execution_id: e.id.clone(),
            target: "discord:dm:1".into(),
            body: json!({"kind": "reminder", "text": f.wake.note}),
            retry_class: RetryClass::SafeToRepeat,
        }),
        extra: vec![],
    })
}

/// A conversation that parked on input, with a reminder due `in_ms` from now.
fn with_reminder(w: &World, in_ms: u64) -> (Execution, String) {
    let (_, e, g) = running(w);
    w.kernel
        .end_turn(g, TurnEnd::Wait { wake: Wake::Input })
        .unwrap();
    let due = w.kernel.now_ms() + in_ms;
    let set = w
        .kernel
        .create_wake(&e.id, None, reminder(due, "stretch"), "the CLI")
        .unwrap();
    assert!(set.set);
    (exec(w, &e.id), set.wake.id)
}

#[test]
fn a_reminder_fires_in_one_frame_with_no_turn_while_its_conversations_turn_runs() {
    let w = world_with(cfg());
    // The turn is running (it holds the execution), and the reminder is due.
    let (_, e, g) = running(&w);
    let due = w.kernel.now_ms() + 10_000;
    w.kernel
        .create_wake(&e.id, None, reminder(due, "stretch"), "the CLI")
        .unwrap();
    w.clock.advance(11_000);
    // The turn-wake scan has nothing to queue: a notice is not a turn.
    assert!(w.kernel.reconcile(&NoEvidence).unwrap().woke_due.is_empty());
    assert_eq!(
        w.kernel.notice_executions().unwrap().len(),
        1,
        "the notice scan finds it in a running execution"
    );
    let before = theseus_store::frames_written_here();
    let sent = w.kernel.fire_notices(&e.id, post_of).unwrap();
    assert_eq!(
        theseus_store::frames_written_here() - before,
        1,
        "the wake leaves, its row and its post: one frame"
    );
    assert_eq!(sent.len(), 1);
    assert!(sent[0].post.is_some());
    assert_eq!(sent[0].fired.late_ms, 1_000);
    let after = exec(&w, &e.id);
    assert!(after.wakes.is_empty(), "a one-shot reminder leaves");
    assert_eq!(
        after.state,
        ExecState::Running,
        "no turn was queued or taken"
    );
    assert_eq!(after.turns, e.turns, "no turn");
    let row = &rows(&w, &e.session_id, "wake.fired")[0];
    assert_eq!(row["notice"], true);
    assert_eq!(row["purpose"], "remind");
    // Nothing is due now: a second scan writes nothing.
    let before = theseus_store::frames_written_here();
    assert!(w.kernel.fire_notices(&e.id, post_of).unwrap().is_empty());
    assert_eq!(theseus_store::frames_written_here(), before);
    w.kernel
        .end_turn(g, TurnEnd::Wait { wake: Wake::Input })
        .unwrap();
}

#[test]
fn a_turn_that_takes_its_wakes_leaves_a_notice_alone() {
    let w = world_with(cfg());
    let (_, e, g) = running(&w);
    let call = new_id("act");
    let due = w.kernel.now_ms() + 5_000;
    // A check-in and a reminder, both due.
    w.kernel
        .set_wake(&g, &call, due, "check the build", None, None)
        .unwrap();
    w.kernel
        .create_wake(&e.id, None, reminder(due, "stretch"), "the CLI")
        .unwrap();
    w.clock.advance(6_000);
    let taken = w.kernel.take_wakes(&g, |_| Ok(vec![])).unwrap();
    assert_eq!(taken.len(), 1, "the turn takes the check-in alone");
    assert_eq!(taken[0].wake.note, "check the build");
    assert_eq!(
        exec(&w, &e.id).wakes.len(),
        1,
        "the reminder waits for its own scan"
    );
}

#[test]
fn a_paused_check_in_is_not_due_and_a_turn_leaves_it_until_it_is_resumed() {
    let w = world_with(cfg());
    let (_, e, g) = running(&w);
    let call = new_id("act");
    let due = w.kernel.now_ms() + 5_000;
    let set = w
        .kernel
        .set_wake(&g, &call, due, "check the build", None, None)
        .unwrap();
    let id = set.wake.id;
    let paused = w
        .kernel
        .pause_wake(&e.id, &id, None, "the CLI")
        .unwrap()
        .unwrap();
    assert!(paused.wake.paused_at_ms.is_some());
    w.clock.advance(6_000);
    let now = w.kernel.now_ms();
    assert!(
        !crate::wakes::wake_due(&exec(&w, &e.id), now),
        "a paused check-in is not due"
    );
    let taken = w.kernel.take_wakes(&g, |_| Ok(vec![])).unwrap();
    assert!(
        taken.is_empty(),
        "a turn leaves a paused check-in: {taken:?}"
    );
    assert_eq!(exec(&w, &e.id).wakes.len(), 1, "it is still held");
    let resumed = w
        .kernel
        .resume_wake(&e.id, &id, "the CLI")
        .unwrap()
        .unwrap();
    assert_eq!(resumed.wake.paused_at_ms, None);
    let at = resumed.wake.due_at_ms.max(now);
    assert!(
        crate::wakes::wake_due(&exec(&w, &e.id), at),
        "resumed, it is due again"
    );
}

#[test]
fn a_paused_series_is_passed_over_and_resume_keeps_its_numbering() {
    let w = world_with(cfg());
    let (_, e, g) = running(&w);
    w.kernel
        .end_turn(g, TurnEnd::Wait { wake: Wake::Input })
        .unwrap();
    let first = w.kernel.now_ms() + DAY;
    let repeat = Repeat {
        every: Every::parse("1d").unwrap(),
        first_ms: first,
        days: vec![],
        until_ms: None,
    };
    let set = w
        .kernel
        .create_wake(
            &e.id,
            None,
            NewWake {
                due_at_ms: first,
                note: "water the plants".into(),
                repeat: Some(repeat),
                purpose: Purpose::Remind,
                run: WakeRun::Notice,
                ..Default::default()
            },
            "the CLI",
        )
        .unwrap();
    let id = set.wake.id.clone();
    assert_eq!(set.wake.occurrence, 1);
    // Paused on day 0, held through days 1, 2 and 3.
    let paused = w
        .kernel
        .pause_wake(&e.id, &id, None, "the CLI")
        .unwrap()
        .unwrap();
    assert!(paused.wake.paused_at_ms.is_some());
    w.clock.advance(3 * DAY + 60 * MIN);
    assert!(
        w.kernel.notice_executions().unwrap().is_empty(),
        "a paused one is not scanned"
    );
    assert!(w
        .kernel
        .fire_notices(&e.id, post_of)
        .unwrap()
        .iter()
        .all(|n| n.fired.wake.id != id));
    // Resume: the next occurrence after now, numbered by its place (#4).
    let resumed = w
        .kernel
        .resume_wake(&e.id, &id, "the CLI")
        .unwrap()
        .unwrap();
    assert_eq!(resumed.wake.paused_at_ms, None);
    assert_eq!(resumed.wake.occurrence, 4, "days 1, 2 and 3 passed over");
    assert!(resumed.wake.due_at_ms > w.kernel.now_ms());
    // It fires at that time as #4, and puts #5 back.
    w.clock.set(resumed.wake.due_at_ms);
    let sent = w.kernel.fire_notices(&e.id, post_of).unwrap();
    let mine = sent.iter().find(|n| n.fired.wake.id == id).unwrap();
    assert_eq!(mine.fired.wake.occurrence, 4);
    let back = exec(&w, &e.id)
        .wakes
        .into_iter()
        .find(|x| x.id == id)
        .unwrap();
    assert_eq!(back.occurrence, 5);
}

#[test]
fn pause_until_moves_a_series_past_the_time_and_counts_what_it_skipped() {
    let w = world_with(cfg());
    let (e, _) = with_reminder(&w, DAY);
    let first = w.kernel.now_ms() + DAY;
    let set = w
        .kernel
        .create_wake(
            &e.id,
            None,
            NewWake {
                due_at_ms: first,
                note: "water the plants".into(),
                repeat: Some(Repeat {
                    every: Every::parse("1d").unwrap(),
                    first_ms: first,
                    days: vec![],
                    until_ms: None,
                }),
                purpose: Purpose::Remind,
                run: WakeRun::Notice,
                ..Default::default()
            },
            "the CLI",
        )
        .unwrap();
    // Until day 3.5: occurrences #1, #2 and #3 pass, #4 is next.
    let until = first + 2 * DAY + DAY / 2;
    let moved = w
        .kernel
        .pause_wake(&e.id, &set.wake.id, Some(until), "the CLI")
        .unwrap()
        .unwrap();
    assert_eq!(moved.wake.due_at_ms, first + 3 * DAY);
    assert_eq!(moved.wake.occurrence, 4);
    assert_eq!(moved.wake.paused_at_ms, None, "moved, not held");
}

#[test]
fn a_snooze_is_one_frame_and_sets_a_fired_reminder_again() {
    let w = world_with(cfg());
    let (e, id) = with_reminder(&w, 10_000);
    // Pending: it moves.
    let before = theseus_store::frames_written_here();
    let moved = w
        .kernel
        .snooze_wake(&e.id, &id, w.kernel.now_ms() + 20_000, false, "the CLI")
        .unwrap()
        .unwrap();
    assert_eq!(theseus_store::frames_written_here() - before, 1);
    assert_eq!(moved.wake.id, id);
    w.clock.advance(21_000);
    let fired = w.kernel.fire_notices(&e.id, post_of).unwrap();
    assert_eq!(fired.len(), 1);
    assert!(exec(&w, &e.id).wakes.is_empty());
    // Fired: a snooze sets it again from its record, in one frame.
    let before = theseus_store::frames_written_here();
    let again = w
        .kernel
        .snooze_wake(&e.id, &id, w.kernel.now_ms() + 10 * MIN, true, "a button")
        .unwrap()
        .unwrap();
    assert_eq!(theseus_store::frames_written_here() - before, 1);
    assert_eq!(again.wake.note, "stretch");
    assert_eq!(again.wake.purpose, Purpose::Remind);
    assert!(again.wake.run.is_notice());
    assert_eq!(exec(&w, &e.id).wakes.len(), 1);
    let rows = rows(&w, &e.session_id, "wake.snoozed");
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[1]["fired"], true);
    // And it fires again.
    w.clock.advance(11 * MIN);
    assert_eq!(w.kernel.fire_notices(&e.id, post_of).unwrap().len(), 1);
    // A name that is neither pending nor fired is no wake.
    assert!(w
        .kernel
        .snooze_wake(&e.id, "wak_nothing", w.kernel.now_ms() + MIN, false, "x")
        .unwrap()
        .is_none());
}

#[test]
fn a_task_is_refused_with_words_and_a_probe_is_a_watch() {
    let w = world_with(cfg());
    let (e, _) = with_reminder(&w, DAY);
    {
        let (run, words) = (WakeRun::Task(TaskRun::default()), "routines row");
        let before = theseus_store::frames_written_here();
        let err = w
            .kernel
            .create_wake(
                &e.id,
                None,
                NewWake {
                    due_at_ms: w.kernel.now_ms() + MIN,
                    note: "later".into(),
                    run,
                    ..Default::default()
                },
                "the CLI",
            )
            .unwrap_err()
            .to_string();
        assert!(
            err.contains("not built yet") && err.contains(words),
            "{err}"
        );
        assert_eq!(
            theseus_store::frames_written_here(),
            before,
            "nothing written"
        );
    }
    // A probe is a watch since theseus-f2r2b: it is set, in one frame.
    let before = theseus_store::frames_written_here();
    let set = w
        .kernel
        .create_wake(
            &e.id,
            None,
            NewWake {
                due_at_ms: w.kernel.now_ms() + MIN,
                note: "later".into(),
                purpose: crate::wake_kinds::Purpose::Watch,
                run: WakeRun::Probe(WakeProbe::default()),
                ..Default::default()
            },
            "the CLI",
        )
        .unwrap();
    assert!(set.wake.probe().is_some());
    assert_eq!(theseus_store::frames_written_here(), before + 1);
}

#[test]
fn the_horizon_and_the_caps_hold() {
    let w = world_with(KernelConfig {
        wakes: WakeLimits {
            max_wakes: 2,
            max_reminders: 3,
            ..WakeLimits::default()
        },
        ..cfg()
    });
    let (_, e, g) = running(&w);
    let now = w.kernel.now_ms();
    // 365 days is accepted, 366 refused.
    assert!(w
        .kernel
        .create_wake(&e.id, None, reminder(now + 365 * DAY, "a year"), "x")
        .is_ok());
    let err = w
        .kernel
        .create_wake(&e.id, None, reminder(now + 366 * DAY + 1, "too far"), "x")
        .unwrap_err()
        .to_string();
    assert!(err.contains("at most 365 days"), "{err}");
    // Reminders are capped over the daemon: 3, and a second conversation
    // counts the first's.
    w.kernel
        .create_wake(&e.id, None, reminder(now + DAY, "two"), "x")
        .unwrap();
    let (_, other, _g2) = running(&w);
    w.kernel
        .create_wake(&other.id, None, reminder(now + DAY, "three"), "x")
        .unwrap();
    let err = w
        .kernel
        .create_wake(&other.id, None, reminder(now + DAY, "four"), "x")
        .unwrap_err()
        .to_string();
    assert!(err.contains("already holds 3 pending reminders"), "{err}");
    // Check-ins are capped per conversation, and reminders do not count there.
    for i in 0..2 {
        w.kernel
            .set_wake(&g, &new_id("act"), now + DAY + i, "check", None, None)
            .unwrap();
    }
    let err = w
        .kernel
        .set_wake(&g, &new_id("act"), now + DAY, "check", None, None)
        .unwrap_err();
    assert!(matches!(
        err.downcast_ref::<KernelError>(),
        Some(KernelError::TooManyWakes { max: 2 })
    ));
}
