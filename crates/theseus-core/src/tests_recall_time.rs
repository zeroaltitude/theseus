//! A question about time through the whole core (theseus-q1tbn): the index
//! is asked the event alone, and recall puts the note that states the event
//! above later notes that only mention it; a question that is not about time
//! recalls in the index's order, as before. The stand-in index answers the
//! newest node first, so the mentions out-rank the event as they do in a
//! store where the subject comes up again and again.

use std::sync::{Arc, Mutex};

use theseus_protocol::index::{IndexHit, IndexQueryResult, IndexSourceRank};
use theseus_protocol::SessionKind;

use crate::config::MemoryMode;
use crate::node::Node;
use crate::recall::{Ask, AskFuture};
use crate::session::SessionRecord;
use crate::tests_recall::{recalls, rig, session, turn};
use crate::Core;

const DAY: u64 = 86_400_000;
/// 2026-03-12 12:00 UTC.
const MARCH_12: u64 = 1_773_316_800_000;

fn said_at(core: &Core, said: &str, at_ms: u64) -> String {
    let r = SessionRecord::new(SessionKind::Conversation, None);
    core.store.put_session(&r.session_id, &r).unwrap();
    let mut n = Node::user(&r.session_id, None, "test", said);
    n.created_at_ms = at_ms;
    core.store.append(&[n.record().unwrap()]).unwrap();
    r.session_id
}

/// Every node of `sessions`, the newest first, with its own time; the texts
/// the index was asked are kept.
fn newest_first(core: &Arc<Core>, sessions: Vec<String>, asked: Arc<Mutex<Vec<String>>>) -> Ask {
    let store = core.store.clone();
    Arc::new(move |p| -> AskFuture {
        asked.lock().unwrap().push(p.text);
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

struct Notes {
    event: String,
    plan: String,
    later: Vec<String>,
    all: Vec<String>,
}

fn notes(c: &Core) -> Notes {
    let plan = said_at(c, "Should we look at the cache TTL some time?", MARCH_12);
    let event = said_at(
        c,
        "We switched the cache TTL from 60 s to 5 minutes.",
        MARCH_12 + 2 * DAY,
    );
    let later: Vec<String> = [
        "The cache TTL is fine, thanks.",
        "Size the box with the cache TTL in mind.",
        "Cache TTL: still the new value.",
    ]
    .iter()
    .enumerate()
    .map(|(i, t)| said_at(c, t, MARCH_12 + (10 + i as u64) * DAY))
    .collect();
    let mut all = vec![plan.clone(), event.clone()];
    all.extend(later.clone());
    Notes {
        event,
        plan,
        later,
        all,
    }
}

fn order_of(m: &theseus_protocol::memory::RecallManifest) -> Vec<&str> {
    m.admitted.iter().map(|a| a.session_id.as_str()).collect()
}

/// "When did we switch the cache TTL?" asks the index for the event alone
/// and recalls the note that states it first, above three later notes that
/// mention it; the row names the kind.
#[tokio::test]
async fn a_temporal_question_recalls_the_event_above_later_mentions() {
    let r = rig(MemoryMode::Shadow);
    let c = &r.core;
    let n = notes(c);
    let here = session(c, None, &[]);
    let asked = Arc::new(Mutex::new(Vec::new()));
    c.runner
        .memory
        .set_ask(newest_first(c, n.all.clone(), asked.clone()));
    turn(c, &here, "When did we switch the cache TTL?").await;
    let m = &recalls(c, &here)[0];
    assert_eq!(m.temporal.as_deref(), Some("when"));
    let got = order_of(m);
    assert_eq!(got[0], n.event, "{m:?}");
    // The plan, which only mentions it, comes before the later mentions.
    assert_eq!(got[1], n.plan);
    assert_eq!(
        got[2..],
        n.later.iter().map(String::as_str).collect::<Vec<_>>()
    );
    let q = asked.lock().unwrap()[0].clone();
    assert!(q.contains("switch the cache TTL"), "{q}");
    assert!(!q.to_lowercase().contains("when did"), "{q}");
}

/// A question that is not about time recalls in the index's order, the
/// newest first here, and the index is asked the question as written.
#[tokio::test]
async fn a_question_that_is_not_about_time_keeps_the_indexs_order() {
    let r = rig(MemoryMode::Shadow);
    let c = &r.core;
    let n = notes(c);
    let here = session(c, None, &[]);
    let asked = Arc::new(Mutex::new(Vec::new()));
    c.runner
        .memory
        .set_ask(newest_first(c, n.all.clone(), asked.clone()));
    turn(c, &here, "What is the cache TTL?").await;
    let m = &recalls(c, &here)[0];
    assert_eq!(m.temporal, None);
    let mut want: Vec<&str> = n.later.iter().rev().map(String::as_str).collect();
    want.extend([n.event.as_str(), n.plan.as_str()]);
    assert_eq!(order_of(m), want);
    assert!(asked.lock().unwrap()[0].starts_with("What is the cache TTL?"));
}
