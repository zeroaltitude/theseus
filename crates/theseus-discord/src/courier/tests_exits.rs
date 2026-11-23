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

/// A value encoded in what a place is told (a probe row, a task report, a
/// tool's output the model quoted) leaves by name too: base64, percent-encoded
/// and JSON-escaped, each in a post.
#[tokio::test]
async fn an_encoded_vault_value_leaves_as_its_name() {
    let d = tempfile::tempdir().unwrap();
    let fake = FakeDiscord::start();
    let mut lane = lane(d.path(), &fake, true);
    // `user:` and VALUE in base64, as a Basic header carries it.
    let basic = "dXNlcjp2YXVsdC10b2tlbi00YzhlMWY3YTkyYjM=";
    let percent = "vault-token-4c8e1f7a92b3"
        .bytes()
        .map(|c| format!("%{c:02X}"))
        .collect::<String>();
    for (i, (said, leaked)) in [
        (format!("Authorization: Basic {basic}"), basic.to_string()),
        (format!("GET /cb?token={percent}&n=1"), percent),
        (
            r#"{"token": "vault-token-4c8e1f7a92b3"}"#.replace('-', r"\u002d"),
            r"\u002d".into(),
        ),
    ]
    .into_iter()
    .enumerate()
    {
        lane.write(&post(&format!("k:enc{i}"), &said))
            .await
            .unwrap();
        let out = last(&fake);
        assert!(out.contains("[redacted:courier_token]"), "{said} -> {out}");
        assert!(!out.contains(&leaked), "{said} -> {out}");
    }
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

/// A notice card names the call it is about: a vault value in its command line
/// or a field is replaced too, and a card without one is the same card.
#[tokio::test]
async fn a_notice_card_leaves_with_the_value_replaced() {
    let d = tempfile::tempdir().unwrap();
    let fake = FakeDiscord::start();
    let lane = lane(d.path(), &fake, true);
    let card = NoticeCard {
        title: "🔔 notified".into(),
        color: 0,
        description: format!("`proc.run` curl -H 'Authorization: {VALUE}' {LOOKALIKE}"),
        fields: vec![
            ("env".into(), format!("TOKEN={VALUE}")),
            ("cwd".into(), "/work".into()),
        ],
        ask: None,
    };
    let out = mask_card(&lane.shared.core.exits, &card);
    assert_eq!(
        out.description,
        format!("`proc.run` curl -H 'Authorization: [redacted:courier_token]' {LOOKALIKE}")
    );
    assert_eq!(
        out.fields[0],
        ("env".into(), "TOKEN=[redacted:courier_token]".into())
    );
    assert_eq!(out.fields[1], card.fields[1]);
    let plain = NoticeCard {
        description: "`fs.read` a.txt".into(),
        fields: vec![],
        ..card
    };
    assert_eq!(mask_card(&lane.shared.core.exits, &plain), plain);
}

#[tokio::test]
async fn question_choices_are_masked_before_button_clipping_on_create_and_edit() {
    let d = tempfile::tempdir().unwrap();
    let fake = FakeDiscord::start();
    let mut lane = lane(d.path(), &fake, true);
    for (i, buttons) in [
        Buttons::Ask("q".into(), vec![format!("{}{}", "x".repeat(45), VALUE)]),
        Buttons::Card("q".into(), vec![VALUE.into()]),
    ]
    .into_iter()
    .enumerate()
    {
        let mut w = post("k:choice", &format!("Choose {i}"));
        w.buttons = buttons;
        lane.write(&w).await.unwrap();
        let components = fake
            .messages(CHANNEL)
            .pop()
            .unwrap()
            .components_json
            .to_string();
        assert!(!components.contains("vault-token"), "{components}");
        assert!(components.contains("[redacted:"), "{components}");
    }
}

#[tokio::test]
async fn a_reply_masks_a_vault_value_before_message_splitting() {
    let d = tempfile::tempdir().unwrap();
    let fake = FakeDiscord::start();
    let mut lane = lane(d.path(), &fake, true);
    let text = format!("{}{}{}", "x".repeat(1890), VALUE, "x".repeat(500));
    let node = theseus_core::node::Node::assistant(
        "s",
        "turn_split",
        0,
        theseus_core::node::Body::AssistantMessage {
            blocks: vec![json!({"type": "text", "text": text})],
            model: "fake".into(),
            provider: "fake".into(),
            stop_reason: None,
            usage: Default::default(),
            cost_usd: None,
            catalog_version: None,
            request_id: None,
            correlation_id: None,
            compilation_id: None,
            request_digest: None,
            effort: None,
        },
    );
    lane.shared
        .core
        .store
        .append(&[node.record().unwrap()])
        .unwrap();
    let plan = lane.reply(
        &json!({"turn_id": "turn_split", "loops": [[0, node.id]]}),
        CHANNEL,
    );
    assert!(plan.writes.len() > 1);
    for w in &plan.writes {
        lane.write(w).await.unwrap();
    }
    let sent: String = fake
        .messages(CHANNEL)
        .iter()
        .map(|m| m.content.as_str())
        .collect();
    assert!(sent.contains("[redacted:courier_token]"), "{sent}");
    assert!(
        !sent.contains(VALUE),
        "a split value must not be reconstructed"
    );
}

#[tokio::test]
async fn live_text_and_notice_summaries_mask_before_split_or_clip() {
    let d = tempfile::tempdir().unwrap();
    let fake = FakeDiscord::start();
    let mut lane = lane(d.path(), &fake, true);
    let mut renderer = lane.shared.renderer();
    renderer.on_notification(
        "turn.started",
        &json!({"session_id": "s", "turn_id": "turn_live"}),
    );
    let text = format!("{}{}{}", "x".repeat(1890), VALUE, "x".repeat(500));
    let ops = renderer.on_notification(
        "model.answered",
        &json!({"turn_id": "turn_live", "loop_index": 0, "text": text}),
    );
    for op in ops {
        lane.take(LaneMsg::Live(op));
    }
    lane.apply_live().await;
    let sent: String = fake
        .messages(CHANNEL)
        .iter()
        .map(|m| m.content.as_str())
        .collect();
    assert!(sent.contains("[redacted:courier_token]"), "{sent}");
    assert!(!sent.contains(VALUE));
    let summary = format!("{}{}", "x".repeat(990), VALUE);
    let mut renderer =
        crate::render::Renderer::new(true).with_exits(lane.shared.core.exits.clone());
    let ops = renderer.on_notification(
        "policy.notified",
        &json!({"session_id": "s", "turn_id": "turn_live",
        "tool_use_id": "u1", "tool": "proc.run", "input": {"argv": ["true"]}, "summary": summary,
        "kind": "notify", "setting": "enforcement = notify", "rule": "notify"}),
    );
    let card = ops
        .into_iter()
        .find_map(|op| match op {
            Op::Notice { card, .. } => Some(card),
            _ => None,
        })
        .unwrap();
    let masked = mask_card(&lane.shared.core.exits, &card);
    assert!(!masked.fields[0].1.contains("vault"), "{:?}", masked.fields);
    assert!(masked.fields[0].1.contains("[redacted"));
}

#[tokio::test]
async fn the_live_preview_never_publishes_a_known_values_prefix() {
    let d = tempfile::tempdir().unwrap();
    let fake = FakeDiscord::start();
    let mut lane = lane(d.path(), &fake, true);
    let mut renderer = lane.shared.renderer();
    renderer.on_notification(
        "turn.started",
        &json!({"session_id": "s", "turn_id": "turn_prefix"}),
    );
    let prefix = &VALUE[..VALUE.len() - 1];
    let ops = renderer.on_notification(
        "model.delta",
        &json!({"turn_id": "turn_prefix", "loop_index": 0, "text": prefix}),
    );
    for op in ops.into_iter().chain(renderer.tick()) {
        lane.take(LaneMsg::Live(op));
    }
    lane.apply_live().await;
    let leaked = fake
        .messages(CHANNEL)
        .iter()
        .any(|m| m.content.contains(prefix));
    let ops = renderer.on_notification(
        "model.answered",
        &json!({"turn_id": "turn_prefix", "loop_index": 0, "text": VALUE}),
    );
    for op in ops {
        lane.take(LaneMsg::Live(op));
    }
    lane.apply_live().await;
    assert!(
        !leaked,
        "a later edit cannot recall the prefix already sent"
    );
    assert_eq!(last(&fake), "[redacted:courier_token]");
}

#[tokio::test]
async fn refolded_tool_inputs_are_masked_before_summary_clipping() {
    let d = tempfile::tempdir().unwrap();
    let fake = FakeDiscord::start();
    let mut lane = lane(d.path(), &fake, true);
    let mut renderer = lane.shared.renderer();
    let body = theseus_core::node::Body::ToolCall {
        tool_use_id: "u1".into(),
        tool: "fs.read".into(),
        wire_name: "fs_read".into(),
        input: json!({"path": format!("{}{}", "x".repeat(75), VALUE)}),
        assistant_node: "msg_1".into(),
        correlation_id: None,
        gate: Some(Box::new(
            serde_json::from_value(json!({"result": {"gate": "allow"}})).unwrap(),
        )),
    };
    let node = theseus_core::node::Node::tool_call("s", Some("old"), Some(0), body);
    renderer.rebuild(&[(0, node)]);
    let ops = renderer.on_notification("tool.ended", &json!({"session_id": "s", "turn_id": "old", "tool_use_id": "u1", "status": "ok", "duration_ms": 12}));
    for op in ops.into_iter().chain(renderer.tick()) {
        lane.take(LaneMsg::Live(op));
    }
    lane.apply_live().await;
    let sent = last(&fake);
    assert!(!sent.contains("vault"), "{sent}");
    assert!(sent.contains("[redacted"), "{sent}");
}


#[tokio::test]
async fn thinking_never_publishes_a_partial_known_value() {
    use crate::render::process::{Process, Shown};
    let d = tempfile::tempdir().unwrap();
    let fake = FakeDiscord::start();
    let mut lane = lane(d.path(), &fake, true);
    let mut process = Process::default().with_exits(lane.shared.core.exits.clone());
    let shown = Shown {
        tools: true,
        thinking: true,
    };
    let now = tokio::time::Instant::now();
    for (name, body) in [
        ("turn.started", json!({"session_id":"s", "turn_id":"think"})),
        (
            "model.thinking",
            json!({"turn_id":"think", "loop_index":0, "text": &VALUE[..VALUE.len()-1]}),
        ),
    ] {
        let e = theseus_protocol::Event::from_notification(name, &body)
            .unwrap()
            .unwrap();
        for op in process
            .on_event(&e, now, shown)
            .into_iter()
            .chain(process.tick(shown))
        {
            lane.take(LaneMsg::Live(op));
        }
        lane.apply_live().await;
    }
    assert!(!fake
        .messages(CHANNEL)
        .iter()
        .any(|m| m.content.contains("vault")));
}
