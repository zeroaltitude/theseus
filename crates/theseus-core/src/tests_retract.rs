//! `fact.retract` through the whole core (theseus-2ced): a planted fact,
//! recalled into sessions that used it, saw it, or ran it as a task, and
//! cited by a synthesis, is marked wrong, and the retraction finds every
//! exposure and sorts each right (ground truth from the plant); the live
//! session's next turn carries the correction note once; the synthesis is
//! stale and out of recall; one card names the tasks; recall never serves
//! the node again; a second retraction writes nothing; a shared place's run
//! hides private text. Facts and names are invented.

use std::sync::Arc;

use theseus_protocol::memory::MemorySearchParams;
use theseus_protocol::retract::{exposure_kind as kind, FactRetractParams, FactRetractResult};
use theseus_protocol::{LedgerKind, SessionKind};

use crate::approval::{Answerer, Surface};
use crate::config::MemoryMode;
use crate::fact::memory::MemoryUsed;
use crate::graph::{Edge, EdgeKind};
use crate::ledger::LedgerRow;
use crate::memory_pass::attribution::Use;
use crate::node::{Body, Node, Origin};
use crate::places::BoundPlace;
use crate::session::{SessionRecord, TaskOf};
use crate::tests_recall::{index_of, rig_with, session, turn, Rig, DEN, PIER};
use crate::Core;

const FACT: &str = "The Kestrel relay listens on port 7714.";
const WHY: &str = "the relay moved to port 7715 in the autumn change";
const DM: &str = "discord:dm:5150";

fn rig() -> Rig {
    rig_with(MemoryMode::Live, |c| {
        c.memory.recall_deadline_ms = 2000;
        c.memory.recall_max_items = 10;
    })
}

/// The places as the binding tells them: the owner's DM and a private den,
/// and a shared pier.
fn bind(core: &Core) {
    core.bind_places(vec![
        BoundPlace {
            target: DM.into(),
            name: "DM @owner".into(),
            private: true,
            ..Default::default()
        },
        BoundPlace {
            target: format!("discord:channel:{DEN}"),
            name: "#den".into(),
            private: true,
            ..Default::default()
        },
        BoundPlace {
            target: format!("discord:channel:{PIER}"),
            name: "#pier".into(),
            private: false,
            ..Default::default()
        },
    ]);
}

fn retract(core: &Core, node: &str) -> anyhow::Result<FactRetractResult> {
    core.fact_retract(
        &FactRetractParams {
            node: node.into(),
            why: WHY.into(),
            session_id: None,
        },
        "cli",
    )
}

/// The planted fact: a session that said it, and its node.
fn plant(core: &Arc<Core>) -> (String, String) {
    let src = session(core, None, &[FACT]);
    core.runner
        .memory
        .set_ask(index_of(core, vec![src.clone()]));
    let id = core.store.session_nodes(&src).unwrap()[0].1.id.clone();
    (src, id)
}

/// A session that asks, in a turn whose recall admits the fact.
async fn asker(core: &Arc<Core>, first: &str) -> String {
    let sid = session(core, None, &[]);
    turn(core, &sid, first).await;
    sid
}

/// The recall node `sid`'s turn wrote, and the recall's id.
fn recall_of(core: &Core, sid: &str) -> (String, String) {
    core.store
        .session_nodes(sid)
        .unwrap()
        .into_iter()
        .find_map(|(_, n)| match &n.body {
            Body::Recall { recall_id, .. } => Some((n.id.clone(), recall_id.clone())),
            _ => None,
        })
        .expect("the turn's recall node")
}

/// What the memory pass writes of a recalled item: `memory.used`, scoped
/// to the session's recalls.
fn used(core: &Core, sid: &str, recall_id: &str, node_id: &str, yes: bool) {
    let u = Use {
        used: yes,
        by: vec![],
    };
    let f = MemoryUsed {
        recall_id,
        arm: "baseline",
        node_id,
        source_session: "ses_src",
        used: &u,
        outcome: None,
        entities_unavailable: None,
    };
    let row = crate::fact::row(&f, Some(sid), None)
        .unwrap()
        .scoped(&crate::fact::recall::scope(sid));
    core.store.append(&[row]).unwrap();
}

/// A task session that recalled `of` (a recall node with its edge and its
/// `memory.used` row), written by hand: never run.
fn task_that_recalled(core: &Core, title: &str, of: &str, yes: bool) -> String {
    let parent = session(core, None, &[]);
    let mut rec = SessionRecord::new(SessionKind::Task, None);
    rec.title = Some(title.into());
    rec.task = Some(TaskOf {
        parent_session: parent,
        parent_execution: "exe_none".into(),
        by: "call_none".into(),
        target: None,
        arrangement: None,
        check: None,
        notify_done: false,
    });
    core.store.put_session(&rec.session_id, &rec).unwrap();
    let rn = Node::recall(&rec.session_id, "turn_t", "rcl_task", "baseline", vec![]);
    let edge = Edge::new(EdgeKind::DerivedFrom, &rn.id, of, crate::graph::VIA_RECALL);
    core.store
        .append(&[rn.record().unwrap(), edge.record().unwrap()])
        .unwrap();
    used(core, &rec.session_id, "rcl_task", of, yes);
    rec.session_id
}

/// A synthesis citing `of`, in the memory's harness session, as the retired
/// nightly writer left them in a store: a retraction still reads it.
fn synthesis_of(core: &Core, of: &str) -> String {
    let memory = match core.memory_session().unwrap() {
        Some(id) => id,
        None => {
            let rec = core
                .open_session(theseus_protocol::SessionOpenParams {
                    dir: None,
                    kind: None,
                    label: Some("memory: consolidation's syntheses (never compiled)".into()),
                    opened_from: None,
                    persona: None,
                })
                .unwrap();
            core.store
                .put_meta(crate::consolidate::MEMORY_SESSION, &rec.session_id)
                .unwrap();
            core.runner.memory.harness_opened(&rec.session_id);
            rec.session_id
        }
    };
    let body = Body::Synthesis {
        text: "The Kestrel relay listens on port 7714 [1].".into(),
        sources: vec![of.into()],
        check: crate::consolidate::CitationCheck::Unchecked { why: "test".into() },
        stage: crate::consolidate::Stage::Shadow,
        cluster: "0123456789abcdef".into(),
        profile: "test".into(),
        model: "test".into(),
        cost_usd: None,
    };
    let n = Node::synthesis(&memory, body);
    let edge = Edge::new(
        EdgeKind::DerivedFrom,
        &n.id,
        of,
        crate::graph::VIA_SYNTHESIS,
    );
    core.store
        .append(&[n.record().unwrap(), edge.record().unwrap()])
        .unwrap();
    n.id
}

fn rows(core: &Core, kind: LedgerKind) -> Vec<LedgerRow> {
    let all: Vec<(u64, LedgerRow)> = core.store.ledger_tail(20_000).unwrap();
    all.into_iter()
        .map(|(_, r)| r)
        .filter(|r| r.kind == kind.as_str())
        .collect()
}

/// The posts of `kind` waiting for the owner.
fn posts(core: &Core, kind: &str) -> Vec<serde_json::Value> {
    core.outbox
        .open_for(crate::outbox::OPERATOR_TARGET)
        .iter()
        .filter(|a| crate::outbox::kind_of(a) == kind)
        .map(|a| crate::outbox::body_of(a).clone())
        .collect()
}

/// How many times `needle` is in the request the model last got.
fn in_last_request(r: &Rig, needle: &str) -> usize {
    let req = r.model.requests().pop().expect("a request");
    serde_json::to_string(&req.messages)
        .unwrap()
        .matches(needle)
        .count()
}

fn kinds_of(r: &FactRetractResult) -> Vec<&str> {
    r.exposures.iter().map(|e| e.kind.as_str()).collect()
}

/// The planted fact is recalled into a session that used it, a task that
/// used it, and a session that only saw it, and is cited by a synthesis: the
/// retraction finds the four and sorts each right, labels the node and the
/// synthesis, posts one card naming the task, and recall never serves the
/// node again.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_retraction_finds_every_exposure_and_sorts_each_right() {
    let r = rig();
    let c = &r.core;
    bind(c);
    let (_, x) = plant(c);
    let a = asker(c, "What port does the Kestrel relay use?").await;
    let s = asker(c, "Which port is the Kestrel relay on, again?").await;
    let (_, a_rcl) = recall_of(c, &a);
    let (_, s_rcl) = recall_of(c, &s);
    used(c, &a, &a_rcl, &x, true);
    used(c, &s, &s_rcl, &x, false);
    let t = task_that_recalled(c, "rotate the relay's certificates", &x, true);
    let syn = synthesis_of(c, &x);

    let out = retract(c, &x).unwrap();
    assert!(!out.already);
    assert_eq!(out.node_id, x);
    assert_eq!(out.why, WHY);
    assert_eq!(
        kinds_of(&out),
        [kind::SESSION, kind::SYNTHESIS, kind::TASK, kind::SEEN],
        "{out:#?}"
    );
    let by = |k: &str| out.exposures.iter().find(|e| e.kind == k).unwrap();
    assert_eq!(by(kind::SESSION).session_id, a);
    assert_eq!(by(kind::SESSION).used, Some(true));
    assert_eq!(by(kind::SYNTHESIS).node_id, syn);
    assert_eq!(by(kind::TASK).session_id, t);
    assert!(by(kind::TASK).place.contains("rotate the relay"));
    assert_eq!(by(kind::SEEN).session_id, s);
    assert_eq!(by(kind::SEEN).used, Some(false));
    assert!(!out.partial);

    // The node is wrong and the synthesis stale: recall leaves both out.
    let labeled = c.runner.memory.labeled(&c.store).unwrap();
    assert!(labeled.contains(&x), "{labeled:?}");
    assert!(labeled.contains(&syn), "{labeled:?}");
    let labels = rows(c, LedgerKind::MemoryLabel);
    let of = |n: &str| {
        labels
            .iter()
            .find(|l| l.data["node_id"] == n)
            .map(|l| l.data["label"].as_str().unwrap().to_string())
    };
    assert_eq!(of(&x).as_deref(), Some("wrong"));
    assert_eq!(of(&syn).as_deref(), Some("stale"));

    // One card, naming the task, to the owner's DM.
    assert!(out.card);
    let cards = posts(c, "retraction");
    assert_eq!(cards.len(), 1, "{cards:?}");
    let text = cards[0]["text"].as_str().unwrap();
    assert!(text.contains("rotate the relay's certificates"), "{text}");
    assert!(text.contains("Re-run?"), "{text}");

    // Recall never serves it again.
    let m = c
        .memory_search(MemorySearchParams {
            query: "Kestrel relay port".into(),
            session_id: None,
            k: None,
            arm: None,
            as_of: None,
        })
        .await
        .unwrap();
    assert!(!m.admitted.iter().any(|i| i.node_id == x), "{m:?}");
    assert!(
        m.dropped
            .iter()
            .any(|d| d.node_id == x && d.reason == "labeled_wrong"),
        "{m:?}"
    );
}

/// A fact said in a view turn is in the retraction's spread (issue 44): the turn's own calls laid it, so
/// `node.reach` names its compilation and session, never "nowhere", and the retraction finds the session.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_fact_a_view_turn_was_sent_is_in_the_spread() {
    let r = rig();
    let c = &r.core;
    let src = session(c, None, &[]);
    turn(c, &src, FACT).await;
    assert!(
        in_last_request(&r, "port 7714") >= 1,
        "the turn sent the fact"
    );
    let x = (c.store.session_nodes(&src).unwrap().into_iter())
        .find(|(_, n)| matches!(&n.body, Body::UserMessage { text, .. } if text == FACT))
        .expect("the turn's message")
        .1
        .id;
    let reach = crate::reach::reach(&c.store, &x, None).unwrap().unwrap();
    assert!(reach.direct.loops >= 1, "{reach:?}");
    assert!(
        (reach.direct.compilations.iter()).any(|k| k.session_id == src),
        "{reach:?}"
    );
    let out = retract(c, &x).unwrap();
    assert!(
        out.exposures.iter().any(|e| e.session_id == src),
        "{out:#?}"
    );
}

/// A recalled fact is where the asking turn sent it (issue 44): the request held it, and reach and the
/// retraction name the asking session.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_recalled_fact_is_where_the_asking_turn_sent_it() {
    let r = rig();
    let c = &r.core;
    let (_, x) = plant(c);
    let a = asker(c, "What port does the Kestrel relay use?").await;
    assert!(
        in_last_request(&r, "port 7714") >= 1,
        "the recalled fact was sent"
    );
    let reach = crate::reach::reach(&c.store, &x, None).unwrap().unwrap();
    let laid = (reach.direct.compilations.iter()).any(|k| k.session_id == a)
        || (reach.descendants.iter()).any(|d| d.session_id == a && d.exposure.loops >= 1);
    assert!(laid, "{reach:?}");
    let out = retract(c, &x).unwrap();
    assert!(out.exposures.iter().any(|e| e.session_id == a), "{out:#?}");
}

/// A fact one session said is laid by another session's turn at the same place with no copy to follow
/// (the place's window, issue 44): reach names that session's compilation, and the retraction finds it.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_fact_laid_by_another_sessions_window_is_in_the_spread() {
    let r = rig();
    let c = &r.core;
    bind(c);
    let one = session(c, Some(DM), &[]);
    turn(c, &one, FACT).await;
    let x = (c.store.session_nodes(&one).unwrap().into_iter())
        .find(|(_, n)| matches!(&n.body, Body::UserMessage { text, .. } if text == FACT))
        .expect("the turn's message")
        .1
        .id;
    let two = session(c, Some(DM), &[]);
    turn(c, &two, "And what else?").await;
    assert!(
        in_last_request(&r, "port 7714") >= 1,
        "the other session's request held the fact"
    );
    let reach = crate::reach::reach(&c.store, &x, None).unwrap().unwrap();
    assert!(reach.descendants.is_empty(), "no copy: {reach:?}");
    assert!(
        (reach.direct.compilations.iter()).any(|k| k.session_id == two),
        "{reach:?}"
    );
    assert_eq!(reach.totals.sessions, 2, "{reach:?}");
    let out = retract(c, &x).unwrap();
    assert!(
        out.exposures.iter().any(|e| e.session_id == two),
        "{out:#?}"
    );
}

/// A copy's consumer with no further edge is in the spread (issue 44): a fact's copy said in one session
/// is laid by another session's window at the same place, and the retraction finds that session too.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_copys_consumer_with_no_further_edge_is_in_the_spread() {
    let r = rig();
    let c = &r.core;
    bind(c);
    let (_, x) = plant(c);
    let two = session(c, Some(DM), &[]);
    turn(c, &two, "The relay listens on port 7714, the note says.").await;
    let y = (c.store.session_nodes(&two).unwrap().into_iter())
        .find(|(_, n)| matches!(n.body, Body::UserMessage { .. }))
        .expect("the copy")
        .1
        .id;
    let e = Edge::new(EdgeKind::DerivedFrom, &y, &x, crate::graph::VIA_REPORT);
    c.store.append(&[e.record().unwrap()]).unwrap();
    let three = session(c, Some(DM), &[]);
    turn(c, &three, "And what else?").await;
    let reach = crate::reach::reach(&c.store, &x, None).unwrap().unwrap();
    let d = (reach.descendants.iter())
        .find(|d| d.node_id == y)
        .expect("the copy is a descendant");
    assert!(
        (d.exposure.compilations.iter()).any(|k| k.session_id == three),
        "{reach:?}"
    );
    let out = retract(c, &x).unwrap();
    assert!(
        out.exposures.iter().any(|e| e.session_id == three),
        "{out:#?}"
    );
}

/// A copy that was seen and not used does not hide a later, independent exposure of its session (issue
/// 44): a session holds a recall copy of the fact that it did not use (seen), and a later turn of it lays
/// the fact's exchange in its window, so the retraction gives it a correction note, once; a second
/// retraction writes nothing.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_seen_copy_does_not_hide_a_later_exposure_of_its_session() {
    let r = rig();
    let c = &r.core;
    bind(c);
    let src = session(c, Some(DM), &[]);
    turn(c, &src, FACT).await;
    let x = (c.store.session_nodes(&src).unwrap().into_iter())
        .find(|(_, n)| matches!(&n.body, Body::UserMessage { text, .. } if text == FACT))
        .expect("the fact")
        .1
        .id;
    // The seen copy: a recall's edge into the fact, and its use recorded as not used.
    let s = session(c, Some(DM), &["an earlier note"]);
    let y = c.store.session_nodes(&s).unwrap()[0].1.id.clone();
    let e = Edge::new(EdgeKind::DerivedFrom, &y, &x, crate::graph::VIA_RECALL);
    c.store.append(&[e.record().unwrap()]).unwrap();
    used(c, &s, "rcl_seen", &x, false);
    // A later turn of the same session lays the fact's exchange in its window.
    turn(c, &s, "And what else?").await;
    assert!(
        in_last_request(&r, "port 7714") >= 1,
        "the later turn sent the fact"
    );
    let out = retract(c, &x).unwrap();
    let mine: Vec<&str> = (out.exposures.iter())
        .filter(|e| e.session_id == s)
        .map(|e| e.kind.as_str())
        .collect();
    assert_eq!(mine, [kind::SESSION], "{out:#?}");
    turn(c, &s, "Anything else?").await;
    assert_eq!(in_last_request(&r, "Correction:"), 1);
    let again = retract(c, &x).unwrap();
    assert!(again.already, "{again:#?}");
}

/// The live session's next turn carries the correction once; a session that
/// only saw the fact, and a task, get none.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_live_sessions_next_turn_carries_the_note_once() {
    let r = rig();
    let c = &r.core;
    let (_, x) = plant(c);
    let a = asker(c, "What port does the Kestrel relay use?").await;
    let s = asker(c, "Which port is the Kestrel relay on, again?").await;
    let (_, a_rcl) = recall_of(c, &a);
    let (_, s_rcl) = recall_of(c, &s);
    used(c, &a, &a_rcl, &x, true);
    used(c, &s, &s_rcl, &x, false);
    retract(c, &x).unwrap();
    // Nothing is written into a session until its turn.
    let before = c.store.session_nodes(&a).unwrap().len();

    turn(c, &a, "And its log path?").await;
    assert_eq!(in_last_request(&r, "Correction:"), 1);
    assert_eq!(in_last_request(&r, WHY), 1);
    assert!(c.store.session_nodes(&a).unwrap().len() > before);
    turn(c, &a, "Thanks.").await;
    assert_eq!(in_last_request(&r, "Correction:"), 1);
    let noted: Vec<_> = rows(c, LedgerKind::FactCorrected)
        .into_iter()
        .filter(|r| r.session_id.as_deref() == Some(a.as_str()))
        .collect();
    assert_eq!(noted.len(), 1, "{noted:?}");
    assert_eq!(noted[0].data["node_id"], x);

    turn(c, &s, "And its log path?").await;
    // Anything goes (2026-11-22): nothing is withheld by place, so s's request lays a's exchange, the
    // correction note in it, once. The note is still a's alone: none is written for s.
    assert_eq!(in_last_request(&r, "Correction:"), 1);
    let s_noted = rows(c, LedgerKind::FactCorrected)
        .into_iter()
        .filter(|r| r.session_id.as_deref() == Some(s.as_str()))
        .count();
    assert_eq!(
        s_noted, 0,
        "a session that only saw the fact gets no correction note"
    );
}

/// A second retraction writes nothing and answers the first one's table.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_second_retraction_writes_nothing_and_returns_the_same_table() {
    let r = rig();
    let c = &r.core;
    bind(c);
    let (_, x) = plant(c);
    let a = asker(c, "What port does the Kestrel relay use?").await;
    let (_, rcl) = recall_of(c, &a);
    used(c, &a, &rcl, &x, true);
    task_that_recalled(c, "rotate the relay's certificates", &x, true);
    let first = retract(c, &x).unwrap();
    let at = c.store.last_position();
    let (cards, retracted) = (
        posts(c, "retraction").len(),
        rows(c, LedgerKind::FactRetracted).len(),
    );

    // By an id's ending, and by a handle, it is the same node.
    let ending = &x[x.len() - 8..];
    let again = retract(c, ending).unwrap();
    let handled = retract(c, &format!("[m:{}]", &x[x.len() - 6..])).unwrap();
    assert!(again.already && handled.already);
    assert_eq!(again.exposures, first.exposures);
    assert_eq!(handled.exposures, first.exposures);
    assert_eq!((again.at_ms, &again.by), (first.at_ms, &first.by));
    assert_eq!(c.store.last_position(), at, "a second retraction wrote");
    assert_eq!(posts(c, "retraction").len(), cards);
    assert_eq!(rows(c, LedgerKind::FactRetracted).len(), retracted);
}

/// A task or two that used it share one card, never one each.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn two_tasks_share_one_card() {
    let r = rig();
    let c = &r.core;
    bind(c);
    let (_, x) = plant(c);
    task_that_recalled(c, "rotate the relay's certificates", &x, true);
    task_that_recalled(c, "audit the relay's ports", &x, true);
    task_that_recalled(c, "unrelated sweep", &x, false);
    let out = retract(c, &x).unwrap();
    assert_eq!(
        kinds_of(&out),
        [kind::TASK, kind::TASK, kind::SEEN],
        "{out:#?}"
    );
    let cards = posts(c, "retraction");
    assert_eq!(cards.len(), 1, "{cards:?}");
    let text = cards[0]["text"].as_str().unwrap();
    assert!(text.contains("rotate the relay's certificates"), "{text}");
    assert!(text.contains("audit the relay's ports"), "{text}");
    assert!(!text.contains("unrelated sweep"), "{text}");
}

/// With no DM bound there is no card, and with no task there is none to send.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn no_task_or_no_dm_means_no_card() {
    let r = rig();
    let c = &r.core;
    let (_, x) = plant(c);
    task_that_recalled(c, "rotate the relay's certificates", &x, true);
    let out = retract(c, &x).unwrap();
    assert!(!out.card);
    assert!(posts(c, "retraction").is_empty());
    assert_eq!(kinds_of(&out), [kind::TASK]);

    bind(c);
    let (_, y) = plant(c);
    let out = retract(c, &y).unwrap();
    assert!(out.exposures.is_empty(), "{out:#?}");
    assert!(posts(c, "retraction").is_empty());
}

/// A copy of a copy is found: the walk goes past the first generation.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_walk_goes_past_the_first_generation() {
    let r = rig();
    let c = &r.core;
    let (_, x) = plant(c);
    // The owner published the fact into a place: a copy, one generation out.
    let pier = session(c, Some(&format!("channel:{PIER}")), &[]);
    let copy = Node::user(&pier, None, "owner", FACT);
    let edge = Edge::new(
        EdgeKind::DerivedFrom,
        &copy.id,
        &x,
        crate::graph::VIA_PUBLISH,
    );
    c.store
        .append(&[copy.record().unwrap(), edge.record().unwrap()])
        .unwrap();
    // A task recalled that copy: two generations out.
    let t = task_that_recalled(c, "audit the relay's ports", &copy.id, true);
    let out = retract(c, &x).unwrap();
    let task = out.exposures.iter().find(|e| e.kind == kind::TASK).unwrap();
    assert_eq!(task.session_id, t, "{out:#?}");
    assert_eq!(task.generation, 2);
    let published = out
        .exposures
        .iter()
        .find(|e| e.kind == kind::PUBLISHED)
        .unwrap();
    assert_eq!(published.node_id, copy.id);
    // The published copy is the node's text: recall leaves it out too.
    assert!(c
        .runner
        .memory
        .labeled(&c.store)
        .unwrap()
        .contains(&copy.id));
}

/// An authorised retraction naming a shared session returns the same complete
/// audit, including private exposures, as one naming a private session.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_shared_places_run_keeps_the_complete_retraction_audit() {
    let r = rig();
    let c = &r.core;
    bind(c);
    let (_, x) = plant(c);
    let a = asker(c, "What port does the zephyrine relay use?").await;
    let (_, rcl) = recall_of(c, &a);
    used(c, &a, &rcl, &x, true);
    let pier = session(c, Some(&format!("channel:{PIER}")), &[]);
    let copy = Node::user(&pier, None, "owner", FACT);
    let edge = Edge::new(
        EdgeKind::DerivedFrom,
        &copy.id,
        &x,
        crate::graph::VIA_PUBLISH,
    );
    c.store
        .append(&[copy.record().unwrap(), edge.record().unwrap()])
        .unwrap();

    // The private run sees it all, and writes the retraction.
    let p = FactRetractParams {
        node: x.clone(),
        why: WHY.into(),
        session_id: Some(session(c, None, &[])),
    };
    let private = c.fact_retract(&p, "cli").unwrap();
    let all = serde_json::to_string(&private).unwrap();
    assert!(all.contains("zephyrine"), "{all}");
    assert!(all.contains("Kestrel"), "{all}");

    // An authorised shared-place request receives the same complete audit.
    let p = FactRetractParams {
        session_id: Some(pier),
        ..p
    };
    let shared = c.fact_retract(&p, "cli").unwrap();
    assert!(shared.already);
    let text = serde_json::to_string(&shared).unwrap();
    assert!(text.contains("zephyrine"), "{text}");
    assert!(text.contains("Kestrel"), "{text}");
    assert_eq!(shared.gist, private.gist);
    let session_row = shared
        .exposures
        .iter()
        .find(|e| e.kind == kind::SESSION)
        .unwrap();
    assert!(!session_row.hidden);
    assert_eq!(
        session_row.place,
        private
            .exposures
            .iter()
            .find(|e| e.kind == kind::SESSION)
            .unwrap()
            .place
    );
    let published = shared
        .exposures
        .iter()
        .find(|e| e.kind == kind::PUBLISHED)
        .unwrap();
    assert!(!published.hidden);
    assert_eq!(published.place, "#pier");
}

/// Only the owner retracts: a connection that is no private place's writes
/// nothing, and the refusal is the label's kind.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_refused_retraction_writes_nothing() {
    let r = rig();
    let c = &r.core;
    let (_, x) = plant(c);
    let at = c.store.last_position();
    let who = Answerer {
        label: "mcp#1".into(),
        surface: Surface::Mcp,
        discord: None,
        agent: None,
    };
    let p = FactRetractParams {
        node: x.clone(),
        why: WHY.into(),
        session_id: None,
    };
    let e = c.fact_retract(&p, who).unwrap_err();
    assert!(
        e.downcast_ref::<crate::approval::Refusal>().is_some(),
        "{e:#}"
    );
    assert!(rows(c, LedgerKind::FactRetracted).is_empty());
    assert!(!c.runner.memory.labeled(&c.store).unwrap().contains(&x));
    // Its refusal is the only record.
    let refused = rows(c, LedgerKind::ApprovalRefused);
    assert_eq!(refused.len(), 1);
    assert_eq!(
        refused[0].data["act"],
        theseus_protocol::method::FACT_RETRACT
    );
    assert!(c.store.last_position() > at);
}

/// A reason is required, and an unknown node is refused.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_retraction_needs_a_reason_and_a_node() {
    let r = rig();
    let c = &r.core;
    let (_, x) = plant(c);
    let e = c
        .fact_retract(
            &FactRetractParams {
                node: x,
                why: "  ".into(),
                session_id: None,
            },
            "cli",
        )
        .unwrap_err();
    assert!(format!("{e:#}").contains("says why"), "{e:#}");
    let e = retract(c, "msg_no_such_node_at_all").unwrap_err();
    assert!(format!("{e:#}").contains("no node"), "{e:#}");
}

/// A reason over 500 characters (characters, not bytes) is refused when it is given, saying the limit and
/// the length given, and nothing is written; one of 500 is taken whole.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_reason_over_500_characters_is_refused_when_given() {
    let r = rig();
    let c = &r.core;
    let (_, x) = plant(c);
    let ask = |why: String| FactRetractParams {
        node: x.clone(),
        why,
        session_id: None,
    };
    let e = c
        .fact_retract(&ask("\u{e9}".repeat(501)), "cli")
        .unwrap_err();
    let said = format!("{e:#}");
    assert!(said.contains("500 characters"), "{said}");
    assert!(said.contains("501"), "{said}");
    assert!(rows(c, LedgerKind::FactRetracted).is_empty());
    assert!(!c.runner.memory.labeled(&c.store).unwrap().contains(&x));
    // 500 characters is 1,000 bytes here: taken, and kept whole.
    let most = "\u{e9}".repeat(500);
    let out = c.fact_retract(&ask(most.clone()), "cli").unwrap();
    assert_eq!(out.why, most);
    assert_eq!(rows(c, LedgerKind::FactRetracted).len(), 1);
}

/// The note is the harness's own user-role node, never an operator's words.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_note_is_the_harnesss_own_node() {
    let r = rig();
    let c = &r.core;
    let (_, x) = plant(c);
    let a = asker(c, "What port does the Kestrel relay use?").await;
    let (_, rcl) = recall_of(c, &a);
    used(c, &a, &rcl, &x, true);
    retract(c, &x).unwrap();
    turn(c, &a, "And its log path?").await;
    let note = c
        .store
        .session_nodes(&a)
        .unwrap()
        .into_iter()
        .map(|(_, n)| n)
        .find(|n| n.author.as_deref() == Some(crate::retract::NOTE_AUTHOR))
        .expect("the note's node");
    assert_eq!(note.origin, Origin::Harness);
    let Body::UserMessage { text, .. } = &note.body else {
        panic!("{note:?}");
    };
    assert!(
        text.starts_with("Correction: \u{201c}The Kestrel relay"),
        "{text}"
    );
    assert!(text.contains(" was marked wrong on 20"), "{text}");
    assert!(text.ends_with(WHY), "{text}");
}

/// A note waits across a restart: it is the META record's, read again.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_waiting_note_is_read_back_from_the_store() {
    let r = rig();
    let c = &r.core;
    let (_, x) = plant(c);
    let a = asker(c, "What port does the Kestrel relay use?").await;
    let (_, rcl) = recall_of(c, &a);
    used(c, &a, &rcl, &x, true);
    retract(c, &x).unwrap();
    let fresh = crate::retract::Notes::default();
    assert!(fresh.waiting(&c.store, &a));
    assert!(!fresh.waiting(&c.store, "ses_nobody"));
    let delivered = fresh.deliver(&c.store, &a, |_| Ok(vec![])).unwrap();
    assert_eq!(delivered.unwrap().len(), 1);
    assert!(!fresh.waiting(&c.store, &a));
    assert!(fresh
        .deliver(&c.store, &a, |_| Ok(vec![]))
        .unwrap()
        .is_none());
}
