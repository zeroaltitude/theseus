//! A lane after a restart edits the messages a turn wrote before it, rather
//! than posting them again (issue 8): the lane's map from a key to its message
//! is memory, so it is rebuilt from the durable `discord.message.out` rows, which
//! now carry the channel.

use std::time::Duration;

use serde_json::json;
use theseus_protocol::LedgerKind;
use theseus_sim::fake_discord::FakeDiscord;

use super::tests_bound::{lane, upsert, CHANNEL};
use super::*;

const KEY: &str = "turn_restart:L1:p0";

/// A lane with no memory, over the same core and place as `old`: what a new
/// process builds when the binding starts again.
fn restarted(old: &Lane) -> Lane {
    Lane::new(
        old.shared.clone(),
        old.target.clone(),
        "channel",
        old.label.clone(),
        Some(CHANNEL),
        None,
    )
}

/// The rows of the lane's creates, once the ledger answers them.
async fn rows(lane: &Lane, want: usize) -> Vec<serde_json::Value> {
    let got = tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let tail: theseus_protocol::LedgerTailResult = lane
                .shared
                .rpc
                .call(
                    theseus_protocol::method::LEDGER_TAIL,
                    theseus_protocol::LedgerTailParams {
                        n: Some(1000),
                        kind: Some(LedgerKind::DiscordMessageOut.as_str().into()),
                        ..Default::default()
                    },
                )
                .await
                .unwrap();
            if tail.rows.len() >= want {
                break tail.rows;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    })
    .await
    .expect("the ledger answers the row");
    got.into_iter().map(|r| r.data).collect()
}

fn shown(fake: &FakeDiscord) -> Vec<String> {
    fake.messages(CHANNEL)
        .iter()
        .map(|m| m.content.clone())
        .collect()
}

/// The red: before, the first write of a key after a restart posted a second
/// message (past Discord's nonce window, which is all that held it before),
/// and left the first as it was. Now it edits the one the row names.
#[tokio::test]
async fn a_restarted_lane_edits_the_message_a_turn_wrote_before_the_restart() {
    let d = tempfile::tempdir().unwrap();
    let fake = FakeDiscord::start();
    let mut first = lane(d.path(), &fake);
    first.take(upsert(KEY, "Harbor report, so far"));
    first.apply_live().await;
    assert_eq!(shown(&fake), ["Harbor report, so far"]);
    let made = rows(&first, 1).await;
    assert_eq!(made[0]["channel"], CHANNEL.to_string(), "{made:?}");
    assert_eq!(made[0]["part"], KEY);

    fake.set_nonce_window_ms(0);
    let mut again = restarted(&first);
    assert!(again.msgs.is_empty(), "a new process remembers nothing");
    again.take(upsert(KEY, "Harbor report, so far, and the rest"));
    again.apply_live().await;
    assert_eq!(
        shown(&fake),
        ["Harbor report, so far, and the rest"],
        "edited in place, not posted again"
    );
    assert!(again.msgs.contains_key(KEY), "and remembered again");
}

/// A key no row names is posted, as any new key is: the rebuild finds only
/// what was written.
#[tokio::test]
async fn a_key_no_row_names_is_posted_after_a_restart() {
    let d = tempfile::tempdir().unwrap();
    let fake = FakeDiscord::start();
    let mut first = lane(d.path(), &fake);
    first.take(upsert(KEY, "one"));
    first.apply_live().await;
    rows(&first, 1).await;
    let mut again = restarted(&first);
    again.take(upsert("turn_new:L1:p0", "two"));
    again.apply_live().await;
    assert_eq!(shown(&fake), ["one", "two"]);
}

/// A row an older build wrote has no channel: its message cannot be edited,
/// so the key is posted again, as before. And another place's row, though it
/// names the same key and even the same label (two channels can share a
/// name), is not this lane's: its message is never touched.
#[tokio::test]
async fn a_row_with_no_channel_or_of_another_place_is_not_used() {
    let d = tempfile::tempdir().unwrap();
    let fake = FakeDiscord::start();
    let first = lane(d.path(), &fake);
    let core = first.shared.core.clone();
    core.binding_ledger(
        LedgerKind::DiscordMessageOut,
        None,
        json!({"place": first.label, "target": first.target, "message_id": "111", "part": KEY}),
    );
    core.binding_ledger(
        LedgerKind::DiscordMessageOut,
        None,
        json!({"place": first.label, "target": "discord:channel:333", "message_id": "222", "channel": "333", "part": KEY}),
    );
    rows(&first, 2).await;
    let mut again = restarted(&first);
    again.take(upsert(KEY, "posted"));
    again.apply_live().await;
    assert_eq!(shown(&fake), ["posted"], "a new message, in this channel");
    assert!(
        !fake
            .seen()
            .iter()
            .any(|s| s.path.contains("/333/") || s.path.contains("/222")),
        "another place's message was addressed: {:?}",
        fake.seen()
    );
    assert_eq!(again.msgs.get(KEY).map(|m| m.0), Some(CHANNEL));
}

/// The lane's bound holds: what the rows give back goes through the same
/// recency bookkeeping as any key, so a lane that recalls many keys is held
/// to `KEYS_KEPT` of them like one that wrote them.
#[tokio::test]
async fn what_is_recalled_is_held_to_the_lanes_bound() {
    let d = tempfile::tempdir().unwrap();
    let fake = FakeDiscord::start();
    let first = lane(d.path(), &fake);
    let core = first.shared.core.clone();
    let n = KEYS_KEPT + 20;
    for i in 0..n {
        core.binding_ledger(
            LedgerKind::DiscordMessageOut,
            None,
            json!({"place": first.label, "target": first.target, "message_id": (1000 + i).to_string(),
                   "channel": CHANNEL.to_string(), "part": format!("turn_{i}:L1:p0")}),
        );
    }
    rows(&first, n).await;
    let mut again = restarted(&first);
    for i in 0..n {
        assert!(again.recall(&format!("turn_{i}:L1:p0")).await.is_some());
    }
    assert!(
        again.msgs.len() <= KEYS_KEPT + 1,
        "{} keys held of {n} recalled",
        again.msgs.len()
    );
}

/// The two halves together, end to end: a turn that was running when the
/// process ended is read back by a renderer that never saw it start; its late
/// call end comes out under the keys the first run wrote, and a lane with no
/// memory edits those messages, so the turn's text and its tool line each show
/// once.
#[tokio::test]
async fn a_late_end_after_a_restart_edits_the_turns_messages_and_posts_nothing_twice() {
    use crate::render::Renderer;
    use serde_json::json;

    let d = tempfile::tempdir().unwrap();
    let fake = FakeDiscord::start();
    let mut first = lane(d.path(), &fake);
    let core = first.shared.core.clone();
    let turn = "turn_late_end";
    // The first run: the turn's text and its call's line were shown.
    let mut live = Renderer::default();
    let mut ops =
        live.on_notification("turn.started", &json!({"session_id": "s", "turn_id": turn}));
    ops.extend(live.on_notification(
        "model.delta",
        &json!({"turn_id": turn, "loop_index": 0, "text": "Reading the chart now."}),
    ));
    ops.extend(live.on_notification(
        "tool.proposed",
        &json!({"turn_id": turn, "tool_use_id": "u1", "tool": "fs.read",
                "input": {"path": "src/lib.rs"}, "gate": {"result": {"gate": "allow"}}}),
    ));
    ops.extend(live.tick());
    for op in ops {
        first.take(LaneMsg::Live(op));
    }
    first.apply_live().await;
    assert_eq!(
        shown(&fake).len(),
        2,
        "the text and the tool line: {:?}",
        shown(&fake)
    );
    rows(&first, 2).await;
    // What the session stored of it.
    let gate = serde_json::from_value(json!({"result": {"gate": "allow"}})).unwrap();
    let nodes = [
        theseus_core::node::Node::assistant(
            "s",
            turn,
            0,
            theseus_core::node::Body::AssistantMessage {
                blocks: vec![json!({"type": "text", "text": "Reading the chart now."})],
                model: "glm-x".into(),
                provider: "zed".into(),
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
        ),
        theseus_core::node::Node::tool_call(
            "s",
            Some(turn),
            Some(0),
            theseus_core::node::Body::ToolCall {
                tool_use_id: "u1".into(),
                tool: "fs.read".into(),
                wire_name: "fs_read".into(),
                input: json!({"path": "src/lib.rs"}),
                assistant_node: "msg_1".into(),
                correlation_id: None,
                gate: Some(Box::new(gate)),
            },
        ),
    ];
    for n in &nodes {
        core.store.append(&[n.record().unwrap()]).unwrap();
    }

    // The process starts again: a renderer and a lane with no memory.
    fake.set_nonce_window_ms(0);
    let mut renderer = Renderer::default();
    let mut again = restarted(&first);
    let stored = core.store.session_nodes("s").unwrap();
    let late = theseus_protocol::Event::from_notification(
        "tool.ended",
        &json!({"session_id": "s", "turn_id": turn, "tool_use_id": "u1", "status": "ok", "duration_ms": 12}),
    )
    .unwrap()
    .unwrap();
    assert!(renderer.needs_rebuild(&late));
    renderer.rebuild(&stored);
    let mut ops = renderer.on_notification(
        "tool.ended",
        &json!({"turn_id": turn, "tool_use_id": "u1", "status": "ok", "duration_ms": 12}),
    );
    ops.extend(renderer.on_notification("loop.ended", &json!({"turn_id": turn, "loop_index": 0})));
    ops.extend(renderer.tick());
    for op in ops {
        again.take(LaneMsg::Live(op));
    }
    again.apply_live().await;
    let all = shown(&fake);
    assert_eq!(all.len(), 2, "nothing posted twice: {all:?}");
    assert!(all.iter().any(|m| m == "Reading the chart now."), "{all:?}");
    assert!(
        all.iter()
            .any(|m| m == "-# ✅ `fs.read` src/lib.rs · 12 ms"),
        "the tool line was edited to its end: {all:?}"
    );
}
