//! Compaction roots and the core overage (M6 step 30c, theseus-6fn.4),
//! through the whole core, with a scripted stand-in for the session's model
//! (`anthropic`, on a small window) and one for the summary profile's
//! (`glm`, on `zai`):
//! - where the ring would cut, the `glm` profile summarizes the dropped
//!   range into a `Summary` node, and the request is the summary, then the
//!   kept turns (`strategy: compaction`); the next request begins with its
//!   bytes, also after a restart;
//! - the summary's call is reserved before it runs and settled at its real
//!   cost;
//! - a second compaction folds the first summary in;
//! - the ring runs when the summary call fails, and when the profile is off;
//! - the newest exchange alone past the window fails the turn before any
//!   call (`context_overage`), and nothing retries it.

use std::path::Path;
use std::sync::Arc;

use serde_json::{json, Value};
use theseus_kernel::{ActionState, ExecState, Wake};
use theseus_protocol::{SessionKind, TurnSubmitResult, Usage};

use crate::bus::EventSink;
use crate::compiler::compaction;
use crate::node::Body;
use crate::provider::{FakeProvider, ProviderError, ProviderRequest, Scripted};
use crate::session::SessionRecord;
use crate::store::Store;
use crate::turn::compaction::OVERAGE_CLASS;
use crate::turn::{TurnError, TurnRequest};
use crate::{Config, Core};

/// The session's model's window in this suite, and its output cap.
const WINDOW: u64 = 40_000;
const OUTPUT: u32 = 2_000;

struct Rig {
    core: Arc<Core>,
    model: Arc<FakeProvider>,
    glm: Arc<FakeProvider>,
    dir: tempfile::TempDir,
}

fn config(root: &Path, state: &Path, summary: &str) -> Config {
    let mut cfg = Config::example();
    cfg.server.state_dir = state.to_string_lossy().into_owned();
    cfg.tools.projects_dir = Some(root.to_string_lossy().into_owned());
    cfg.tools.roots = vec![];
    cfg.memory.summary_profile = summary.into();
    cfg.catalog.insert(
        "claude-sonnet-5-5".into(),
        crate::catalog::CatalogRow {
            context_window: Some(WINDOW),
            max_output_tokens: Some(OUTPUT),
            ..Default::default()
        },
    );
    cfg
}

fn build(dir: &Path, summary: &str, model: Arc<FakeProvider>, glm: Arc<FakeProvider>) -> Arc<Core> {
    let root = dir.join("work");
    std::fs::create_dir_all(&root).unwrap();
    let cfg = config(&root.canonicalize().unwrap(), dir, summary);
    let store = Store::open(&dir.join("store")).unwrap();
    let mut parts = crate::rpc::Parts::for_tests(cfg, model, store);
    parts.providers.insert("zai".into(), glm);
    Core::build(parts).unwrap()
}

fn rig_with(summary: &str, model: Vec<Scripted>, glm: Vec<Scripted>) -> Rig {
    let dir = tempfile::tempdir().unwrap();
    let model = Arc::new(FakeProvider::scripted(model));
    let glm = Arc::new(FakeProvider::scripted(glm));
    let core = build(dir.path(), summary, model.clone(), glm.clone());
    Rig {
        core,
        model,
        glm,
        dir,
    }
}

fn rig(model: Vec<Scripted>, glm: Vec<Scripted>) -> Rig {
    rig_with("glm", model, glm)
}

fn session(core: &Core) -> String {
    let rec = SessionRecord::new(SessionKind::Conversation, None);
    core.store.put_session(&rec.session_id, &rec).unwrap();
    rec.session_id
}

async fn turn(core: &Arc<Core>, sid: &str, input: &str) -> anyhow::Result<TurnSubmitResult> {
    let rec: SessionRecord = core.store.get_session(sid).unwrap().unwrap();
    let (live, _) = core.live_profile();
    let target = core.runner.resolve_target(&live, None, None, None)?;
    let sink = EventSink::new(core.bus.clone(), sid, None);
    core.runner
        .run(TurnRequest {
            session: rec,
            input: Some(input.into()),
            target,
            sink,
            author: "test".into(),
            recompile: None,
            attachments: vec![],
            arrived: None,
            reply_to: None,
        })
        .await
}

/// The ledger rows of `kind`, oldest first.
fn rows(core: &Core, kind: &str) -> Vec<Value> {
    let rows: Vec<(u64, crate::ledger::LedgerRow)> = core.store.ledger_tail(2000).unwrap();
    rows.into_iter()
        .filter(|(_, r)| r.kind == kind)
        .map(|(_, r)| r.data)
        .collect()
}

/// A message of about `tokens` tokens of prose, its first word `tag`.
fn words(tag: &str, tokens: usize) -> String {
    let mut s = format!("{tag} ");
    while s.len() < tokens * 4 {
        s.push_str("the keeper logged the tide and the wind before the lamp was lit. ");
    }
    s
}

/// A summary's answer, billed at these tokens.
fn summary(text: &str, input: u64, output: u64) -> Scripted {
    Scripted::Billed {
        usage: Usage {
            input_tokens: input,
            output_tokens: output,
            ..Default::default()
        },
        then: Box::new(Scripted::text(text)),
    }
}

/// The text blocks of a request's first message.
fn first_text(q: &ProviderRequest) -> String {
    q.messages[0]["content"][0]["text"]
        .as_str()
        .unwrap_or_default()
        .to_string()
}

fn text_of(q: &ProviderRequest) -> String {
    serde_json::to_string(&q.messages).unwrap()
}

/// Drive turns of about `tokens` each until a compilation is not the
/// transcript; returns how many turns that took.
async fn until_recompiled(r: &Rig, sid: &str, tokens: usize) -> usize {
    for k in 0..12 {
        turn(&r.core, sid, &words(&format!("turn{k}"), tokens))
            .await
            .unwrap();
        let compiled = rows(&r.core, "context.compiled");
        if compiled
            .last()
            .is_some_and(|c| c["strategy"] != "transcript")
        {
            return k + 1;
        }
    }
    panic!("never rang");
}

/// Where the ring would cut, `glm` summarizes the dropped range: the
/// request is the summary first, then the kept turns, the dropped turns
/// gone; the `Summary` node carries the range's positions and count; the
/// row, the compilation, and the line say so. Its call was reserved before
/// it ran and settled at its real cost.
#[tokio::test]
async fn compaction_replaces_the_ring_on_overflow() {
    let r = rig(
        vec![],
        vec![summary(
            "The keeper logged six tides; the operator asked for each one.",
            9_000,
            120,
        )],
    );
    let sid = session(&r.core);
    let turns = until_recompiled(&r, &sid, 6_000).await;
    assert!(turns >= 3, "rang after {turns} turns");

    // The compilation and its row.
    let compiled = rows(&r.core, "context.compiled");
    let last = compiled.last().unwrap();
    assert_eq!(
        (&last["trigger"], &last["strategy"]),
        (&json!("overflow"), &json!("compaction")),
        "{last}"
    );
    let comps = r.core.store.session_compilations(&sid).unwrap();
    let c = comps.last().unwrap();
    assert_eq!(c.strategy, "compaction");
    assert!(c.manifest.strip_thinking);
    let budget = c.budget.as_ref().unwrap();
    assert!(budget.overage.is_none(), "{budget:?}");
    let range = budget
        .dropped
        .iter()
        .find(|d| d.tier == "compaction")
        .unwrap();
    assert_eq!(range.reason, "summarized");

    // The summary's node and its range: every message the ring dropped.
    let nodes = r.core.store.transcript(&sid).unwrap();
    let (spos, s) = nodes
        .iter()
        .find(|(_, n)| matches!(n.body, Body::Summary { .. }))
        .unwrap();
    assert_eq!(c.includes[0], s.id, "the summary heads the prefix");
    let Body::Summary {
        first,
        last: end,
        nodes: count,
        text,
        profile,
        model,
        cost_usd,
        header,
    } = &s.body
    else {
        unreachable!()
    };
    let kept: std::collections::HashSet<&str> = c.includes.iter().map(String::as_str).collect();
    let dropped: Vec<u64> = nodes
        .iter()
        .filter(|(p, n)| {
            *p < *spos && crate::compiler::renderable(n) && !kept.contains(n.id.as_str())
        })
        .map(|(p, _)| *p)
        .collect();
    assert_eq!(
        (*first, *end, *count as usize),
        (dropped[0], *dropped.last().unwrap(), dropped.len())
    );
    assert_eq!(range.range.as_ref().unwrap().nodes, dropped.len() as u64);
    assert_eq!((profile.as_str(), model.as_str()), ("glm", "glm-5.3-flash"));
    assert!(header.starts_with(&format!("[Summary of {count} earlier messages, ")));
    assert!(header.ends_with(", written by glm]"), "{header}");

    // The summary call: the dropped turns went to glm, never the kept ones.
    let asked = r.glm.requests();
    assert_eq!(asked.len(), 1);
    assert!(text_of(&asked[0]).contains("turn0 "));
    let newest = format!("turn{} ", turns - 1);
    assert!(!text_of(&asked[0]).contains(&newest));

    // The request: the summary first, the dropped turns gone, the newest kept.
    let sent = r.model.requests();
    let q = sent.last().unwrap();
    assert_eq!(first_text(q), compaction::rendered(header, text));
    assert!(!text_of(q).contains("turn0 "));
    assert!(text_of(q).contains(&newest));

    // The row, with the call's reservation and its settlement.
    let row = &rows(&r.core, "context.compacted")[0];
    assert_eq!(row["outcome"], "compaction");
    assert_eq!(row["node_id"], json!(s.id));
    assert_eq!(row["messages"], json!(count));
    assert_eq!(row["summary_tokens"], 120);
    settled_as_reserved(&r, &sid, row, &s.id, *cost_usd);
}

/// The summary's call was reserved before it ran and settled at its real
/// cost: the row, the kernel's action, the execution's budget, and the
/// session's books agree.
fn settled_as_reserved(r: &Rig, sid: &str, row: &Value, node: &str, cost_usd: Option<f64>) {
    let settled = row["settled_micros"].as_u64().unwrap();
    let reserved = row["reserved_micros"].as_u64().unwrap();
    // glm-5.3-flash: $0.15 in and $0.50 out a million.
    assert_eq!(settled, 9_000 * 15 / 100 + 120 * 50 / 100);
    assert!(reserved > settled, "{reserved} > {settled}");
    assert_eq!(cost_usd, Some(settled as f64 / 1e6));
    let actions = r.core.kernel.actions().unwrap();
    let call = actions
        .iter()
        .find(|a| a.correlation_id == row["correlation_id"].as_str().unwrap())
        .unwrap();
    assert_eq!(call.reserved_micros, reserved);
    assert_eq!(call.state, ActionState::Succeeded);
    assert_eq!(call.result_ref.as_deref(), Some(node));
    let rec: SessionRecord = r.core.store.get_session(sid).unwrap().unwrap();
    let exec = r
        .core
        .kernel
        .execution(rec.execution_id.as_deref().unwrap())
        .unwrap()
        .unwrap();
    assert_eq!(exec.budget.reserved_micros, 0, "nothing is left held");
    assert!(exec.budget.spent_micros >= settled);
    assert!(
        rec.cost_usd >= settled as f64 / 1e6,
        "the turn's books carry the summary's cost"
    );
}

/// The next turn after a compaction appends: its request begins with the
/// compacted request's bytes. So does the first turn after a restart, the
/// compaction rebuilt from its manifest with the summary first.
#[tokio::test]
async fn the_next_request_begins_with_the_compacted_requests_bytes_after_a_restart() {
    let r = rig(vec![], vec![summary("Six tides were logged.", 9_000, 40)]);
    let sid = session(&r.core);
    until_recompiled(&r, &sid, 6_000).await;
    let compacted = r.model.requests().last().unwrap().clone();
    assert!(first_text(&compacted).starts_with("[Summary of "));

    turn(&r.core, &sid, "and the wind?").await.unwrap();
    let next = r.model.requests().last().unwrap().clone();
    assert_eq!(
        &next.messages[..compacted.messages.len() - 1],
        &compacted.messages[..compacted.messages.len() - 1],
        "the next request begins with the compacted one's"
    );
    let compiled = rows(&r.core, "context.compiled");
    assert_eq!(compiled.last().unwrap()["decision"], "append");

    // A restart: a new core over the same store.
    let Rig { core, dir, .. } = r;
    drop(core);
    let model = Arc::new(FakeProvider::scripted(vec![]));
    let glm = Arc::new(FakeProvider::scripted(vec![]));
    let core = build(dir.path(), "glm", model.clone(), glm.clone());
    turn(&core, &sid, "and the lamp?").await.unwrap();
    let after = model.requests().pop().unwrap();
    assert_eq!(
        &after.messages[..next.messages.len() - 1],
        &next.messages[..next.messages.len() - 1],
        "rebuilt byte for byte from its manifest"
    );
    assert!(
        first_text(&after).starts_with("[Summary of "),
        "the summary heads it"
    );
    assert!(glm.requests().is_empty(), "no second summary");
}

/// A second compaction folds the first summary in: its range starts where
/// the first's did, it counts the first's messages, and its call reads the
/// first's text; the request carries the second alone.
#[tokio::test]
async fn a_second_compaction_folds_the_first_summary_in() {
    let r = rig(
        vec![],
        vec![
            summary("FIRST: the early tides.", 9_000, 30),
            summary("SECOND: every tide so far.", 9_000, 30),
        ],
    );
    let sid = session(&r.core);
    until_recompiled(&r, &sid, 6_000).await;
    for k in 0..12 {
        turn(&r.core, &sid, &words(&format!("later{k}"), 6_000))
            .await
            .unwrap();
        if r.glm.requests().len() == 2 {
            break;
        }
    }
    let asked = r.glm.requests();
    assert_eq!(asked.len(), 2, "a second compaction");
    assert!(text_of(&asked[1]).contains("FIRST: the early tides."));
    let summaries: Vec<(u64, u64, u32)> = r
        .core
        .store
        .transcript(&sid)
        .unwrap()
        .iter()
        .filter_map(|(_, n)| match &n.body {
            Body::Summary {
                first, last, nodes, ..
            } => Some((*first, *last, *nodes)),
            _ => None,
        })
        .collect();
    assert_eq!(summaries.len(), 2);
    assert_eq!(
        summaries[1].0, summaries[0].0,
        "the range starts where the first's did"
    );
    assert!(summaries[1].1 > summaries[0].1 && summaries[1].2 > summaries[0].2);
    let q = r.model.requests().pop().unwrap();
    assert!(first_text(&q).contains("SECOND: every tide so far."));
    assert!(!text_of(&q).contains("FIRST: the early tides."));
    let row = rows(&r.core, "context.compacted").pop().unwrap();
    assert!(row["folded"].is_string(), "{row}");
}

/// The summary call fails: the ring runs as before, the turn answers, and
/// the row says why. The failed call was settled, nothing left held.
#[tokio::test]
async fn the_ring_runs_when_the_summary_call_fails() {
    let r = rig(
        vec![],
        vec![Scripted::Fail(ProviderError::Server {
            status: 500,
            message: "overloaded".into(),
        })],
    );
    let sid = session(&r.core);
    until_recompiled(&r, &sid, 6_000).await;
    let compiled = rows(&r.core, "context.compiled");
    assert_eq!(compiled.last().unwrap()["strategy"], "ring");
    let row = &rows(&r.core, "context.compacted")[0];
    assert_eq!(row["outcome"], "ring");
    assert!(
        row["why"]
            .as_str()
            .unwrap()
            .contains("the summary call failed"),
        "{row}"
    );
    assert!(r
        .core
        .store
        .transcript(&sid)
        .unwrap()
        .iter()
        .all(|(_, n)| !matches!(n.body, Body::Summary { .. })));
    let q = r.model.requests().pop().unwrap();
    assert!(!text_of(&q).contains("turn0 "), "the ring dropped it");
    assert!(!first_text(&q).starts_with("[Summary"));
    let rec: SessionRecord = r.core.store.get_session(&sid).unwrap().unwrap();
    let exec = r
        .core
        .kernel
        .execution(rec.execution_id.as_deref().unwrap())
        .unwrap()
        .unwrap();
    assert_eq!(exec.budget.reserved_micros, 0);
}

/// `summary_profile = "off"` keeps the ring: no call, no row.
#[tokio::test]
async fn summary_profile_off_keeps_the_ring() {
    let r = rig_with("off", vec![], vec![]);
    let sid = session(&r.core);
    until_recompiled(&r, &sid, 6_000).await;
    assert_eq!(
        rows(&r.core, "context.compiled").last().unwrap()["strategy"],
        "ring"
    );
    assert!(r.glm.requests().is_empty());
    assert!(rows(&r.core, "context.compacted").is_empty());
}

/// The newest exchange alone does not fit, with every earlier turn the
/// ring could drop gone: the turn fails at once with
/// `context_overage`, names the window, the estimate, and the overage, and
/// sends nothing; the session waits on its next message, which no retry
/// repeats.
#[tokio::test]
async fn the_newest_exchange_past_the_window_fails_with_context_overage() {
    let r = rig(
        vec![
            Scripted::text("The lamp is lit."),
            Scripted::text("Short and sweet."),
        ],
        vec![],
    );
    let sid = session(&r.core);
    turn(&r.core, &sid, "is the lamp lit?").await.unwrap();
    let err = turn(&r.core, &sid, &words("huge", 50_000))
        .await
        .expect_err("past the window alone");
    let te = err.downcast_ref::<TurnError>().unwrap();
    assert_eq!(te.class, OVERAGE_CLASS);
    let message = format!("{:#}", te.source);
    for part in [
        "the newest exchange alone does not fit claude-sonnet-5-5's window of 40,000",
        "upper bound) against the 33,904 the window leaves",
        "Nothing was sent.",
    ] {
        assert!(message.contains(part), "{part:?} in {message}");
    }
    assert_eq!(r.model.requests().len(), 1, "no provider call for it");
    assert!(r.glm.requests().is_empty());
    let over = rows(&r.core, "context.overage");
    assert_eq!(over.len(), 1);
    assert_eq!(over[0]["window"], WINDOW);
    assert!(over[0]["over"].as_u64().unwrap() > 0);
    assert!(over[0]["budget"]["overage"].is_object(), "{}", over[0]);
    // The ring's last cut kept only it, and still it did not fit: no
    // compilation was kept for it, and no summary was tried.
    assert_eq!(r.core.store.session_compilations(&sid).unwrap().len(), 1);
    assert!(rows(&r.core, "context.compacted").is_empty());
    // It waits on the next message: no retry repeats it.
    let rec: SessionRecord = r.core.store.get_session(&sid).unwrap().unwrap();
    let e = r
        .core
        .kernel
        .execution(rec.execution_id.as_deref().unwrap())
        .unwrap()
        .unwrap();
    assert_eq!(
        (e.state, e.wake, e.resume_pending),
        (ExecState::Waiting, Some(Wake::Input), false)
    );
    let ok = turn(&r.core, &sid, "a small question").await.unwrap();
    assert_eq!(ok.output, "Short and sweet.");
}
