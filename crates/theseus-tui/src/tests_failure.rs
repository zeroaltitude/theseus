//! A failed task's card (theseus-7zgsm): `[r] retry  [d] dismiss  [o] open`,
//! `r` and `d` acting on the failed row under the cursor through
//! `task.retry` and `execution.dismiss`, the daemon's refusal said on the
//! card, and the row leaving needs-you when the daemon's push says it is
//! dismissed.

use std::sync::{Arc, Mutex};

use crossterm::event::KeyCode;
use serde_json::json;
use theseus_protocol::{attention, error_code, utc_hm};

use crate::tests::*;

/// The harbour with its task `ses_tide01` failed and no other question
/// waiting: the failure is the only thing that needs the owner.
fn with_a_failed_task() -> Arc<Mutex<World>> {
    let world = harbour_world();
    {
        let mut w = world.lock().unwrap();
        let mut v = task(96, "ses_tide01", "ses_dm0001", "failed", 0.10);
        v.ended_reason = Some("the provider refused the request (400)".into());
        v.attention = attention(&v, &utc_hm);
        replace_view(&mut w.board, &v);
        replace_view(
            &mut w.board,
            &conv(98, "ses_spec01", "waiting", input(), 1.2),
        );
        w.board["confirms"] = json!([]);
    }
    world
}

/// The failed task's view once the owner dismissed it, as the daemon's push
/// says it.
fn dismissed_view() -> theseus_protocol::ExecutionView {
    let mut v = task(97, "ses_tide01", "ses_dm0001", "failed", 0.10);
    v.ended_reason = Some("the provider refused the request (400)".into());
    v.dismissed = true;
    v.attention = attention(&v, &utc_hm);
    v
}

/// The card on a failed task says why and offers the three keys; `d` sends
/// `execution.dismiss` for it, and the row stops needing the owner when the
/// daemon's push says dismissed.
#[tokio::test]
async fn a_failed_task_draws_its_card_and_d_dismisses_it() {
    let world = with_a_failed_task();
    world.lock().unwrap().answers.insert(
        "execution.dismiss".into(),
        Ok(json!({"execution_id": "exe_ses_tide01", "dismissed_at_ms": 1})),
    );
    let mut rig = Rig::new(120, 24, script(world));
    rig.shows("check the tide").await;
    rig.press(&[KeyCode::Tab]).await;
    rig.shows("[r] retry  [d] dismiss  [o] open").await;
    rig.shows("✗ failed: the provider refused the request (400)")
        .await;
    rig.press(&[KeyCode::Char('d')]).await;
    rig.asked("execution.dismiss", 1).await;
    assert_eq!(
        rig.daemon().asked("execution.dismiss"),
        [json!({"execution_id": "exe_ses_tide01"})]
    );
    assert!(rig.daemon().asked("task.retry").is_empty());
    rig.shows("dismissed tide01").await;

    // The push says so: the card goes, and the row reads dismissed.
    rig.daemon()
        .notify("execution.changed", changed(&dismissed_view()));
    rig.settle().await;
    let screen = rig.screen().join("\n");
    assert!(!screen.contains("[r] retry"), "{screen}");
    assert!(screen.contains("dismissed"), "{screen}");
}

/// `r` retries a failed task: `task.retry` for its execution, and the line
/// says what was carved.
#[tokio::test]
async fn r_retries_a_failed_task() {
    let world = with_a_failed_task();
    world.lock().unwrap().answers.insert(
        "task.retry".into(),
        Ok(
            json!({"task_id": "ses_retry01", "execution_id": "exe_ses_retry01",
                  "retry_of": "exe_ses_tide01", "limit_usd": 2.5, "failed_limit_usd": 5.0}),
        ),
    );
    let mut rig = Rig::new(120, 24, script(world));
    rig.shows("check the tide").await;
    rig.press(&[KeyCode::Tab, KeyCode::Char('r')]).await;
    rig.asked("task.retry", 1).await;
    assert_eq!(
        rig.daemon().asked("task.retry"),
        [json!({"execution_id": "exe_ses_tide01"})]
    );
    rig.shows("retried").await;
    let screen = rig.screen().join("\n");
    assert!(
        screen.contains("$2.50") && screen.contains("had $5)"),
        "{screen}"
    );
}

/// A daemon that refuses (a shared place's connection, or the owner's word
/// missing) leaves the failure as it was, and the card says why.
#[tokio::test]
async fn a_refused_dismissal_leaves_the_failure_and_says_why() {
    let world = with_a_failed_task();
    world.lock().unwrap().answers.insert(
        "execution.dismiss".into(),
        Err((
            error_code::REFUSED,
            "only the owner dismisses, from a private place".into(),
        )),
    );
    let mut rig = Rig::new(120, 24, script(world));
    rig.shows("check the tide").await;
    rig.press(&[KeyCode::Tab, KeyCode::Char('d')]).await;
    rig.asked("execution.dismiss", 1).await;
    rig.shows("refused: only the owner dismisses").await;
    rig.shows("[r] retry  [d] dismiss  [o] open").await;
}

/// Off a failed row, `r` and `d` filter as they always did: ready, done.
#[tokio::test]
async fn off_a_failure_r_and_d_still_filter() {
    let world = harbour_world();
    let mut rig = Rig::new(120, 24, script(world));
    rig.shows("check the tide").await;
    rig.press(&[KeyCode::Char('r')]).await;
    rig.press(&[KeyCode::Char('d')]).await;
    rig.settle().await;
    assert!(rig.daemon().asked("task.retry").is_empty());
    assert!(rig.daemon().asked("execution.dismiss").is_empty());
}
