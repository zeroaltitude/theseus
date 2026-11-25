//! A lane back from an outage tells its place only once its posts have all
//! gone (issue 56): a card held for its call, with a post behind it, is not
//! "all gone", though the stream may go on meanwhile. Driven through the
//! lane's own loop and the fake Discord, by what each side observes, never by
//! a sleep: the place's mailbox here is the test's.

use std::time::Duration;

use serde_json::json;
use theseus_core::secrets::SecretBoard;
use theseus_sim::fake_discord::{FakeDiscord, Mode};
use tokio::sync::mpsc;

use super::tests::core_with;
use super::{shared_for_tests, PlaceMsg};
use crate::courier::{Lane, LaneMsg};
use crate::render::{Buttons, Op};

const CHANNEL: u64 = 900_000_000_000_000_077;
const SESSION: &str = "sess_harbor";

fn upsert(key: &str, content: &str) -> LaneMsg {
    LaneMsg::Live(Op::Upsert {
        key: key.into(),
        content: content.into(),
        buttons: Buttons::Keep,
    })
}

/// Waits for `f`, which reads what the fake or the store says.
async fn until(what: &str, f: impl Fn() -> bool) {
    let deadline = std::time::Instant::now() + Duration::from_secs(30);
    while !f() {
        assert!(std::time::Instant::now() < deadline, "timed out: {what}");
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

fn made(fake: &FakeDiscord, s: &str) -> Option<u64> {
    fake.messages(CHANNEL)
        .into_iter()
        .find(|m| m.content.contains(s))
        .map(|m| m.id.parse().unwrap())
}

/// Discord refuses the lane's first post, so it is away; meanwhile a card
/// for a question of a session its place renders is posted, and a notice
/// behind it. Back, the first post goes and the card is held for its call:
/// the place is not told yet, whatever else the lane writes meanwhile. The
/// call's `Asked` then comes together with live progress: the progress may
/// go first (a call's progress goes before its card), then the card and the
/// notice behind it, and only then is the place told, and the fold it
/// writes again lands after all of them.
#[tokio::test]
async fn a_lane_back_tells_its_place_only_when_no_post_waits_not_while_a_card_is_held() {
    let d = tempfile::tempdir().unwrap();
    let fake = FakeDiscord::start();
    let core = core_with(d.path(), SecretBoard::empty(), |c| {
        c.discord.rest_proxy = Some(fake.addr.clone());
    });
    let shared = shared_for_tests(&core);
    let target = format!("discord:channel:{CHANNEL}");
    let (place, mut mailbox) = mpsc::unbounded_channel::<PlaceMsg>();
    {
        let mut r = shared.routes.lock().unwrap();
        r.by_channel.insert(CHANNEL, place.clone());
        r.by_session.insert(SESSION.into(), place);
    }
    let lane = Lane::new(
        shared.clone(),
        target.clone(),
        "channel",
        "#harbor".into(),
        Some(CHANNEL),
        None,
    );
    let (tx, rx) = mpsc::unbounded_channel();
    let run = tokio::spawn(lane.run(rx));
    let outbox = core.outbox.clone();
    let failed = || fake.seen().iter().any(|s| s.outcome == "failed");

    fake.set_mode(Mode::Fail);
    let notice = |t: &str| json!({"kind": "notice", "text": t});
    outbox
        .post(SESSION, "", &target, notice("First, refused."))
        .unwrap();
    tx.send(LaneMsg::Wake).unwrap();
    until("the lane refused by Discord", failed).await;
    // The card's question: an action of the place's session.
    let q = outbox
        .post(SESSION, "", "discord:channel:1", notice("the question"))
        .unwrap()
        .correlation_id;
    outbox
        .post(SESSION, "", &target, json!({"kind": "card", "question": q}))
        .unwrap();
    outbox
        .post(SESSION, "", &target, notice("Behind the card."))
        .unwrap();
    fake.set_mode(Mode::Up);
    tx.send(LaneMsg::Wake).unwrap();
    until("the first post, once Discord is back", || {
        made(&fake, "First, refused.").is_some()
    })
    .await;
    // A live message through the lane: written after the posts' turn of the
    // loop that met the card, so the lane's decision for that turn is made.
    tx.send(upsert("probe:a", "probe")).unwrap();
    until("the probe", || made(&fake, "probe").is_some()).await;
    // The lane's posts still open: the card and the notice behind it while
    // the card is held.
    let pending = || outbox.open_for(&target).len();
    if let Ok(m) = mailbox.try_recv() {
        assert!(matches!(m, PlaceMsg::LaneBack));
        assert_eq!(pending(), 0, "the place was told while the card was held");
    }

    // The call's Asked, with live progress, together.
    tx.send(LaneMsg::Asked(q.clone())).unwrap();
    tx.send(upsert("turn_q:L0:p0", "Progress first.")).unwrap();
    let told = tokio::time::timeout(Duration::from_secs(30), mailbox.recv())
        .await
        .expect("the place is told")
        .expect("a message");
    assert!(matches!(told, PlaceMsg::LaneBack));
    assert_eq!(pending(), 0, "every post gone before the place is told");
    let behind = made(&fake, "Behind the card.").expect("the post behind the card");
    // The place writes its fold again, now.
    tx.send(upsert("turn_q:fold", "-# 1 step: 1 command")).unwrap();
    until("the fold again", || made(&fake, "1 step: 1 command").is_some()).await;
    let fold = made(&fake, "1 step: 1 command").unwrap();
    assert!(behind < fold, "the fold after every post");
    assert!(made(&fake, "Progress first.").is_some());
    drop(tx);
    run.await.unwrap();
}
