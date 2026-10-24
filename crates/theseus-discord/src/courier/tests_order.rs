//! A lane's order (theseus-l1y1): a loop's thinking, queued as a new message
//! when a post is waiting, goes before the post, as a call's progress goes
//! before its card (theseus-50p); any other new live message goes after the
//! lane's posts, as always. Both are queued before the lane runs, so the
//! order is the lane's alone, whatever the load.

use serde_json::json;
use theseus_sim::fake_discord::FakeDiscord;

use super::tests_bound::{lane, upsert, CHANNEL};

/// The channel's messages, in the order they were made, once `n` are there.
async fn made(fake: &FakeDiscord, n: usize) -> Vec<String> {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    loop {
        let got = fake.messages(CHANNEL);
        if got.len() >= n {
            return got.into_iter().map(|m| m.content).collect();
        }
        assert!(
            std::time::Instant::now() < deadline,
            "{n} messages: {got:#?}"
        );
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
}

/// A post and one new live message, both waiting when the lane wakes.
async fn order_of(key: &str, content: &str) -> Vec<String> {
    let d = tempfile::tempdir().unwrap();
    let fake = FakeDiscord::start();
    let lane = lane(d.path(), &fake);
    let body = json!({"kind": "notice", "text": "The tide turned."});
    let outbox = &lane.shared.core.outbox;
    outbox.post("sess_harbor", "", &lane.target, body).unwrap();
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
    tx.send(upsert(key, content)).unwrap();
    let run = tokio::spawn(lane.run(rx));
    let got = made(&fake, 2).await;
    drop(tx);
    run.await.unwrap();
    got
}

#[tokio::test]
async fn a_loops_thinking_goes_before_the_lanes_next_post() {
    let thinking = "-# 💭 thinking\n-# The chart is in work/.";
    let got = order_of("turn_a:L0:tools", thinking).await;
    assert_eq!(got.len(), 2, "{got:#?}");
    assert_eq!(got[0], thinking, "the thinking first: {got:#?}");
    assert!(got[1].contains("The tide turned."), "{got:#?}");
    // A tool line alone waits for the posts, as it always did.
    let line = "▫️ `fs.read` work/chart";
    let got = order_of("turn_a:L0:tools", line).await;
    assert!(
        got[0].contains("The tide turned."),
        "the post first: {got:#?}"
    );
    assert_eq!(got[1], line, "{got:#?}");
}

/// An invented owner's message, the turn's anchor.
const ASKED: u64 = 900_000_000_000_000_101;

/// A turn's reply post whose loop 0 said `text`, answering `to`.
fn reply_post(lane: &super::Lane, turn: &str, text: &str, to: u64) {
    let core = &lane.shared.core;
    let node = crate::tests_cite::assistant(core, "sess_harbor", text);
    let mut body = crate::tests_cite::reply_body(turn, &node, "sess_harbor", vec![]);
    body["loops"] = json!([[0, node]]);
    body["reply_to"] = json!(to.to_string());
    core.outbox
        .post("sess_harbor", "", &lane.target, body)
        .unwrap();
}

/// The channel's messages once `done` holds of them and no post waits.
async fn settled(
    fake: &FakeDiscord,
    outbox: &theseus_core::outbox::Outbox,
    done: impl Fn(&[theseus_sim::fake_discord::Msg]) -> bool,
) -> Vec<theseus_sim::fake_discord::Msg> {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    loop {
        let got = fake.messages(CHANNEL);
        if done(&got) && outbox.status("discord").pending == 0 {
            return got;
        }
        assert!(std::time::Instant::now() < deadline, "{got:#?}");
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
}

/// theseus-nxnqc: the turn's reply post and its stream, all waiting when the
/// lane wakes (a lane that slept through the turn, under load). The post goes
/// first and, with nothing of the turn streamed, answers the owner's message;
/// the stream's new tool line after it must not answer it a second time.
#[tokio::test]
async fn a_turn_replies_to_its_message_once_when_its_post_goes_before_its_stream() {
    let d = tempfile::tempdir().unwrap();
    let fake = FakeDiscord::start();
    let lane = lane(d.path(), &fake);
    reply_post(&lane, "turn_reef", "Starting.", ASKED);
    let outbox = lane.shared.core.outbox.clone();
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
    tx.send(super::LaneMsg::Anchor(ASKED)).unwrap();
    tx.send(upsert("turn_reef:L0:p0", "Starting.")).unwrap();
    let line = "✅ `fs.read` reef.txt";
    tx.send(upsert("turn_reef:L0:tools", line)).unwrap();
    let run = tokio::spawn(lane.run(rx));
    let got = settled(&fake, &outbox, |m| {
        m.iter().any(|m| m.content.starts_with("Starting.")) && m.iter().any(|m| m.content == line)
    })
    .await;
    drop(tx);
    run.await.unwrap();
    let asked = ASKED.to_string();
    let replies: Vec<&str> = got
        .iter()
        .filter(|m| m.reply_to.as_deref() == Some(asked.as_str()))
        .map(|m| m.content.as_str())
        .collect();
    assert_eq!(
        replies.len(),
        1,
        "one reply to the owner's message: {got:#?}"
    );
    // The scenario: the post wrote the turn's text, as a reply, before the
    // stream wrote anything of the turn.
    assert!(replies[0].starts_with("Starting."), "{got:#?}");
    assert!(
        got[0].content.starts_with("Starting."),
        "the post first: {got:#?}"
    );
    let tools: Vec<_> = got.iter().filter(|m| m.content == line).collect();
    assert_eq!(tools.len(), 1, "{got:#?}");
    assert_eq!(tools[0].reply_to, None, "the tool line replies to nothing");
}

/// theseus-nxnqc: an older turn's post leaves a newer anchor (the owner's next
/// message, already heard) to the turn that answers it.
#[tokio::test]
async fn a_newer_anchor_survives_an_older_turns_post() {
    const NEXT: u64 = 900_000_000_000_000_102;
    let d = tempfile::tempdir().unwrap();
    let fake = FakeDiscord::start();
    let lane = lane(d.path(), &fake);
    reply_post(&lane, "turn_reef", "Starting.", ASKED);
    let outbox = lane.shared.core.outbox.clone();
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
    tx.send(super::LaneMsg::Anchor(ASKED)).unwrap();
    tx.send(super::LaneMsg::Anchor(NEXT)).unwrap();
    tx.send(upsert("turn_b:L0:p0", "Next, the jetty.")).unwrap();
    let run = tokio::spawn(lane.run(rx));
    let got = settled(&fake, &outbox, |m| {
        m.iter().any(|m| m.content.starts_with("Starting."))
            && m.iter().any(|m| m.content == "Next, the jetty.")
    })
    .await;
    drop(tx);
    run.await.unwrap();
    let next: Vec<_> = got
        .iter()
        .filter(|m| m.content == "Next, the jetty.")
        .collect();
    assert_eq!(next.len(), 1, "{got:#?}");
    assert_eq!(next[0].reply_to, Some(NEXT.to_string()), "{got:#?}");
    let first: Vec<_> = got
        .iter()
        .filter(|m| m.content.starts_with("Starting."))
        .collect();
    assert_eq!(first[0].reply_to, Some(ASKED.to_string()), "{got:#?}");
}
