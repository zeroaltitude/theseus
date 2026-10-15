//! Recall's precision through the whole core (theseus-55hlu): the relevance
//! floor keeps a candidate that is only near the question out of the pack,
//! and a pack of none brings no `Recall` node; the subject bar holds a
//! candidate about another host to a higher bar; an answerable question's
//! gold is still admitted; and a note's header names its session and says
//! the same bytes however often it is rendered. The index is a stand-in that
//! answers the hits a test gives it, each with the fused score it names as a
//! share of the best its one source (`bm25`, weight 1) allows.

use std::sync::Arc;

use theseus_protocol::index::{IndexHit, IndexQueryResult, IndexSourceRank};
use theseus_protocol::memory::{MemorySearchParams, RecallManifest};

use crate::config::MemoryMode;
use crate::node::{Body, Node};
use crate::recall::{Ask, AskFuture};
use crate::session::SessionRecord;
use crate::tests_recall::{dropped_for, rig_with, session, turn, Rig};
use crate::Core;

/// The fused score a lone `bm25` hit at rank 1 has: the ceiling.
const CEILING: f64 = 1.0 / 61.0;

/// One stand-in hit: `node`, at `share` of the ceiling, holding `entities`
/// (of which `matched` are the question's).
struct Spec {
    node: Node,
    position: u64,
    share: f64,
    entities: Vec<&'static str>,
    matched: Vec<&'static str>,
}

fn spec(core: &Core, sid: &str, i: usize, share: f64) -> Spec {
    let (position, node) = core.store.session_nodes(sid).unwrap().remove(i);
    Spec {
        node,
        position,
        share,
        entities: vec![],
        matched: vec![],
    }
}

fn ask(specs: Vec<Spec>) -> Ask {
    let hits: Vec<IndexHit> = specs
        .into_iter()
        .enumerate()
        .map(|(i, s)| IndexHit {
            text: crate::recall::text_of(&s.node),
            node_id: s.node.id,
            chunk: 0,
            session_id: s.node.session_id,
            position: s.position,
            kind: "user_message".into(),
            origin: "operator".into(),
            author: None,
            place: None,
            tool: None,
            time_ms: 0,
            external: false,
            entities_matched: s.matched.iter().map(|e| e.to_string()).collect(),
            entities: s.entities.iter().map(|e| e.to_string()).collect(),
            sources: [(
                "bm25".to_string(),
                IndexSourceRank {
                    rank: i + 1,
                    score: 1.0,
                },
            )]
            .into(),
            fused: s.share * CEILING,
        })
        .collect();
    Arc::new(move |_| -> AskFuture {
        let r = IndexQueryResult {
            hits: hits.clone(),
            indexed_through: 0,
            lag: Default::default(),
            timings: Default::default(),
            skipped: Default::default(),
            weights: [("bm25".to_string(), 1.0)].into(),
        };
        Box::pin(async move { Ok(r) })
    })
}

fn live(floor: f64, bar: f64) -> Rig {
    rig_with(MemoryMode::Live, |c| {
        c.memory.recall_floor = floor;
        c.memory.recall_subject_bar = bar;
    })
}

async fn search(core: &Core, q: &str) -> RecallManifest {
    core.memory_search(MemorySearchParams {
        query: q.into(),
        ..Default::default()
    })
    .await
    .unwrap()
}

/// A question whose every hit is faint gets no pack: the floor admits no
/// candidate for being the best, a live turn writes no `Recall` node and
/// its request carries no note. The same hit with the floor off is recalled.
#[tokio::test]
async fn a_below_floor_query_admits_nothing_and_the_turn_gets_no_note() {
    for (floor, want) in [(0.5, 0), (0.0, 1)] {
        let r = live(floor, 0.0);
        let c = &r.core;
        let a = session(c, None, &["The cedar relay answers on port 4411."]);
        let b = session(c, None, &[]);
        c.runner.memory.set_ask(ask(vec![spec(c, &a, 0, 0.3)]));
        let res = turn(c, &b, "Which port is the alder relay on?").await;
        assert_eq!(res.recalled, want, "floor {floor}");
        let notes = c
            .store
            .session_nodes(&b)
            .unwrap()
            .into_iter()
            .filter(|(_, n)| matches!(n.body, Body::Recall { .. }))
            .count();
        assert_eq!(notes, want as usize, "floor {floor}");
        let sent =
            serde_json::to_string(&r.model.requests.lock().unwrap().last().unwrap().messages)
                .unwrap();
        assert_eq!(
            sent.contains("[Recalled by the harness"),
            want == 1,
            "{sent}"
        );
        if want == 0 {
            let m = search(c, "Which port is the alder relay on?").await;
            assert!(m.admitted.is_empty());
            assert_eq!(dropped_for(&m, "threshold").len(), 1);
            let note = m.relevance.expect("the manifest says what the floor was");
            assert!((note.min_fused - 0.5 * CEILING).abs() < 1e-12);
            assert!((note.ceiling - CEILING).abs() < 1e-12);
        }
    }
}

/// An answerable question's gold, a strong hit, is admitted at the floor
/// that drops the faint one beside it.
#[tokio::test]
async fn an_answerable_questions_gold_is_still_admitted() {
    let r = live(0.4, 0.0);
    let c = &r.core;
    let a = session(
        c,
        None,
        &[
            "The alder relay moved to port 23817 on Monday.",
            "Someone mentioned a relay once.",
        ],
    );
    c.runner
        .memory
        .set_ask(ask(vec![spec(c, &a, 0, 0.9), spec(c, &a, 1, 0.35)]));
    let m = search(c, "Which port is the alder relay on?").await;
    assert_eq!(m.admitted.len(), 1);
    assert!(m.admitted[0].text.as_deref().unwrap().contains("23817"));
    assert_eq!(dropped_for(&m, "threshold").len(), 1);
}

/// A distractor holding another host than the one the question names is
/// held to the subject bar: admitted with no bar, dropped as `subject`
/// under it, while the note that names the asked host is kept.
#[tokio::test]
async fn a_same_kind_distractor_is_demoted_under_the_subject_bar() {
    for (bar, want) in [(0.0, vec!["prod", "stage"]), (0.8, vec!["prod"])] {
        let r = live(0.2, bar);
        let c = &r.core;
        let a = session(
            c,
            None,
            &[
                "The production database host is db-prod.example.",
                "The staging database host is db-stage.example.",
            ],
        );
        let mut prod = spec(c, &a, 0, 0.9);
        (prod.entities, prod.matched) =
            (vec!["host:db-prod.example"], vec!["host:db-prod.example"]);
        let mut stage = spec(c, &a, 1, 0.6);
        stage.entities = vec!["host:db-stage.example"];
        c.runner.memory.set_ask(ask(vec![prod, stage]));
        let m = search(c, "What is the production database host db-prod.example?").await;
        let texts: Vec<&str> = m
            .admitted
            .iter()
            .map(|i| i.text.as_deref().unwrap())
            .collect();
        let names: Vec<&str> = want
            .iter()
            .map(|w| {
                *texts
                    .iter()
                    .find(|t| t.contains(w) || (*w == "prod" && t.contains("production")))
                    .unwrap_or(&"?")
            })
            .collect();
        assert_eq!(texts.len(), want.len(), "bar {bar}: {texts:?}");
        assert!(!names.contains(&"?"), "bar {bar}: {texts:?}");
        assert_eq!(
            dropped_for(&m, "subject").len(),
            2 - want.len(),
            "bar {bar}"
        );
    }
}

/// An admitted note's frozen header names its session (short id and title);
/// rendering the node twice gives one set of bytes, and a second request
/// begins with the first's.
#[tokio::test]
async fn a_recalled_notes_header_names_its_session_and_stays_frozen() {
    let r = live(0.0, 0.0);
    let c = &r.core;
    let a = session(c, None, &["The cedar relay answers on port 4411."]);
    let mut rec = c.store.get_session::<SessionRecord>(&a).unwrap().unwrap();
    rec.title = Some("Relay migration".into());
    c.store.put_session(&a, &rec).unwrap();
    let b = session(c, None, &[]);
    c.runner.memory.set_ask(ask(vec![spec(c, &a, 0, 0.9)]));
    turn(c, &b, "Which port is the cedar relay on?").await;
    let note = c
        .store
        .session_nodes(&b)
        .unwrap()
        .into_iter()
        .find(|(_, n)| matches!(n.body, Body::Recall { .. }))
        .expect("a Recall node")
        .1;
    let Body::Recall { items, .. } = &note.body else {
        unreachable!()
    };
    let short = crate::recall::evidence::short(&a);
    assert!(
        items[0]
            .header
            .contains(&format!("session {short} \"Relay migration\"")),
        "{}",
        items[0].header
    );
    let sources =
        crate::recall::render::read_sources(&c.store, std::iter::once(&note), &Default::default());
    let once = crate::recall::render::render(items, &sources);
    assert_eq!(once, crate::recall::render::render(items, &sources));
    assert!(once.contains("a note about another subject is not an answer"));
    // The next request carries the same note, byte for byte.
    c.runner.memory.set_ask(ask(vec![spec(c, &a, 0, 0.9)]));
    turn(c, &b, "And the alder relay?").await;
    let reqs = r.model.requests.lock().unwrap().clone();
    let first = serde_json::to_string(&reqs[0].messages).unwrap();
    let last = serde_json::to_string(&reqs[reqs.len() - 1].messages).unwrap();
    let head = |s: &str| {
        let at = s.find("(1) ").unwrap();
        s[at..at + 160].to_string()
    };
    assert_eq!(head(&first), head(&last));
}
