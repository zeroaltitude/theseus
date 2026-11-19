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

/// A task's own reply in the place its session posts to says it is a subagent's, after the model it ran
/// on; an ordinary conversation's reply does not.
#[tokio::test]
async fn a_tasks_own_reply_in_a_channel_says_it_is_a_subagents() {
    use theseus_core::session::{SessionRecord, TaskOf};
    use theseus_protocol::SessionKind;
    let d = tempfile::tempdir().unwrap();
    let fake = FakeDiscord::start();
    let mut lane = lane(d.path(), &fake);
    let core = lane.shared.core.clone();
    let mut rec = SessionRecord::new(SessionKind::Conversation, None);
    rec.task = Some(TaskOf {
        parent_session: SESSION.into(),
        parent_execution: "exe_x".into(),
        by: "tool_x".into(),
        target: None,
        arrangement: None,
        check: None,
        notify_done: false,
    });
    core.store.put_session(&rec.session_id, &rec).unwrap();
    let a = assistant(&core, &rec.session_id, REPLY);
    core.outbox
        .post(
            &rec.session_id,
            "",
            &lane.target,
            reply_body(TURN, &a, &rec.session_id, vec![]),
        )
        .unwrap();
    assert!(lane.deliver_posts().await);
    let all = fake.messages(CHANNEL);
    assert_eq!(all.len(), 1, "{all:#?}");
    assert!(
        all[0].content.contains("glm-x · subagent task reply"),
        "{all:#?}"
    );

    // The same reply from an ordinary conversation keeps its footer.
    let d2 = tempfile::tempdir().unwrap();
    let fake2 = FakeDiscord::start();
    let mut lane2 = super::tests_bound::lane(d2.path(), &fake2);
    let core2 = lane2.shared.core.clone();
    let b = assistant(&core2, SESSION, REPLY);
    core2
        .outbox
        .post(
            SESSION,
            "",
            &lane2.target,
            reply_body("turn_plain", &b, SESSION, vec![]),
        )
        .unwrap();
    assert!(lane2.deliver_posts().await);
    let plain = fake2.messages(CHANNEL);
    assert!(!plain[0].content.contains("subagent"), "{plain:#?}");
}
