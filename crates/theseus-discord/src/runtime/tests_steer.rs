//! A message that arrives while the place's turn runs (theseus-qu1lc) goes to
//! the daemon at once, so its running turn can take it in; with the daemon's
//! switch off it waits here for the turn to end, as it always did.

use std::time::Duration;

use std::sync::Arc;

use theseus_core::session::SessionRecord;
use theseus_core::Core;
use twilight_model::id::Id;

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
            if let Some(PlaceMsg::Steered(r)) = rx.recv().await {
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
