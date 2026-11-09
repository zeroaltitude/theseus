//! A message that arrives while the place's turn runs (theseus-qu1lc) goes to
//! the daemon at once, so its running turn can take it in; with the daemon's
//! switch off it waits here for the turn to end, as it always did.

use std::time::Duration;

use std::sync::Arc;

use theseus_core::session::SessionRecord;
use theseus_core::Core;
use twilight_model::id::Id;

use super::steer::steer_params;
use super::tests::{core_edited, place_for_tests};
use super::*;

/// A session bound to the DM place.
fn bound(core: &Arc<Core>) -> String {
    let r = SessionRecord::new(theseus_protocol::SessionKind::Conversation, None);
    core.store.put_session(&r.session_id, &r).unwrap();
    core.outbox.bind_place("dm:42", &r.session_id).unwrap();
    r.session_id
}

fn message(text: &str) -> Inbound {
    Inbound {
        author: "zeroaltitude".into(),
        author_id: 42,
        text: text.into(),
        files: None,
        message: Id::new(7),
        channel: Id::new(43),
        guild: None,
        thread: None,
        reply_turn: None,
        task: None,
    }
}

/// A turn is in flight: the message is sent now and queued nowhere here, and
/// the daemon answers it (here, with a turn of its own, as none was running to
/// take it).
#[tokio::test]
async fn a_message_sent_while_a_turn_runs_goes_to_the_daemon_at_once() {
    let d = tempfile::tempdir().unwrap();
    let core = core_edited(d.path(), vec![], |_| {});
    let sid = bound(&core);
    let (mut place, mut rx) = place_for_tests(&core, &sid);
    place.inflight = true;
    place
        .handle(PlaceMsg::Inbound(message("while you work: stop")))
        .await;
    assert!(place.queued.is_empty(), "not held here");
    let done = tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            if let Some(PlaceMsg::Steered(r, _)) = rx.recv().await {
                break r;
            }
        }
    })
    .await
    .unwrap();
    done.unwrap();
    let said: Vec<String> = core
        .store
        .session_nodes(&sid)
        .unwrap()
        .into_iter()
        .filter_map(|(_, n)| match n.body {
            theseus_core::node::Body::UserMessage { text, .. } => Some(text),
            _ => None,
        })
        .collect();
    assert_eq!(said, ["while you work: stop"]);
}

/// With `[kernel] steer_queued` off the message waits for the turn to end.
#[tokio::test]
async fn with_the_switch_off_it_waits_here_for_the_turn_to_end() {
    let d = tempfile::tempdir().unwrap();
    let core = core_edited(d.path(), vec![], |c| c.kernel.steer_queued = false);
    let sid = bound(&core);
    let (mut place, _rx) = place_for_tests(&core, &sid);
    place.inflight = true;
    place.handle(PlaceMsg::Inbound(message("later"))).await;
    assert_eq!(place.queued.len(), 1);
}

/// The text of each post with any waiting for the DM: a failed turn's own
/// reply has none.
fn posts(core: &Arc<Core>) -> Vec<String> {
    core.outbox
        .open_for("discord:dm:42")
        .iter()
        .filter_map(|a| theseus_core::outbox::body_of(a)["text"].as_str())
        .filter(|t| !t.is_empty())
        .map(str::to_string)
        .collect()
}

/// The next steered request's answer in the place's mailbox, not yet handled.
async fn steered(rx: &mut mpsc::UnboundedReceiver<PlaceMsg>) -> PlaceMsg {
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            if let Some(m @ PlaceMsg::Steered(..)) = rx.recv().await {
                break m;
            }
        }
    })
    .await
    .expect("a steered request's answer")
}

/// A turn is in flight and a message was steered to the daemon, whose answer
/// is still to come: the model refuses it (and the next one), so the answer is
/// an error.
async fn steered_into_a_failing_turn() -> (Arc<Core>, Place, mpsc::UnboundedReceiver<PlaceMsg>) {
    let refused = || {
        theseus_core::provider::Scripted::Fail(theseus_core::provider::ProviderError::Auth {
            status: 401,
            message: "invalid x-api-key".into(),
        })
    };
    let d = tempfile::tempdir().unwrap();
    let core = core_edited(d.path(), vec![refused(), refused()], |_| {});
    let sid = bound(&core);
    let (mut place, rx) = place_for_tests(&core, &sid);
    place.inflight = true;
    place
        .handle(PlaceMsg::Inbound(message("while you work")))
        .await;
    (core, place, rx)
}

/// A steered request that fails says so, once: the baseline the next test
/// leaves out of the room by stopping first.
#[tokio::test]
async fn a_steered_request_that_fails_says_so() {
    let (core, mut place, mut rx) = steered_into_a_failing_turn().await;
    let answer = steered(&mut rx).await;
    place.handle(answer).await;
    let said = posts(&core);
    assert_eq!(said.len(), 1, "{said:?}");
    assert!(said[0].starts_with("⚠️ "), "{said:?}");
}

/// A `/stop` that came while a steered request was outstanding is its answer:
/// the request ends in the error the stopped turn gives, which the stop's
/// own reply has said already (theseus-07m8b).
#[tokio::test]
async fn a_steered_request_a_stop_ended_says_nothing_more() {
    let (core, mut place, mut rx) = steered_into_a_failing_turn().await;
    place.control(Control::Stop, "discord:zeroaltitude").await;
    let answer = steered(&mut rx).await;
    place.handle(answer).await;
    assert_eq!(posts(&core), Vec::<String>::new());
}

/// A steer sent after the stop is not the stop's: its failure is said.
#[tokio::test]
async fn a_steer_sent_after_a_stop_still_says_its_failure() {
    let (core, mut place, mut rx) = steered_into_a_failing_turn().await;
    let before = steered(&mut rx).await;
    place.control(Control::Stop, "discord:zeroaltitude").await;
    place
        .handle(PlaceMsg::Inbound(message("and then this")))
        .await;
    place.handle(before).await;
    assert_eq!(posts(&core), Vec::<String>::new(), "the stopped one");
    let after = steered(&mut rx).await;
    place.handle(after).await;
    let said = posts(&core);
    assert_eq!(said.len(), 1, "{said:?}");
    assert!(said[0].starts_with("⚠️ "), "{said:?}");
}

/// The turn in flight ended and the daemon runs the steered message as a turn
/// of its own: the place is busy until that request is answered, so it shows
/// "typing…" (theseus-07m8b).
#[tokio::test]
async fn the_place_is_busy_while_a_steered_request_runs_after_its_turn_ended() {
    let d = tempfile::tempdir().unwrap();
    let core = core_edited(d.path(), vec![], |_| {});
    let sid = bound(&core);
    let (mut place, mut rx) = place_for_tests(&core, &sid);
    place.inflight = true;
    place.handle(PlaceMsg::Inbound(message("one more"))).await;
    place.handle(PlaceMsg::SubmitDone(Ok(()))).await;
    assert!(!place.inflight, "its own submit is over");
    assert!(place.typing_due(), "the steered request still runs");
    let answer = steered(&mut rx).await;
    place.handle(answer).await;
    assert!(!place.typing_due(), "and is over");
}

/// A message that comes while a steered request runs alone is steered too,
/// not made a turn of the place's own (theseus-07m8b).
#[tokio::test]
async fn a_message_while_a_steered_request_runs_alone_is_steered_too() {
    let d = tempfile::tempdir().unwrap();
    let core = core_edited(d.path(), vec![], |_| {});
    let sid = bound(&core);
    let (mut place, mut rx) = place_for_tests(&core, &sid);
    place.inflight = true;
    place.handle(PlaceMsg::Inbound(message("one more"))).await;
    place.handle(PlaceMsg::SubmitDone(Ok(()))).await;
    place
        .handle(PlaceMsg::Inbound(message("and another")))
        .await;
    assert!(!place.inflight, "no turn of its own was made of it");
    for _ in 0..2 {
        let answer = steered(&mut rx).await;
        place.handle(answer).await;
    }
    assert!(!place.typing_due(), "both are answered");
}

/// The prompt is refused while a steered request runs alone, as it is while
/// a turn of the place's own does (theseus-07m8b).
#[tokio::test]
async fn a_prompt_is_refused_while_a_steered_request_runs_alone() {
    let d = tempfile::tempdir().unwrap();
    let core = core_edited(d.path(), vec![], |_| {});
    let sid = bound(&core);
    let (mut place, _rx) = place_for_tests(&core, &sid);
    place.inflight = true;
    place.handle(PlaceMsg::Inbound(message("one more"))).await;
    place.handle(PlaceMsg::SubmitDone(Ok(()))).await;
    let prompt = theseus_protocol::mcp::McpPromptRef {
        server: "fake".into(),
        name: "greet".into(),
        arguments: Default::default(),
    };
    let said = place.run_prompt(prompt, "discord:zeroaltitude");
    assert!(said.starts_with("A turn is running here"), "{said}");
}

/// A Discord reply to one of the place's replies keeps the turn it answered,
/// through a steer as through any turn of the place's own, so the router
/// can address the thread that reply came from (theseus-07m8b).
#[test]
fn a_steer_carries_the_message_it_answers_and_the_turn_it_replies_to() {
    let m = Inbound {
        reply_turn: Some("turn_9".into()),
        ..message("and this")
    };
    let p = steer_params("ses_1", &m);
    assert_eq!(p.session_id.as_deref(), Some("ses_1"));
    assert_eq!(p.author.as_deref(), Some("discord:zeroaltitude"));
    assert_eq!(p.input, "and this");
    assert_eq!(p.reply_to.as_deref(), Some("7"), "the message it answers");
    assert_eq!(
        p.in_reply_to.as_deref(),
        Some("turn_9"),
        "the turn it replies to"
    );
    let plain = steer_params("ses_1", &message("and this"));
    assert_eq!(plain.in_reply_to, None, "none when it replies to none");
}
