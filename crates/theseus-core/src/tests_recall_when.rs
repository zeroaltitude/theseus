//! Dates in the question (theseus-w9qv, fix B): a turn whose words name a
//! span of time ("March 2026") asks the index for hits inside it and keeps
//! recall there, so a March item is recalled over a September one; a turn
//! that names none recalls as before. The stand-in index answers every node
//! whatever the span, as a tender of an older build would, so the core's
//! own guard is what these hold.

use std::sync::{Arc, Mutex};

use theseus_protocol::index::{IndexFilters, IndexHit, IndexQueryResult, IndexSourceRank};
use theseus_protocol::SessionKind;

use crate::config::MemoryMode;
use crate::node::Node;
use crate::recall::{Ask, AskFuture};
use crate::session::SessionRecord;
use crate::tests_recall::{admitted, recalls, rig, session, turn};
use crate::Core;

/// 2026-03-12 and 2026-09-12, noon UTC: inside March and September in any
/// time zone the daemon runs in.
const MARCH_12: u64 = 1_773_316_800_000;
const SEPTEMBER_12: u64 = 1_789_214_400_000;

/// A session saying `said` at `at_ms`.
fn said_at(core: &Core, said: &str, at_ms: u64) -> String {
    let r = SessionRecord::new(SessionKind::Conversation, None);
    core.store.put_session(&r.session_id, &r).unwrap();
    let mut n = Node::user(&r.session_id, None, "test", said);
    n.created_at_ms = at_ms;
    core.store.append(&[n.record().unwrap()]).unwrap();
    r.session_id
}

/// A stand-in index over `sessions` that keeps every query's filters and
/// answers every node with its own time, the newest first: the span is
/// ignored, as an older tender ignores it.
fn dated_index(
    core: &Arc<Core>,
    sessions: Vec<String>,
    asked: Arc<Mutex<Vec<IndexFilters>>>,
) -> Ask {
    let store = core.store.clone();
    Arc::new(move |p| -> AskFuture {
        asked.lock().unwrap().push(p.filters);
        let mut nodes: Vec<(u64, Node)> = sessions
            .iter()
            .flat_map(|s| store.session_nodes(s).unwrap())
            .collect();
        nodes.sort_by_key(|(_, n)| std::cmp::Reverse(n.created_at_ms));
        let hits = nodes
            .into_iter()
            .enumerate()
            .map(|(i, (position, n))| IndexHit {
                text: crate::recall::text_of(&n),
                time_ms: n.created_at_ms,
                node_id: n.id,
                chunk: 0,
                session_id: n.session_id,
                position,
                kind: "user_message".into(),
                origin: "operator".into(),
                author: None,
                place: None,
                tool: None,
                external: false,
                entities_matched: vec![],
                sources: [(
                    "bm25".to_string(),
                    IndexSourceRank {
                        rank: i + 1,
                        score: 1.0,
                    },
                )]
                .into(),
                fused: 1.0 / (61 + i) as f64,
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

/// A question naming March 2026 asks the index for March alone, and recalls
/// the March session over the September one, which ranked first; the row
/// says the span and counts the drop.
#[tokio::test]
async fn a_question_naming_march_recalls_a_march_item_over_a_september_one() {
    let r = rig(MemoryMode::Shadow);
    let c = &r.core;
    let march = said_at(c, "we refactored the tide-table parser", MARCH_12);
    let september = said_at(c, "we refactored the tide-table renderer", SEPTEMBER_12);
    let here = session(c, None, &[]);
    let asked = Arc::new(Mutex::new(Vec::new()));
    c.runner.memory.set_ask(dated_index(
        c,
        vec![march.clone(), september.clone()],
        asked.clone(),
    ));
    turn(c, &here, "remember our coding sessions from March 2026?").await;
    let m = &recalls(c, &here)[0];
    assert_eq!(admitted(m), [march.as_str()], "{m:?}");
    assert_eq!(
        m.drops.get("when"),
        Some(&1),
        "the September hit, by the span"
    );
    let w = m.when.as_ref().expect("the row names the span");
    assert_eq!(w.said, "March 2026");
    let (from, to) = (w.from_ms.unwrap(), w.to_ms.unwrap());
    assert!(
        from <= MARCH_12 && MARCH_12 < to && to <= SEPTEMBER_12,
        "{w:?}"
    );
    // The index was asked for the span itself.
    let f = asked.lock().unwrap()[0].clone();
    assert_eq!((f.from_ms, f.to_ms), (w.from_ms, w.to_ms));
}

/// A question that names no time recalls both, the newer first, and asks the
/// index for no span.
#[tokio::test]
async fn a_question_naming_no_time_recalls_across_all_of_it() {
    let r = rig(MemoryMode::Shadow);
    let c = &r.core;
    let march = said_at(c, "we refactored the tide-table parser", MARCH_12);
    let september = said_at(c, "we refactored the tide-table renderer", SEPTEMBER_12);
    let here = session(c, None, &[]);
    let asked = Arc::new(Mutex::new(Vec::new()));
    c.runner.memory.set_ask(dated_index(
        c,
        vec![march.clone(), september.clone()],
        asked.clone(),
    ));
    // "march" here is a verb: no time.
    turn(c, &here, "how did we march the tide-table refactor along?").await;
    let m = &recalls(c, &here)[0];
    assert_eq!(admitted(m), [september.as_str(), march.as_str()]);
    assert!(m.when.is_none() && !m.drops.contains_key("when"), "{m:?}");
    assert!(asked.lock().unwrap()[0].is_empty());
}
