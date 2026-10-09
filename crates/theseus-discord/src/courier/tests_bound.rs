//! A lane's maps stay bounded (theseus-celu.37): `msgs`, `sent`, and `sealed`
//! keep the keys named most recently, `KEYS_KEPT` at most besides the task
//! board's and the held turns' (theseus-6809), and a stream state that comes
//! after its post is still dropped.

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
        ping: false,
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
    // The board's message and the held turns' are kept aside from the bound.
    let aside = |k: &String| k == render::BOARD_KEY || lane.held_key(k);
    let sizes = (
        lane.msgs.keys().filter(|k| !aside(k)).count(),
        lane.sent.keys().filter(|k| !aside(k)).count(),
        lane.sealed.iter().filter(|k| !aside(k)).count(),
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

/// theseus-6809: a background job's tool line, in a turn the renderer still
/// holds, is edited by its late result after later turns named more keys
/// than the bound holds, past Discord's nonce window; and forgotten when the
/// renderer drops its turn.
#[tokio::test]
async fn a_held_turns_tool_line_is_edited_after_later_turns_pass_the_bound() {
    let d = tempfile::tempdir().unwrap();
    let fake = FakeDiscord::start();
    let mut lane = lane(d.path(), &fake);
    // The lane at its bound, with keys of no turn.
    for i in 0..KEYS_KEPT {
        lane.take(upsert(&format!("glide:call_{i}"), &format!("Glide {i}.")));
    }
    lane.apply_live().await;
    assert_eq!(lane.touched.len(), KEYS_KEPT, "the bound was reached");
    // What the place's actor sends at each turn's start: the held turns.
    let turns: Vec<String> = (0..8).map(|n| format!("turn_held_{n}")).collect();
    lane.take(LaneMsg::Held(turns[..1].to_vec()));
    let job = format!("{}:L0:tools", turns[0]);
    lane.take(upsert(
        &job,
        "⚙️ `proc.run` long-build · ⏳ running in the background",
    ));
    lane.apply_live().await;
    // Seven turns of 19 loops each: a text part and a tool line a loop.
    for n in 1..8 {
        lane.take(LaneMsg::Held(turns[..=n].to_vec()));
        for l in 0..19 {
            let t = &turns[n];
            lane.take(upsert(&format!("{t}:L{l}:p0"), &format!("Loop {l}.")));
            lane.take(upsert(
                &format!("{t}:L{l}:tools"),
                &format!("⚙️ `fs.read` loop {l} · ✅ ok"),
            ));
            lane.apply_live().await;
        }
    }
    assert_bounded(&lane);
    // Discord no longer returns the first message for the line's nonce.
    fake.set_nonce_window_ms(0);
    lane.take(upsert(&job, "⚙️ `proc.run` long-build · ✅ ok · 312000 ms"));
    lane.apply_live().await;
    let lines: Vec<_> = fake
        .messages(CHANNEL)
        .into_iter()
        .filter(|m| m.versions[0].contains("long-build"))
        .collect();
    assert_eq!(lines.len(), 1, "one tool line: {lines:#?}");
    assert_eq!(
        lines[0].content,
        "⚙️ `proc.run` long-build · ✅ ok · 312000 ms"
    );
    // The renderer drops the job's turn: the lane forgets its keys.
    let mut next = turns[1..].to_vec();
    next.push("turn_held_8".into());
    lane.take(LaneMsg::Held(next));
    let of_job = |k: &String| k.starts_with(&format!("{}:", turns[0]));
    assert!(
        !lane.msgs.keys().any(of_job)
            && !lane.sent.keys().any(of_job)
            && !lane.sealed.iter().any(of_job)
            && !lane.touched.keys().any(of_job),
        "the dropped turn's keys are forgotten"
    );
    assert!(lane.msgs.contains_key(&format!("{}:L18:tools", turns[7])));
    assert_bounded(&lane);
}

/// theseus-8u7m: the task board, written through the lane as the daemon
/// writes it (a live upsert under `BOARD_KEY`), is kept past the bound: after
/// more keys than the bound holds and past Discord's nonce window, its next
/// state edits the one board, pinned once.
#[tokio::test]
async fn the_board_written_through_the_lane_is_edited_past_the_bound() {
    let d = tempfile::tempdir().unwrap();
    let fake = FakeDiscord::start();
    let mut lane = lane(d.path(), &fake);
    let board = |line: &str| format!("{}\n- `…a1b2c3` {line}", render::BOARD_HEAD);
    lane.take(upsert(render::BOARD_KEY, &board("Tidy the slipway — open")));
    lane.apply_live().await;
    for i in 0..KEYS_KEPT {
        lane.take(upsert(&format!("glide:call_{i}"), &format!("Glide {i}.")));
    }
    lane.apply_live().await;
    assert_bounded(&lane);
    fake.set_nonce_window_ms(0);
    lane.take(upsert(render::BOARD_KEY, &board("Tidy the slipway — done")));
    lane.apply_live().await;
    let boards: Vec<_> = fake
        .messages(CHANNEL)
        .into_iter()
        .filter(|m| m.versions[0].starts_with(render::BOARD_HEAD))
        .collect();
    assert_eq!(boards.len(), 1, "one board: {boards:#?}");
    assert_eq!(boards[0].content, board("Tidy the slipway — done"));
    assert!(boards[0].pinned);
    let pins = fake
        .seen()
        .into_iter()
        .filter(|s| s.method == "PUT" && s.path.contains("/pins/"))
        .count();
    assert_eq!(pins, 1, "pinned once");
}
