//! The router's question and its placing, in the chat (theseus-wapgp,
//! theseus-0rfjx). A message the router cannot place waits in the daemon for
//! the owner's word, `[threads] ask_wait_secs` (900 s) at most, and the chat
//! once said nothing of the question: the message was echoed and then held
//! for 15 minutes, with every later message behind it. The chat now shows the
//! question and answers it by its key; a turn the router placed in a thread
//! says so.

use std::sync::Arc;

use crossterm::event::KeyCode;
use serde_json::json;
use theseus_protocol::thread_route::{AskOption, RouteHow, ThreadAsk, ThreadTag};
use theseus_protocol::{Event, TurnSubmitResult};

use super::tests::{Rig, SID, T0};
use crate::tests::Answer;

/// A message of the length that hung (159 characters and up).
fn long() -> String {
    "Start two tasks for me, one to count the words in every markdown file under sample and one to \
     time the slow load script, then tell me when both are done."
        .to_string()
}

fn ask(id: &str, session: &str, expires_ms: u64) -> ThreadAsk {
    let opt = |key: &str, label: &str| AskOption {
        key: key.into(),
        label: label.into(),
    };
    ThreadAsk {
        ask_id: id.into(),
        session_id: session.into(),
        place: None,
        question: "Is this about the security fix or the pr round?".into(),
        options: vec![
            opt("1", "pr round"),
            opt("2", "security fix"),
            opt("home", "home"),
        ],
        message: "Start two tasks for me, one to count the words".into(),
        expires_ms,
        others: Vec::new(),
    }
}

/// The chat typed `long()` and the router asked about it.
async fn asked_rig(expires_in_ms: u64) -> Rig {
    let mut r = Rig::new(100, 40).await;
    r.ready().await;
    r.paste(&long()).await;
    r.press(KeyCode::Enter).await;
    r.asked("turn.submit", 1).await;
    r.tell(Event::ThreadAsked(ask("ask_1", SID, T0 + expires_in_ms)));
    r
}

#[tokio::test(start_paused = true)]
async fn the_routers_question_is_shown_and_its_key_answers_it() {
    assert!(long().len() > 150, "the length the issue names");
    let mut r = asked_rig(900_000).await;
    r.shows("Is this about the security fix or the pr round?")
        .await;
    let text = r.text();
    assert!(text.contains("[2] security fix"), "{text}");
    assert!(text.contains("[h]ome"), "{text}");
    r.type_text("2").await;
    r.press(KeyCode::Enter).await;
    r.asked("thread.answer", 1).await;
    let sent = &r.daemon.asked("thread.answer")[0];
    assert_eq!(
        (&sent["ask_id"], &sent["choice"]),
        (&json!("ask_1"), &json!("2"))
    );
    // The key answered the question; it was not a message.
    assert_eq!(r.daemon.asked("turn.submit").len(), 1);
}

#[tokio::test(start_paused = true)]
async fn a_line_that_is_no_option_is_a_message_and_the_question_stays() {
    let mut r = asked_rig(900_000).await;
    r.shows("security fix").await;
    r.type_text("and what is the weather").await;
    r.press(KeyCode::Enter).await;
    r.asked("turn.submit", 2).await;
    assert!(r.daemon.asked("thread.answer").is_empty());
    // The question still takes its key.
    r.type_text("h").await;
    r.press(KeyCode::Enter).await;
    r.asked("thread.answer", 1).await;
    assert_eq!(r.daemon.asked("thread.answer")[0]["choice"], json!("home"));
}

#[tokio::test(start_paused = true)]
async fn an_expired_question_takes_no_keys_and_another_sessions_is_not_shown() {
    let mut r = asked_rig(60_000).await;
    r.shows("security fix").await;
    r.advance(61_000).await;
    r.type_text("2").await;
    r.press(KeyCode::Enter).await;
    r.asked("turn.submit", 2).await;
    assert!(r.daemon.asked("thread.answer").is_empty());
    let mut other = Rig::new(100, 40).await;
    other.ready().await;
    other.tell(Event::ThreadAsked(ask(
        "ask_9",
        "ses_someone_else",
        T0 + 900_000,
    )));
    other.advance(100).await;
    assert!(!other.text().contains("security fix"), "{}", other.text());
}

/// The daemon's script, `turn.submit` answered as a turn the router placed in
/// the thread `security-fix` (its own session).
fn placed_script() -> crate::tests::Script {
    let base = super::tests::script();
    Arc::new(move |m, p| -> Answer {
        if m != "turn.submit" {
            return base(m, p);
        }
        let placed = TurnSubmitResult {
            session_id: "ses_thread_security".into(),
            turn_id: "turn_t1".into(),
            output: "Looked at the vault.".into(),
            model: "claude-sonnet-5-5".into(),
            elapsed_ms: 900,
            stop_reason: "done".into(),
            thread: Some(ThreadTag {
                slug: Some("security-fix".into()),
                title: "security fix".into(),
                how: RouteHow::Quiet,
                confidence: 0.95,
                movable: false,
            }),
            ..Default::default()
        };
        Ok(serde_json::to_value(placed).unwrap())
    })
}

#[tokio::test(start_paused = true)]
async fn a_message_the_router_placed_in_a_thread_says_so() {
    let mut r = Rig::with(100, 40, placed_script()).await;
    r.ready().await;
    r.type_text("why is the vault build red").await;
    r.press(KeyCode::Enter).await;
    r.shows("Looked at the vault.").await;
    let text = r.text();
    assert!(text.contains("→ thread security fix"), "{text}");
    assert!(text.contains("ses_thread_security"), "{text}");
}
