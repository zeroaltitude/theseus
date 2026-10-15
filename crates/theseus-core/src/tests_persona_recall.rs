//! The persona's recall boost through the whole core (theseus-a8n7): while a
//! persona is engaged, an imported episode of its own agent comes first at
//! equal relevance, and its recalled header names the source; with no
//! persona engaged, or `own_boost` at 1.0, neither happens.

use std::sync::Arc;

use theseus_protocol::index::{IndexHit, IndexQueryResult, IndexSourceRank};

use crate::config::{MemoryMode, PersonaConfig};
use crate::import::ImportedFrom;
use crate::node::{Body, Node};
use crate::recall::{Ask, AskFuture};
use crate::session::SessionRecord;
use crate::tests_recall::{rig_with, session, turn, Rig};
use crate::Core;
use theseus_protocol::SessionKind;

/// An imported session of `agent`'s, with one note in it.
fn imported(c: &Core, n: u32, agent: &str, text: &str) -> String {
    let sid = format!("ses_ep{n:064x}");
    let mut r = SessionRecord::with_id(
        sid.clone(),
        SessionKind::Conversation,
        Some("imported".into()),
    );
    let from: ImportedFrom = serde_json::from_value(serde_json::json!({
        "tag": "fixture", "episode_id": format!("ep_{n:064x}"), "hash": "h",
        "source": "openclaw-store", "agent": agent,
        "place": {"kind": "dm", "name": null, "id": null},
        "labels": {"sensitivity": "personal"},
        "as_of": {"start_ms": 1_000, "end_ms": 2_000},
        "messages": 1, "summary": false, "file": "f.jsonl", "line": n, "imported_at_ms": 3_000,
    }))
    .unwrap();
    r.imported = Some(Box::new(from));
    c.store.put_session(&sid, &r).unwrap();
    let node = Node::user(&sid, None, "wren", text);
    c.store.append(&[node.record().unwrap()]).unwrap();
    sid
}

/// A stand-in index answering `picks` (session, fused) at the same score.
fn scored(c: &Arc<Core>, picks: Vec<(String, f64)>) -> Ask {
    let store = c.store.clone();
    Arc::new(move |_| -> AskFuture {
        let hits = picks
            .iter()
            .enumerate()
            .map(|(i, (sid, fused))| {
                let (position, n) = store.session_nodes(sid).unwrap().remove(0);
                IndexHit {
                    text: crate::recall::text_of(&n),
                    node_id: n.id,
                    chunk: 0,
                    session_id: n.session_id,
                    position,
                    kind: "user_message".into(),
                    origin: "operator".into(),
                    author: None,
                    place: None,
                    tool: None,
                    time_ms: 0,
                    external: false,
                    entities_matched: vec![],
                    entities: vec![],
                    sources: [(
                        "bm25".to_string(),
                        IndexSourceRank {
                            rank: i + 1,
                            score: 1.0,
                        },
                    )]
                    .into(),
                    fused: *fused,
                }
            })
            .collect();
        Box::pin(async move {
            Ok(IndexQueryResult {
                hits,
                indexed_through: 0,
                lag: Default::default(),
                timings: Default::default(),
                skipped: Default::default(),
                weights: Default::default(),
            })
        })
    })
}

fn rig(default_persona: Option<&str>, boost: f64) -> Rig {
    rig_with(MemoryMode::Canary, |c| {
        c.memory.canary_fraction = 1.0;
        c.personas.insert("wren".into(), PersonaConfig::default());
        c.context.default_persona = default_persona.map(str::to_string);
        c.persona.own_boost = boost;
    })
}

/// The recalled items' sessions in order, with their frozen headers.
async fn recalled(r: &Rig) -> Vec<(String, String)> {
    let c = &r.core;
    let wren = imported(c, 1, "Wren", "The tide log lives in the boathouse ledger.");
    let finch = imported(
        c,
        2,
        "finch",
        "The tide log lives in the boathouse ledger too.",
    );
    let here = session(c, None, &["hello"]);
    // The other agent's note is a tenth more relevant: only a boost of the
    // persona's own can put hers first.
    c.runner.memory.set_ask(scored(
        c,
        vec![(finch.clone(), 0.0180), (wren.clone(), 0.0164)],
    ));
    let res = turn(c, &here, "Where does the tide log live?").await;
    assert_eq!(res.recalled, 2);
    let nodes = c.store.session_nodes(&here).unwrap();
    let items = nodes
        .iter()
        .find_map(|(_, n)| match &n.body {
            Body::Recall { items, .. } => Some(items.clone()),
            _ => None,
        })
        .expect("a Recall node");
    let name = |s: &str| {
        if s == wren {
            "wren".to_string()
        } else {
            "finch".to_string()
        }
    };
    items
        .into_iter()
        .map(|i| (name(&i.session_id), i.header))
        .collect()
}

/// With wren engaged her own episode is first though the other agent's note
/// scored a tenth higher, and its header names her and the day.
#[tokio::test]
async fn the_engaged_personas_own_episode_is_first_and_named() {
    let r = rig(Some("wren"), 1.2);
    let got = recalled(&r).await;
    assert_eq!(got[0].0, "wren", "{got:?}");
    assert!(
        got[0].1.contains("[from Wren, 20") && got[0].1.contains(", @"),
        "{}",
        got[0].1
    );
    assert!(!got[1].1.contains("[from Wren"), "{}", got[1].1);
}

/// With no persona engaged, or the boost off, the index's order stands and
/// no header names a source.
#[tokio::test]
async fn with_no_persona_or_no_boost_nothing_changes() {
    for (default, boost) in [(None, 1.2), (Some("wren"), 1.0)] {
        let r = rig(default, boost);
        let got = recalled(&r).await;
        assert_eq!(got[0].0, "finch", "{default:?} {boost}: {got:?}");
        assert!(
            got.iter().all(|(_, h)| !h.contains("[from Wren")),
            "{got:?}"
        );
    }
}
