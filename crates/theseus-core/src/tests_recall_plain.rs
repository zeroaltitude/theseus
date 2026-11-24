//! Recall reads no English from the turn's message (issue 46): a question
//! about time is searched as written, a named month narrows nothing, and a
//! "now" question is packed and headed as any other. A time is the model's
//! to ask, through the history tool's `when` or `from`/`to`. And a row an
//! older build wrote, with its `temporal` key, still reads.

use std::sync::{Arc, Mutex};

use serde_json::json;
use theseus_protocol::index::{IndexFilters, IndexHit, IndexQueryResult, IndexSourceRank};
use theseus_protocol::memory::RecallManifest;
use theseus_protocol::{LedgerKind, SessionKind};
use theseus_store::{kinds, NewRecord};

use crate::config::MemoryMode;
use crate::ledger::LedgerRow;
use crate::node::Node;
use crate::recall::{Ask, AskFuture};
use crate::session::SessionRecord;
use crate::tests_recall::{admitted, recalls, rig, session, turn};
use crate::tests_rerank::sent;
use crate::Core;

const DAY: u64 = 86_400_000;
/// 2026-03-12 and 2026-09-12, noon UTC: inside March and September in any
/// time zone the daemon runs in.
const MARCH_12: u64 = 1_773_316_800_000;
const SEPTEMBER_12: u64 = 1_789_214_400_000;

/// What the index was asked: each query's text and filters.
type Asked = Arc<Mutex<Vec<(String, IndexFilters)>>>;

/// A session saying `said` at `at_ms`.
fn said_at(core: &Core, said: &str, at_ms: u64) -> String {
    let r = SessionRecord::new(SessionKind::Conversation, None);
    core.store.put_session(&r.session_id, &r).unwrap();
    let mut n = Node::user(&r.session_id, None, "test", said);
    n.created_at_ms = at_ms;
    core.store.append(&[n.record().unwrap()]).unwrap();
    r.session_id
}

/// A stand-in index over `sessions`, in the order given (the first is the
/// best), each hit with its node's own time; every span is ignored, so what
/// recall keeps is the core's doing.
fn index_in_order(core: &Arc<Core>, sessions: Vec<String>, asked: Asked) -> Ask {
    let store = core.store.clone();
    Arc::new(move |p| -> AskFuture {
        asked.lock().unwrap().push((p.text, p.filters));
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

/// The row as it is stored: no `temporal` key on a recall this build ran.
fn has_temporal(m: &RecallManifest) -> bool {
    serde_json::to_value(m).unwrap().get("temporal").is_some()
}

/// "When did we switch the cache TTL?" asks the index the message's own
/// words, and the row names no kind of question about time.
#[tokio::test]
async fn a_question_about_time_is_searched_as_written() {
    let r = rig(MemoryMode::Live);
    let c = &r.core;
    let plan = said_at(c, "Should we look at the cache TTL some time?", MARCH_12);
    let event = said_at(
        c,
        "We switched the cache TTL from 60 s to 5 minutes.",
        MARCH_12 + 2 * DAY,
    );
    let here = session(c, None, &[]);
    let asked: Asked = Arc::default();
    c.runner
        .memory
        .set_ask(index_in_order(c, vec![plan, event], asked.clone()));
    turn(c, &here, "When did we switch the cache TTL?").await;
    let m = &recalls(c, &here)[0];
    assert!(!has_temporal(m), "{m:?}");
    let (q, _) = asked.lock().unwrap()[0].clone();
    assert!(q.starts_with("When did we switch the cache TTL?"), "{q}");
}

/// A message naming March 2026 keeps recall to no span: both the March and
/// the September note are admitted, nothing is dropped for a span, and the
/// index is asked for none. Paging by date is the history tool's `when`.
#[tokio::test]
async fn a_named_month_does_not_narrow_the_turns_recall() {
    let r = rig(MemoryMode::Live);
    let c = &r.core;
    let march = said_at(c, "we refactored the tide-table parser", MARCH_12);
    let september = said_at(c, "we refactored the tide-table renderer", SEPTEMBER_12);
    let here = session(c, None, &[]);
    let asked: Asked = Arc::default();
    c.runner.memory.set_ask(index_in_order(
        c,
        vec![september.clone(), march.clone()],
        asked.clone(),
    ));
    turn(c, &here, "what did we decide in March 2026?").await;
    let m = &recalls(c, &here)[0];
    assert!(m.when.is_none(), "{m:?}");
    assert!(!m.drops.contains_key("when"), "{m:?}");
    assert_eq!(admitted(m), [september.as_str(), march.as_str()], "{m:?}");
    let (_, f) = asked.lock().unwrap()[0].clone();
    assert!(f.is_empty(), "{f:?}");
}

/// "How many days of WAL segments do we keep now?" recalls in the order the
/// same question without "now" recalls in, and no header says a note is
/// superseded: the model reads the dated notes and judges.
#[tokio::test]
async fn a_now_question_keeps_the_packs_order_and_marks_nothing() {
    let r = rig(MemoryMode::Live);
    let c = &r.core;
    let old = said_at(c, "example.test keeps 7 days of WAL segments.", MARCH_12);
    let new = said_at(
        c,
        "example.test WAL segments: keep 21 days from today.",
        MARCH_12 + 30 * DAY,
    );
    let asked: Asked = Arc::default();
    c.runner.memory.set_ask(index_in_order(
        c,
        vec![old.clone(), new.clone()],
        asked.clone(),
    ));
    let plain = session(c, None, &[]);
    turn(c, &plain, "How many days of WAL segments do we keep?").await;
    let now = session(c, None, &[]);
    turn(c, &now, "How many days of WAL segments do we keep now?").await;
    let (p, n) = (&recalls(c, &plain)[0], &recalls(c, &now)[0]);
    assert_eq!(admitted(n).len(), 2, "both notes are admitted: {n:?}");
    assert_eq!(admitted(n), admitted(p), "{n:?}");
    assert!(!has_temporal(n), "{n:?}");
    let req = sent(&r.model).pop().unwrap();
    assert!(
        req.contains("7 days of WAL") && req.contains("keep 21 days"),
        "{req}"
    );
    assert!(!req.contains("superseded: older than"), "{req}");
}

/// A `recall.ran` row an older build wrote, holding the `temporal` key and a
/// turn's `when`, still reads through `memory.recalls`: the manifest has no
/// unknown-field guard, and the row's decoder takes its data as JSON.
#[test]
fn an_old_recall_row_with_a_temporal_key_still_reads() {
    let r = rig(MemoryMode::Live);
    let c = &r.core;
    let here = session(c, None, &[]);
    let mut data = serde_json::to_value(RecallManifest {
        recall_id: "rcl_old".into(),
        mode: "live".into(),
        science: "baseline".into(),
        outcome: "ran".into(),
        session_id: Some(here.clone()),
        turn_id: Some("turn_old".into()),
        candidates: 2,
        ..RecallManifest::default()
    })
    .unwrap();
    // The two keys an older build wrote on a turn's row.
    data["when"] = json!({"said": "March 2026", "from_ms": MARCH_12, "to_ms": SEPTEMBER_12});
    data["temporal"] = json!("current");
    let row = LedgerRow::new(LedgerKind::RecallRan, Some(&here), Some("turn_old"), data);
    let rec = NewRecord::json(kinds::LEDGER, Some(&here), &row)
        .unwrap()
        .scoped(&crate::fact::recall::scope(&here));
    c.store.append(&[rec]).unwrap();
    let got = recalls(c, &here);
    assert_eq!(got.len(), 1, "the old row reads: {got:?}");
    assert_eq!(got[0].recall_id, "rcl_old");
    assert_eq!(got[0].candidates, 2);
    assert_eq!(
        got[0].when.as_ref().map(|w| w.said.as_str()),
        Some("March 2026")
    );
}
