//! What the conversation that asked is told when a keep card settles (issue 37, line 1): a turn
//! after the card was settled reads the outcome, by name and never the value, once, and the words
//! the tool quoted for the card do not say there is no form.

use super::*;
use crate::config_change::tests::OWNER;

/// The words a settled card's note begins with.
const SETTLED: &str = "card is settled and no longer waits for the owner";

/// Everything the model was given in its latest request, as one text.
fn given(r: &Rig) -> String {
    let all = r.requests();
    serde_json::to_string(all.last().expect("the model was asked")).unwrap()
}

/// A turn in the owner's direct messages asks to keep a secret; the session and the card's id.
async fn asked_in_dm(r: &Rig, item: &str) -> (String, String) {
    // The owner's DM, typed in person: the view lays the exchange that asked on the next turn.
    r.in_person();
    let sid = r.session_at(&format!("dm:{OWNER}"));
    r.turn_with("vault_keep", &sid, input(item, "credential"))
        .await;
    let q = r.asked().pop().expect("one card waits").correlation_id;
    (sid, q)
}

/// The owner's next message, and what the model is given for it.
async fn next_turn(r: &Rig, sid: &str) -> String {
    r.says("is it kept?");
    r.run_turn(sid).await;
    given(r)
}

/// The note, once, after the sentence it supersedes, with the target by name and no value.
fn assert_told(text: &str, ends_with: &str) {
    let waits = text
        .find("Nothing is kept until")
        .expect("the tool's own result is there");
    let note = text.find(SETTLED).expect("the turn is told it settled");
    assert!(waits < note, "the outcome comes after what it supersedes");
    assert_eq!(text.matches(SETTLED).count(), 1, "once");
    assert!(text.contains(ends_with), "{text}");
    assert!(!text.contains(VALUE), "never the value");
}

/// The owner's case: the value was given on the card, and the next message's turn reads it was kept.
#[tokio::test]
async fn the_next_turn_reads_that_the_card_was_kept() {
    let (r, _dir) = rig_vault("ok");
    let secret = secret_of(&r);
    let (sid, q) = asked_in_dm(&r, "Lantern new").await;
    let first = r.result_of(&sid, "p1").1;
    assert!(
        !first.contains("no Approve") && !first.contains("Type the value in your own terminal"),
        "the tool quotes no how-to for a place with no form: {first}"
    );
    give(&r, cli(), &q, VALUE, Some(&code(&secret, 0)))
        .await
        .unwrap();
    let text = next_turn(&r, &sid).await;
    assert_told(
        &text,
        "Kept in the vault \\\"Harbor\\\", item \\\"Lantern new\\\"",
    );
    // A second answer is refused as already answered, and adds nothing.
    assert!(give(&r, cli(), &q, VALUE, Some(&code(&secret, 1)))
        .await
        .is_err());
    assert_eq!(next_turn(&r, &sid).await.matches(SETTLED).count(), 1);
}

/// A replace says it replaced.
#[tokio::test]
async fn a_replace_is_told_as_replaced() {
    let (r, _dir) = rig_vault("ok");
    let secret = r.core.proofs.open_stand_in_window("cli");
    let (sid, q) = asked_in_dm(&r, "Lantern key").await;
    give(&r, cli(), &q, VALUE, Some(&code(&secret, 1)))
        .await
        .unwrap();
    assert_told(
        &next_turn(&r, &sid).await,
        "It replaced the value that was there",
    );
}

/// A write the vault refused is told with its reason, and nothing was kept.
#[tokio::test]
async fn a_refused_write_is_told_with_its_reason() {
    let (r, _dir) = rig_vault("fail-write");
    let secret = secret_of(&r);
    let (sid, q) = asked_in_dm(&r, "Lantern new").await;
    give(&r, cli(), &q, VALUE, Some(&code(&secret, 0)))
        .await
        .unwrap_err();
    assert_told(&next_turn(&r, &sid).await, "Nothing was kept in the vault");
}

/// A decline is told.
#[tokio::test]
async fn a_decline_is_told() {
    let (r, _dir) = rig_vault("ok");
    let (sid, q) = asked_in_dm(&r, "Lantern new").await;
    rpc_as(
        &r.core,
        cli(),
        method::ACTION_CONFIRM,
        json!({"correlation_id": q, "approve": false}),
    )
    .await
    .unwrap();
    assert_told(
        &next_turn(&r, &sid).await,
        "the owner declined it. Nothing was kept.",
    );
}

/// A card nobody answered is told to have expired.
#[tokio::test]
async fn an_expired_card_is_told() {
    let (r, _dir) = rig_vault("ok");
    let (sid, _q) = asked_in_dm(&r, "Lantern new").await;
    let asked = r.asked().pop().unwrap();
    let ttl = r.core.kernel.config().confirm_ttl_ms;
    assert_eq!(r.core.expire_questions(asked.requested_at_ms + ttl + 1), 1);
    let text = next_turn(&r, &sid).await;
    assert_eq!(text.matches(SETTLED).count(), 1, "{text}");
    assert!(
        text.contains("it expired: nobody answered within"),
        "{text}"
    );
    assert!(!text.contains(VALUE));
}
