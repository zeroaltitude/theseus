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

/// A long reply's words: more than an open shows, so its page fits it (cut in words, never past it).
const LONG: &str = "the long tide table: ";

/// A model whose message `CALL {…}` calls `context_history` with those arguments, and whose message saying
/// `LONG` is answered at length.
fn rig() -> Rig {
    rig_scripted(|_| {}, script)
}

/// [`rig`]'s model.
fn script(req: &crate::provider::ProviderRequest) -> Scripted {
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
        None if last_user(req).contains("LONG") => Scripted::text(&LONG.repeat(1_500)),
        None => Scripted::text("Noted."),
    }
}

/// The session the line was said in, its message's node and its reply's, and its exchange's turn and place.
struct Said {
    session: String,
    node: String,
    reply: String,
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
    let ex = crate::exchange::exchange(&core.store, &turn)
        .unwrap()
        .expect("its exchange");
    Said {
        session,
        node: n.id,
        reply: ex.reply.expect("its reply"),
        turn,
        place: ex.place,
    }
}

/// `sid`'s turn calls the tool with `args`: its stored result's node and text (the newest call's).
async fn read_in(core: &Arc<Core>, sid: &str, args: Value) -> (String, String) {
    turn(core, sid, &format!("CALL {args}")).await;
    (core.store.session_nodes(sid).unwrap().into_iter())
        .rev()
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
    // The reply's words are on it too: Theseus's part is a source as the person's is.
    assert!(text.contains("Noted."), "{text}");
    finds(&reach(&r.core, &said.reply), &copy, &other);
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
        turn(
            &r.core,
            &said.session,
            &format!("and the tide table, part {n}"),
        )
        .await;
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
/// where it lies: it is the reader's own result, as glide's borrowed node was. What that leaves is asserted as
/// it is, so a change to it is seen: the copy is not labeled (a seen copy never was), so recall may still
/// serve its text, and the reader's next request still holds its own result.
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
    assert_eq!(
        (e.kind.as_str(), e.session_id.as_str()),
        (kind::SEEN, other.as_str()),
        "{e:?}"
    );
    assert!(e.what.contains("history"), "{e:?}");
    r.core.runner.memory.set_ask(crate::tests_recall::index_of(
        &r.core,
        vec![said.session.clone(), other.clone()],
    ));
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
    assert!(
        m.admitted.iter().any(|i| i.node_id == copy),
        "the seen copy is not labeled, so recall still admits it: {m:?}"
    );
    // The reader's next request still holds its own result (the copy is listed, never removed).
    turn(&r.core, &other, "and the weir?").await;
    let last = r.requests().pop().expect("a request");
    let sent = serde_json::to_string(&last.messages).unwrap();
    assert!(sent.contains(WORD), "{sent}");
}

/// A page line that shows a stored account (`did`) carries the opening message's words through it, whatever
/// the page could read of the message itself (D6 round 5, 1): here the message reads as a tombstone, so the
/// line's own excerpt of it is empty, and the account still names the heron. The result links the message.
#[tokio::test]
async fn a_stored_account_links_the_opening_message_whose_words_were_not_read() {
    let r = rig();
    let said = say(&r.core).await;
    let (_, ask) = r.core.store.get_node(&said.node).unwrap().unwrap();
    let account = "The owner said the zircon heron nests by the north weir; Theseus noted it.";
    let d = crate::ladder::did::DidLine {
        turn_id: said.turn.clone(),
        session_id: said.session.clone(),
        line: account.into(),
        model: "m".into(),
        writer: crate::ladder::writer::WRITER_VERSION,
        marks: crate::view::Marks {
            place: Some(said.place.clone()),
            audience: crate::view::Audience::Private,
            author: crate::view::Author::Theseus,
            outside: false,
            sealed: false,
            veiled: false,
        },
        cost_usd: 0.0,
        at_ms: 0,
    };
    r.core
        .store
        .append(&[
            ask.erased(theseus_protocol::now_unix_ms(), "a test")
                .record()
                .unwrap(),
            crate::ladder::did::did_record(&d).unwrap(),
        ])
        .unwrap();
    let other = session(&r.core, Some("channel:lab"));
    let (copy, text) = read_in(&r.core, &other, json!({ "place": said.place })).await;
    assert!(text.contains(account), "the account is on the page: {text}");
    assert!(
        !text.contains(SAID),
        "the message's own excerpt is not: {text}"
    );
    finds(&reach(&r.core, &said.node), &copy, &other);
    finds(&reach(&r.core, &said.reply), &copy, &other);
}

/// A session page that fits one long reply keeps its source (the cut words are still its words), and the
/// page `before` it, read by its continuation, links what it shows.
#[tokio::test]
async fn a_fitted_line_and_the_page_before_it_link_what_they_show() {
    let r = rig();
    let said = say(&r.core).await;
    turn(&r.core, &said.session, "LONG, please").await;
    let long = (r
        .core
        .store
        .session_nodes(&said.session)
        .unwrap()
        .into_iter())
    .rev()
    .map(|(_, n)| n)
    .find(|n| matches!(&n.body, Body::AssistantMessage { .. }))
    .expect("the long reply")
    .id;
    let other = session(&r.core, None);
    let (first, text) = read_in(&r.core, &other, json!({ "open": said.session })).await;
    assert!(text.contains("[cut here"), "one long reply, fitted: {text}");
    assert!(!text.contains(WORD), "{text}");
    finds(&reach(&r.core, &long), &first, &other);
    let before: Value = text
        .split_once("before=")
        .and_then(|(_, c)| serde_json::from_str(c.trim().trim_end_matches('.')).ok())
        .unwrap_or_else(|| panic!("a continuation: {text}"));
    let args = json!({ "open": said.session, "before": before });
    let (second, text) = read_in(&r.core, &other, args).await;
    assert!(text.contains(WORD), "the page before it: {text}");
    finds(&reach(&r.core, &said.node), &second, &other);
}

/// A words search's page and a time span's page link their lines as a bare page does.
#[tokio::test]
async fn a_search_page_and_a_span_page_link_their_lines() {
    let r = rig();
    let said = say(&r.core).await;
    r.core
        .runner
        .memory
        .set_ask(crate::tests_memory_lookup::index_over(
            &r.core,
            vec![said.session.clone()],
        ));
    let other = session(&r.core, Some("channel:lab"));
    let args = json!({ "place": said.place, "words": WORD });
    let (found, text) = read_in(&r.core, &other, args).await;
    assert!(text.contains(WORD), "{text}");
    finds(&reach(&r.core, &said.node), &found, &other);
    let args = json!({ "place": said.place, "from": "2020-01-01" });
    let (spanned, text) = read_in(&r.core, &other, args).await;
    assert!(text.contains(WORD), "{text}");
    finds(&reach(&r.core, &said.node), &spanned, &other);
}

/// A result that shows another person's words is still marked outside text, and links them as well.
#[tokio::test]
async fn an_outside_result_keeps_its_mark_and_links_its_source() {
    let r = rig();
    let guest = session(&r.core, None);
    let now = theseus_protocol::now_unix_ms();
    crate::tests_mcp_history::exchange_said(
        &r.core,
        &guest,
        ("turn_guest_1", now),
        ("channel:pier", "guest:kestrel"),
        (
            "does the zircon heron nest here?",
            "It nests by the north weir.",
        ),
    );
    let ask = (r.core.store.session_nodes(&guest).unwrap().into_iter())
        .map(|(_, n)| n)
        .find(|n| matches!(&n.body, Body::UserMessage { .. }))
        .expect("the guest's message")
        .id;
    let other = session(&r.core, None);
    let (copy, text) = read_in(&r.core, &other, json!({ "open": "turn_guest_1" })).await;
    assert!(text.contains(WORD), "{text}");
    let (_, node) = r.core.store.get_node(&copy).unwrap().unwrap();
    assert!(
        matches!(
            &node.body,
            Body::ToolResult {
                external: Some(_),
                ..
            }
        ),
        "the result is marked outside text: {node:?}"
    );
    finds(&reach(&r.core, &ask), &copy, &other);
}

/// **The runtime's cap decides a page's copies** (D6 round 6). A page of eight long lines, read with `[tools]
/// result_max_chars` under its size, is cut again, head and tail, after the tool gave its sources: a line that
/// cut leaves out is no copy of its message; a line it keeps is.
#[tokio::test]
async fn a_page_line_the_runtime_cap_leaves_out_is_no_copy() {
    let r = rig_scripted(|c| c.tools.result_max_chars = 2_000, script);
    let said = session(&r.core, None);
    // Each message's words are its own: any of them on the page shows some of it.
    let tag = |k: usize| format!("m{k:02}w");
    for k in 0..8 {
        let words: Vec<String> = (0..30).map(|j| format!("{}{j:02}", tag(k))).collect();
        turn(&r.core, &said, &words.join(" ")).await;
    }
    let messages: Vec<(usize, String, String)> = (r.core.store.session_nodes(&said).unwrap())
        .into_iter()
        .filter_map(|(_, n)| match &n.body {
            Body::UserMessage { text, .. } => {
                let k = (0..8).find(|k| text.starts_with(&tag(*k)))?;
                Some((k, n.id.clone(), n.turn_id.clone()?))
            }
            _ => None,
        })
        .collect();
    let place = crate::exchange::exchange(&r.core.store, &messages[0].2)
        .unwrap()
        .expect("its exchange")
        .place;
    let other = session(&r.core, Some("channel:lab"));
    let (copy, text) = read_in(&r.core, &other, json!({ "place": place })).await;
    assert!(text.contains("not shown"), "the cap cut it: {text}");
    let (shown, cut): (Vec<_>, Vec<_>) =
        (messages.iter()).partition(|(k, _, _)| text.contains(&tag(*k)));
    assert!(
        !shown.is_empty() && !cut.is_empty(),
        "shown {shown:?}, cut {cut:?}: {text}"
    );
    let lists = |node: &str| {
        reach(&r.core, node)
            .descendants
            .iter()
            .any(|d| d.node_id == copy)
    };
    for (k, node, _) in cut {
        assert!(
            !lists(node),
            "{} is not on the page, yet node.reach lists it",
            tag(*k)
        );
    }
    for (k, node, _) in shown {
        assert!(lists(node), "{} is on the page", tag(*k));
    }
}
