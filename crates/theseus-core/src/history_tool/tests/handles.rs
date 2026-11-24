//! A recalled note names the handle that opens its whole exchange (issue 44, step 5), and only a handle the
//! history tool's `open` gives the session asking: every handle the recall view names opens through the tool
//! itself; a note with no exchange, one a fork's cut holds, and one outside a self-loop session's own place
//! name none.

use std::sync::Arc;

use serde_json::json;
use theseus_protocol::SessionKind;

use super::{answer, args, ex, pov, put, self_session, Board, Ex, DM, HERE};
use crate::node::{Body, Node, RecalledRef};
use crate::session::SessionRecord;
use crate::store::Store;
use crate::view::Present;
use crate::Core;

/// The handle a recalled note's last line names, as the model is sent it.
fn handle_of(text: &str) -> Option<&str> {
    text.lines()
        .next_back()?
        .strip_prefix("    open: ")?
        .strip_suffix(" (context_history)")
}

/// Node `id` as recall's pack refers to it: its position, its whole text, a header frozen in `place`.
fn recalled(store: &Store, id: &str, place: &str) -> RecalledRef {
    let (position, n) = store.get_node(id).unwrap().unwrap();
    let text = crate::recall::text_of(&n);
    RecalledRef {
        node_id: n.id.clone(),
        session_id: n.session_id.clone(),
        position,
        chunk: (0, u32::try_from(text.len()).unwrap()),
        header: crate::recall::render::header(&n, position, place),
        tokens: 10,
    }
}

/// `handle` opened by `session`'s model, through the tool's own answer.
async fn opened(core: &Arc<Core>, session: &str, handle: &str) -> Result<String, String> {
    let a = args(json!({ "open": handle }));
    let at = pov(HERE, Present::Open);
    let got = answer(core, &Board::default(), (session, "t"), &at, &a).await;
    got.map(|o| o.text)
}

/// Each note of an exchange (its message and its reply) ends with the handle of that exchange, and the tool
/// opens it: the whole exchange, both sides.
#[tokio::test]
async fn a_recalled_item_names_the_handle_that_opens_its_exchange() {
    use crate::tests_places::{rig, session};
    let r = rig();
    let (me, there) = (session(&r.core, None), session(&r.core, None));
    let (ask, said) = put(&r.core.store, &ex(1, DM, &there));
    let items = vec![
        recalled(&r.core.store, &ask.id, DM),
        recalled(&r.core.store, &said.id, DM),
    ];
    let (kept, laid) = r.core.runner.view_recall(&me, items).unwrap();
    assert_eq!((kept.len(), laid.len()), (2, 2));
    for l in &laid {
        assert_eq!(handle_of(&l.text), Some("turn_0001"), "{}", l.text);
        let got = opened(&r.core, &me, "turn_0001").await.unwrap();
        assert!(
            got.contains("[Exchange \"turn_0001\"") && got.contains("reply to message 1"),
            "{got}"
        );
    }
}

/// A note with no exchange names no handle: a node of no turn, a turn the ledger never recorded (said before
/// it), and an imported message. The tool refuses such a turn, so a handle there would be one it refuses.
#[tokio::test]
async fn a_recalled_item_with_no_exchange_names_no_handle() {
    use crate::import::Integrity;
    use crate::tests_places::{rig, session};
    let r = rig();
    let (me, there) = (session(&r.core, None), session(&r.core, None));
    let loose = Node::user(&there, None, "owner", "the heron nests by the weir");
    let early = Node::user(
        &there,
        Some("turn_early"),
        "owner",
        "the pier's heron is grey",
    );
    let book = format!("{}{}", crate::import::SESSION_PREFIX, "b".repeat(64));
    let imported = Node::imported(
        "imp_heron".into(),
        &book,
        "openclaw",
        1_000_000,
        Body::Imported {
            text: "the imported heron".into(),
            integrity: Integrity::Operator,
            source: "openclaw-store".into(),
            unit: "u0".into(),
            sha256: "0".repeat(64),
            idx: 0,
        },
    );
    let recs: Vec<_> = [&loose, &early, &imported]
        .iter()
        .map(|n| n.record().unwrap())
        .collect();
    r.core.store.append(&recs).unwrap();
    let items = [&loose, &early, &imported]
        .iter()
        .map(|n| recalled(&r.core.store, &n.id, HERE))
        .collect();
    let (_, laid) = r.core.runner.view_recall(&me, items).unwrap();
    assert_eq!(laid.len(), 3, "each source reads");
    for l in &laid {
        assert_eq!(handle_of(&l.text), None, "{}", l.text);
        assert!(!l.text.contains("open: "), "{}", l.text);
    }
    let refused = opened(&r.core, &me, "turn_early").await.unwrap_err();
    assert!(refused.contains("no exchange"), "{refused}");
}

/// A fork names no handle its cut holds: the exchange its point falls in (its message before the point, its
/// reply after) is one the fork's `open` refuses, so its message's note names none; an exchange wholly before
/// the point names its handle, and the fork opens it.
#[tokio::test]
async fn a_forks_recalled_note_names_no_handle_its_cut_holds() {
    use crate::tests_places::{rig, session};
    let r = rig();
    let store = &r.core.store;
    let source = session(&r.core, None);
    let (before, _) = put(store, &ex(1, HERE, &source));
    let (through, _) = put(store, &ex(2, HERE, &source));
    let (point, _) = store.get_node(&through.id).unwrap().unwrap();
    let mut rec = SessionRecord::new(SessionKind::Conversation, None);
    rec.forked_from = Some(Box::new(theseus_protocol::ForkOrigin {
        session: source.clone(),
        node: through.id.clone(),
        position: point,
        compilation: None,
        at_ms: 0,
    }));
    store.put_session(&rec.session_id, &rec).unwrap();
    let fork = rec.session_id;
    let items = vec![
        recalled(store, &before.id, HERE),
        recalled(store, &through.id, HERE),
    ];
    let (_, laid) = r.core.runner.view_recall(&fork, items).unwrap();
    assert_eq!(laid.len(), 2, "neither message is past the point");
    assert_eq!(
        handle_of(&laid[0].text),
        Some("turn_0001"),
        "{}",
        laid[0].text
    );
    assert!(opened(&r.core, &fork, "turn_0001").await.is_ok());
    assert_eq!(handle_of(&laid[1].text), None, "{}", laid[1].text);
    let refused = opened(&r.core, &fork, "turn_0002").await.unwrap_err();
    assert!(refused.contains("no exchange"), "{refused}");
}

/// A session the self loop opened names a handle only within its own place, as its `open` reads: its own
/// exchange's note names it and opens; another place's names none, and the tool refuses that handle there.
#[tokio::test]
async fn a_self_loop_sessions_recalled_note_names_no_handle_outside_its_place() {
    use crate::tests_places::{rig, session};
    let r = rig();
    let me = self_session(&r.core);
    let mine =
        crate::view_gate::place_name(&r.core.runner, &me).unwrap_or_else(|| "unknown".into());
    let other = session(&r.core, None);
    let (own, _) = put(&r.core.store, &ex(1, &mine, &me));
    let (far, _) = put(&r.core.store, &ex(2, DM, &other));
    let items = vec![
        recalled(&r.core.store, &own.id, &mine),
        recalled(&r.core.store, &far.id, DM),
    ];
    let (_, laid) = r.core.runner.view_recall(&me, items).unwrap();
    assert_eq!(laid.len(), 2);
    assert_eq!(
        handle_of(&laid[0].text),
        Some("turn_0001"),
        "{}",
        laid[0].text
    );
    assert!(opened(&r.core, &me, "turn_0001").await.is_ok());
    assert_eq!(handle_of(&laid[1].text), None, "{}", laid[1].text);
    let refused = opened(&r.core, &me, "turn_0002").await.unwrap_err();
    assert!(refused.contains("no exchange"), "{refused}");
    // The control: an ordinary session names that handle, and opens it.
    let items = vec![recalled(&r.core.store, &far.id, DM)];
    let (_, laid) = r.core.runner.view_recall(&other, items).unwrap();
    assert_eq!(
        handle_of(&laid[0].text),
        Some("turn_0002"),
        "{}",
        laid[0].text
    );
    assert!(opened(&r.core, &other, "turn_0002").await.is_ok());
}

/// A played turn: an exchange said long ago at another place (so no window and no slot of the view holds it) is
/// recalled before the first word, its note names its handle in the request, and the played model opens that
/// handle with the history tool and is given the whole exchange.
#[tokio::test]
async fn a_played_turn_opens_the_exchange_a_recalled_note_names() {
    use crate::config::MemoryMode;
    use crate::provider::Scripted;
    use crate::tests_recall::{admitted, index_of, recalls, rig, session, turn};
    let r = rig(MemoryMode::Live);
    let c = &r.core;
    let (there, here) = (session(c, None, &[]), session(c, None, &[]));
    put(
        &c.store,
        &Ex {
            words: "Pin the tide table at berth seven, please.".into(),
            ..ex(1, DM, &there)
        },
    );
    c.runner
        .memory
        .set_ask(index_of(c, vec![there.clone(), here.clone()]));
    let handle = "open: turn_0001 (context_history)";
    {
        let mut q = r.model.script.lock().unwrap();
        q.push_back(Scripted::tools(
            "",
            &[("c1", "context_history", json!({"open": "turn_0001"}))],
        ));
        q.push_back(Scripted::text("Berth seven."));
    }
    let since = r.model.requests().len();
    turn(c, &here, "Which berth did I say the tide table is at?").await;
    // Recall found the earlier exchange before the first word.
    let rows = recalls(c, &here);
    let m = rows.last().expect("the turn's recall row");
    assert!(
        admitted(m).contains(&there.as_str()),
        "{} admitted {:?}, dropped {:?}",
        m.outcome,
        admitted(m),
        (m.dropped.iter())
            .map(|d| (&d.session_id, &d.reason))
            .collect::<Vec<_>>()
    );
    let reqs = r.model.requests();
    let wire = |i: usize| serde_json::to_string(&reqs[i]).unwrap();
    // The turn's first request lays it in slot H, after recall's opening line.
    let named = (since..reqs.len())
        .find(|i| wire(*i).contains("[Recalled by the harness: "))
        .unwrap_or_else(|| panic!("no request holds recall's notes: {}", wire(since)));
    let w = wire(named);
    assert!(w.contains("Pin the tide table at berth seven"), "{w}");
    assert!(w.contains(handle), "no note names {handle}: {w}");
    assert!(
        (reqs[named].tools.iter()).any(|t| t["name"] == "context_history"),
        "the tool is offered on the turn"
    );
    let opened = (named + 1..reqs.len())
        .map(wire)
        .find(|w| w.contains("[Exchange "))
        .expect("a request after the call holds the opened exchange");
    assert!(
        opened.contains(r#"[Exchange \"turn_0001\""#)
            && opened.contains("reply to Pin the tide table at berth seven"),
        "{opened}"
    );
}
