//! Replies that cite what they remember (theseus-ejdk, provable memory):
//! each recalled item's header carries a handle and the system block says to
//! cite it; the handles a reply holds are resolved against the turn's own
//! `Recall` node, one `memory.cited` row each, and a handle the turn was not
//! given is left as text; the sources ride in the result and `memory.cites`;
//! a Discord message's id is kept beside its node, so a citation can link to
//! it; and a shared place never shows a private or imported source.

use std::sync::Arc;

use serde_json::{json, Value};
use theseus_protocol::cite::{handles_for, split_header, MemoryCitesParams};
use theseus_protocol::{SessionKind, TurnSubmitResult};

use crate::bus::EventSink;
use crate::config::MemoryMode;
use crate::node::{Body, Node, RecalledRef};
use crate::provider::Scripted;
use crate::session::SessionRecord;
use crate::test_support::recall_rows as rows;
use crate::tests_recall::{index_of, rig_with, session, turn, Rig, DEN, PIER};
use crate::turn::TurnRequest;
use crate::Core;

const HERON: &str = "Remember: the grey heron nests by the old weir at Millbrook.";
const TIDE: &str = "The tide tables are in the harbour office.";

fn canary() -> Rig {
    rig_with(MemoryMode::Live, |_| {})
}

/// The model's next answer: `text`.
fn says(r: &Rig, text: &str) {
    r.model.script.lock().unwrap().push_back(Scripted::Blocks {
        blocks: vec![json!({"type": "text", "text": text})],
        stop_reason: "end_turn".into(),
    });
}

fn node_ids(core: &Core, sid: &str) -> Vec<String> {
    core.store
        .session_nodes(sid)
        .unwrap()
        .into_iter()
        .map(|(_, n)| n.id)
        .collect()
}

fn recall_items(core: &Core, sid: &str) -> Vec<RecalledRef> {
    core.store
        .session_nodes(sid)
        .unwrap()
        .into_iter()
        .find_map(|(_, n)| match n.body {
            Body::Recall { items, .. } => Some(items),
            _ => None,
        })
        .expect("a Recall node")
}

/// The header's handle and the guidance (the goldens): a recalled item's
/// header begins with the tail of its node's id, `[m:a1b2c3]`, and the
/// request's system block holds the one sentence that tells the model to cite
/// it. Its note renders the handle after the item's number.
#[tokio::test]
async fn a_recalled_items_header_carries_its_handle() {
    let r = canary();
    let c = &r.core;
    let a = session(c, None, &[HERON]);
    let b = session(c, None, &[]);
    c.runner.memory.set_ask(index_of(c, vec![a.clone()]));
    turn(c, &b, "Where does the grey heron nest?").await;
    let source = node_ids(c, &a).remove(0);
    let want = format!("m:{}", &source[source.len() - 6..]);
    let items = recall_items(c, &b);
    let (handle, rest) = split_header(&items[0].header).expect("a handle");
    assert_eq!(handle, want);
    assert!(rest.starts_with("a message from test in "), "{rest}");
    let q = r.model.requests.lock().unwrap().pop().unwrap();
    let system = serde_json::to_string(&q.system).unwrap();
    assert!(
        system.contains("When you state something from recalled testimony, cite its handle."),
        "{system}"
    );
    let note = serde_json::to_string(q.messages.last().unwrap()).unwrap();
    assert!(
        note.contains(&format!("[{want}] a message from test")),
        "{note}"
    );
}

/// A reply that cites two of its given handles and one it was not given
/// records two `memory.cited` rows, in the order cited; the unknown handle is
/// no citation and stays in the text; the result carries the two sources, each
/// with its header and excerpt.
#[tokio::test]
async fn a_reply_citing_two_given_handles_and_one_unknown_records_two_rows() {
    let r = canary();
    let c = &r.core;
    let a = session(c, None, &[HERON, TIDE]);
    let b = session(c, None, &[]);
    c.runner.memory.set_ask(index_of(c, vec![a.clone()]));
    let ids = node_ids(c, &a);
    let h = |id: &str| format!("m:{}", &id[id.len() - 6..]);
    let (heron, tide) = (h(&ids[0]), h(&ids[1]));
    says(
        &r,
        &format!("The tides [{tide}] and the heron [{heron}] agree; I made up [m:ffffff]."),
    );
    let res: TurnSubmitResult = turn(c, &b, "Where does the grey heron nest?").await;
    assert!(
        res.output.contains("[m:ffffff]"),
        "the text keeps its handles"
    );
    assert!(res.output.contains(&format!("[{tide}]")), "{}", res.output);
    let cited = rows(c, &b, "memory.cited");
    assert_eq!(cited.len(), 2, "{cited:?}");
    let handles: Vec<&str> = cited
        .iter()
        .map(|r| r.data["handle"].as_str().unwrap())
        .collect();
    assert_eq!(handles, [tide.as_str(), heron.as_str()]);
    assert_eq!(cited[0].data["node_id"], ids[1]);
    assert_eq!(cited[0].data["source_session"], a);
    assert_eq!(cited[1].data["node_id"], ids[0]);
    assert!(cited[0].turn_id.is_some());
    let recall_id = recall_node_id(c, &b);
    assert_eq!(cited[0].data["recall_id"], recall_id);
    // The sources, for the surfaces.
    let got: Vec<&str> = res.cites.iter().map(|c| c.handle.as_str()).collect();
    assert_eq!(got, [tide.as_str(), heron.as_str()]);
    assert_eq!(res.cites[1].excerpt, HERON);
    assert!(res.cites[1].header.starts_with("a message from test in "));
    assert!(
        res.cites[1].discord.is_none(),
        "a CLI message has no Discord id"
    );
    // A reply that cites nothing records nothing.
    let again = turn(c, &b, "And again?").await;
    assert!(again.cites.is_empty());
    assert_eq!(rows(c, &b, "memory.cited").len(), 2);
}

fn recall_node_id(core: &Core, sid: &str) -> String {
    core.store
        .session_nodes(sid)
        .unwrap()
        .into_iter()
        .find(|(_, n)| matches!(n.body, Body::Recall { .. }))
        .map(|(_, n)| n.id)
        .unwrap()
}

/// A handle the turn was given is a citation only for that turn's recall: a
/// handle from a session's earlier recall resolves for `memory.cites` (which
/// reads the session's recalls), but the same text in a reply of a turn whose
/// recall did not give it records nothing.
#[tokio::test]
async fn a_handle_the_turn_was_not_given_is_not_a_citation() {
    let r = canary();
    let c = &r.core;
    let a = session(c, None, &[HERON]);
    let b = session(c, None, &[]);
    c.runner.memory.set_ask(index_of(c, vec![a.clone()]));
    let id = node_ids(c, &a).remove(0);
    let handle = format!("m:{}", &id[id.len() - 6..]);
    // A turn with no recall at all cites nothing, whatever it writes.
    c.runner.memory.set_ask(index_of(c, vec![]));
    says(&r, &format!("About [{handle}]."));
    let res = turn(c, &b, "unrelated").await;
    assert!(res.cites.is_empty());
    assert!(rows(c, &b, "memory.cited").is_empty());
}

/// `memory.cites` resolves handles in a session's own recalls, newest first,
/// and leaves out one no recall of the session gave.
#[tokio::test]
async fn memory_cites_resolves_in_the_sessions_own_recalls() {
    let r = canary();
    let c = &r.core;
    let a = session(c, None, &[HERON]);
    let b = session(c, None, &[]);
    c.runner.memory.set_ask(index_of(c, vec![a.clone()]));
    turn(c, &b, "Where does the grey heron nest?").await;
    let id = node_ids(c, &a).remove(0);
    let handle = format!("m:{}", &id[id.len() - 6..]);
    let got = c
        .memory_cites(MemoryCitesParams {
            session_id: b.clone(),
            handles: vec![handle.clone(), "m:ffffff".into()],
        })
        .unwrap();
    assert_eq!(got.cites.len(), 1);
    assert_eq!(got.cites[0].node_id, id);
    assert_eq!(got.cites[0].session_id, a);
    assert_eq!(
        got.cites[0].position,
        c.store.session_nodes(&a).unwrap()[0].0
    );
    // The session's history carries the sources of the replies it shows.
    says(&r, &format!("It nests by the weir [{handle}]."));
    turn(c, &b, "Say it again.").await;
    let h = c
        .session_history(theseus_protocol::SessionHistoryParams {
            session_id: b.clone(),
            ..Default::default()
        })
        .unwrap();
    assert!(h.nodes.iter().any(|n| n.text.contains(&handle)));
    assert_eq!(h.cites.len(), 1, "{:?}", h.cites);
    let none = c
        .session_history(theseus_protocol::SessionHistoryParams {
            session_id: a,
            ..Default::default()
        })
        .unwrap();
    assert!(none.cites.is_empty());
}

/// A person's Discord message keeps its id beside its node, in the frame that
/// writes it, and a citation of it names its place and message.
#[tokio::test]
async fn a_cited_discord_message_names_its_id() {
    let r = canary();
    let c = &r.core;
    let den = session(c, Some(&format!("channel:{DEN}")), &[]);
    let rec = c.store.get_session::<SessionRecord>(&den).unwrap().unwrap();
    let (live, _) = c.live_profile();
    let target = c.runner.resolve_target(&live, None, None, None).unwrap();
    c.runner
        .run(TurnRequest {
            prompt: None,
            surface: None,
            session: rec,
            input: Some(HERON.into()),
            target,
            sink: EventSink::new(c.bus.clone(), &den, None),
            author: "discord:ada".into(),
            attachments: vec![],
            arrived: None,
            reply_to: Some("200000000000000002".into()),
            answer_at: None,
        })
        .await
        .unwrap();
    let said = node_ids(c, &den).remove(0);
    let b = session(c, None, &[]);
    c.runner.memory.set_ask(index_of(c, vec![den.clone()]));
    let handle = format!("m:{}", &said[said.len() - 6..]);
    says(&r, &format!("The heron nests by the weir [{handle}]."));
    let res = turn(c, &b, "Where does the grey heron nest?").await;
    let cite = &res.cites[0];
    let d = cite.discord.as_ref().expect("a Discord source");
    assert_eq!(d.message_id, "200000000000000002");
    assert_eq!(d.target, format!("discord:channel:{DEN}"));
    assert!(cite.header.contains("#den"), "{}", cite.header);
    let _: Value = serde_json::to_value(cite).unwrap();
}

/// A recalled item of `sid`'s first node, with its handle in its header.
fn first_item(c: &Core, sid: &str) -> (RecalledRef, String) {
    let (position, n) = c.store.session_nodes(sid).unwrap().remove(0);
    let text = crate::recall::text_of(&n);
    let handle = handles_for(&[&n.id]).remove(0);
    let item = RecalledRef {
        node_id: n.id,
        session_id: sid.into(),
        position,
        chunk: (0, text.len() as u32),
        header: theseus_protocol::cite::header_with(&handle, "a recalled note"),
        tokens: 5,
    };
    (item, handle)
}

/// A shared place's own source is cited there, and an imported one is cited
/// too (its excerpt its words); once its import is erased it is no citation,
/// whether its source was read before the erase (a warm reader) or never (a
/// cold one), directly and through a shared session's `memory.cites`.
/// Recall's own rendering reads the same source and gets none either.
#[tokio::test]
async fn a_shared_and_an_imported_source_are_cited_and_an_erased_one_never() {
    use crate::tests_memory_lookup::{episode, import};
    let r = canary();
    let c = &r.core;
    let shared = session(c, Some(&format!("channel:{PIER}")), &[TIDE]);
    let (own, _) = first_item(c, &shared);
    let cite = c
        .runner
        .citation(&own)
        .expect("a shared place's own source");
    assert!(cite.excerpt.contains(TIDE), "{}", cite.excerpt);
    let ids = import(
        c,
        &[
            episode(0, "personal", "2026-03-03T18:00:00Z", "the warm lamp"),
            episode(1, "personal", "2026-03-04T18:00:00Z", "the cold lamp"),
        ],
    );
    let (warm, warm_handle) = first_item(c, &ids[0]);
    let (cold, _) = first_item(c, &ids[1]);
    let before = c.runner.citation(&warm).expect("an imported source");
    assert!(
        before.excerpt.contains("the warm lamp"),
        "{}",
        before.excerpt
    );
    crate::import::write::erase(&c.store, "kelp-2026", Some("a test"), "cli").unwrap();
    assert!(c.runner.citation(&warm).is_none(), "a warm reader");
    assert!(c.runner.citation(&cold).is_none(), "a cold reader");
    for item in [&warm, &cold] {
        assert!(c.runner.memory.read_source(&c.store, item).is_none());
    }
    let recall = Node::recall(&shared, "turn_x", "rcl_x", "baseline", vec![warm.clone()]);
    c.store.append(&[recall.record().unwrap()]).unwrap();
    let got = c
        .memory_cites(MemoryCitesParams {
            session_id: shared.clone(),
            handles: vec![warm_handle],
        })
        .unwrap();
    assert!(got.cites.is_empty(), "{:?}", got.cites);
}

/// A citation is shown wherever its item is (anything goes, 2026-11-22): a
/// private place's source is cited in a shared place's reply and through a
/// shared session's `memory.cites`, in the same form as anywhere: its handle,
/// its frozen header, its excerpt.
#[tokio::test]
async fn a_citation_is_shown_wherever_its_item_is() {
    let r = canary();
    let c = &r.core;
    let private = session(c, Some(&format!("channel:{DEN}")), &[HERON]);
    let shared = session(c, Some(&format!("channel:{PIER}")), &[TIDE]);
    let n = c.store.session_nodes(&private).unwrap().remove(0).1;
    let handle = handles_for(&[&n.id]).remove(0);
    let item = RecalledRef {
        node_id: n.id,
        session_id: private.clone(),
        position: 1,
        chunk: (0, HERON.len() as u32),
        header: theseus_protocol::cite::header_with(&handle, "a message from ada in #x"),
        tokens: 5,
    };
    let cite = c.runner.citation(&item).expect("a citation");
    assert_eq!(cite.handle, handle);
    assert_eq!(cite.header, "a message from ada in #x");
    assert!(cite.excerpt.contains(HERON), "{}", cite.excerpt);
    // Through `memory.cites`: a shared session's recall and the private
    // session's own both give it, the same.
    for sid in [&shared, &private] {
        let recall = Node::recall(sid, "turn_x", "rcl_x", "baseline", vec![item.clone()]);
        c.store.append(&[recall.record().unwrap()]).unwrap();
        let got = c
            .memory_cites(MemoryCitesParams {
                session_id: sid.clone(),
                handles: vec![handle.clone()],
            })
            .unwrap();
        assert_eq!(got.cites, vec![cite.clone()], "{sid}");
    }
    let _ = (SessionKind::Conversation, Arc::new(()));
}
