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
//! - `session`, the default, summarizes on the session's own model: glm is
//!   never asked; a turn submitted on glm's model for one message summarizes
//!   on glm, and the next turn is the session's own again;
//! - the summary's call is reserved on the estimate's upper bound, so a
//!   provider that counts more than the estimate settles within it;
//! - the newest exchange alone past the window fails the turn before any
//!   call (`context_overage`), and nothing retries it.
//! - the assembled strategy (recall live, a stand-in index): a task's first
//!   compile, and a compaction, carry a recall section first in the prefix,
//!   at `assembled_budget_tokens`, recorded as the compilation's `recall_id`.

use std::path::Path;
use std::sync::Arc;

use serde_json::{json, Value};
use theseus_kernel::{ActionState, ExecState, Wake};
use theseus_protocol::{SessionKind, TurnSubmitResult, Usage};

use crate::bus::EventSink;
use crate::compiler::compaction;
use crate::config::memory::SUMMARY_SESSION;
use crate::node::Body;
use crate::provider::{FakeProvider, ProviderError, ProviderRequest, Scripted};
use crate::session::SessionRecord;
use crate::store::Store;
pub(crate) use crate::test_support::rows_2k as rows;
use crate::turn::compaction::OVERAGE_CLASS;
use crate::turn::{TurnError, TurnRequest};
use crate::{Config, Core};

/// The session's model's window in this suite, and its output cap. The
/// request carries every tool's definition (32 KB of 59 at 40,000), so the
/// window grows with the catalog: 40,000 left the kept turns 225 to 264
/// tokens past the summary's room once theseus-ggqf's terminal words joined
/// (the join's review). 48,000 leaves room; at 44,000 the override test's
/// probe and its glm turn ring at different turns.
/// And 48,000 until `task_fan_out` joined the catalog (theseus-z9mze.3): its definition grows the tools
/// block by about 500 tokens. 48,250 and up hold (to 52,000, the sweep's end) for this suite, and 48,500 and up for the
/// others in this family of tests; 49,000 is the middle of the near end of that band.
/// And 49,000 until `routine.create` took a start, a stop and a carry
/// (theseus-97b8c), which grows the tools block again: a sweep at its join over
/// the family (compaction, commitments, tiering, standing_ring, private_view,
/// situation, home_size, route_keep, projects) rang one more test at 49,000 and
/// none from 49,500 to 52,000, the sweep's end. 50,000 is inside that band.
const WINDOW: u64 = 50_000;
/// The window of the session-summary rig (`session_rig`): its override test's
/// probe and its glm turn must ring at the same turn, which holds in a band
/// that moves with the tools block. 48,000 was in it until `routine.create`
/// and `watch` joined the request (theseus-pvm5c, theseus-f2r2b); with them
/// and `thread.read` (theseus-f2g0) it holds at 40,000 to 44,500 and not at
/// 45,000 or above (their joins' review).
const SESSION_WINDOW: u64 = 44_000;
const OUTPUT: u32 = 2_000;

pub(crate) struct Rig {
    pub(crate) core: Arc<Core>,
    pub(crate) model: Arc<FakeProvider>,
    glm: Arc<FakeProvider>,
    dir: tempfile::TempDir,
}

fn config(root: &Path, state: &Path, summary: &str) -> Config {
    let mut cfg = Config::example();
    cfg.server.state_dir = state.to_string_lossy().into_owned();
    cfg.tools.projects_dir = Some(root.to_string_lossy().into_owned());
    cfg.tools.roots = vec![];
    cfg.memory.summary_profile = Some(summary.into());
    // The summarizer is only a second provider here; it reads the session,
    // so it runs an unrestricted model (theseus-a1wvq.3).
    cfg.serve_glm_unrestricted();
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
    build_with(dir, summary, model, glm, false)
}

/// `build`, its tools deferred as a daemon's are when `defer` (theseus-7m7w.2).
fn build_with(
    dir: &Path,
    summary: &str,
    model: Arc<FakeProvider>,
    glm: Arc<FakeProvider>,
    defer: bool,
) -> Arc<Core> {
    let root = dir.join("work");
    std::fs::create_dir_all(&root).unwrap();
    let cfg = config(&root.canonicalize().unwrap(), dir, summary);
    let store = Store::open(&dir.join("store")).unwrap();
    let mut parts = crate::rpc::Parts::for_tests(cfg, model, store);
    parts.cfg.tools.defer.enabled = defer;
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

/// `rig`, its tools deferred as a daemon's are (theseus-7m7w.2).
fn rig_deferring(model: Vec<Scripted>, glm: Vec<Scripted>) -> Rig {
    let dir = tempfile::tempdir().unwrap();
    let model = Arc::new(FakeProvider::scripted(model));
    let glm = Arc::new(FakeProvider::scripted(glm));
    let core = build_with(dir.path(), "glm", model.clone(), glm.clone(), true);
    Rig {
        core,
        model,
        glm,
        dir,
    }
}

pub(crate) fn rig(model: Vec<Scripted>, glm: Vec<Scripted>) -> Rig {
    rig_with("glm", model, glm)
}

pub(crate) fn session(core: &Core) -> String {
    let rec = SessionRecord::new(SessionKind::Conversation, None);
    core.store.put_session(&rec.session_id, &rec).unwrap();
    rec.session_id
}

pub(crate) async fn turn(
    core: &Arc<Core>,
    sid: &str,
    input: &str,
) -> anyhow::Result<TurnSubmitResult> {
    turn_on(core, sid, input, None).await
}

/// A turn, on `on`'s provider and model for this message alone when given,
/// as `turn.submit`'s `provider` and `model` ask.
async fn turn_on(
    core: &Arc<Core>,
    sid: &str,
    input: &str,
    on: Option<(&str, &str)>,
) -> anyhow::Result<TurnSubmitResult> {
    let rec: SessionRecord = core.store.get_session(sid).unwrap().unwrap();
    let (live, _) = core.live_profile();
    let (provider, model) = on.unzip();
    let target = core.runner.resolve_target(&live, None, provider, model)?;
    let sink = EventSink::new(core.bus.clone(), sid, None);
    core.runner
        .run(TurnRequest {
            prompt: None,
            surface: None,
            session: rec,
            input: Some(input.into()),
            target,
            sink,
            author: "test".into(),
            recompile: None,
            attachments: vec![],
            arrived: None,
            reply_to: None,
            answer_at: None,
        })
        .await
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
pub(crate) fn summary(text: &str, input: u64, output: u64) -> Scripted {
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
pub(crate) async fn until_recompiled(r: &Rig, sid: &str, tokens: usize) -> usize {
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

/// A compaction is a boundary, and its request carries the tools its
/// compilation's manifest names: the loaded families as the selection chose
/// them, the rest deferred with the search tool first, not the whole catalog
/// (theseus-7m7w.2).
#[tokio::test]
async fn a_compaction_sends_the_tools_its_manifest_names() {
    let r = rig_deferring(
        vec![],
        vec![summary("The keeper logged six tides.", 9_000, 120)],
    );
    let sid = session(&r.core);
    until_recompiled(&r, &sid, 6_000).await;
    let comps = r.core.store.session_compilations(&sid).unwrap();
    let c = comps.last().unwrap();
    assert_eq!(c.strategy, "compaction");
    let sent = r.model.requests();
    let q = sent.last().unwrap();
    assert_eq!(q.tools[0]["name"], crate::tools_defer::SEARCH_TOOL);
    let term = q.tools.iter().find(|t| t["name"] == "term_open").unwrap();
    assert_eq!(term["defer_loading"], true);
    let digest = theseus_kernel::digest_json(&Value::Array(q.tools.clone()));
    assert_eq!(
        c.manifest.tools_digest,
        digest[..16],
        "the manifest names the tools the request carried"
    );
    assert!(c
        .manifest
        .loaded
        .as_ref()
        .is_some_and(|l| l.iter().any(|f| f == "files")));
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
    assert_eq!((profile.as_str(), model.as_str()), ("glm", GLM));
    assert!(header.starts_with(&format!("[Summary of {count} earlier messages, ")));
    // Its testimony (35a): the range's positions, the profile, and its model.
    assert!(
        header.ends_with(&format!(" (@{first} to @{end}), written by glm on {GLM}]")),
        "{header}"
    );

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

/// The loop's model calls and the summary's call are dispatched with the
/// whole-call deadline (theseus-5pg8u), not the stream's `total_secs`.
#[tokio::test]
async fn the_loop_and_summary_calls_are_dispatched_to_the_whole_call() {
    let r = rig(
        vec![],
        vec![summary("The keeper logged six tides.", 9_000, 120)],
    );
    let sid = session(&r.core);
    until_recompiled(&r, &sid, 6_000).await;
    let timeouts = Config::example().model.timeouts;
    let whole = timeouts.whole_call().as_millis() as u64;
    assert_ne!(
        whole,
        timeouts.total_secs * 1000,
        "the config can tell them apart"
    );
    let row = &rows(&r.core, "context.compacted")[0];
    let corr = row["correlation_id"].as_str().unwrap();
    let actions = r.core.kernel.actions().unwrap();
    let span = |a: &theseus_kernel::Action| a.deadline_at_ms - a.planned_at_ms;
    let summary_call = actions.iter().find(|a| a.correlation_id == corr).unwrap();
    assert_eq!(span(summary_call), whole, "the summary's call");
    let loops: Vec<_> = actions
        .iter()
        .filter(|a| a.tool == theseus_kernel::PROVIDER_TOOL && a.correlation_id != corr)
        .collect();
    assert!(!loops.is_empty(), "the loop made model calls");
    for a in loops {
        assert_eq!(span(a), whole, "the loop's call {}", a.correlation_id);
    }
}

/// The summary's call was reserved before it ran and settled at its real
/// cost: the row, the kernel's action, the execution's budget, and the
/// session's books agree.
fn settled_as_reserved(r: &Rig, sid: &str, row: &Value, node: &str, cost_usd: Option<f64>) {
    let settled = row["settled_micros"].as_u64().unwrap();
    let reserved = row["reserved_micros"].as_u64().unwrap();
    // Haiku 5.5, the glm profile's model here: $0.10 in and $0.50 out a million.
    assert_eq!(settled, 9_000 * 10 / 100 + 120 * 50 / 100);
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

/// A summary cut at its own cap is a failed summary (theseus-2c6iz): its text
/// is a fragment of the range, so it never replaces the range. The ring runs,
/// the row says the call hit its cap, no `Summary` node is written, and the
/// call was settled, nothing left held.
#[tokio::test]
async fn a_summary_cut_at_its_cap_leaves_the_range_uncompacted() {
    let cut = Scripted::Blocks {
        blocks: vec![json!({"type": "text", "text": "The keeper logged the tide and the w"})],
        stop_reason: "max_tokens".into(),
    };
    let r = rig(vec![], vec![cut]);
    let sid = session(&r.core);
    until_recompiled(&r, &sid, 6_000).await;
    let compiled = rows(&r.core, "context.compiled");
    assert_eq!(compiled.last().unwrap()["strategy"], "ring");
    let row = &rows(&r.core, "context.compacted")[0];
    assert_eq!(row["outcome"], "ring", "{row}");
    assert!(
        row["why"].as_str().unwrap().contains("max_tokens"),
        "the row names the cap: {row}"
    );
    // One subject: cap_phrase already starts with "the answer".
    assert!(
        row["why"]
            .as_str()
            .unwrap()
            .contains("the summary call was cut: the answer stopped at its cap of"),
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

/// The summary call turns thinking off where the model can (theseus-2c6iz):
/// Sonnet 5.5 thinks by default, and a summary's cap is spent on thinking
/// first. It takes `between_tools` (`catalog::thinking_off`), not `disabled`.
#[tokio::test]
async fn the_summary_call_turns_thinking_off_on_an_adaptive_model() {
    let r = session_rig(vec![], vec![]);
    let sid = session(&r.core);
    until_recompiled(&r, &sid, 6_000).await;
    let asked: Vec<ProviderRequest> = r.model.requests().into_iter().filter(is_summary).collect();
    assert_eq!(asked.len(), 1, "one summary call, on the session's model");
    assert_eq!(asked[0].model, "claude-sonnet-5-5");
    assert_eq!(asked[0].thinking, Some(json!({"type": "between_tools"})));
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

/// `summary_profile = "session"`, the default: the session's own profile,
/// provider and model summarize the range, so it goes nowhere the session's
/// turns don't. glm is never asked; the node, its header and the row name
/// the session's profile.
#[tokio::test]
async fn summary_profile_session_summarizes_on_the_sessions_own_model() {
    assert_eq!(
        crate::config::MemoryConfig::default().summary_profile,
        None,
        "unset asks Jev; while the pack asks no `model` that runs as `session`"
    );
    let r = rig_with(SUMMARY_SESSION, vec![], vec![]);
    let sid = session(&r.core);
    until_recompiled(&r, &sid, 6_000).await;
    assert_eq!(
        rows(&r.core, "context.compiled").last().unwrap()["strategy"],
        "compaction"
    );
    assert!(r.glm.requests().is_empty(), "no second provider");

    let (live, _) = r.core.live_profile();
    let nodes = r.core.store.transcript(&sid).unwrap();
    let Some(Body::Summary {
        profile,
        model,
        header,
        ..
    }) = nodes
        .iter()
        .map(|(_, n)| &n.body)
        .find(|b| matches!(b, Body::Summary { .. }))
    else {
        panic!("no summary node");
    };
    assert_eq!(
        (profile.as_str(), model.as_str()),
        (live.as_str(), "claude-sonnet-5-5")
    );
    assert!(
        header.ends_with(&format!(", written by {live} on claude-sonnet-5-5]")),
        "{header}"
    );
    let row = &rows(&r.core, "context.compacted")[0];
    assert_eq!(
        (&row["outcome"], &row["profile"]),
        (&json!("compaction"), &json!(live)),
        "{row}"
    );

    // The summary call went to the session's provider, with the dropped
    // turns.
    let calls: Vec<ProviderRequest> = r
        .model
        .requests()
        .into_iter()
        .filter(|q| {
            q.system
                .first()
                .and_then(|s| s["text"].as_str())
                .is_some_and(|s| s.starts_with("You summarize the earlier part"))
        })
        .collect();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].model, "claude-sonnet-5-5");
    assert!(text_of(&calls[0]).contains("turn0 "));
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
        &format!(
            "the newest exchange alone does not fit claude-sonnet-5-5's window of {},{:03}",
            WINDOW / 1000,
            WINDOW % 1000
        ),
        &format!(
            "upper bound) against the {},{:03} the window leaves",
            (WINDOW - 6_096) / 1000,
            (WINDOW - 6_096) % 1000
        ),
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

const HERON: &str = "Remember: the grey heron nests by the old weir at Millbrook.";

/// A session that said `HERON`, and an index over it, with recall live.
fn recalling(r: &Rig) -> String {
    let a = crate::tests_recall::session(&r.core, None, &[HERON]);
    r.core
        .runner
        .memory
        .set_ask(crate::tests_recall::index_of(&r.core, vec![a.clone()]));
    a
}

fn live(model: Vec<Scripted>, glm: Vec<Scripted>) -> Rig {
    let dir = tempfile::tempdir().unwrap();
    let model = Arc::new(FakeProvider::scripted(model));
    let glm = Arc::new(FakeProvider::scripted(glm));
    let root = dir.path().join("work");
    std::fs::create_dir_all(&root).unwrap();
    let mut cfg = config(&root.canonicalize().unwrap(), dir.path(), "glm");
    cfg.memory.mode = crate::config::MemoryMode::Live;
    let store = Store::open(&dir.path().join("store")).unwrap();
    let mut parts = crate::rpc::Parts::for_tests(cfg, model.clone(), store);
    parts.providers.insert("zai".into(), glm.clone());
    let core = Core::build(parts).unwrap();
    Rig {
        core,
        model,
        glm,
        dir,
    }
}

/// A task's first compile is assembled: its recall section first in the
/// prefix, at `assembled_budget_tokens`, then the brief; the compilation
/// records the section's node.
#[tokio::test]
async fn a_tasks_first_compile_is_assembled_with_its_recall_section_first() {
    let r = live(vec![], vec![]);
    recalling(&r);
    let parent = session(&r.core);
    turn(&r.core, &parent, "start a task").await.unwrap();
    let parent_rec: SessionRecord = r.core.store.get_session(&parent).unwrap().unwrap();
    let mut task = SessionRecord::new(SessionKind::Task, None);
    task.task = Some(crate::session::TaskOf {
        parent_session: parent.clone(),
        parent_execution: parent_rec.execution_id.clone().unwrap(),
        by: "act_brief".into(),
        target: None,
        arrangement: None,
        check: None,
        notify_done: false,
    });
    r.core.store.put_session(&task.session_id, &task).unwrap();
    let sid = task.session_id.clone();
    turn(&r.core, &sid, "Where does the grey heron nest?")
        .await
        .unwrap();
    let q = r.model.requests().pop().unwrap();
    assert!(
        first_text(&q).starts_with("[Recalled by the harness: 1 note from earlier sessions,"),
        "{}",
        first_text(&q)
    );
    assert!(text_of(&q).contains("old weir"));
    let section = r
        .core
        .store
        .transcript(&sid)
        .unwrap()
        .iter()
        .find(|(_, n)| matches!(n.body, Body::Recall { .. }))
        .map(|(_, n)| n.id.clone())
        .unwrap();
    let comps = r.core.store.session_compilations(&sid).unwrap();
    assert_eq!(comps[0].recall_id.as_deref(), Some(section.as_str()));
    let ran = r
        .core
        .store
        .scope_after(&crate::fact::recall::scope(&sid), 0)
        .unwrap()
        .iter()
        .map(|x| x.decode::<crate::ledger::LedgerRow>().unwrap())
        .find(|x| x.kind == "recall.ran")
        .unwrap();
    assert_eq!(ran.data["budget_tokens"], 4_000, "the assembled budget");

    // A task ends with its turn: the compaction tests hold the append.
}

/// A compaction is assembled: the recall section, then the summary, then
/// the kept turns; the compilation records the section.
#[tokio::test]
async fn a_compaction_is_assembled_with_recall_before_the_summary() {
    let r = live(vec![], vec![summary("Six tides were logged.", 9_000, 40)]);
    recalling(&r);
    let sid = session(&r.core);
    until_recompiled(&r, &sid, 6_000).await;
    let comps = r.core.store.session_compilations(&sid).unwrap();
    let c = comps.last().unwrap();
    assert_eq!(c.strategy, "compaction");
    let section = c.recall_id.clone().expect("an assembled recall section");
    let q = r.model.requests().pop().unwrap();
    let first: Vec<&str> = q.messages[0]["content"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|b| b["text"].as_str())
        .collect();
    assert!(
        first[0].starts_with("[Recalled by the harness: "),
        "{first:?}"
    );
    assert!(first[1].starts_with("[Summary of "), "{first:?}");
    assert!(r
        .core
        .store
        .transcript(&sid)
        .unwrap()
        .iter()
        .any(|(_, n)| n.id == section));
    turn(&r.core, &sid, "and the wind?").await.unwrap();
    let next = r.model.requests().pop().unwrap();
    assert_eq!(
        &next.messages[..q.messages.len() - 1],
        &q.messages[..q.messages.len() - 1]
    );
}

/// glm's model, on `zai`: an unrestricted one (theseus-a1wvq.3).
const GLM: &str = crate::config::UNRESTRICTED_SECOND;

/// A session whose summary profile is `session`, glm given the session
/// model's window and output cap, so a turn on either rings at the same
/// point.
fn session_rig(model: Vec<Scripted>, glm: Vec<Scripted>) -> Rig {
    let dir = tempfile::tempdir().unwrap();
    let model = Arc::new(FakeProvider::scripted(model));
    let glm = Arc::new(FakeProvider::scripted(glm));
    let root = dir.path().join("work");
    std::fs::create_dir_all(&root).unwrap();
    let mut cfg = config(&root.canonicalize().unwrap(), dir.path(), SUMMARY_SESSION);
    cfg.catalog.insert(
        GLM.into(),
        crate::catalog::CatalogRow {
            context_window: Some(SESSION_WINDOW),
            max_output_tokens: Some(OUTPUT),
            // GLM's denser tokenizer, which this rig's band was set on: the
            // stand-in model reads a request as GLM did (theseus-a1wvq.3).
            bytes_per_token: Some(crate::catalog::TokenRates::GLM),
            ..Default::default()
        },
    );
    let store = Store::open(&dir.path().join("store")).unwrap();
    let mut parts = crate::rpc::Parts::for_tests(cfg, model.clone(), store);
    parts.providers.insert("zai".into(), glm.clone());
    Rig {
        core: Core::build(parts).unwrap(),
        model,
        glm,
        dir,
    }
}

/// Whether `q` is a summary call.
fn is_summary(q: &ProviderRequest) -> bool {
    q.system
        .first()
        .and_then(|s| s["text"].as_str())
        .is_some_and(|s| s.starts_with("You summarize the earlier part"))
}

/// `summary_profile = "session"` follows a one-message override
/// (theseus-6fn.9): the session's own profile is the session's target, but a
/// compacting turn submitted on glm's model summarizes on glm, and the
/// `Summary` node, its header and the `context.compacted` row name glm's
/// model. The session's next turn, with no override, runs on its own model.
#[tokio::test]
async fn summary_profile_session_follows_a_one_message_override() {
    // How many turns the session takes to ring, on its own model.
    let probe = session_rig(vec![], vec![]);
    let sid = session(&probe.core);
    let turns = until_recompiled(&probe, &sid, 6_000).await;
    assert!(turns >= 2, "rang after {turns} turns");

    let r = session_rig(
        vec![],
        vec![summary("GLM: the keeper logged the tides.", 9_000, 60)],
    );
    let sid = session(&r.core);
    for k in 0..turns - 1 {
        turn(&r.core, &sid, &words(&format!("turn{k}"), 6_000))
            .await
            .unwrap();
    }
    assert!(rows(&r.core, "context.compacted").is_empty());
    let last = format!("turn{}", turns - 1);
    turn_on(&r.core, &sid, &words(&last, 6_000), Some(("zai", GLM)))
        .await
        .unwrap();
    let compiled = rows(&r.core, "context.compiled");
    assert_eq!(compiled.last().unwrap()["strategy"], "compaction");

    // The summary call went to glm, with the dropped turns; the session's
    // provider wrote none.
    let asked: Vec<ProviderRequest> = r.glm.requests().into_iter().filter(is_summary).collect();
    assert_eq!(asked.len(), 1, "one summary call, to glm");
    assert_eq!(asked[0].model, GLM);
    assert!(text_of(&asked[0]).contains("turn0 "));
    assert!(!r.model.requests().iter().any(is_summary));

    // The node, its header, and the row name the override's model.
    let (live, _) = r.core.live_profile();
    let nodes = r.core.store.transcript(&sid).unwrap();
    let Some(Body::Summary {
        profile,
        model,
        header,
        text,
        ..
    }) = nodes
        .iter()
        .map(|(_, n)| &n.body)
        .find(|b| matches!(b, Body::Summary { .. }))
    else {
        panic!("no summary node");
    };
    assert_eq!((profile.as_str(), model.as_str()), (live.as_str(), GLM));
    assert_eq!(text, "GLM: the keeper logged the tides.");
    assert!(
        header.ends_with(&format!(", written by {live} on {GLM}]")),
        "{header}"
    );
    let row = &rows(&r.core, "context.compacted")[0];
    assert_eq!(
        (&row["outcome"], &row["model"]),
        (&json!("compaction"), &json!(GLM)),
        "{row}"
    );

    // The next turn, with no override, is the session's own model's.
    let before = r.glm.requests().len();
    turn(&r.core, &sid, "and the lamp?").await.unwrap();
    assert_eq!(r.glm.requests().len(), before, "glm is asked nothing more");
    let q = r.model.requests().pop().unwrap();
    assert_eq!(q.model, "claude-sonnet-5-5");
    assert!(!is_summary(&q));
}

/// What a provider that counts 25 % more input than the estimate's
/// `tokens`, and writes all `max_tokens`, bills a summary call.
fn over_the_estimate(q: &ProviderRequest) -> Usage {
    let rates = crate::catalog::Catalog::builtin()
        .get(GLM)
        .unwrap()
        .bytes_per_token;
    let est = crate::compiler::estimate(q, rates, None);
    Usage {
        input_tokens: (est.tokens * 125).div_ceil(100),
        output_tokens: u64::from(q.max_tokens),
        ..Default::default()
    }
}

/// The summary's call is reserved on the estimate's upper bound
/// (theseus-6fn.8): a provider that counts its input 25 % over the
/// estimate's tokens and writes the most it may settles within the
/// reservation, read from the row as `settled_as_reserved` reads it.
#[tokio::test]
async fn the_summary_is_reserved_on_the_estimates_upper_bound() {
    let r = rig(
        vec![],
        vec![Scripted::BilledBy {
            usage: over_the_estimate,
            then: Box::new(Scripted::text("The keeper logged six tides.")),
        }],
    );
    let sid = session(&r.core);
    until_recompiled(&r, &sid, 6_000).await;
    let row = &rows(&r.core, "context.compacted")[0];
    assert_eq!(row["outcome"], "compaction", "{row}");
    let asked = r.glm.requests();
    assert_eq!(asked.len(), 1);
    let billed = over_the_estimate(&asked[0]);
    assert_eq!(
        row["input_tokens"].as_u64(),
        Some(billed.input_tokens),
        "{row}"
    );
    assert_eq!(
        row["summary_tokens"].as_u64(),
        Some(billed.output_tokens),
        "{row}"
    );
    let settled = row["settled_micros"].as_u64().unwrap();
    let reserved = row["reserved_micros"].as_u64().unwrap();
    println!(
        "billed {} in and {} out: settled {settled} micros, reserved {reserved}",
        billed.input_tokens, billed.output_tokens
    );
    assert!(
        settled <= reserved,
        "settled {settled} past reserved {reserved}"
    );
    let actions = r.core.kernel.actions().unwrap();
    let call = actions
        .iter()
        .find(|a| a.correlation_id == row["correlation_id"].as_str().unwrap())
        .unwrap();
    assert_eq!(call.reserved_micros, reserved);
    assert_eq!(call.state, ActionState::Succeeded);
}

/// The daemon's day ceiling (theseus-kp20) refuses the summary's call
/// before it is made: the ring runs, the row says why in the ceiling's
/// words, glm is never asked, and the turn's own call is refused next, so
/// the turn fails `daily_ceiling`; the day's `spend.ceiling` row names
/// compaction, which met it first.
#[tokio::test]
async fn the_day_ceiling_refuses_the_summary_before_its_call() {
    // A first run finds the turn that compacts.
    let probe = rig(vec![], vec![summary("Six tides.", 9_000, 120)]);
    let sid = session(&probe.core);
    let k = until_recompiled(&probe, &sid, 6_000).await;
    drop(probe);
    let r = rig(vec![], vec![summary("Six tides.", 9_000, 120)]);
    let sid = session(&r.core);
    for i in 0..k - 1 {
        turn(&r.core, &sid, &words(&format!("turn{i}"), 6_000))
            .await
            .unwrap();
    }
    let d = r.core.kernel.day_ceiling();
    let t = d.today(d.now());
    // These tests are the stop mode's (theseus-9e02): the ceiling refuses.
    d.set_rules(false, d.runaway(), d.step(), 0);
    d.set_limit(t.spent + t.held);
    let asked = r.model.requests().len();
    let err = turn(&r.core, &sid, &words(&format!("turn{}", k - 1), 6_000))
        .await
        .unwrap_err();
    assert_eq!(
        err.downcast_ref::<TurnError>().unwrap().class,
        "daily_ceiling"
    );
    assert!(r.glm.requests().is_empty(), "the summary was never asked");
    assert_eq!(r.model.requests().len(), asked, "nor the turn's own call");
    let row = rows(&r.core, "context.compacted").pop().unwrap();
    assert_eq!(row["outcome"], "ring");
    assert!(
        row["why"].as_str().unwrap().contains("daily ceiling"),
        "{row}"
    );
    let ceiling = rows(&r.core, "spend.ceiling");
    assert_eq!(ceiling.len(), 1, "{ceiling:?}");
    assert_eq!(ceiling[0]["what"], "compaction");
}

/// A `"self"` task's turn that compacts: the summary's call is a model call
/// of the turn, so the self budget counts it with the call after it, and the
/// budget holds the turn's whole cost (theseus-pw1q.3, a join fix: the
/// summary's cost was not counted). A task's execution ends with its turn,
/// so the session's earlier turns are a conversation's and the one that
/// compacts is the self task's; a first rig counts the turns to it.
#[tokio::test]
async fn a_self_tasks_compaction_is_counted_on_the_self_budget() {
    let self_rig = || {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("work");
        std::fs::create_dir_all(&root).unwrap();
        let mut cfg = config(&root.canonicalize().unwrap(), dir.path(), "glm");
        cfg.self_improve.mode = theseus_protocol::rsi::SelfMode::Act;
        // A thousand tokens over the other rigs' window: the ring's six kept
        // turns leave the 4,096-token summary its room whatever a few bytes
        // of the tool list move (the open tasks' schemas, theseus-a1wvq.6,
        // left it 4,045).
        cfg.catalog
            .get_mut("claude-sonnet-5-5")
            .unwrap()
            .context_window = Some(WINDOW + 1_000);
        let model = Arc::new(FakeProvider::scripted(vec![]));
        let glm = Arc::new(FakeProvider::scripted(vec![summary(
            "The keeper logged six tides; the operator asked for each one.",
            9_000,
            120,
        )]));
        let store = Store::open(&dir.path().join("store")).unwrap();
        let mut parts = crate::rpc::Parts::for_tests(cfg, model.clone(), store);
        parts.providers.insert("zai".into(), glm.clone());
        Rig {
            core: Core::build(parts).unwrap(),
            model,
            glm,
            dir,
        }
    };
    let probe = self_rig();
    let sid = session(&probe.core);
    let k = until_recompiled(&probe, &sid, 6_000).await;
    let r = self_rig();
    let parent = session(&r.core);
    let sid = session(&r.core);
    for i in 0..k - 1 {
        turn(&r.core, &sid, &words(&format!("turn{i}"), 6_000))
            .await
            .unwrap();
    }
    assert!(rows(&r.core, "context.compacted").is_empty());
    crate::tests_self_budget::make_task(&r.core, &sid, &parent, crate::rsi::SELF_PRINCIPAL, None);
    let out = turn(&r.core, &sid, &words(&format!("turn{}", k - 1), 6_000))
        .await
        .unwrap();
    let compacted = rows(&r.core, "context.compacted");
    assert_eq!(compacted[0]["outcome"], "compaction", "{compacted:?}");
    let v = r.core.runner.self_budget.view().unwrap();
    let cost = out.cost_usd.unwrap();
    assert!(
        (v.day_spent_usd - cost).abs() < 1e-5,
        "the self budget holds {} of the turn's {cost}",
        v.day_spent_usd
    );
    assert_eq!(v.branches[0].open_usd, 0.0, "nothing is left open");
}

/// A stand-in summarizer that does what the instructions' clause on format,
/// length and tone asks, and only when they ask it: it writes, in the
/// operator's words, each sentence of theirs that names how to answer. With
/// the clause gone from its instructions it writes the plain recap a
/// summary without the clause would.
struct StyleKeeper {
    asked: std::sync::Mutex<Vec<ProviderRequest>>,
}

const KEEP_CLAUSE: &str =
    "Keep any instruction the operator gave about format, length or tone, in their words.";

impl crate::provider::Provider for StyleKeeper {
    fn name(&self) -> &str {
        "fake"
    }
    fn stream_message<'a>(
        &'a self,
        req: &'a ProviderRequest,
        on_delta: crate::provider::DeltaSink<'a>,
    ) -> crate::provider::ProviderFuture<'a> {
        self.asked.lock().unwrap().push(req.clone());
        let instructions = req.system[0]["text"].as_str().unwrap_or_default();
        let read = text_of(req);
        let mut kept = Vec::new();
        if instructions.contains(KEEP_CLAUSE) {
            for sentence in read.split(['.', '?', '!', '\n']) {
                let s = sentence.trim();
                if ["sentences", "brief", "bullet", "formal tone"]
                    .iter()
                    .any(|w| s.contains(w))
                {
                    kept.push(s.trim_matches(|c| c == '"' || c == '\\').to_string());
                }
            }
        }
        let text = match kept.is_empty() {
            true => "The operator asked about the tides and the keeper answered.".to_string(),
            false => format!("The operator said: \"{}\".", kept.join("\"; \"")),
        };
        Box::pin(async move {
            FakeProvider::scripted(vec![summary(&text, 9_000, 60)])
                .stream_message(req, on_delta)
                .await
        })
    }
}

/// "For now, two sentences max" is the operator's, said once and early; a
/// forced compaction summarizes it away unless the summary is told to keep
/// style requests, and the summary call is. The stand-in obeys that clause,
/// so the next request, which begins with the summary, still carries the
/// words as the operator wrote them.
#[tokio::test]
async fn a_style_request_survives_a_compaction_in_the_operators_words() {
    const ASK: &str = "for now, two sentences max";
    let dir = tempfile::tempdir().unwrap();
    let model = Arc::new(FakeProvider::scripted(vec![]));
    let glm = Arc::new(StyleKeeper {
        asked: Default::default(),
    });
    let root = dir.path().join("work");
    std::fs::create_dir_all(&root).unwrap();
    let cfg = config(&root.canonicalize().unwrap(), dir.path(), "glm");
    let store = Store::open(&dir.path().join("store")).unwrap();
    let mut parts = crate::rpc::Parts::for_tests(cfg, model.clone(), store);
    parts.providers.insert("zai".into(), glm.clone());
    let core = Core::build(parts).unwrap();
    let sid = session(&core);
    turn(&core, &sid, &format!("Chart the tides. {ASK}, please."))
        .await
        .unwrap();
    for k in 0..12 {
        turn(&core, &sid, &words(&format!("turn{k}"), 6_000))
            .await
            .unwrap();
        if rows(&core, "context.compiled")
            .last()
            .is_some_and(|c| c["strategy"] == "compaction")
        {
            break;
        }
    }
    assert_eq!(rows(&core, "context.compacted").len(), 1, "it rang");
    let asked = glm.asked.lock().unwrap().clone();
    assert_eq!(asked.len(), 1);
    // The summary was asked to keep it, and was shown it whole.
    assert!(asked[0].system[0]["text"]
        .as_str()
        .unwrap()
        .contains(KEEP_CLAUSE));
    assert!(text_of(&asked[0]).contains(ASK));
    // The next request begins with a summary that says it in the operator's words.
    let sent = model.requests();
    let q = sent.last().unwrap();
    assert!(
        first_text(q).contains(&format!("\"{ASK}")),
        "{}",
        first_text(q)
    );
    assert!(
        !text_of(q).contains("Chart the tides"),
        "the message itself was summarized away"
    );
}

// Route.v10 row 4 (theseus-ej6bj): `[memory] summary_profile` is a pin when
// set, and with it unset Jev chooses the model each summary is written on.
// `session` keeps its meaning (the pin tests above hold it).

/// A rig with the fake Jev, whose route pack is `mode: chat` (no move) for the
/// turns and whose job pack asks `model`.
fn rig_jev(
    jev: &theseus_judge::fake::FakeJev,
    summary: Option<&str>,
    ask_model: bool,
    tweak: impl FnOnce(&mut Config),
) -> Rig {
    let dir = tempfile::tempdir().unwrap();
    let model = Arc::new(FakeProvider::scripted(vec![]));
    let glm = Arc::new(FakeProvider::scripted(vec![summary_for_glm()]));
    let root = dir.path().join("work");
    std::fs::create_dir_all(&root).unwrap();
    let mut cfg = config(&root.canonicalize().unwrap(), dir.path(), "session");
    cfg.memory.summary_profile = summary.map(str::to_string);
    cfg.judge.enabled = true;
    cfg.judge.api_base = jev.base();
    cfg.judge.connect_secs = 1;
    cfg.judge.total_secs = 30;
    cfg.routing.max_wait_ms = 300;
    cfg.name_every_provider_key();
    tweak(&mut cfg);
    let store = Store::open(&dir.path().join("store")).unwrap();
    let mut parts = crate::rpc::Parts::for_tests(cfg, model.clone(), store);
    parts.providers.insert("zai".into(), glm.clone());
    // DeepSeek's provider is there, so a Flash pick is refused by the data
    // rule and not for want of somewhere to go.
    parts
        .providers
        .insert("deepseek".into(), Arc::new(FakeProvider::default()));
    parts.secrets = crate::tests_judge::board();
    let core = Core::build(parts).unwrap();
    crate::tests_judge::warm(&core);
    if ask_model {
        core.runner
            .judge
            .job_pack_with(crate::tests_job_ask::model_pack());
    }
    Rig {
        core,
        model,
        glm,
        dir,
    }
}

/// Jev's `model` answer for the summary's ask alone. The route pack that asks
/// `model` is the acting one for every ask (the unified switch,
/// `JudgeService::route_open`), so the turns' own asks must not answer it too:
/// they stay on their profile, as the tests under them assume.
fn pick_summary(jev: &theseus_judge::fake::FakeJev, option: &str, confidence: f64) {
    jev.script_when(
        "Summarize the oldest",
        None,
        "model",
        theseus_judge::fake::Scripted::Choice {
            option: option.into(),
            confidence,
        },
    );
}

fn summary_for_glm() -> Scripted {
    summary("The keeper logged six tides.", 9_000, 120)
}

fn jev_chat(jev: &theseus_judge::fake::FakeJev) {
    crate::tests_route::mode(jev, "chat", 0.95);
}

/// The one request Jev saw that was the summary's ask.
fn summary_asks(jev: &theseus_judge::fake::FakeJev) -> Vec<String> {
    jev.seen()
        .iter()
        .map(|s| s.body["state"].to_string())
        .filter(|s| s.contains("Summarize the oldest"))
        .collect()
}

/// The models of the requests the session's provider got.
fn models_asked(r: &Rig) -> Vec<String> {
    r.model.requests().into_iter().map(|q| q.model).collect()
}

/// Unset, with a pack that asks `model`: Jev names Opus, and the summary is
/// written there: the request, the node, and the row say so.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_unset_summary_profile_asks_jev_and_writes_on_its_pick() {
    let jev = theseus_judge::fake::FakeJev::start().unwrap();
    jev_chat(&jev);
    pick_summary(&jev, "opus", 0.95);
    let r = rig_jev(&jev, None, true, |_| {});
    let sid = session(&r.core);
    until_recompiled(&r, &sid, 6_000).await;
    let asks = summary_asks(&jev);
    assert!(!asks.is_empty(), "Jev was asked about the summary");
    assert!(asks[0].contains("job"), "{}", asks[0]);
    assert!(models_asked(&r).contains(&"claude-opus-5-5".to_string()));
    let nodes = r.core.store.transcript(&sid).unwrap();
    let model = nodes.iter().find_map(|(_, n)| match &n.body {
        Body::Summary { model, .. } => Some(model.clone()),
        _ => None,
    });
    assert_eq!(model.as_deref(), Some("claude-opus-5-5"));
    assert!(r.glm.requests().is_empty(), "glm was not asked");
}

/// A set key is a pin: Jev is asked nothing about the summary.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_set_summary_profile_asks_jev_nothing() {
    let jev = theseus_judge::fake::FakeJev::start().unwrap();
    jev_chat(&jev);
    pick_summary(&jev, "opus", 0.95);
    let r = rig_jev(&jev, Some("glm"), true, |_| {});
    let sid = session(&r.core);
    until_recompiled(&r, &sid, 6_000).await;
    assert!(summary_asks(&jev).is_empty());
    assert!(!models_asked(&r).contains(&"claude-opus-5-5".to_string()));
    assert!(!r.glm.requests().is_empty(), "glm wrote the summary");
}

/// A slow Jev never holds a compaction: past `max_wait_ms` the summary is
/// written on the turn's own model, and the turn goes on.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_slow_jev_does_not_hold_a_compaction() {
    let jev = theseus_judge::fake::FakeJev::start().unwrap();
    jev_chat(&jev);
    pick_summary(&jev, "opus", 0.95);
    // Only the summary's ask is held: the turns' own asks answer, so the one
    // thing that is slow is the one under test.
    jev.hold_when("Summarize the oldest");
    let r = rig_jev(&jev, None, true, |_| {});
    let sid = session(&r.core);
    let t0 = std::time::Instant::now();
    until_recompiled(&r, &sid, 6_000).await;
    assert!(
        t0.elapsed() < std::time::Duration::from_secs(60),
        "the turns went on"
    );
    assert!(
        !summary_asks(&jev).is_empty(),
        "Jev was asked about the summary, and held its answer"
    );
    assert!(!models_asked(&r).contains(&"claude-opus-5-5".to_string()));
    let nodes = r.core.store.transcript(&sid).unwrap();
    assert!(
        nodes
            .iter()
            .any(|(_, n)| matches!(n.body, Body::Summary { .. })),
        "the summary was written"
    );
    jev.release();
}

/// The owner-material floor is the labels': a session with a label that keeps
/// restricted models off is told `not_allowed`, and a pick of Flash is refused.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_labelled_session_is_told_not_allowed_and_never_summarized_on_flash() {
    let jev = theseus_judge::fake::FakeJev::start().unwrap();
    jev_chat(&jev);
    pick_summary(&jev, "deepseek_flash", 0.97);
    // `cn_models = never`: no model of restricted origin reads any session.
    let r = rig_jev(&jev, None, true, |c| {
        c.content_based_security.cn_models = crate::config::content_based_security::CnModels::Never;
    });
    let sid = session(&r.core);
    until_recompiled(&r, &sid, 6_000).await;
    let asks = summary_asks(&jev);
    assert!(!asks.is_empty());
    assert!(!models_asked(&r).contains(&"deepseek-flash".to_string()));
}
