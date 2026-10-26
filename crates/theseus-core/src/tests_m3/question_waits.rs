//! A question that waits past its life is not cancelled (theseus-v2dnc, the
//! owner's 2026-10-25 13:58 and 14:16 lines): the call stays pending, one
//! quiet notice says how long it has waited, and the next comes a life
//! later. A budget question holds as before. A child of `tests_m3` for its
//! rig and helpers.

use super::*;

/// The notices that say a question is still waiting, as posted.
pub(super) fn waiting(r: &Rig) -> Vec<Value> {
    r.core
        .kernel
        .outbox_actions()
        .unwrap()
        .into_iter()
        .filter(|a| crate::outbox::kind_of(a) == "notice")
        .map(|a| crate::outbox::body_of(&a).clone())
        .filter(|b| b["purpose"] == "question_waiting")
        .collect()
}

/// A DM is bound, as on the owner's machine: the notice has somewhere to go.
pub(super) fn bind_dm(r: &Rig) {
    r.core.bind_places(vec![crate::places::BoundPlace {
        target: "discord:dm:42".into(),
        name: "DM @owner".into(),
        private: true,
        ..Default::default()
    }]);
}

async fn write_waits() -> (Rig, String, String, u64, u64) {
    let r = rig(vec![
        Scripted::tools(
            "",
            &[(
                "t1",
                "fs_write",
                json!({"path": "note.txt", "content": "hi"}),
            )],
        ),
        Scripted::text("Done."),
    ]);
    let res = turn(&r.core, None, "write a note").await;
    let corr = res.awaiting_confirm.clone().expect("the write waits");
    bind_dm(&r);
    let planned = r.core.kernel.action(&corr).unwrap().unwrap().planned_at_ms;
    let ttl = r.core.kernel.config().confirm_ttl_ms;
    (r, res.session_id, corr, planned, ttl)
}

/// At twice its life a call's question is still pending, and the call was
/// never answered declined.
#[tokio::test]
async fn a_question_at_twice_its_life_is_still_pending_and_never_declined() {
    let (r, sid, corr, planned, ttl) = write_waits().await;
    assert_eq!(r.core.expire_questions(planned + 2 * ttl), 0);
    let a = r.core.kernel.action(&corr).unwrap().unwrap();
    assert!(a.awaits_confirm(), "still pending: {a:?}");
    assert_eq!(r.core.confirm_list().unwrap().len(), 1);
    // The absence of the decline is a negative assertion: a failure is a finding.
    assert_ne!(a.state, theseus_kernel::ActionState::Cancelled, "{a:?}");
    assert!(ledgered(&r, "action.expired").is_empty());
    assert!(ledgered(&r, "action.declined").is_empty());
    assert!(
        results(&r.core, &sid).is_empty(),
        "the call was not answered: {:?}",
        results(&r.core, &sid)
    );
    assert!(!r.root.join("note.txt").exists());
    // It can still be approved, and then runs.
    r.core.confirm_action(&corr, true, None, "test").unwrap();
    assert!(r.core.kernel.pending_confirms().unwrap().is_empty());
}

/// One notice at one life and one at two, naming the question and the wait;
/// none between, and none a minute.
#[tokio::test]
async fn a_waiting_question_notices_once_a_life_not_once_a_sweep() {
    let (r, _, corr, planned, ttl) = write_waits().await;
    assert_eq!(r.core.expire_questions(planned + ttl - 1), 0);
    assert!(waiting(&r).is_empty(), "nothing before its life");
    r.core.expire_questions(planned + ttl);
    let first = waiting(&r);
    assert_eq!(first.len(), 1, "{first:?}");
    let text = first[0]["text"].as_str().unwrap();
    assert!(text.contains("fs.write"), "{text}");
    assert!(text.contains("15 minutes"), "{text}");
    assert_eq!(first[0]["correlation_id"], corr.as_str());
    assert_eq!(first[0]["quiet"], true, "inform: never a ping");
    // A sweep every minute through the second life notices nothing more.
    let mut at = planned + ttl;
    while at + 60_000 < planned + 2 * ttl {
        at += 60_000;
        r.core.expire_questions(at);
    }
    assert_eq!(waiting(&r).len(), 1, "one a life");
    r.core.expire_questions(planned + 2 * ttl);
    let both = waiting(&r);
    assert_eq!(both.len(), 2, "{both:?}");
    assert!(both[1]["text"].as_str().unwrap().contains("30 minutes"));
    r.core.expire_questions(planned + 2 * ttl + 1_000);
    assert_eq!(waiting(&r).len(), 2, "not again within the life");
}

/// A budget question neither expires nor gains the notice: it holds until it
/// is answered, as before.
#[tokio::test]
async fn a_budget_question_is_untouched_by_the_notice() {
    let r = over_limit_rig();
    bind_dm(&r);
    let res = turn(&r.core, None, "hello").await;
    assert_eq!(res.stop_reason, "budget", "{res:?}");
    let q = r.core.confirm_list().unwrap().remove(0);
    let ttl = r.core.kernel.config().confirm_ttl_ms;
    assert_eq!(r.core.expire_questions(q.requested_at_ms + 10 * ttl), 0);
    assert!(waiting(&r).is_empty(), "a budget question gains no notice");
    assert_eq!(r.core.confirm_list().unwrap().len(), 1, "still held");
    assert!(ledgered(&r, "action.expired").is_empty());
    assert_eq!(
        r.core
            .tools
            .question_due
            .load(std::sync::atomic::Ordering::SeqCst),
        u64::MAX,
        "a budget question is never scheduled"
    );
}

/// A sweep in which a question notified leaves `question_due` at the next
/// notice: finite and in the future, so the driver neither goes silent nor
/// spins; and a sweep before it stages nothing.
#[tokio::test]
async fn a_notice_leaves_the_next_one_scheduled() {
    use std::sync::atomic::Ordering::SeqCst;
    let (r, _, _, planned, ttl) = write_waits().await;
    let now = planned + ttl + 5_000;
    r.core.expire_questions(now);
    assert_eq!(waiting(&r).len(), 1);
    let due = r.core.tools.question_due.load(SeqCst);
    assert_ne!(due, u64::MAX, "the next notice must be scheduled");
    assert!(due > now, "in the future: {due} vs {now}");
    assert_eq!(due, planned + 2 * ttl, "the next life's end");
    r.core.expire_questions(due - 1);
    assert_eq!(waiting(&r).len(), 1, "a sweep before it stages none");
    assert_eq!(r.core.tools.question_due.load(SeqCst), due);
}

/// theseus-x29x3: a retired session's held question ends at its life, by the
/// decline that releases a reservation it holds, and is not noticed. A live
/// session's stays held (the tests above). No real path reserves for a
/// pending call (`plan_confirm` reserves nothing), so the reservation is
/// written in by hand. Before, the sweep noticed it at every life and left
/// it pending, its reservation held.
#[tokio::test]
async fn a_retired_sessions_held_question_ends_and_frees_its_reservation() {
    use theseus_store::{kinds, NewRecord};
    let (r, sid, corr, planned, ttl) = write_waits().await;
    let mut a = r.core.kernel.action(&corr).unwrap().unwrap();
    a.reservation_id = Some("rsv_held".into());
    a.reserved_micros = 5_000;
    let mut e = r.core.kernel.execution(&a.execution_id).unwrap().unwrap();
    e.budget.reservations.insert("rsv_held".into(), 5_000);
    e.budget.reserved_micros = 5_000;
    r.core
        .store
        .append(&[
            NewRecord::json(kinds::ACTION, Some(&corr), &a)
                .unwrap()
                .scoped(&a.session_id),
            NewRecord::json(kinds::EXECUTION, Some(&e.id), &e)
                .unwrap()
                .scoped(&e.session_id),
        ])
        .unwrap();
    let held = r.core.kernel.execution(&a.execution_id).unwrap().unwrap();
    assert_eq!(
        held.budget.reserved_micros, 5_000,
        "the reservation is held"
    );
    r.core
        .retire_session(&sid, &crate::approval::Answerer::from("cli"))
        .unwrap();
    // Before its life it waits as any question does.
    assert_eq!(r.core.expire_questions(planned + ttl - 1), 0);
    assert_eq!(r.core.kernel.pending_confirms().unwrap().len(), 1);
    // At its life it ends: declined, its reservation released, no notice.
    assert_eq!(r.core.expire_questions(planned + ttl), 1);
    assert!(r.core.kernel.pending_confirms().unwrap().is_empty());
    let a = r.core.kernel.action(&corr).unwrap().unwrap();
    assert_eq!(a.state, theseus_kernel::ActionState::Cancelled, "{a:?}");
    assert!(a
        .resolution
        .as_deref()
        .unwrap()
        .contains("session was retired"));
    let e = r.core.kernel.execution(&a.execution_id).unwrap().unwrap();
    assert_eq!(e.budget.reserved_micros, 0, "{:?}", e.budget);
    assert!(e.budget.reservations.is_empty(), "{:?}", e.budget);
    assert_eq!(e.budget.spent_micros, 0, "released, not spent");
    assert_eq!(e.budget.held_unknown_micros, 0, "released, not held");
    assert_eq!(ledgered(&r, "action.expired").len(), 1);
    assert!(waiting(&r).is_empty(), "an ended question is not noticed");
    assert!(r.core.confirm_list().unwrap().is_empty());
}
