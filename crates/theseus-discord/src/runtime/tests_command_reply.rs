//! A slash command's reply leaves through the same exit as every other
//! message (theseus-qtofh): the vault's values replaced by their names, no
//! mention it names allowed to ping, and at most Discord's 2000 characters.
//! Each test presses `/status`, lets a stand-in place answer with `said`, and
//! reads what the fake Discord was sent as the edit of the original response.

use std::time::{Duration, Instant};

use serde_json::Value;
use theseus_core::secrets::{Secret, SecretBoard};
use theseus_sim::fake_discord::{FakeDiscord, Reply};

use super::tests::{core_with, slash, OWNER};
use super::{shared_for_tests, Control, PlaceMsg};

const VALUE: &str = "vault-token-4c8e1f7a92b3";
/// Looks exactly like a key, and the vault never held it.
const LOOKALIKE: &str = "sk-ant-api03-AbCdEfGhIjKlMnOpQrStUvWxYz0123456789";

/// The edit the binding sent for a `/status` whose place said `said`.
async fn answered(said: &str) -> Reply {
    let fake = FakeDiscord::start();
    let d = tempfile::tempdir().unwrap();
    let secrets = SecretBoard::new(["courier_token".to_string()], Instant::now());
    secrets.publish(
        [("courier_token".to_string(), Ok(Secret::new(VALUE.into())))].into(),
        "fake",
    );
    let addr = fake.addr.clone();
    let core = core_with(d.path(), secrets, |c| c.discord.rest_proxy = Some(addr));
    let shared = shared_for_tests(&core);
    let (dm_tx, mut dm_rx) = tokio::sync::mpsc::unbounded_channel();
    shared
        .routes
        .lock()
        .unwrap()
        .by_dm_user
        .insert(OWNER, dm_tx);
    let h = tokio::spawn(shared.clone().on_interaction(slash(
        None,
        444_444_444_444_444_444,
        OWNER,
        "status",
    )));
    let got = tokio::time::timeout(Duration::from_secs(10), dm_rx.recv())
        .await
        .unwrap();
    let Some(PlaceMsg::Control { cmd, reply, .. }) = got else {
        panic!("not a control");
    };
    assert_eq!(cmd, Control::Status);
    let _ = reply.send(said.into());
    h.await.unwrap();
    let seen: Vec<String> = fake
        .seen()
        .iter()
        .map(|s| format!("{} {} {}", s.method, s.path, s.outcome))
        .collect();
    let seen = format!("{seen:?} {:?}", shared.board.st.lock().unwrap().last_error);
    fake.replies()
        .into_iter()
        .find(|r| r.kind == "original")
        .unwrap_or_else(|| panic!("the command's reply was edited in: {seen}"))
}

/// A vault value in a command's reply leaves as its name; a string that only
/// looks like a key is sent as it is.
#[tokio::test]
async fn a_secret_in_a_command_reply_is_redacted() {
    let sent = answered(&format!("token {VALUE}, key {LOOKALIKE}")).await;
    assert_eq!(
        sent.content.as_deref(),
        Some(format!("token [redacted:courier_token], key {LOOKALIKE}").as_str()),
    );
}

/// A reply that names a role, `@everyone` and a user pings none of them: the
/// edit carries an `allowed_mentions` that allows nothing.
#[tokio::test]
async fn a_command_reply_names_nobody_it_can_ping() {
    let said = "<@&1234567890> @everyone <@42> the deploy is done";
    let sent = answered(said).await;
    assert_eq!(sent.content.as_deref(), Some(said), "the words are kept");
    let allowed = &sent.allowed_mentions;
    assert!(allowed.is_object(), "no allowed_mentions sent: {allowed}");
    let none = |k: &str| match &allowed[k] {
        Value::Null => true,
        Value::Array(a) => a.is_empty(),
        _ => false,
    };
    assert!(none("parse") && none("users") && none("roles"), "{allowed}");
    assert!(
        !allowed["replied_user"].as_bool().unwrap_or(false),
        "{allowed}"
    );
}

/// A reply past Discord's limit is cut by us to exactly 2000 characters.
#[tokio::test]
async fn a_long_command_reply_is_clipped_to_exactly_2000() {
    let sent = answered(&"x".repeat(2500)).await;
    let content = sent.content.unwrap();
    assert_eq!(content.chars().count(), 2000);
    assert!(content.starts_with(&"x".repeat(1999)), "cut from the end");
}

/// The value is replaced before the cut, never after it: a value that
/// straddles the limit must not leave a piece of itself behind.
#[tokio::test]
async fn a_secret_at_the_limit_is_replaced_before_the_cut() {
    let sent = answered(&format!("{}{VALUE}", "a".repeat(1980))).await;
    let content = sent.content.unwrap();
    assert_eq!(content.chars().count(), 2000);
    assert!(
        content.contains("[redacted:"),
        "the name is there, so the value was replaced: {content}"
    );
    assert!(!content.contains(&VALUE[..8]), "a piece of the value");
}
