//! A place's session (theseus-emqx, `runtime/succession.rs`): a fresh bind
//! and `/new` open nothing, the place's first message opens the session and
//! records the supersession both ways, and a session the owner retired, or
//! one that can take no more turns, is succeeded the same way.

use std::sync::Arc;

use theseus_core::session::SessionRecord;
use theseus_core::Core;
use theseus_protocol::sessions::RetiredReason;

use super::tests::{core_scripted, place_for_tests};
use super::{shared_for_tests, Control, PlaceMsg};
use crate::rpc_client::CallError;

const PLACE: &str = "dm:42";

/// The owner, from the CLI.
fn owner() -> theseus_core::approval::Answerer {
    theseus_core::approval::Answerer {
        label: "cli".into(),
        surface: theseus_core::approval::Surface::Cli,
        discord: None,
    }
}

fn sessions(core: &Core) -> usize {
    core.store.live_sessions::<SessionRecord>().unwrap().len()
}

fn rec(core: &Core, id: &str) -> SessionRecord {
    core.store.get_session(id).unwrap().unwrap()
}

/// A session bound to the DM place, as an older build's bind left one.
fn bound(core: &Arc<Core>) -> String {
    let r = SessionRecord::new(theseus_protocol::SessionKind::Conversation, None);
    core.store.put_session(&r.session_id, &r).unwrap();
    core.outbox.bind_place(PLACE, &r.session_id).unwrap();
    r.session_id
}

/// `old` was superseded by `new` at the DM place, both ways, with its row.
fn superseded(core: &Core, old: &str, new: &str) {
    let (o, n) = (rec(core, old), rec(core, new));
    assert_eq!(o.superseded_by.as_ref().unwrap().session_id, new);
    assert_eq!(n.supersedes.as_ref().unwrap().session_id, old);
    assert_eq!(
        o.superseded_by.as_ref().unwrap().place.as_deref(),
        Some(PLACE)
    );
    assert_eq!(
        core.outbox.place_session(PLACE).unwrap().as_deref(),
        Some(new)
    );
    let rows: Vec<_> = core
        .store
        .ledger_tail::<theseus_core::ledger::LedgerRow>(200)
        .unwrap()
        .into_iter()
        .filter(|(_, r)| r.kind == "session.superseded" && r.session_id.as_deref() == Some(old))
        .collect();
    assert_eq!(rows.len(), 1, "one row for the move");
    assert_eq!(rows[0].1.data["superseded_by"], new);
}

/// A place with no stored session, or one retired, starts fresh: its bind
/// opens nothing.
#[tokio::test]
async fn a_fresh_bind_opens_no_session() {
    let d = tempfile::tempdir().unwrap();
    let core = core_scripted(d.path(), vec![]);
    let shared = shared_for_tests(&core);
    assert_eq!(shared.resumable(PLACE, "DM").await.unwrap(), (None, true));
    assert_eq!(sessions(&core), 0, "nothing opened at the bind");
    // Bound again before its first message (a restart, or put back in the
    // file live): no second bind notice.
    assert_eq!(shared.resumable(PLACE, "DM").await.unwrap(), (None, false));
    assert_eq!(sessions(&core), 0);
    let sid = bound(&core);
    assert_eq!(
        shared.resumable(PLACE, "DM").await.unwrap(),
        (Some(sid.clone()), false)
    );
    core.retire_session(&sid, &owner()).unwrap();
    assert_eq!(
        shared.resumable(PLACE, "DM").await.unwrap(),
        (None, false),
        "retired by hand"
    );
    assert_eq!(sessions(&core), 1);
}

/// `/new` opens nothing; the place's next message opens the session, and
/// its move is recorded both ways.
#[tokio::test]
async fn new_opens_nothing_and_the_next_message_opens_the_successor() {
    let d = tempfile::tempdir().unwrap();
    let core = core_scripted(d.path(), vec![]);
    let sid = bound(&core);
    let (mut place, _rx) = place_for_tests(&core, &sid);
    let answer = place.control(Control::New, "discord:zeroaltitude").await;
    assert!(
        answer.starts_with(
            "🆕 A fresh session starts here with your next message. The previous one, `"
        ),
        "{answer}"
    );
    let status = place.control(Control::Status, "discord:zeroaltitude").await;
    assert!(status.starts_with("No session yet"), "{status}");
    assert_eq!(sessions(&core), 1, "no session before the message");
    assert_eq!(place.session_id, sid);
    assert!(place.ready().await, "the first message's step");
    let new = place.session_id.clone();
    assert_ne!(new, sid);
    assert_eq!(sessions(&core), 2);
    superseded(&core, &sid, &new);
    assert_eq!(
        rec(&core, &sid).retired.unwrap().reason,
        RetiredReason::Superseded
    );
    assert!(place.ready().await, "the next message stays on it");
    assert_eq!(
        (place.session_id.as_str(), sessions(&core)),
        (new.as_str(), 2)
    );
    // A place bound fresh opens its first session with no supersession.
    let (mut first, _rx) = place_for_tests(&core, "");
    first.key = "dm:43".into();
    first.fresh = true;
    assert!(first.ready().await);
    assert!(rec(&core, &first.session_id).supersedes.is_none());
}

/// A session that can take no more turns: the place says so and opens
/// nothing; its next message opens the successor, recorded as a
/// supersession.
#[tokio::test]
async fn a_session_that_can_take_no_turns_is_succeeded_by_the_next_message() {
    let d = tempfile::tempdir().unwrap();
    let core = core_scripted(d.path(), vec![]);
    let sid = bound(&core);
    let (mut place, _rx) = place_for_tests(&core, &sid);
    place.inflight = true;
    let gone = CallError {
        code: theseus_protocol::error_code::NOT_FOUND,
        message: "no such session".into(),
        data: serde_json::Value::Null,
    };
    place.handle(PlaceMsg::SubmitDone(Err(gone))).await;
    assert!(place.fresh);
    assert_eq!(sessions(&core), 1, "nothing opened yet");
    assert!(place.ready().await);
    superseded(&core, &sid, &place.session_id);
}

/// A bound place's session the owner retired by hand: its next message
/// starts a successor, so the place never posts into it silently; the old
/// one keeps its reason, and gains its link.
#[tokio::test]
async fn a_retired_bound_session_is_succeeded_by_the_places_next_message() {
    let d = tempfile::tempdir().unwrap();
    let core = core_scripted(d.path(), vec![]);
    let sid = bound(&core);
    let (mut place, _rx) = place_for_tests(&core, &sid);
    assert!(place.ready().await);
    assert_eq!(place.session_id, sid, "not retired: it stays");
    core.retire_session(&sid, &owner()).unwrap();
    assert!(place.ready().await);
    assert_ne!(place.session_id, sid);
    superseded(&core, &sid, &place.session_id);
    assert_eq!(
        rec(&core, &sid).retired.unwrap().reason,
        RetiredReason::ByHand
    );
}
