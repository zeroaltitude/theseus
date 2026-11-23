//! Threads in recall (theseus-f2g0) through the whole core: a note from
//! another thread names its thread, and `thread.read` cites a thread's notes.
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

/// A recalled note from another thread begins its header with the thread's
/// title, the day and the position, as a canary turn writes it; a note from
/// an unthreaded session has no such mark.
#[tokio::test]
async fn a_cross_thread_hit_names_its_thread() {
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
    let mark = format!(
        "[from thread «security fix», {}, @{}]",
        day_of(c, &from_fix.node_id),
        from_fix.position
    );
    assert!(
        from_fix.header.contains(&mark),
        "{} / {mark}",
        from_fix.header
    );
    let from_plain = items.iter().find(|i| i.session_id == plain).unwrap();
    assert!(
        !from_plain.header.contains("from thread"),
        "{}",
        from_plain.header
    );
}

fn day_of(c: &Core, node: &str) -> String {
    let (_, n) = c.store.get_node(node).unwrap().unwrap();
    let (y, m, d) = crate::wake::civil_from_days((n.created_at_ms / 1000 / 86_400) as i64);
    format!("{y:04}-{m:02}-{d:02}")
}

/// `thread.read` with a query gives the thread's card and its top notes,
/// each cited by thread and position.
#[tokio::test]
async fn a_read_with_a_query_cites_the_threads_notes() {
    let r = canary();
    let c = &r.core;
    new_thread(c, "security-fix", "security fix");
    let fix = thread_session(c, "security-fix");
    let id = say(c, &fix, "The staging proxy listens on port 8443.");
    let asker = session(c, None, &[]);
    c.runner
        .memory
        .set_ask(scored(c, vec![(fix.clone(), 0, 0.04)]));
    let out = crate::thread_read::call(
        c.clone(),
        &asker,
        json!({"thread": "Security Fix", "query": "staging proxy port"}),
    )
    .await
    .unwrap();
    assert!(
        out.text
            .contains("Thread «security fix» (security-fix, active)"),
        "{}",
        out.text
    );
    let (position, _) = c.store.get_node(&id).unwrap().unwrap();
    let cite = format!("[thread «security fix», {}, @{position}]", day_of(c, &id));
    assert!(out.text.contains(&cite), "{}", out.text);
    assert!(out.text.contains("port 8443"), "{}", out.text);
    assert_eq!(out.meta["shown"], 1);
}
