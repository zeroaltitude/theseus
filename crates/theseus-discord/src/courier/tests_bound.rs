//! A lane's maps stay bounded (theseus-celu.37): `msgs`, `sent`, and `sealed`
//! keep the keys named most recently, `KEYS_KEPT` at most besides the task
//! board's, and a stream state that comes after its post is still dropped.

use std::time::Instant;

use serde_json::json;
use theseus_core::secrets::SecretBoard;
use theseus_sim::fake_discord::FakeDiscord;

use super::*;

/// An invented channel.
const CHANNEL: u64 = 900_000_000_000_000_077;

fn lane(dir: &std::path::Path, fake: &FakeDiscord) -> Lane {
    let secrets = SecretBoard::new([], Instant::now());
    let core = crate::runtime::tests::core_with(dir, secrets, |c| {
        c.discord.rest_proxy = Some(fake.addr.clone());
    });
    let shared = crate::runtime::shared_for_tests(&core);
    let target = format!("discord:channel:{CHANNEL}");
    Lane::new(
        shared,
        target,
        "channel",
        "#harbor".into(),
        Some(CHANNEL),
        None,
    )
}

fn upsert(key: &str, content: &str) -> LaneMsg {
    LaneMsg::Live(Op::Upsert {
        key: key.into(),
        content: content.into(),
        buttons: Buttons::Keep,
    })
}

/// What a reply's post does to one of its keys (`Lane::reply`, then
/// `write`): sealed, then its final text written over the stream's.
async fn post_final(lane: &mut Lane, key: &str, content: &str) {
    lane.seal(key);
    let w = Write {
        key: key.into(),
        channel: CHANNEL,
        content: content.into(),
        buttons: Buttons::Keep,
        reply_to: None,
        message: None,
        mentions: vec![],
    };
    lane.write(&w).await.unwrap();
}

/// A turn's reply: its stream's partial state, then its post's final one.
async fn a_turn(lane: &mut Lane, turn: usize) -> String {
    let key = format!("turn_{turn}:L0:p0");
    lane.take(upsert(&key, &format!("Turn {turn}, partly")));
    lane.apply_live().await;
    post_final(lane, &key, &format!("Turn {turn}, whole.")).await;
    key
}

fn assert_bounded(lane: &Lane) {
    // The board's message is kept aside from the bound.
    let board = usize::from(lane.msgs.contains_key(render::BOARD_KEY));
    let sizes = (
        lane.msgs.len() - board,
        lane.sent.len(),
        lane.sealed.len(),
        lane.touched.len(),
    );
    assert!(
        sizes.0 <= KEYS_KEPT
            && sizes.1 <= KEYS_KEPT
            && sizes.2 <= KEYS_KEPT
            && sizes.3 <= KEYS_KEPT,
        "msgs, sent, sealed, touched: {sizes:?}"
    );
}

/// theseus-celu.37: a lane put through more posts and turns than its bound
/// holds at most the bound in each map, the task board's message aside,
/// which it keeps.
#[tokio::test]
async fn a_lane_put_through_more_posts_than_its_bound_holds_at_most_the_bound() {
    let d = tempfile::tempdir().unwrap();
    let fake = FakeDiscord::start();
    let mut lane = lane(d.path(), &fake);
    lane.msgs.insert(render::BOARD_KEY.into(), (CHANNEL, 1));
    let sid = "sess_harbor";
    let notices: Vec<String> = (0..KEYS_KEPT / 2)
        .map(|i| {
            let body = json!({"kind": "notice", "text": format!("Notice {i}.")});
            let post = lane.shared.core.outbox.post(sid, "", &lane.target, body);
            post.unwrap().correlation_id
        })
        .collect();
    assert!(lane.deliver_posts().await);
    assert_eq!(lane.shared.core.outbox.status("discord").pending, 0);
    for turn in 0..KEYS_KEPT {
        let key = format!("turn_{turn}:L0:p0");
        post_final(&mut lane, &key, &format!("Turn {turn}, whole.")).await;
        assert_bounded(&lane);
    }
    assert_bounded(&lane);
    assert!(
        lane.msgs.contains_key(render::BOARD_KEY),
        "the board is kept"
    );
    // Half again the bound's worth of keys went through: the oldest are
    // gone, the newest kept.
    let oldest = format!("note:{}", notices[0]);
    assert!(!lane.msgs.contains_key(&oldest) && !lane.sent.contains_key(&oldest));
    let newest = format!("turn_{}:L0:p0", KEYS_KEPT - 1);
    assert!(lane.sealed.contains(&newest) && lane.msgs.contains_key(&newest));
    assert_eq!(fake.messages(CHANNEL).len(), KEYS_KEPT / 2 + KEYS_KEPT);
}

/// theseus-celu.37: a stream state that comes after its post is dropped, so
/// a reply's final text is never edited back to a partial one, with the
/// lane's maps at their bound and more posts and turns after it.
#[tokio::test]
async fn a_stream_state_after_its_post_is_still_dropped() {
    let d = tempfile::tempdir().unwrap();
    let fake = FakeDiscord::start();
    let mut lane = lane(d.path(), &fake);
    // The maps at their bound: keys named by a stream, as many as it holds.
    let named = |lane: &mut Lane, from: usize, n: usize| {
        for i in from..from + n {
            lane.take(upsert(&format!("turn_{i}:L0:tools"), "a tool line"));
        }
        lane.live.clear();
    };
    named(&mut lane, 0, KEYS_KEPT + 8);
    let key = a_turn(&mut lane, 9_999).await;
    // Keys named after it, as many as the bound forgets at a time, and more.
    named(&mut lane, 10_000, KEYS_KEPT / 4 + 8);
    assert_bounded(&lane);
    assert!(lane.touched.len() > KEYS_KEPT / 2, "the bound was reached");
    // The stream's late state of the reply's key.
    lane.take(upsert(&key, "Turn 9999, partly"));
    assert!(
        lane.live.is_empty(),
        "a late state of a sealed key is dropped"
    );
    lane.apply_live().await;
    let m = fake
        .messages(CHANNEL)
        .into_iter()
        .find(|m| m.versions[0].starts_with("Turn 9999"))
        .unwrap();
    assert_eq!(m.content, "Turn 9999, whole.", "{:?}", m.versions);
}
