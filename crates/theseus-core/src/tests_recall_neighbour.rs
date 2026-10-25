//! The subject beside a same-kind value through the whole core (theseus-96cyh,
//! join fix 2): a live recall whose candidates include a chunk about another
//! host than the asked one freezes the note into that item's header on its
//! Recall node, and the asked host's own chunk keeps the header it has
//! without the feature, byte for byte. The unit tests of `recall::neighbour`
//! pin the rule; these pin that `hold_node` puts it on the node.

use std::sync::Arc;

use theseus_protocol::index::{IndexHit, IndexQueryResult, IndexSourceRank};

use crate::config::MemoryMode;
use crate::node::Body;
use crate::recall::{Ask, AskFuture};
use crate::tests_recall::{rig_with, session, turn, Rig};
use crate::Core;

const ASKED: &str = "host:db-prod.example.test";
const NEAR: &str = "host:db-stage.example.test";

/// What the stand-in index says of one note: its session, its score, the
/// entities it holds and the asked ones among them.
struct Pick {
    session: String,
    fused: f64,
    entities: Vec<String>,
    matched: Vec<String>,
}

fn pick(session: &str, fused: f64, entities: &[&str], matched: &[&str]) -> Pick {
    Pick {
        session: session.to_string(),
        fused,
        entities: entities.iter().map(|e| e.to_string()).collect(),
        matched: matched.iter().map(|e| e.to_string()).collect(),
    }
}

/// A stand-in index answering `picks`, each with its entities.
fn scored(c: &Arc<Core>, picks: Vec<Pick>) -> Ask {
    let store = c.store.clone();
    Arc::new(move |_| -> AskFuture {
        let hits = picks
            .iter()
            .enumerate()
            .map(|(i, p)| {
                let (position, n) = store.session_nodes(&p.session).unwrap().remove(0);
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
                    entities_matched: p.matched.clone(),
                    entities: p.entities.clone(),
                    sources: [(
                        "bm25".to_string(),
                        IndexSourceRank {
                            rank: i + 1,
                            score: 1.0,
                        },
                    )]
                    .into(),
                    fused: p.fused,
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

fn rig() -> Rig {
    rig_with(MemoryMode::Canary, |c| c.memory.canary_fraction = 1.0)
}

/// The headers frozen on the Recall node of a turn that asked after the
/// asked host, with the index holding `entities` (asked chunk, near chunk)
/// and `matched` for the asked chunk.
async fn headers(r: &Rig, asked: &str, near: &str, tagged: bool) -> (String, String) {
    let c = &r.core;
    let here = session(c, None, &["hello"]);
    let (e_asked, m_asked, e_near): (&[&str], &[&str], &[&str]) = if tagged {
        (&[ASKED], &[ASKED], &[NEAR])
    } else {
        (&[], &[], &[])
    };
    c.runner.memory.set_ask(scored(
        c,
        vec![
            pick(asked, 0.0180, e_asked, m_asked),
            pick(near, 0.0170, e_near, &[]),
        ],
    ));
    let res = turn(c, &here, "Which port does db-prod.example.test listen on?").await;
    assert_eq!(res.recalled, 2);
    let nodes = c.store.session_nodes(&here).unwrap();
    let items = nodes
        .iter()
        .find_map(|(_, n)| match &n.body {
            Body::Recall { items, .. } => Some(items.clone()),
            _ => None,
        })
        .expect("a Recall node");
    let header_of = |sid: &str| {
        items
            .iter()
            .find(|i| i.session_id == sid)
            .map(|i| i.header.clone())
            .expect("an item of the session")
    };
    (header_of(asked), header_of(near))
}

/// A chunk about another host carries the note in its frozen header, after
/// the header the same chunk has without it; the asked host's chunk has
/// exactly the header it has without the feature.
#[tokio::test]
async fn a_recall_of_another_host_names_it_in_the_frozen_header() {
    let r = rig();
    let c = &r.core;
    let asked = session(c, None, &["db-prod.example.test listens on port 5432."]);
    let near = session(c, None, &["db-stage.example.test listens on port 5433."]);

    // The base: the index tags no entities, so no note is made.
    let (base_asked, base_near) = headers(&r, &asked, &near, false).await;
    assert!(!base_near.contains("not the asked"), "{base_near}");

    let (got_asked, got_near) = headers(&r, &asked, &near, true).await;
    assert_eq!(got_asked, base_asked, "the asked host's header moved");
    let note = "about host db-stage.example.test, not the asked host db-prod.example.test";
    assert_eq!(got_near, format!("{base_near}, {note}"), "{got_near}");
}
