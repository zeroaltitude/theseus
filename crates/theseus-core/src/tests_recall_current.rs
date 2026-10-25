//! A current-value question through a live rerank (theseus-mo1vm, join fix
//! 3): the older statement's "superseded" mark is set by `current::mark`
//! after the recall's fill, and `Memory::refill` (Jev's order, 32d) rebuilds
//! the admitted items through `fill`, which sets none. The refill puts the
//! marks back, so the frozen header on the Recall node, and the request the
//! model gets, still says the older note is out of date.

use std::sync::Arc;

use theseus_judge::fake::{FakeJev, Scripted as Jev};
use theseus_protocol::index::{IndexHit, IndexQueryResult, IndexSourceRank};
use theseus_protocol::SessionKind;

use crate::config::MemoryMode;
use crate::node::Node;
use crate::recall::{Ask, AskFuture};
use crate::session::SessionRecord;
use crate::tests_rerank::{recalls, rig, sent, session, turn};
use crate::Core;

const DAY: u64 = 86_400_000;
/// 2026-03-12 12:00 UTC.
const MARCH_12: u64 = 1_773_316_800_000;
const OLD: &str = "example.test keeps 14 days of audit logs.";
const NEW: &str = "example.test audit logs: keep 30 days now.";
const MARK: &str = "superseded: older than the ";

fn said_at(core: &Core, said: &str, at_ms: u64) -> String {
    let r = SessionRecord::new(SessionKind::Conversation, None);
    core.store.put_session(&r.session_id, &r).unwrap();
    let mut n = Node::user(&r.session_id, None, "test", said);
    n.created_at_ms = at_ms;
    core.store.append(&[n.record().unwrap()]).unwrap();
    r.session_id
}

/// A stand-in index over `sessions`, in the order given (the first is the
/// best), each hit with its node's own time.
fn index_with_times(core: &Arc<Core>, sessions: Vec<String>) -> Ask {
    let store = core.store.clone();
    Arc::new(move |_| -> AskFuture {
        let hits = sessions
            .iter()
            .flat_map(|s| store.session_nodes(s).unwrap())
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
                entities: vec![],
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

/// "How many days of audit logs does example.test keep now?" is a
/// current-value question; the older note says 14, the newer 30. Jev puts
/// the older first and the rerank applies (`Memory::refill`), yet the older
/// note's header still says it is superseded, and the newer's says nothing.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_live_rerank_keeps_the_superseded_marks_of_a_current_question() {
    let jev = FakeJev::start().unwrap();
    // Jev admits the older note first: its question is asked about first.
    jev.script("helps.1", Jev::Noul(0.97));
    jev.script("helps.2", Jev::Noul(0.10));
    let r = rig(Some(&jev), |c| {
        c.memory.mode = MemoryMode::Live;
        c.memory.recall_max_items = 2;
        // A loaded machine's local Jev may take more than 200 ms.
        c.memory.rerank_wait_ms = 600;
    });
    let c = &r.core;
    let old = said_at(c, OLD, MARCH_12);
    let new = said_at(c, NEW, MARCH_12 + 30 * DAY);
    let here = session(c, None, &[]);
    c.runner.memory.set_ask(index_with_times(c, vec![old, new]));
    turn(
        c,
        &here,
        "How many days of audit logs does example.test keep now?",
    )
    .await;
    let m = &recalls(c, &here)[0];
    assert_eq!(m.temporal.as_deref(), Some("current"), "{m:?}");
    let rr = m.rerank.as_ref().expect("the manifest's rerank");
    assert!(rr.applied, "Jev's order was applied: {rr:?}");
    assert_eq!(m.admitted.len(), 2, "both notes are admitted: {m:?}");
    let req = sent(&r.model).pop().unwrap();
    assert!(req.contains(OLD) && req.contains(NEW), "{req}");
    // One mark: the newest statement carries none, so it is the older note's.
    assert_eq!(req.matches(MARK).count(), 1, "{req}");
}
