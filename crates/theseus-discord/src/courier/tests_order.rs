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
