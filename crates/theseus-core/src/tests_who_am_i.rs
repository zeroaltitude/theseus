//! Who am I (`view::who`), in the requests a turn sends: it heads the first user message, names the profile
//! the turn runs on, its model and the persona in play, leaves the system part as it was, and is the same
//! bytes from one turn to the next until the profile changes.

use std::sync::Arc;

use crate::bus::EventSink;
use crate::provider::ProviderRequest;
use crate::session::SessionRecord;
use crate::tests_places::{turn, OWNER};
use crate::tests_private_view::session_in;
use crate::tests_view_compile::{asked, view_rig};
use crate::turn::TurnRequest;
use crate::Core;

/// The first block of the request's first message.
fn head(q: &ProviderRequest) -> String {
    q.messages[0]["content"][0]["text"]
        .as_str()
        .unwrap_or_default()
        .to_string()
}

/// The system part, every block's text.
fn system(q: &ProviderRequest) -> String {
    serde_json::to_string(&q.system).unwrap()
}

/// A turn of `sid` on `profile`, typed by the owner in person (as `tests_places::turn`, on another profile).
async fn turn_on(core: &Arc<Core>, sid: &str, input: &str, profile: &str) {
    let rec = core
        .store
        .get_session::<SessionRecord>(sid)
        .unwrap()
        .unwrap();
    let (live, _) = core.live_profile();
    let target = (core.runner)
        .resolve_target(&live, Some(profile), None, None)
        .unwrap();
    let mark = crate::view_gate::test_mark(&core.runner, sid, true);
    let sink = EventSink::new(core.bus.clone(), sid, None).with_arrival(Some(mark));
    core.runner
        .run(TurnRequest {
            prompt: None,
            surface: None,
            session: rec,
            input: Some(input.into()),
            target,
            sink,
            author: format!("discord:{OWNER}"),
            attachments: vec![],
            arrived: None,
            reply_to: None,
            answer_at: None,
        })
        .await
        .unwrap();
}

#[tokio::test]
async fn who_am_i_names_the_live_profile_model_and_persona() {
    let r = view_rig();
    let c = &r.core;
    let sid = session_in(c, None, &r.root);
    turn(c, &sid, "first: the tide").await;
    let q = asked(&r, "first: the tide").remove(0);
    let who = head(&q);
    let (live, _) = c.live_profile();
    assert!(who.starts_with("Who you are"), "{who}");
    assert!(
        who.contains(&format!("- profile: {live}; model: {}", q.model)),
        "{who}"
    );
    assert!(
        who.contains("- persona: Theseus"),
        "the built-in one: {who}"
    );
    assert!(
        crate::view::text::tokens(&who) < crate::view::who::MOST_TOKENS,
        "{who}"
    );
    assert!(
        !system(&q).contains("Who you are"),
        "not in the system part"
    );
    let message = (q.messages[0]["content"].as_array().unwrap().iter())
        .position(|b| b["text"] == "first: the tide");
    assert!(
        message.is_some_and(|i| i > 0),
        "it comes before the message"
    );
}

/// Two turns on one profile: the block is the same bytes, and so is the system part; a turn on another
/// profile names it, and the system part is the profile's own as ever.
#[tokio::test]
async fn switching_the_live_profile_changes_who_am_i_at_the_next_turn() {
    let r = view_rig();
    let c = &r.core;
    let sid = session_in(c, None, &r.root);
    turn(c, &sid, "one: the tide").await;
    turn(c, &sid, "two: the tide").await;
    let one = asked(&r, "one: the tide").remove(0);
    let two = asked(&r, "two: the tide").remove(0);
    assert_eq!(head(&one), head(&two), "the same bytes, turn to turn");
    assert_eq!(system(&one), system(&two));
    turn_on(c, &sid, "three: the tide", "opus").await;
    let three = asked(&r, "three: the tide").remove(0);
    assert_ne!(head(&three), head(&two));
    assert!(
        head(&three).contains(&format!("- profile: opus; model: {}", three.model)),
        "{}",
        head(&three)
    );
}
