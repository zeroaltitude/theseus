//! A `context.history` result carries the words of what it shows, so `node.reach` of each message it shows
//! finds the result as its copy (P0's rule 3), as `channel.read`'s borrowed node was found from each message
//! it took (D6 round 4). Each form of the read: an exchange opened from another place, a page of another
//! place, a session opened from a later turn of the same session, and an imported conversation's session.
//! Then what a retraction and an erase reach of it: the copy is listed where it lies, as glide's was.

use std::sync::Arc;

use serde_json::{json, Value};
use theseus_protocol::memory::MemorySearchParams;
use theseus_protocol::retract::{exposure_kind as kind, FactRetractParams};
use theseus_protocol::NodeReachResult;

use crate::node::Body;
use crate::provider::Scripted;
use crate::tests_places::{answers_a_call, last_user, rig_scripted, session, turn, Rig};
use crate::Core;

/// What the person says in the first place: its words are the ones each read copies.
const SAID: &str = "the zircon heron nests by the north weir at dawn";
const WORD: &str = "zircon";

/// A model whose message `CALL {…}` calls `context_history` with those arguments.
fn rig() -> Rig {
    rig_scripted(
        |_| {},
        |req| {
            if answers_a_call(req) {
                return Scripted::text("Read.");
            }
            match last_user(req).split_once("CALL ") {
                Some((_, a)) => Scripted::tools(
                    "Reading.",
                    &[(
                        "h1",
                        "context_history",
                        serde_json::from_str(a.trim()).unwrap(),
                    )],
                ),
                None => Scripted::text("Noted."),
            }
        },
    )
}

/// The session the line was said in, its message's node, and its exchange's turn and place.
struct Said {
    session: String,
    node: String,
    turn: String,
    place: String,
}

async fn say(core: &Arc<Core>) -> Said {
    let session = session(core, None);
    turn(core, &session, SAID).await;
    let n = (core.store.session_nodes(&session).unwrap().into_iter())
        .map(|(_, n)| n)
        .find(|n| matches!(&n.body, Body::UserMessage { text, .. } if text.contains(WORD)))
        .expect("the person's message");
    let turn = n.turn_id.clone().expect("its turn");
    let place = crate::exchange::exchange(&core.store, &turn)
        .unwrap()
        .expect("its exchange")
        .place;
    Said {
        session,
        node: n.id,
        turn,
        place,
    }
}

/// `sid`'s turn calls the tool with `args`: its stored result's node and text.
async fn read_in(core: &Arc<Core>, sid: &str, args: Value) -> (String, String) {
    turn(core, sid, &format!("CALL {args}")).await;
    (core.store.session_nodes(sid).unwrap().into_iter())
        .find_map(|(_, n)| match n.body {
            Body::ToolResult {
                tool_use_id,
                content,
                ..
            } if tool_use_id == "h1" => Some((n.id, content)),
            _ => None,
        })
        .expect("the call's result")
}

fn reach(core: &Core, node: &str) -> NodeReachResult {
    crate::reach::reach(&core.store, node, None)
        .unwrap()
        .expect("the node")
}

/// The copy `walk` found: the result `copy` in `sid`, one generation from the message, by the history's
/// route; and the requests of `sid` after it held it (the reply derived from it).
fn finds(walk: &NodeReachResult, copy: &str, sid: &str) {
    let d = (walk.descendants.iter())
        .find(|d| d.node_id == copy)
        .unwrap_or_else(|| panic!("node.reach does not list the history result {copy}: {walk:#?}"));
    assert_eq!(d.session_id, sid, "{d:?}");
    assert_eq!(d.generation, 1, "{d:?}");
    assert_eq!(d.route, crate::graph::VIA_HISTORY, "{d:?}");
    assert!(
        !d.exposure.compilations.is_empty() || d.exposure.loops > 0,
        "the reply after it was written with it in the request: {d:?}"
    );
}

/// An exchange opened from another place: the result holds the line, and reach finds it.
#[tokio::test]
async fn an_exchange_opened_in_another_place_is_found_from_its_message() {
    let r = rig();
    let said = say(&r.core).await;
    let other = session(&r.core, Some("channel:lab"));
    let (copy, text) = read_in(&r.core, &other, json!({ "open": said.turn })).await;
    assert!(text.contains(WORD), "{text}");
    finds(&reach(&r.core, &said.node), &copy, &other);
}

/// A page of another place: its line holds the person's words, and reach finds it.
#[tokio::test]
async fn a_page_of_another_place_is_found_from_its_message() {
    let r = rig();
    let said = say(&r.core).await;
    let other = session(&r.core, Some("channel:lab"));
    let (copy, text) = read_in(&r.core, &other, json!({ "place": said.place })).await;
    assert!(text.contains(WORD), "{text}");
    finds(&reach(&r.core, &said.node), &copy, &other);
}

/// The same session's later turn opens the session itself: the copy in its own session is found too.
#[tokio::test]
async fn a_session_opened_from_its_own_later_turn_is_found_from_its_message() {
    let r = rig();
    let said = say(&r.core).await;
    for n in 0..3 {
        turn(&r.core, &said.session, &format!("and the tide table, part {n}")).await;
    }
    let (copy, text) = read_in(&r.core, &said.session, json!({ "open": said.session })).await;
    assert!(text.contains(WORD), "{text}");
    finds(&reach(&r.core, &said.node), &copy, &said.session);
}

/// An imported conversation's message, read by opening its session: found from the imported node.
#[tokio::test]
async fn an_imported_message_opened_by_its_session_is_found_from_it() {
    use crate::tests_memory_lookup::{episode, import};
    let r = rig();
    let ids = import(
        &r.core,
        &[episode(7, "personal", "2026-03-03T18:00:00Z", SAID)],
    );
    let node = (r.core.store.session_nodes(&ids[0]).unwrap().into_iter())
        .map(|(_, n)| n)
        .find(|n| matches!(&n.body, Body::Imported { text, .. } if text.contains(WORD)))
        .expect("the imported message")
        .id;
    let other = session(&r.core, None);
    let (copy, text) = read_in(&r.core, &other, json!({ "open": ids[0] })).await;
    assert!(text.contains(WORD), "{text}");
    finds(&reach(&r.core, &node), &copy, &other);
    // An erase follows no copy (for any route): the import's own nodes are its tombstones, and the result that
    // copied its words stays where it lies, listed by reach from the tombstone, as glide's copies were.
    crate::import::write::erase(&r.core.store, "kelp-2026", Some("a test"), "cli").unwrap();
    assert!(
        crate::tests_places::result_of(&r.core, &other, "h1").contains(WORD),
        "the copy is not erased with its source"
    );
    finds(&reach(&r.core, &node), &copy, &other);
}

/// A retraction of the message finds the history result as a copy that was seen (`retract.rs`'s route for a
/// copy no other kind names, glide's), labels the message wrong so recall never serves it, and leaves the copy
/// where it lies: it is the reader's own result, as glide's borrowed node was.
#[tokio::test]
async fn a_retraction_of_the_message_lists_the_history_result() {
    let r = rig();
    let said = say(&r.core).await;
    let other = session(&r.core, Some("channel:lab"));
    let (copy, _) = read_in(&r.core, &other, json!({ "open": said.turn })).await;
    let out = r
        .core
        .fact_retract(
            &FactRetractParams {
                node: said.node.clone(),
                why: "the heron nests by the south weir".into(),
                session_id: None,
            },
            "cli",
        )
        .unwrap();
    let e = (out.exposures.iter())
        .find(|e| e.node_id == copy)
        .unwrap_or_else(|| panic!("the retraction does not list the history result: {out:#?}"));
    assert_eq!((e.kind.as_str(), e.session_id.as_str()), (kind::SEEN, other.as_str()), "{e:?}");
    assert!(e.what.contains("history"), "{e:?}");
    r.core
        .runner
        .memory
        .set_ask(crate::tests_recall::index_of(&r.core, vec![said.session.clone(), other.clone()]));
    let m = r
        .core
        .memory_search(MemorySearchParams {
            query: "zircon heron weir".into(),
            session_id: None,
            k: None,
            arm: None,
            as_of: None,
        })
        .await
        .unwrap();
    assert!(!m.admitted.iter().any(|i| i.node_id == said.node), "{m:?}");
    eprintln!(
        "after the retraction, recall admits the copy: {}",
        m.admitted.iter().any(|i| i.node_id == copy)
    );
    // The reader's next request still holds its own result (the copy is listed, never removed).
    turn(&r.core, &other, "and the weir?").await;
    let last = r.requests().pop().expect("a request");
    eprintln!(
        "after the retraction, the reader's next request holds the words: {}",
        serde_json::to_string(&last.messages).unwrap().contains(WORD)
    );
}
