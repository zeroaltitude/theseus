//! A subagent's answer in the place (issue 28): a message of its own, posted as the bot every reply
//! comes from, whose footer names the model that answered and says it is a subagent's, delivered
//! once and in order before the reply that follows it.

use serde_json::json;
use theseus_sim::fake_discord::FakeDiscord;

use super::tests_bound::{lane, CHANNEL};
use crate::tests_cite::{assistant, reply_body};

const TURN: &str = "turn_sub";
const SESSION: &str = "sess_harbor";
const WORDS: &str = "Hello from the other model.";
const REPLY: &str = "It said hello.";

fn subagent(call: &str, text: &str, model: &str) -> serde_json::Value {
    json!({"kind": "subagent", "turn_id": TURN, "session_id": SESSION, "reply_to": null,
        "answer": {"profile": "haiku", "provider": "anthropic", "model": model,
            "asked_by": "sonnet", "text": text, "cost_usd": 0.0031, "elapsed_ms": 1300,
            "output_tokens": 12, "cut": false, "call": call}})
}

/// The answer is its own message, before the reply, with the footer that names the model that
/// answered (never the reply's) and says it is a subagent's; delivering again posts it no more.
#[tokio::test]
async fn a_subagents_answer_is_its_own_message_before_the_reply() {
    let d = tempfile::tempdir().unwrap();
    let fake = FakeDiscord::start();
    let mut lane = lane(d.path(), &fake);
    let core = lane.shared.core.clone();
    core.outbox
        .post(
            SESSION,
            "",
            &lane.target,
            subagent("act_1", WORDS, "claude-haiku-5-5"),
        )
        .unwrap();
    let a = assistant(&core, SESSION, REPLY);
    core.outbox
        .post(
            SESSION,
            "",
            &lane.target,
            reply_body(TURN, &a, SESSION, vec![]),
        )
        .unwrap();
    assert!(lane.deliver_posts().await);
    let all = fake.messages(CHANNEL);
    assert_eq!(all.len(), 2, "{all:#?}");
    assert!(
        all[0].content.starts_with(&format!("🛰️ {WORDS}")),
        "{all:#?}"
    );
    assert!(
        all[0].content.contains(
            "-# haiku · claude-haiku-5-5 · subagent answer, asked by sonnet · $0.0031 · 1.3 s"
        ),
        "{all:#?}"
    );
    assert!(all[1].content.contains(REPLY), "{all:#?}");
    assert!(
        !all[1].content.contains("subagent"),
        "the reply's own footer: {all:#?}"
    );
    lane.deliver_posts().await;
    assert_eq!(fake.messages(CHANNEL).len(), 2, "no message twice");
}

/// Two answers are two messages, in the order they were posted.
#[tokio::test]
async fn two_answers_are_two_messages_in_order() {
    let d = tempfile::tempdir().unwrap();
    let fake = FakeDiscord::start();
    let mut lane = lane(d.path(), &fake);
    let core = lane.shared.core.clone();
    for (call, text, model) in [
        ("act_1", "First answer.", "claude-haiku-5-5"),
        ("act_2", "Second answer.", "claude-opus-5-5"),
    ] {
        core.outbox
            .post(SESSION, "", &lane.target, subagent(call, text, model))
            .unwrap();
    }
    assert!(lane.deliver_posts().await);
    let all = fake.messages(CHANNEL);
    assert_eq!(all.len(), 2, "{all:#?}");
    assert!(
        all[0].content.contains("First answer.") && all[0].content.contains("claude-haiku-5-5")
    );
    assert!(
        all[1].content.contains("Second answer.") && all[1].content.contains("claude-opus-5-5")
    );
}
