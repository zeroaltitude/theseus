//! A reply delivered after its turn left the renderer's held list posts each
//! part once (theseus-llo6): the lane keeps a released turn's keys while an
//! open post of its place names the turn, so the post edits the messages the
//! stream wrote instead of creating them again.

use std::sync::Arc;

use serde_json::json;
use theseus_sim::fake_discord::FakeDiscord;

use super::tests_bound::{lane, upsert, CHANNEL};
use super::*;
use crate::tests_cite::{assistant, reply_body};

const TURN: &str = "turn_late";
const SESSION: &str = "sess_harbor";
const FIRST: &str = "First part, final.";
const SECOND: &str = "Second part, final.";

/// How many of the channel's messages now read `text` (the footer rides on a
/// reply's last part, so a part is found by what it begins with).
fn showing(fake: &FakeDiscord, text: &str) -> usize {
    fake.messages(CHANNEL)
        .iter()
        .filter(|m| m.content.contains(text))
        .count()
}

/// The turn's stream wrote two parts, partly; its reply (two parts, final)
/// is staged but not delivered; the renderer drops the turn; then the reply
/// is delivered, past Discord's nonce window.
async fn late_reply() -> Arc<FakeDiscord> {
    let d = tempfile::tempdir().unwrap();
    let fake = FakeDiscord::start();
    let mut lane = lane(d.path(), &fake);
    let core = lane.shared.core.clone();
    lane.take(LaneMsg::Held(vec![TURN.into()]));
    lane.take(upsert(&format!("{TURN}:L1:p0"), "First part, partly"));
    lane.take(upsert(&format!("{TURN}:L2:p0"), "Second part, partly"));
    lane.apply_live().await;
    assert_eq!(fake.messages(CHANNEL).len(), 2, "the stream's two parts");
    // The reply is staged: its loops' text is on the store.
    let (a, b) = (
        assistant(&core, SESSION, FIRST),
        assistant(&core, SESSION, SECOND),
    );
    let mut body = reply_body(TURN, &a, SESSION, vec![]);
    body["loops"] = json!([[1, a], [2, b]]);
    core.outbox.post(SESSION, "", &lane.target, body).unwrap();
    // Eight turns start while it waits (an outage, or a `/new`): the renderer
    // drops this one.
    lane.take(LaneMsg::Held(vec!["turn_later".into()]));
    fake.set_nonce_window_ms(0);
    assert!(lane.deliver_posts().await);
    fake
}

/// The red of theseus-llo6: each part of the late reply shows once, exactly,
/// and the stream's two messages are the ones edited.
#[tokio::test]
async fn a_reply_delivered_after_its_turn_left_posts_each_part_once() {
    let fake = late_reply().await;
    let all = fake.messages(CHANNEL);
    assert_eq!(showing(&fake, FIRST), 1, "{all:#?}");
    assert_eq!(showing(&fake, SECOND), 1, "{all:#?}");
    assert_eq!(all.len(), 2, "no part twice: {all:#?}");
}

/// The control: a post that names another turn does not hold this turn's
/// keys, so the released turn is forgotten at once, as it was.
#[tokio::test]
async fn a_post_of_another_turn_does_not_keep_a_released_turns_keys() {
    let d = tempfile::tempdir().unwrap();
    let fake = FakeDiscord::start();
    let mut lane = lane(d.path(), &fake);
    let core = lane.shared.core.clone();
    lane.take(LaneMsg::Held(vec![TURN.into()]));
    lane.take(upsert(&format!("{TURN}:L1:p0"), "First part, partly"));
    lane.apply_live().await;
    let a = assistant(&core, SESSION, FIRST);
    let body = reply_body("turn_other", &a, SESSION, vec![]);
    core.outbox.post(SESSION, "", &lane.target, body).unwrap();
    lane.take(LaneMsg::Held(vec!["turn_later".into()]));
    assert!(lane.released.is_empty(), "{:?}", lane.released);
    assert!(!lane.msgs.keys().any(|k| k.starts_with(TURN)));
}

/// The kept keys go once the post settles: the lane's maps do not keep a
/// turn past its last open post.
#[tokio::test]
async fn a_released_turns_keys_are_forgotten_once_its_post_settles() {
    let d = tempfile::tempdir().unwrap();
    let fake = FakeDiscord::start();
    let mut lane = lane(d.path(), &fake);
    let core = lane.shared.core.clone();
    lane.take(LaneMsg::Held(vec![TURN.into()]));
    lane.take(upsert(&format!("{TURN}:L1:p0"), "First part, partly"));
    lane.apply_live().await;
    let a = assistant(&core, SESSION, FIRST);
    let body = reply_body(TURN, &a, SESSION, vec![]);
    core.outbox.post(SESSION, "", &lane.target, body).unwrap();
    lane.take(LaneMsg::Held(vec!["turn_later".into()]));
    assert_eq!(lane.released, vec![TURN.to_string()], "kept while it waits");
    assert!(lane.msgs.keys().any(|k| k.starts_with(TURN)));
    assert!(lane.deliver_posts().await);
    assert!(lane.released.is_empty(), "{:?}", lane.released);
    assert!(!lane.msgs.keys().any(|k| k.starts_with(TURN)));
    assert!(!lane.sent.keys().any(|k| k.starts_with(TURN)));
    assert!(!lane.sealed.iter().any(|k| k.starts_with(TURN)));
    assert!(!lane.touched.keys().any(|k| k.starts_with(TURN)));
}
