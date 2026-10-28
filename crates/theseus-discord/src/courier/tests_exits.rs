//! The courier is an exit (theseus-gyjyo R7b): a value the vault resolved is
//! replaced by its name in what a place is sent, as a post, an edit, and a
//! live stream's text; a string that only looks like a key is sent as it is;
//! and `[privacy] redact_outbound = false` sends the value as written.

use std::time::Instant;

use theseus_core::secrets::{Secret, SecretBoard};
use theseus_sim::fake_discord::FakeDiscord;

use super::tests_bound::{upsert, CHANNEL};
use super::*;

const VALUE: &str = "vault-token-4c8e1f7a92b3";
/// Looks exactly like a key, and the vault never held it.
const LOOKALIKE: &str = "sk-ant-api03-AbCdEfGhIjKlMnOpQrStUvWxYz0123456789";

fn lane(dir: &std::path::Path, fake: &FakeDiscord, redact: bool) -> Lane {
    let secrets = SecretBoard::new(["courier_token".to_string()], Instant::now());
    secrets.publish(
        [("courier_token".to_string(), Ok(Secret::new(VALUE.into())))].into(),
        "fake",
    );
    let core = crate::runtime::tests::core_with(dir, secrets, |c| {
        c.discord.rest_proxy = Some(fake.addr.clone());
        c.privacy.redact_outbound = redact;
    });
    let shared = crate::runtime::shared_for_tests(&core);
    Lane::new(
        shared,
        format!("discord:channel:{CHANNEL}"),
        "channel",
        "#harbor".into(),
        Some(CHANNEL),
        None,
    )
}

fn post(key: &str, content: &str) -> Write {
    Write {
        key: key.into(),
        channel: CHANNEL,
        content: content.into(),
        buttons: Buttons::Keep,
        reply_to: None,
        message: None,
        mentions: vec![],
        ping: false,
    }
}

fn last(fake: &FakeDiscord) -> String {
    fake.messages(CHANNEL).pop().expect("a message").content
}

/// A post, the edit of it, and a live stream's text each go out with the
/// value replaced by its name, and the lookalike as it was.
#[tokio::test]
async fn a_vault_value_leaves_as_its_name_in_a_post_an_edit_and_a_stream() {
    let d = tempfile::tempdir().unwrap();
    let fake = FakeDiscord::start();
    let mut lane = lane(d.path(), &fake, true);
    lane.write(&post("k:post", &format!("token {VALUE}, key {LOOKALIKE}")))
        .await
        .unwrap();
    let want = format!("token [redacted:courier_token], key {LOOKALIKE}");
    assert_eq!(last(&fake), want, "the post");
    lane.write(&post("k:post", &format!("again {VALUE}")))
        .await
        .unwrap();
    assert_eq!(last(&fake), "again [redacted:courier_token]", "the edit");
    lane.take(upsert("turn_1:L0:p0", &format!("streaming {VALUE}")));
    lane.apply_live().await;
    assert_eq!(
        last(&fake),
        "streaming [redacted:courier_token]",
        "the stream"
    );
    let everything: String = fake
        .messages(CHANNEL)
        .iter()
        .map(|m| m.content.clone())
        .collect();
    assert!(!everything.contains(VALUE), "{everything}");
}

/// `redact_outbound = false`: the operator's choice, the value goes as written.
#[tokio::test]
async fn with_redact_outbound_off_the_value_is_sent_as_written() {
    let d = tempfile::tempdir().unwrap();
    let fake = FakeDiscord::start();
    let mut lane = lane(d.path(), &fake, false);
    lane.write(&post("k:post", &format!("token {VALUE}")))
        .await
        .unwrap();
    assert_eq!(last(&fake), format!("token {VALUE}"));
}
