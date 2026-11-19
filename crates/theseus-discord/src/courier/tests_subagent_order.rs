//! A subagent's answer in the order it happened, as a direct message draws it (issue 28): the
//! words the turn wrote before the call, the call's line, the subagent's message with its icon and
//! its footer, then the words the turn wrote after the call. The turn's posts come at its end, so
//! the answer is shown when its call settles (the stream), and the post finds that message by its
//! key and does not make a second.

use serde_json::{json, Value};
use theseus_sim::fake_discord::FakeDiscord;

use super::tests_bound::{lane_with, CHANNEL};
use super::{Lane, LaneMsg};
use crate::render::{Op, Renderer};

const TURN: &str = "turn_order";
const SESSION: &str = "sess_harbor";
const BEFORE: &str = "I will ask haiku.";
const WORDS: &str = "Hello from the other model.";
const AFTER: &str = "It said hello.";
/// The footer of the subagent's message, as the owner reads it.
const FOOTER: &str =
    "-# haiku · claude-haiku-5-5 · subagent answer, asked by sonnet · $0.0031 · 1.3 s";

/// A lane in a direct message: the place the owner reads.
fn dm_lane(dir: &std::path::Path, fake: &FakeDiscord) -> Lane {
    let plain = lane_with(dir, fake, |_| {});
    Lane::new(
        plain.shared,
        "discord:dm:900000000000000201".into(),
        "dm",
        "DM".into(),
        Some(CHANNEL),
        Some(900_000_000_000_000_201),
    )
}

fn answer(call: &str) -> Value {
    json!({"profile": "haiku", "provider": "anthropic", "model": "claude-haiku-5-5",
        "asked_by": "sonnet", "text": WORDS, "cost_usd": 0.0031, "elapsed_ms": 1300,
        "output_tokens": 12, "cut": false, "call": call})
}

/// What the renderer shows for a turn that wrote `BEFORE`, asked a profile, and wrote `AFTER`: the
/// live ops, in the order the events came.
fn the_turns_stream(call: &str) -> Vec<Op> {
    stream(call, true)
}

/// [`the_turns_stream`], for a fast call: with `quick` the loop's end has not come and the lane has
/// not ticked since the call began, so the call's line is not made until something makes it.
fn stream(call: &str, quick: bool) -> Vec<Op> {
    let loop_ends_first = !quick;
    let mut r = Renderer::default();
    let mut ops = Vec::new();
    let mut tell = |r: &mut Renderer, method: &str, p: Value| {
        ops.extend(r.on_notification(method, &p));
        if !quick {
            ops.extend(r.tick());
        }
    };
    tell(
        &mut r,
        "turn.started",
        json!({"session_id": SESSION, "turn_id": TURN}),
    );
    tell(
        &mut r,
        "model.delta",
        json!({"turn_id": TURN, "loop_index": 0, "text": BEFORE}),
    );
    tell(
        &mut r,
        "tool.proposed",
        json!({"turn_id": TURN, "tool_use_id": "u1", "tool": "profile.ask",
            "input": {"profile": "haiku", "question": "Say hello."},
            "gate": {"result": {"gate": "allow"}}}),
    );
    if loop_ends_first {
        tell(
            &mut r,
            "loop.ended",
            json!({"turn_id": TURN, "loop_index": 0}),
        );
    }
    tell(
        &mut r,
        "tool.ended",
        json!({"turn_id": TURN, "tool_use_id": "u1", "tool": "profile.ask", "status": "ok",
            "duration_ms": 1300, "subagent": answer(call)}),
    );
    tell(
        &mut r,
        "model.delta",
        json!({"turn_id": TURN, "loop_index": 1, "text": AFTER}),
    );
    ops
}

/// The turn's posts, as its end stages them: the subagent's answer, then the reply whose loops are
/// the words before the call and the words after it.
fn the_turns_posts(shared: &std::sync::Arc<crate::runtime::Shared>, target: &str, call: &str) {
    let core = &shared.core;
    core.outbox
        .post(
            SESSION,
            "",
            target,
            json!({"kind": "subagent", "turn_id": TURN, "session_id": SESSION, "reply_to": null,
                "answer": answer(call)}),
        )
        .unwrap();
    let (a, b) = (
        crate::tests_cite::assistant(core, SESSION, BEFORE),
        crate::tests_cite::assistant(core, SESSION, AFTER),
    );
    let mut reply = crate::tests_cite::reply_body(TURN, &b, SESSION, vec![]);
    reply["loops"] = json!([[0, a], [1, b]]);
    core.outbox.post(SESSION, "", target, reply).unwrap();
}

/// The direct message once `n` messages are there and no post waits.
async fn drawn(
    fake: &FakeDiscord,
    outbox: &theseus_core::outbox::Outbox,
    n: usize,
    posts_done: bool,
) -> Vec<theseus_sim::fake_discord::Msg> {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    loop {
        let got = fake.messages(CHANNEL);
        let quiet = !posts_done || outbox.status("discord").pending == 0;
        if got.len() >= n && quiet {
            return got;
        }
        assert!(std::time::Instant::now() < deadline, "{n}: {got:#?}");
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
}

/// The owner reads, top to bottom: the words before the call, its line, the subagent's message
/// (opening with its icon, ending in its footer), the words after the call with the reply's own
/// footer. The post at the turn's end makes no second message of the answer.
#[tokio::test]
async fn a_direct_message_reads_a_subagents_answer_where_it_happened() {
    let d = tempfile::tempdir().unwrap();
    let fake = FakeDiscord::start();
    let lane = dm_lane(d.path(), &fake);
    let outbox = lane.shared.core.outbox.clone();
    let call = "act_order";
    let ops = the_turns_stream(call);
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
    for op in ops {
        tx.send(LaneMsg::Live(op)).unwrap();
    }
    // The stream is drawn as the turn goes; its posts come at its end.
    let posts = lane.shared.clone();
    let target = lane.target.clone();
    let run = tokio::spawn(lane.run(rx));
    drawn(&fake, &outbox, 4, false).await;
    the_turns_posts(&posts, &target, call);
    tx.send(LaneMsg::Wake).unwrap();
    let got = drawn(&fake, &outbox, 4, true).await;
    drop(tx);
    run.await.unwrap();
    let said: Vec<&str> = got.iter().map(|m| m.content.as_str()).collect();
    assert_eq!(got.len(), 4, "{said:#?}");
    assert!(said[0].starts_with(BEFORE), "{said:#?}");
    assert!(
        said[1].contains("profile.ask"),
        "the call's line: {said:#?}"
    );
    assert!(
        said[2].starts_with(&format!(
            "{} {WORDS}",
            theseus_protocol::subagent::SUBAGENT_ICON
        )),
        "the subagent's message opens with its icon: {said:#?}"
    );
    assert!(
        said[2].ends_with(FOOTER),
        "and ends in its footer: {said:#?}"
    );
    assert!(said[3].starts_with(AFTER), "{said:#?}");
    assert!(
        !said[3].contains("subagent") && said[3].contains("\n-# "),
        "the reply's own footer, on the last part: {said:#?}"
    );
    assert!(
        !said[0].contains("\n-# ") || said[0].contains(BEFORE),
        "the footer belongs to the last part: {said:#?}"
    );
    assert_eq!(
        said.iter()
            .filter(|m| m.contains("subagent answer"))
            .count(),
        1,
        "one message of the answer: {said:#?}"
    );
}

/// The footer is on the subagent's message in a direct message when the answer is drawn from its
/// post alone (a stream that never saw the call settle, a restart, Discord away).
#[tokio::test]
async fn the_subagents_message_carries_its_footer_when_only_its_post_draws_it() {
    let d = tempfile::tempdir().unwrap();
    let fake = FakeDiscord::start();
    let lane = dm_lane(d.path(), &fake);
    let outbox = lane.shared.core.outbox.clone();
    the_turns_posts(&lane.shared, &lane.target, "act_post");
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
    tx.send(LaneMsg::Wake).unwrap();
    let run = tokio::spawn(lane.run(rx));
    let got = drawn(&fake, &outbox, 3, true).await;
    drop(tx);
    run.await.unwrap();
    let sub = got
        .iter()
        .find(|m| m.content.contains("subagent answer"))
        .expect("the subagent's message");
    assert!(sub.content.starts_with("🛰️ "), "{}", sub.content);
    assert!(sub.content.ends_with(FOOTER), "{}", sub.content);
}

/// The stream says it in order: the words before the call, the call's line, the subagent's message,
/// then the words after the call. A subagent's message made last would be drawn below them.
#[test]
fn the_stream_makes_the_answer_between_the_call_and_the_words_after_it() {
    let keys: Vec<String> = the_turns_stream("act_keys")
        .iter()
        .filter_map(|o| o.key().map(str::to_string))
        .fold(Vec::new(), |mut seen, k| {
            if !seen.contains(&k) {
                seen.push(k);
            }
            seen
        });
    assert_eq!(
        keys,
        [
            format!("{TURN}:L0:p0"),
            format!("{TURN}:L0:tools"),
            "subagent:act_keys:p0".to_string(),
            format!("{TURN}:L1:p0"),
        ]
    );
}

/// The call's line is made before the answer even when the call settles before the lane has ticked
/// since it began (a fast call): the answer waits behind the line it answers.
#[test]
fn the_calls_line_is_made_before_the_answer_when_the_call_was_fast() {
    let keys: Vec<String> = stream("act_early", true)
        .iter()
        .filter_map(|o| o.key().map(str::to_string))
        .collect();
    let at = |k: &str| keys.iter().position(|x| x == k);
    let (line, said) = (
        at(&format!("{TURN}:L0:tools")).expect("the call's line"),
        at("subagent:act_early:p0").expect("the answer"),
    );
    assert!(line < said, "{keys:?}");
}

/// A turn stopped after its call settled: the live message stays, and the post edits it to say
/// the turn that asked was stopped. One message of the answer, with the note on its footer.
#[tokio::test]
async fn a_stopped_turns_live_message_stays_and_is_footed_as_the_answer_of_a_stopped_turn() {
    let d = tempfile::tempdir().unwrap();
    let fake = FakeDiscord::start();
    let lane = dm_lane(d.path(), &fake);
    let outbox = lane.shared.core.outbox.clone();
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
    for op in the_turns_stream("act_stop") {
        tx.send(LaneMsg::Live(op)).unwrap();
    }
    let (shared, target) = (lane.shared.clone(), lane.target.clone());
    let run = tokio::spawn(lane.run(rx));
    drawn(&fake, &outbox, 4, false).await;
    shared
        .core
        .outbox
        .post(
            SESSION,
            "",
            &target,
            json!({"kind": "subagent", "turn_id": TURN, "session_id": SESSION, "reply_to": null,
                "turn_ended": "stopped", "answer": answer("act_stop")}),
        )
        .unwrap();
    tx.send(LaneMsg::Wake).unwrap();
    let got = drawn(&fake, &outbox, 4, true).await;
    drop(tx);
    run.await.unwrap();
    let sub: Vec<&str> = got
        .iter()
        .map(|m| m.content.as_str())
        .filter(|c| c.contains("subagent answer"))
        .collect();
    assert_eq!(sub.len(), 1, "{sub:#?}");
    assert!(sub[0].starts_with("🛰️ "), "{sub:#?}");
    assert!(
        sub[0].ends_with("1.3 s · the turn that asked was stopped"),
        "{sub:#?}"
    );
}
