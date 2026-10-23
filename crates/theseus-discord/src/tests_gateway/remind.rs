//! A reminder in the owner's DM (theseus-8ux6x): the core posts its note as
//! it is, with Done, Snooze 10 min, Snooze 1 h and Tomorrow 9:00, and always
//! pings; Snooze 10 min sets it again (and it fires again); Done settles it;
//! `/remind`'s suggestions are the core's to read.

use std::sync::Arc;

use theseus_core::provider::{FakeProvider, Provider};

use super::{bindings, guild, Rig, ANA, ANA_DM};

#[tokio::test]
async fn a_reminder_reaches_the_dm_with_four_buttons_and_a_snooze_sets_it_again() {
    let model =
        |_: &std::path::Path, _| -> Arc<dyn Provider> { Arc::new(FakeProvider::scripted(vec![])) };
    let r = Rig::start_tweaked(model, guild(true), &bindings(false), &[], |_| {}).await;
    // A reminder in Home, due in 2 s: no model is involved.
    let home = r.core.home().unwrap();
    let due = r.core.kernel.now_ms() + 2_000;
    r.core
        .kernel
        .create_wake(
            &home.execution_id,
            None,
            theseus_kernel::wake_owner::reminder(due, "stretch"),
            "the test",
        )
        .unwrap();
    let is_it = |m: &theseus_sim::fake_discord::Msg| m.versions[0].contains("stretch");
    r.until("the reminder in ana's DM", || {
        r.posted(ANA_DM).iter().any(is_it)
    })
    .await;
    let post = r.posted(ANA_DM).into_iter().find(is_it).unwrap();
    assert!(
        post.content
            .starts_with("✋ Action · Reminder\n⏰ a reminder you set")
            && post.content.ends_with("\nstretch"),
        "{}",
        post.content
    );
    let labels: Vec<&str> = post.buttons.iter().map(|b| b.label.as_str()).collect();
    assert_eq!(
        labels,
        ["Done", "Snooze 10 min", "Snooze 1 h", "Tomorrow 9:00"]
    );
    assert!(!post.silent(), "a reminder always pings: {post:?}");

    // Snooze 10 min: the wake is set again, ten minutes ahead.
    let pressed = r.press(&post.id, "Snooze 10 min", (ANA, "ana"));
    r.until("the snooze", || !r.ledger("wake.snoozed").is_empty())
        .await;
    let now = r.core.kernel.now_ms();
    let pending = r.core.kernel.pending_wakes().unwrap();
    assert_eq!(pending.len(), 1, "set again: {pending:?}");
    let ahead = pending[0].1.due_at_ms.saturating_sub(now);
    assert!(
        (590_000..=600_000).contains(&ahead),
        "ten minutes ahead, not {ahead} ms"
    );
    assert!(pending[0].1.run.is_notice());
    r.until("the answer in the message", || {
        r.fake
            .replies()
            .iter()
            .any(|x| x.interaction.as_deref() == Some(pressed.as_str()))
    })
    .await;

    // Done on the same reminder: a row, nothing else.
    r.press(&post.id, "Done", (ANA, "ana"));
    r.until("done", || !r.ledger("wake.done").is_empty()).await;
    assert_eq!(
        r.core.kernel.pending_wakes().unwrap().len(),
        1,
        "Done moves nothing"
    );
}
