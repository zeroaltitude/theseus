//! A failure's dismissal and a failed task's retry (theseus-7zgsm): each one
//! frame, the dismissal on the execution's own record so a crash keeps it,
//! and a retry that carves no more than the failed task had, links to it,
//! and dismisses it in the same frame.

use theseus_store::NewRecord;

use crate::kernel::*;
use crate::tests::*;
use crate::types::*;

fn exec(w: &World, id: &str) -> Execution {
    w.kernel.execution(id).unwrap().unwrap()
}

/// A parent with a turn, and a task it opened that then failed: the parent's
/// guard, the parent, and the failed task.
fn with_failed_task(w: &World, want: Micros) -> (TurnGuard, Execution, Execution) {
    let (_, parent, g) = running(w);
    let call = new_id("act");
    let t = w
        .kernel
        .open_task(&g, &call, want, Some("discord:dm:1".into()), true, |_| {
            Ok(vec![])
        })
        .unwrap();
    let failed = fail(w, &t.task.id, "the provider refused the request (400)");
    (g, exec(w, &parent.id), failed)
}

fn fail(w: &World, id: &str, reason: &str) -> Execution {
    let guard = w.kernel.admit(id).unwrap();
    w.kernel
        .end_turn(
            guard,
            TurnEnd::Fail {
                reason: reason.into(),
            },
        )
        .unwrap()
}

fn retry(w: &World, failed: &str) -> anyhow::Result<crate::retry::RetryOpen> {
    w.kernel
        .open_retry(failed, &new_id("act"), |_, _| Ok(vec![]))
}

/// A dismissal is one frame, on the execution's own record, with its row; it
/// survives a crash, and dismissing again writes nothing and answers the
/// first time.
#[test]
fn a_dismissal_is_one_frame_on_the_record_and_survives_a_crash() {
    let w = world();
    let (_g, _, failed) = with_failed_task(&w, 30_000);
    assert_eq!(failed.state, ExecState::Failed);
    assert_eq!(failed.dismissal(), None);

    w.clock.advance(5_000);
    let before = theseus_store::frames_written_here();
    let d = w.kernel.dismiss_execution(&failed.id).unwrap();
    assert_eq!(
        theseus_store::frames_written_here() - before,
        1,
        "one frame"
    );
    assert!(d.written);
    let now = exec(&w, &failed.id);
    assert_eq!(now.dismissal(), Some(d.at_ms));
    let row = &rows(&w, &failed.session_id, "execution.dismissed")[0];
    assert_eq!(row["execution_id"], failed.id);
    assert_eq!(row["state"], "failed");
    assert_eq!(row["reason"], "the provider refused the request (400)");

    // Again: nothing written, the first time answered.
    w.clock.advance(5_000);
    let before = theseus_store::frames_written_here();
    let again = w.kernel.dismiss_execution(&failed.id).unwrap();
    assert_eq!(theseus_store::frames_written_here(), before, "no frame");
    assert_eq!(
        (again.at_ms, again.written),
        (d.at_ms, false),
        "the first dismissal stands"
    );

    // A crash and a restart: the record says so.
    let (w, _) = crash(w, KernelConfig::default());
    assert_eq!(exec(&w, &failed.id).dismissal(), Some(d.at_ms));
}

/// Only a failure is dismissed, and a refusal writes nothing.
#[test]
fn only_a_failure_is_dismissed() {
    let w = world();
    let (_, parent, g) = running(&w);
    let before = theseus_store::frames_written_here();
    let e = w.kernel.dismiss_execution(&parent.id).unwrap_err();
    assert!(
        matches!(
            e.downcast_ref::<crate::retry::RetryError>(),
            Some(crate::retry::RetryError::NotAFailure { .. })
        ),
        "{e:#}"
    );
    assert_eq!(
        theseus_store::frames_written_here(),
        before,
        "nothing written"
    );
    let e = w.kernel.dismiss_execution("exe_nope").unwrap_err();
    assert!(matches!(
        e.downcast_ref::<KernelError>(),
        Some(KernelError::UnknownExecution(_))
    ));
    drop(g);
}

/// A dismissal belongs to the turn it dismissed: an execution that took
/// another turn and failed again asks again, and one out of its failure
/// asks no one.
#[test]
fn a_dismissal_ends_with_the_turn_it_dismissed() {
    let w = world();
    let (_g, _, failed) = with_failed_task(&w, 30_000);
    w.kernel.dismiss_execution(&failed.id).unwrap();
    let mut e = exec(&w, &failed.id);
    assert!(e.dismissal().is_some());
    e.turns += 1;
    assert_eq!(e.dismissal(), None, "another turn, another failure");
    e.turns -= 1;
    e.state = ExecState::Running;
    assert_eq!(e.dismissal(), None, "not in a failure at all");
}

/// A retry opens a queued task linked to the failed one, under the same
/// parent and with the same reports, carves no more than the failed one had
/// (the parent has far more left), and dismisses the failed one: all in one
/// frame, with the caller's records.
#[test]
fn a_retry_carves_no_more_than_the_failed_task_had_and_dismisses_it_in_one_frame() {
    let w = world();
    let (_g, parent, failed) = with_failed_task(&w, 30_000);
    assert!(
        exec(&w, &parent.id).budget.available() > 30_000 * 2,
        "the parent has far more left than the failed task had"
    );
    let before = theseus_store::frames_written_here();
    let call = new_id("act");
    let opened = w
        .kernel
        .open_retry(&failed.id, &call, |f, t| {
            assert_eq!(
                (f.id.as_str(), t.retry_of.as_deref()),
                (failed.id.as_str(), Some(f.id.as_str()))
            );
            Ok(vec![NewRecord::json(
                theseus_store::kinds::META,
                Some("test.marker"),
                &"x",
            )?])
        })
        .unwrap();
    assert_eq!(
        theseus_store::frames_written_here() - before,
        1,
        "one frame"
    );
    let t = &opened.task;
    assert_eq!(t.state, ExecState::Queued);
    assert!(t.resume_pending);
    assert_eq!(t.retry_of.as_deref(), Some(failed.id.as_str()));
    assert_eq!(t.parent, failed.parent);
    assert_eq!(t.reports_to, failed.reports_to);
    assert_eq!(t.wake_parent, failed.wake_parent);
    assert_eq!(
        t.budget.limit_micros, 30_000,
        "never more than the failed one had"
    );
    assert_eq!(exec(&w, &t.id), *t, "as stored");
    // The failed one is dismissed by the same frame, and the carve is held.
    assert!(exec(&w, &failed.id).dismissal().is_some());
    let p = exec(&w, &parent.id);
    assert_eq!(
        p.budget.reservations.get(&crate::tasks::carve_key(&t.id)),
        Some(&30_000)
    );
    assert_eq!(
        rows(&w, &failed.session_id, "execution.dismissed")[0]["retried_as"],
        t.id
    );
    assert_eq!(
        rows(&w, &t.session_id, "execution.opened")[0]["parent"],
        parent.id
    );
    assert_eq!(
        rows(&w, &parent.session_id, "budget.carved")
            .last()
            .unwrap()["retry_of"],
        failed.id
    );
    // The same call run again finds its task and writes nothing.
    let before = theseus_store::frames_written_here();
    let again = w
        .kernel
        .open_retry(&failed.id, &call, |_, _| unreachable!("opened already"))
        .unwrap();
    assert_eq!(again.task.id, t.id);
    assert_eq!(theseus_store::frames_written_here(), before);
}

/// A parent with less left than the failed task had gives what it has.
#[test]
fn a_retry_is_capped_at_what_the_parent_has_left() {
    let w = world();
    let (g, parent, failed) = with_failed_task(&w, 60_000);
    // Another task takes most of what is left.
    let other = w
        .kernel
        .open_task(&g, &new_id("act"), 90_000, None, false, |_| Ok(vec![]))
        .unwrap();
    assert!(other.opened);
    let left = exec(&w, &parent.id).budget.available();
    assert!(left < failed.budget.limit_micros, "{left}");
    let opened = retry(&w, &failed.id).unwrap();
    assert_eq!(opened.available_before, left);
    assert_eq!(opened.task.budget.limit_micros, left);
}

/// A retry of a retry links to its predecessor, so the chain reads back.
#[test]
fn a_retry_of_a_retry_links_to_its_predecessor() {
    let w = world();
    let (_g, _, first) = with_failed_task(&w, 30_000);
    let second = retry(&w, &first.id).unwrap().task;
    assert_eq!(second.retry_of.as_deref(), Some(first.id.as_str()));
    let failed_again = fail(&w, &second.id, "the build failed");
    let third = retry(&w, &failed_again.id).unwrap().task;
    assert_eq!(third.retry_of.as_deref(), Some(second.id.as_str()));
    assert!(exec(&w, &second.id).dismissal().is_some());
    assert_eq!(third.parent, first.parent);
}

/// What a retry refuses, and writes nothing for: a task that did not fail, a
/// conversation, a parent that has ended.
#[test]
fn a_retry_is_refused_for_what_is_no_failed_task_with_a_live_parent() {
    let w = world();
    let (g, parent, failed) = with_failed_task(&w, 30_000);
    let queued = w
        .kernel
        .open_task(&g, &new_id("act"), 10_000, None, false, |_| Ok(vec![]))
        .unwrap()
        .task;
    let before = theseus_store::frames_written_here();
    let e = retry(&w, &parent.id).unwrap_err();
    assert!(format!("{e:#}").contains("is not a task"), "{e:#}");
    // A task still queued is no failure.
    let e = retry(&w, &queued.id).unwrap_err();
    assert!(
        matches!(
            e.downcast_ref::<crate::retry::RetryError>(),
            Some(crate::retry::RetryError::NotAFailure { .. })
        ),
        "{e:#}"
    );
    assert_eq!(
        theseus_store::frames_written_here(),
        before,
        "nothing written"
    );

    // The parent was cancelled: nothing to carve from or report to.
    w.kernel.cancel_execution(&parent.id, "the owner").unwrap();
    let before = theseus_store::frames_written_here();
    let e = retry(&w, &failed.id).unwrap_err();
    assert!(format!("{e:#}").contains("has ended"), "{e:#}");
    assert_eq!(
        theseus_store::frames_written_here(),
        before,
        "nothing written"
    );
    assert!(
        exec(&w, &failed.id).dismissal().is_none(),
        "nothing dismissed"
    );
}
