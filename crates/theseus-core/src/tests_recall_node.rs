//! Recall in front of the model through the whole core (M6 step 30b; §3.2's
//! tests): a live turn's `Recall` node, its render after the new message,
//! its edges and `node.reach`, the plan frame it rides, the next request
//! beginning with the previous one's bytes, the sticky arm and its row, the
//! session's cap, the operator's labels, and a stalled index. The index is
//! `tests_recall`'s stand-in, which answers every node of the sessions it
//! is given.

use std::sync::Arc;
use std::time::Duration;

use serde_json::Value;
use theseus_protocol::memory::{MemoryLabelParams, RecallManifest};
use theseus_store::kinds;

use crate::approval::Answerer;
use crate::config::memory::MemoryArm;
use crate::config::MemoryMode;
use crate::graph::Edge;
use crate::node::{Body, Node};
use crate::provider::ProviderRequest;
use crate::recall::AskFuture;
use crate::test_support::recall_rows as rows;
use crate::tests_recall::{
    admitted, dropped_for, index_of, recalls, rig, rig_with, session, turn, Rig, OWNER, PIER,
};
use crate::Core;

const HERON: &str = "Remember: the grey heron nests by the old weir at Millbrook.";

/// Canary with every session on the arm.
/// A live rig (the name is the retired canary mode's, which ran these the same way).
fn canary() -> Rig {
    canary_with(|_| {})
}

fn canary_with(tweak: impl FnOnce(&mut crate::Config)) -> Rig {
    rig_with(MemoryMode::Live, |c| {
        tweak(c);
    })
}

/// The session's nodes, in order.
fn nodes(core: &Core, sid: &str) -> Vec<(u64, Node)> {
    core.store.session_nodes(sid).unwrap()
}

/// Its `Recall` nodes.
fn recall_nodes(core: &Core, sid: &str) -> Vec<(u64, Node)> {
    nodes(core, sid)
        .into_iter()
        .filter(|(_, n)| matches!(n.body, Body::Recall { .. }))
        .collect()
}

fn requests(r: &Rig) -> Vec<ProviderRequest> {
    r.model.requests.lock().unwrap().clone()
}

/// The texts of a request's last message.
fn last_texts(q: &ProviderRequest) -> Vec<String> {
    q.messages
        .last()
        .unwrap()
        .get("content")
        .and_then(Value::as_array)
        .unwrap()
        .iter()
        .filter_map(|b| b.get("text").and_then(Value::as_str).map(str::to_string))
        .collect()
}

/// A live turn's recall reaches the model: a `Recall` node after the new
/// message, rendered in the same user turn as testimony; its row is
/// `recall.ran` with the node's recall id and the arm; it rides frames the
/// turn writes anyway; each source gets a `derived_from` edge scoped
/// `in:<source>`, and `node.reach` counts the copy; the reply says how many.
#[tokio::test]
async fn a_live_turn_puts_its_recall_in_front_of_the_model() {
    let mut frames = Vec::new();
    for mode in [MemoryMode::Off, MemoryMode::Live] {
        let r = rig_with(mode, |_| {});
        let c = &r.core;
        let a = session(c, None, &[HERON]);
        let b = session(c, None, &[]);
        // The warm-up turn finds nothing: the index learns A after it.
        c.runner.memory.set_ask(index_of(c, vec![]));
        turn(c, &b, "warm up").await;
        c.runner.memory.set_ask(index_of(c, vec![a.clone()]));
        let before = c.store.stats().unwrap().frames_appended;
        let res = turn(c, &b, "Where does the grey heron nest?").await;
        frames.push(c.store.stats().unwrap().frames_appended - before);
        if mode == MemoryMode::Off {
            assert_eq!(res.recalled, 0);
            continue;
        }
        assert_eq!(res.recalled, 1, "the reply says recall fed it");
        let source = &nodes(c, &a)[0].1;
        // The node: after the turn's input, references and never copies.
        let all = nodes(c, &b);
        let at = all
            .iter()
            .position(|(_, n)| matches!(n.body, Body::Recall { .. }))
            .expect("a Recall node");
        assert!(
            matches!(&all[at - 1].1.body, Body::UserMessage { text, .. } if text.starts_with("Where"))
        );
        let Body::Recall {
            recall_id,
            arm,
            items,
        } = &all[at].1.body
        else {
            unreachable!()
        };
        assert_eq!(arm, "baseline");
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].node_id, source.id);
        assert_eq!(items[0].chunk, (0, HERON.len() as u32));
        let stored = serde_json::to_string(&all[at].1).unwrap();
        assert!(!stored.contains("old weir"), "a copy: {stored}");
        // The request (the view, issue 44): the block beside the message, holding the note, then the
        // question, in one user turn.
        let q = requests(&r).pop().unwrap();
        let texts = last_texts(&q);
        let (beside, asked) = (&texts[0], texts.last().unwrap());
        assert_eq!(
            crate::surface::strip_tag(asked),
            "Where does the grey heron nest?"
        );
        const NOTE: &str = "[Recalled by the harness: 1 note from earlier sessions,";
        assert!(beside.contains(NOTE), "{beside}");
        let note = beside.split_once(NOTE).map_or("", |(_, n)| n);
        assert!(note.contains(&format!("    \"{HERON}\"")), "{note}");
        // The row.
        let ran = rows(c, &b, "recall.ran");
        assert_eq!(ran.len(), 2, "both turns recalled in front of the model");
        assert!(ran[0].data["admitted"].as_array().unwrap().is_empty());
        let m: RecallManifest = serde_json::from_value(ran[1].data.clone()).unwrap();
        assert_eq!(
            (&m.recall_id, m.mode.as_str(), m.arm.as_deref()),
            (recall_id, "live", Some("baseline"))
        );
        assert!(m.admitted[0].text.is_none(), "the row keeps references");
        let budget = m.budget.unwrap();
        assert_eq!(
            (budget.limit_tokens, budget.used_tokens),
            (3000, m.used_tokens)
        );
        // The edge, scoped into the source.
        let edges: Vec<Edge> = c
            .store
            .scope_after(&Edge::scope_into(&source.id), 0)
            .unwrap()
            .iter()
            .filter(|r| r.kind == kinds::EDGE)
            .map(|r| r.decode().unwrap())
            .collect();
        assert_eq!(edges.len(), 1);
        assert_eq!(
            (
                edges[0].kind.as_str(),
                edges[0].from.as_str(),
                edges[0].via.as_str()
            ),
            ("derived_from", all[at].1.id.as_str(), "recall")
        );
        let reach = crate::reach::reach(&c.store, &source.id, None)
            .unwrap()
            .unwrap();
        assert_eq!(reach.descendants.len(), 1);
        assert_eq!(reach.descendants[0].node_id, all[at].1.id);
        assert_eq!(reach.descendants[0].route, "recall");
        assert!(
            reach.descendants[0].exposure.loops >= 1,
            "{:?}",
            reach.descendants[0]
        );
        // The node, the edge, and the row rode the plan frame: the record
        // before the node in the WAL is the provider call's action.
        let plan = theseus_store::Store::get(&**c.store.inner(), all[at].0 - 1).unwrap();
        assert!(plan.is_some());
    }
    assert_eq!(frames[0], frames[1], "the recall wrote a frame of its own");
}

/// A session's arm is sticky and recorded once, as `memory.arm`; a session
/// on arm `none` runs `none` live: the index is asked nothing, and it gets no
/// node.
#[tokio::test]
async fn the_arm_is_sticky_and_recorded_once() {
    let r = rig_with(MemoryMode::Live, |c| c.memory.arm = MemoryArm::None);
    let c = &r.core;
    let a = session(c, None, &[HERON]);
    let b = session(c, None, &[]);
    c.runner.memory.set_ask(index_of(c, vec![a.clone()]));
    for q in ["Where does the grey heron nest?", "And when?"] {
        let res = turn(c, &b, q).await;
        assert_eq!(res.recalled, 0);
    }
    let arms = rows(c, &b, "memory.arm");
    assert_eq!(arms.len(), 1, "one arm row a session");
    assert_eq!(arms[0].data["arm"], "none");
    assert_eq!(arms[0].data["live"], false);
    assert_eq!(arms[0].data["experiment"], "m6-1");
    let shadow = rows(c, &b, "recall.shadow");
    assert_eq!(shadow.len(), 0, "arm none recalls nothing");
    assert!(recall_nodes(c, &b).is_empty());
    assert_eq!(recalls(c, &b).len(), 0);
    // The assignment is the config's, for this session, every time.
    let a1 = c.runner.memory.cfg().assign(&b).unwrap();
    assert_eq!(a1.arm, MemoryArm::None);
    // A daemon that has not seen the row finds it in the store.
    let fresh = crate::recall::Memory::new(c.runner.memory.cfg().clone(), None);
    assert!(fresh.arm_recorded(&c.store, &b).unwrap());
    assert!(!fresh.arm_recorded(&c.store, &a).unwrap());
}

/// `wrong` keeps a node out of recall as `labeled_wrong`, from the next turn
/// on and in every session; `useful` lets it back; the labels are rebuilt
/// from their rows; and a label from a shared place does not count.
#[tokio::test]
async fn a_wrong_label_keeps_a_node_out() {
    let r = canary();
    let c = &r.core;
    let a = session(c, None, &[HERON]);
    c.runner.memory.set_ask(index_of(c, vec![a.clone()]));
    let heron = nodes(c, &a)[0].1.id.clone();
    let label = |l: &str| MemoryLabelParams {
        node_id: heron.clone(),
        label: l.into(),
        recall_id: None,
        note: Some("the heron moved".into()),
    };
    let out = c.memory_label(&label("wrong"), "cli").unwrap();
    assert!(out.excluded);
    let s = session(c, None, &[]);
    let res = turn(c, &s, "Where does the grey heron nest?").await;
    assert_eq!(res.recalled, 0);
    assert!(recall_nodes(c, &s).is_empty());
    let m = &recalls(c, &s)[0];
    assert_eq!(dropped_for(m, "labeled_wrong"), [a.as_str()]);
    // Rebuilt from the rows, as a restart reads them.
    let fresh = crate::recall::Memory::new(c.runner.memory.cfg().clone(), None);
    assert!(fresh.labeled(&c.store).unwrap().contains(&heron));
    // A shared place's word does not count, and writes no label.
    let stranger = Answerer {
        label: "discord".into(),
        surface: crate::approval::Surface::Discord,
        discord: Some(theseus_protocol::DiscordOrigin {
            user_id: "42".into(),
            channel_id: format!("{PIER}"),
            guild_id: Some("7".into()),
        }),
        agent: None,
    };
    assert!(c.memory_label(&label("useful"), stranger).is_err());
    assert!(c.runner.memory.labeled(&c.store).unwrap().contains(&heron));
    // Nor is a word that is not a label, or a node that is not one.
    assert!(c.memory_label(&label("meh"), "cli").is_err());
    let mut ghost = label("wrong");
    ghost.node_id = "msg_nobody".into();
    assert!(c.memory_label(&ghost, "cli").is_err());
    // `useful` lets it back.
    assert!(!c.memory_label(&label("useful"), "cli").unwrap().excluded);
    let t = session(c, None, &[]);
    assert_eq!(
        turn(c, &t, "Where does the grey heron nest?")
            .await
            .recalled,
        1
    );
    let _ = OWNER;
}

/// A stalled index holds a live turn (the canary mode it was written for is
/// retired) no longer than its deadline: the
/// turn goes on without recall, its row says `deadline`, and no node is
/// written. On the paused clock: the bound on the turn's time is its own
/// virtual time.
#[tokio::test(start_paused = true)]
async fn a_stalled_index_holds_a_canary_turn_no_longer_than_its_deadline() {
    let r = canary_with(|c| c.memory.recall_deadline_ms = 50);
    let c = &r.core;
    let b = session(c, None, &[]);
    c.runner.memory.set_ask(Arc::new(|_| -> AskFuture {
        Box::pin(std::future::pending())
    }));
    let t0 = tokio::time::Instant::now();
    let res = turn(c, &b, "is anyone there?").await;
    assert!(t0.elapsed() < Duration::from_secs(2), "{:?}", t0.elapsed());
    assert_eq!((res.loops, res.recalled), (1, 0));
    let m = &recalls(c, &b)[0];
    assert_eq!((m.mode.as_str(), m.outcome.as_str()), ("live", "deadline"));
    assert!(recall_nodes(c, &b).is_empty());
}

/// Past the session's cap, recall pauses until the next recompile: the row
/// says `paused` and why, and no node is written.
#[tokio::test]
async fn past_the_sessions_cap_recall_pauses() {
    let r = canary_with(|c| {
        c.memory.recall_budget_tokens = 30;
        c.memory.recall_max_items = 1;
        c.memory.session_recall_cap_tokens = 30;
    });
    let c = &r.core;
    // Each note about 25 tokens: one fits the cap, two do not.
    let a = session(
        c,
        None,
        &[
            "the heron nests by the weir in spring, and the reeds there are cut back each autumn",
            "the heron fishes at dawn by the mill, where the race runs shallow over the stones",
        ],
    );
    let b = session(c, None, &[]);
    c.runner.memory.set_ask(index_of(c, vec![a]));
    assert_eq!(turn(c, &b, "the heron?").await.recalled, 1);
    assert_eq!(turn(c, &b, "and the heron again?").await.recalled, 0);
    let m = &recalls(c, &b)[1];
    assert_eq!(m.outcome, "paused");
    assert!(m
        .why
        .as_deref()
        .unwrap()
        .contains("session_recall_cap_tokens = 30"));
    assert_eq!(recall_nodes(c, &b).len(), 1);
}

/// A guard for the session's recall cap on the view (issue 44): a long conversation, each of whose turns
/// recalls past what the cap would hold summed over turns, still recalls on its next turn. Today a view
/// turn writes no `Recall` node into its session, so the tail the cap sums holds none; if a later change
/// persists the view's note, this test says whether the cap then pauses a long conversation.
#[tokio::test]
async fn on_the_view_a_long_conversation_still_recalls_past_the_sessions_cap() {
    use crate::config::assembly::{Assembly, AssemblySetting};
    let r = canary_with(|c| {
        c.context.assembly = Some(AssemblySetting::All(Assembly::View));
        c.memory.recall_budget_tokens = 30;
        c.memory.recall_max_items = 1;
        c.memory.session_recall_cap_tokens = 30;
    });
    let c = &r.core;
    // Each note about 25 tokens: one fits the cap, two do not.
    let a = session(
        c,
        None,
        &[
            "the heron nests by the weir in spring, and the reeds there are cut back each autumn",
            "the heron fishes at dawn by the mill, where the race runs shallow over the stones",
        ],
    );
    let b = session(c, None, &[]);
    c.runner.memory.set_ask(index_of(c, vec![a]));
    // The view lays recall beside the message, not as the turn's note: each turn's recall row says
    // whether it recalled.
    for ask in ["the heron?", "and the heron again?", "the heron at dawn?"] {
        turn(c, &b, ask).await;
    }
    turn(c, &b, "where does the heron nest?").await;
    let outcomes: Vec<String> = recalls(c, &b).into_iter().map(|m| m.outcome).collect();
    assert_eq!(outcomes, ["ran"; 4], "every turn recalled, none paused");
}

/// Shadow is unchanged by 30b: its rows, no node, no byte of the request.
#[tokio::test]
async fn shadow_still_writes_no_node() {
    let r = rig(MemoryMode::Shadow);
    let c = &r.core;
    let a = session(c, None, &[HERON]);
    let b = session(c, None, &[]);
    c.runner.memory.set_ask(index_of(c, vec![a]));
    let res = turn(c, &b, "Where does the grey heron nest?").await;
    assert_eq!(res.recalled, 0);
    assert!(recall_nodes(c, &b).is_empty());
    assert!(
        rows(c, &b, "memory.arm").is_empty(),
        "shadow assigns no arm"
    );
    assert_eq!(recalls(c, &b)[0].admitted.len(), 1);
    assert!(!last_texts(&requests(&r).pop().unwrap())
        .iter()
        .any(|t| t.contains("Recalled")));
}

/// A turn whose request carries its recall writes one `Recall` node and counts
/// it (theseus-fpm2): the same stand-in index, one note in the request, one
/// node written, `recalled: 1`. (Its other half, a request bound for a model
/// of restricted origin that carried none, went with that rule on 2026-11-21;
/// the small-talk bar's withholding is `tests_tier_manifest`'s.)
#[tokio::test]
async fn a_turn_whose_request_carried_its_recall_writes_one_recall_node() {
    let r = canary_with(|_| {});
    let c = &r.core;
    let a = session(c, None, &[HERON]);
    let b = session(c, None, &[]);
    // The warm-up turn finds nothing: the index learns A after it.
    c.runner.memory.set_ask(index_of(c, vec![]));
    turn(c, &b, "warm up").await;
    c.runner.memory.set_ask(index_of(c, vec![a.clone()]));
    let res = turn(c, &b, "Where does the grey heron nest?").await;
    let m = recalls(c, &b).pop().expect("a recall.ran row");
    assert_eq!(admitted(&m).len(), 1, "recall admitted the note");
    let q = requests(&r).pop().expect("a request");
    let carried = last_texts(&q)
        .iter()
        .any(|t| t.contains("[Recalled by the harness:"));
    assert!(carried, "the request carries the note");
    assert_eq!(recall_nodes(c, &b).len(), 1, "one `Recall` node");
    assert_eq!(res.recalled, 1, "and the reply counts it");
    // A request that carried it withheld nothing (theseus-ectzg).
    assert!(
        rows(c, &b, "recall.withheld").is_empty(),
        "a request that carried it withheld nothing"
    );
}

/// A compile whose request carries the recall node folds the pack's budget
/// drop into its report (theseus-ectzg, fpm2 F1 limit (i)): recall's pack has
/// room for one of two notes, drops the other for its budget, and the
/// compile's report names it.
#[tokio::test]
async fn a_carried_nodes_compile_reports_the_packs_budget_drop() {
    let r = canary_with(|c| {
        c.memory.recall_budget_tokens = 30;
    });
    let c = &r.core;
    let a = session(
        c,
        None,
        &[
            "the heron nests by the weir in spring, and the reeds there are cut back each autumn",
            "the heron fishes at dawn by the mill, where the race runs shallow over the stones",
        ],
    );
    let b = session(c, None, &[]);
    c.runner.memory.set_ask(index_of(c, vec![a]));
    turn(c, &b, "Where does the heron nest and fish?").await;
    let m = recalls(c, &b).pop().expect("a recall.ran row");
    assert_eq!(admitted(&m).len(), 1, "the pack admitted one note");
    let packs = m.budget.as_ref().map_or(0, |b| b.dropped.len());
    assert_eq!(packs, 1, "and dropped the other for its budget");
    let all: Vec<(u64, crate::ledger::LedgerRow)> = c.store.ledger_tail(500).unwrap();
    let reported: Vec<String> = all
        .into_iter()
        .filter(|(_, row)| row.kind == "context.compiled")
        .filter_map(|(_, row)| {
            let id = row.data["compilation_id"].as_str()?.to_string();
            c.store.get_compilation(&id).ok().flatten()
        })
        .flat_map(|k| k.budget.map(|b| b.dropped).unwrap_or_default())
        .map(|d| d.tier)
        .collect();
    assert_eq!(reported, ["recall"], "the carried node's drop is reported");
}
