//! One long instruction keeps going (theseus-3ki0), through the whole core,
//! with a scripted stand-in for the session's model on a small window and
//! one for the summary profile's (`glm`): a single message whose turn reads
//! hundreds of large results never ends in `context_overage`. Past the
//! clearing line its old results become stubs (a `context.cleared` row a
//! pass), past the window it compacts inside its turn, the instruction
//! pinned verbatim at the top of every request, no call ever without its
//! result; and the turn ends as its model ends it.

use std::path::Path;
use std::sync::Arc;

use serde_json::{json, Value};
use theseus_protocol::{SessionKind, TurnSubmitResult};

use crate::bus::EventSink;
use crate::provider::{FakeProvider, ProviderRequest, Scripted};
use crate::session::SessionRecord;
use crate::store::Store;
use crate::turn::TurnRequest;
use crate::{Config, Core};

const OUTPUT: u32 = 2_000;

const INSTRUCTION: &str = "Read every log under logs/ and tell me which keeper's entries disagree \
with the tide table. Keep going until you have read them all.";

struct Rig {
    core: Arc<Core>,
    model: Arc<FakeProvider>,
    glm: Arc<FakeProvider>,
    _dir: tempfile::TempDir,
}

fn config(root: &Path, state: &Path, window: u64, working: Option<u64>) -> Config {
    let mut cfg = Config::example();
    cfg.server.state_dir = state.to_string_lossy().into_owned();
    cfg.tools.projects_dir = Some(root.to_string_lossy().into_owned());
    cfg.tools.roots = vec![];
    cfg.memory.summary_profile = Some("glm".into());
    // glm only summarizes here: an unrestricted model (theseus-a1wvq.3).
    cfg.serve_glm_unrestricted();
    cfg.context.working_window_tokens = working;
    cfg.catalog.insert(
        "claude-sonnet-5-5".into(),
        crate::catalog::CatalogRow {
            context_window: Some(window),
            max_output_tokens: Some(OUTPUT),
            ..Default::default()
        },
    );
    cfg
}

/// A core whose model reads `calls` logs of `chars` characters each, one
/// call a loop, then answers; its window `window`, lowered to `working`.
fn rig(calls: usize, chars: usize, window: u64, working: Option<u64>) -> Rig {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("work");
    std::fs::create_dir_all(root.join("logs")).unwrap();
    let line = "the keeper logged the tide at the north light and the table agreed. ";
    let body = line.repeat(chars / line.len() + 1)[..chars].to_string();
    let mut script = Vec::new();
    for k in 0..calls {
        let name = format!("logs/keeper-{k:03}.txt");
        std::fs::write(root.join(&name), format!("log {k}\n{body}")).unwrap();
        script.push(Scripted::tools(
            &format!("Reading log {k}."),
            &[(&format!("toolu_{k:03}"), "fs_read", json!({"path": name}))],
        ));
    }
    script.push(Scripted::text("All the logs agree with the tide table."));
    let model = Arc::new(FakeProvider::scripted(script));
    let glm = Arc::new(FakeProvider::scripted(vec![]));
    let cfg = config(&root.canonicalize().unwrap(), dir.path(), window, working);
    let store = Store::open(&dir.path().join("store")).unwrap();
    let mut parts = crate::rpc::Parts::for_tests(cfg, model.clone(), store);
    parts.providers.insert("zai".into(), glm.clone());
    let core = Core::build(parts).unwrap();
    Rig {
        core,
        model,
        glm,
        _dir: dir,
    }
}

async fn turn(core: &Arc<Core>, input: &str) -> (String, anyhow::Result<TurnSubmitResult>) {
    let rec = SessionRecord::new(SessionKind::Conversation, None);
    core.store.put_session(&rec.session_id, &rec).unwrap();
    let sid = rec.session_id.clone();
    let (live, _) = core.live_profile();
    let target = core.runner.resolve_target(&live, None, None, None).unwrap();
    let sink = EventSink::new(core.bus.clone(), &sid, None);
    let r = core
        .runner
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
        .await;
    (sid, r)
}

fn rows(core: &Core, kind: &str) -> Vec<Value> {
    let rows: Vec<(u64, crate::ledger::LedgerRow)> = core.store.ledger_tail(100_000).unwrap();
    rows.into_iter()
        .filter(|(_, r)| r.kind == kind)
        .map(|(_, r)| r.data)
        .collect()
}

/// Every call in a request has its result in the next message, and every
/// result its call in the message before.
fn closed(q: &ProviderRequest) -> Result<(), String> {
    let ids = |m: Option<&Value>, kind: &str, key: &str| -> Vec<String> {
        m.and_then(|m| m["content"].as_array())
            .into_iter()
            .flatten()
            .filter(|b| b["type"] == kind)
            .map(|b| b[key].as_str().unwrap_or_default().to_string())
            .collect()
    };
    for (i, m) in q.messages.iter().enumerate() {
        let results_next = ids(q.messages.get(i + 1), "tool_result", "tool_use_id");
        for id in ids(Some(m), "tool_use", "id") {
            if !results_next.contains(&id) {
                return Err(format!("message {i}: call {id} has no result after it"));
            }
        }
        let calls_before = ids(i.checked_sub(1).map(|p| &q.messages[p]), "tool_use", "id");
        for id in ids(Some(m), "tool_result", "tool_use_id") {
            if !calls_before.contains(&id) {
                return Err(format!("message {i}: result {id} has no call before it"));
            }
        }
    }
    Ok(())
}

fn first_text(q: &ProviderRequest) -> String {
    q.messages[0]["content"][0]["text"]
        .as_str()
        .unwrap_or_default()
        .to_string()
}

/// 300 large results in one turn on a window that holds about 30: old
/// results cleared, then compactions inside the turn, the instruction
/// verbatim at the top after both, every call closed, no overage, and the
/// turn ends with the model's answer.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_single_instruction_reads_300_large_results_without_an_overage() {
    // A window of 80,000 holds about 25 of these exchanges beside the
    // catalog's definitions: a pass keeps the newest results within its
    // token keep (theseus-5lzox: the window was 100,000 when the newest 20
    // exchanges stayed whole), so passes come every dozen loops or so, and
    // the stubs they leave fill the window for two compactions.
    // 80,000 until the three p3h rows joined the catalog together (theseus-z9mze.2, .3, theseus-o1h3f.1: 80,000
    // to 81,000 clear 30 passes or more; 82,000 to 86,000 hold, 87,000 and up do not: a sweep at their join).
    // 82,000 until the persona's "Act, then tell" sentence joined the prefix (theseus-dfoug: +39 tokens; 81,000
    // and 82,000 clear 30 passes or more; 83,000 to 86,000 hold, 87,000 does not: a sweep at its join).
    let r = rig(300, 2_500, 84_000, None);
    let (sid, result) = turn(&r.core, INSTRUCTION).await;
    let result = result.expect("the turn ends as its model ends it");
    assert!(result
        .output
        .ends_with("All the logs agree with the tide table."));
    assert_eq!(result.stop_reason, "no_tool_calls");
    assert!(rows(&r.core, "context.overage").is_empty());

    let compacted: Vec<Value> = rows(&r.core, "context.compacted")
        .into_iter()
        .filter(|c| c["outcome"] == "compaction")
        .collect();
    assert!(compacted.len() >= 2, "{} compactions", compacted.len());
    let cleared = rows(&r.core, "context.cleared");
    assert!(!cleared.is_empty(), "a clearing pass");
    // One row a pass: each names a stored compilation made by it.
    let comps = r.core.store.session_compilations(&sid).unwrap();
    for row in &cleared {
        let id = row["compilation_id"].as_str().unwrap();
        let c = comps.iter().find(|c| c.id == id).expect("stored");
        assert!(
            c.budget
                .as_ref()
                .unwrap()
                .dropped
                .iter()
                .any(|d| d.tier == "cleared"),
            "{id}"
        );
        assert!(row["results"].as_u64().unwrap() > 0);
    }
    assert!(cleared.len() < 300 / 10, "{} passes: rare", cleared.len());

    // Every request: the instruction first, verbatim; no call open.
    let sent = r.model.requests();
    assert_eq!(sent.len(), 301);
    for (k, q) in sent.iter().enumerate() {
        assert_eq!(first_text(q), INSTRUCTION, "request {k}");
        closed(q).unwrap_or_else(|e| panic!("request {k}: {e}"));
    }
    // The summarizer heard what the work in progress is, not the operator.
    let asked = r.glm.requests();
    assert!(!asked.is_empty());
    let system = asked[0].system[0]["text"].as_str().unwrap();
    assert!(system.contains("what is left to do"), "{system}");
    // A summary renders below the instruction in the last request.
    let last = sent.last().unwrap();
    let text = last.messages[0]["content"][1]["text"].as_str().unwrap();
    assert!(text.starts_with("[Summary of "), "{text}");
}

/// `[context] working_window_tokens` lowers the window: the same turn on a
/// 1M catalog window compacts against the working window.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_working_window_lowers_the_budget() {
    let r = rig(80, 3_000, 1_000_000, Some(40_000));
    let (sid, result) = turn(&r.core, INSTRUCTION).await;
    result.expect("the turn ends");
    let comps = r.core.store.session_compilations(&sid).unwrap();
    let c = comps.last().unwrap();
    assert_eq!(c.manifest.context_window, Some(40_000));
    assert_eq!(
        c.budget.as_ref().unwrap().limit_tokens,
        40_000 - u64::from(OUTPUT) - 4_096
    );
    assert!(rows(&r.core, "context.overage").is_empty());
    assert!(!rows(&r.core, "context.compacted").is_empty());
    for q in r.model.requests() {
        assert_eq!(first_text(&q), INSTRUCTION);
    }
}

/// The turn's result carries how full the last request was and the window it
/// compiled against (theseus-wy6v.2): the catalog's, or the working window
/// when one lowers it.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_result_carries_the_last_requests_tokens_and_its_window() {
    for (catalog, working, window) in [
        (1_000_000, None, 1_000_000),
        (1_000_000, Some(40_000), 40_000),
    ] {
        let r = rig(1, 3_000, catalog, working);
        let (_, result) = turn(&r.core, INSTRUCTION).await;
        let result = result.expect("the turn ends");
        assert_eq!(result.context_window, Some(window));
        let tokens = result.context_tokens.expect("a request was compiled");
        assert!(tokens > 0 && tokens < window, "{tokens}");
        // The second loop's request holds the log the first read.
        let first = rows(&r.core, "context.compiled");
        assert!(first.len() >= 2, "two loops compiled");
    }
}
