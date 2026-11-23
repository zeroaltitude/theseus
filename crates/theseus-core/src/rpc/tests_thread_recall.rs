//! Threads in recall (theseus-f2g0) through the whole core: a note from an
//! old thread's session is headed by where it was said, as any note is.
//! The distance and the seal are gone (p17: anything goes); the helpers here
//! still serve the other thread tests.

use std::sync::Arc;

use serde_json::json;
use theseus_protocol::index::{IndexHit, IndexQueryResult, IndexSourceRank};

use super::Core;
use crate::config::MemoryMode;
use crate::node::{Body, Node};
use crate::recall::{Ask, AskFuture};
use crate::tests_recall::{rig_with, session, turn, Rig};

pub(super) fn canary() -> Rig {
    rig_with(MemoryMode::Live, |_| {})
}

/// An old thread's record (`tests_old_threads`).
pub(super) fn new_thread(c: &Core, slug: &str, title: &str) {
    crate::tests_old_threads::thread(c, slug, title, json!({}));
}

/// The old thread's session, opened and bound.
pub(super) fn thread_session(c: &Core, slug: &str) -> String {
    crate::tests_old_threads::session(c, slug)
}

/// A note in `sid`.
pub(super) fn say(c: &Core, sid: &str, text: &str) -> String {
    let n = Node::user(sid, None, "test", text);
    c.store.append(&[n.record().unwrap()]).unwrap();
    n.id
}

/// A stand-in index that answers `picks`: (session, the index of its node,
/// the fused score), in order.
pub(super) fn scored(c: &Arc<Core>, picks: Vec<(String, usize, f64)>) -> Ask {
    scored_with(c, picks, Vec::new())
}

/// The same, each hit with the entities its chunk holds and the question's it
/// shares (`entities[i]` for `picks[i]`; none for a hit past the list).
pub(super) fn scored_with(
    c: &Arc<Core>,
    picks: Vec<(String, usize, f64)>,
    entities: Vec<(Vec<String>, Vec<String>)>,
) -> Ask {
    let store = c.store.clone();
    Arc::new(move |_| -> AskFuture {
        let hits = picks
            .iter()
            .enumerate()
            .map(|(i, (sid, k, fused))| {
                let (position, n) = store.session_nodes(sid).unwrap().remove(*k);
                let (held, shared) = entities.get(i).cloned().unwrap_or_default();
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
                    entities_matched: shared,
                    entities: held,
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

/// A recalled note from an old thread's session is headed by where it was
/// said, its place (`thread:<slug>`), and carries no thread mark of its own
/// (issue 46): recall heads an item by place, session and author. A note
/// from an unthreaded session reads the same way.
#[tokio::test]
async fn an_old_threads_hit_is_headed_by_its_place() {
    let r = canary();
    let c = &r.core;
    new_thread(c, "security-fix", "security fix");
    new_thread(c, "pr-round", "PR round");
    let fix = thread_session(c, "security-fix");
    say(c, &fix, "The embargoed fix ports to the tls-pool crate.");
    let pr = thread_session(c, "pr-round");
    let plain = session(c, None, &["The unthreaded note about the tls-pool crate."]);
    c.runner.memory.set_ask(scored(
        c,
        vec![(fix.clone(), 0, 0.0164), (plain.clone(), 0, 0.0163)],
    ));
    let res = turn(c, &pr, "What about the tls-pool crate?").await;
    assert_eq!(res.recalled, 2);
    let nodes = c.store.session_nodes(&pr).unwrap();
    let items = nodes
        .iter()
        .find_map(|(_, n)| match &n.body {
            Body::Recall { items, .. } => Some(items.clone()),
            _ => None,
        })
        .expect("a Recall node");
    let from_fix = items.iter().find(|i| i.session_id == fix).unwrap();
    assert!(
        from_fix.header.contains(" in thread:security-fix, ")
            && !from_fix.header.contains("from thread"),
        "{}",
        from_fix.header
    );
    let from_plain = items.iter().find(|i| i.session_id == plain).unwrap();
    assert!(
        !from_plain.header.contains("from thread"),
        "{}",
        from_plain.header
    );
}
