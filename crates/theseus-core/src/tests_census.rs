//! `context.census` (theseus-7m7w.1): the next request counted part by part. The parts add up to the
//! compile's own total, each tool's line is its definition's bytes, the system and the tools are the ones a
//! turn then sends, the use is the ledger's, and the answer is stable.

use std::sync::Arc;

use serde_json::{json, Value};
use theseus_protocol::census::{ContextCensusParams, ContextCensusResult, PLACES};
use theseus_protocol::{SessionKind, TurnSubmitResult};

use crate::bus::EventSink;
use crate::provider::{FakeProvider, ProviderRequest, Scripted};
use crate::session::SessionRecord;
use crate::store::Store;
use crate::turn::TurnRequest;
use crate::{Config, Core};

struct Rig {
    core: Arc<Core>,
    fake: Arc<FakeProvider>,
    dir: tempfile::TempDir,
}

fn rig(script: Vec<Scripted>) -> Rig {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("work");
    std::fs::create_dir_all(&root).unwrap();
    let root = root.canonicalize().unwrap();
    std::fs::write(root.join("RULES.md"), "Keep the tide tables dry.\n").unwrap();
    let mut cfg = Config::example();
    cfg.server.state_dir = dir.path().to_string_lossy().into_owned();
    cfg.tools.projects_dir = Some(root.to_string_lossy().into_owned());
    cfg.tools.roots = vec![];
    cfg.context.files = vec![format!("{}/RULES.md", root.display()).into()];
    let store = Store::open(&dir.path().join("store")).unwrap();
    let fake = Arc::new(FakeProvider::scripted(script));
    let core = Core::build(crate::rpc::Parts::for_tests(cfg, fake.clone(), store)).unwrap();
    Rig { core, fake, dir }
}

fn census(
    core: &Core,
    session: Option<&str>,
    place: Option<&str>,
) -> (ContextCensusResult, ProviderRequest) {
    let (r, req, _) = core
        .context_census(&ContextCensusParams {
            session_id: session.map(str::to_string),
            place: place.map(str::to_string),
            exact: false,
        })
        .unwrap();
    (r, req)
}

async fn turn(core: &Arc<Core>, session: Option<&str>, input: &str) -> TurnSubmitResult {
    let rec = match session {
        Some(id) => core
            .store
            .get_session::<SessionRecord>(id)
            .unwrap()
            .unwrap(),
        None => {
            let r = SessionRecord::new(SessionKind::Conversation, None);
            core.store.put_session(&r.session_id, &r).unwrap();
            r
        }
    };
    let (live, _) = core.live_profile();
    let target = core.runner.resolve_target(&live, None, None, None).unwrap();
    let sink = EventSink::new(core.bus.clone(), &rec.session_id, None);
    core.runner
        .run(TurnRequest {
            prompt: None,
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
        .unwrap()
}

fn json_len(v: &Value) -> u64 {
    serde_json::to_string(v).unwrap().len() as u64
}

/// The parts of every kind of place add up to the request's total, which is the compile's own estimate.
#[test]
fn the_parts_add_up_to_the_compiles_total() {
    let r = rig(vec![]);
    for place in PLACES {
        let (c, req) = census(&r.core, None, Some(place));
        assert_eq!(
            c.parts_sum(),
            c.total_tokens,
            "{place}: system + tools + messages + images"
        );
        assert_eq!(
            c.total_tokens,
            req.estimate_tokens(),
            "{place}: the request's estimate"
        );
        assert_eq!(
            c.total_tokens, c.compile_tokens,
            "{place}: the compile's figure"
        );
        let blocks: u64 = c.system.iter().map(|b| b.tokens).sum();
        assert_eq!(blocks, c.system_tokens, "{place}");
        for b in &c.system {
            let parts: u64 = b.parts.iter().map(|p| p.tokens).sum();
            assert_eq!(parts + b.framing, b.tokens, "{place} {}", b.block);
            let bytes: u64 = b.parts.iter().map(|p| p.bytes).sum();
            assert_eq!(
                bytes, b.bytes,
                "{place} {}: the sections cover the block",
                b.block
            );
        }
        let tools: u64 = c.tools.iter().map(|t| t.tokens).sum();
        assert_eq!(tools, c.tools_tokens, "{place}");
        assert!(c.tools_tokens > 0 && c.system_tokens > 0, "{place}");
        assert_eq!(
            c.messages_tokens, 0,
            "{place}: a fresh session has no messages"
        );
    }
}

/// Each tool's line is its definition's bytes: the description's, the schema's, and the rest, and its tokens
/// are its three parts'.
#[test]
fn each_tools_line_matches_its_definition_s_bytes() {
    let r = rig(vec![]);
    let (c, req) = census(&r.core, None, None);
    assert_eq!(
        c.tools.len(),
        req.tools.len(),
        "a tool left out of the census"
    );
    for (line, def) in c.tools.iter().zip(&req.tools) {
        assert_eq!(line.name, def["name"].as_str().unwrap());
        assert_eq!(
            line.schema_bytes,
            json_len(&def["input_schema"]),
            "{}",
            line.name
        );
        assert_eq!(
            line.description_bytes,
            json_len(&def["description"]),
            "{}",
            line.name
        );
        assert_eq!(
            line.bytes,
            json_len(def),
            "{}: the definition as the request carries it",
            line.name
        );
        assert!(
            line.description_tokens + line.schema_tokens <= line.tokens,
            "{}",
            line.name
        );
    }
    let total: u64 = c.tools.iter().map(|t| t.bytes).sum();
    assert_eq!(
        total,
        json_len(&Value::Array(req.tools.clone())) - (req.tools.len() as u64 - 1) - 2
    );
}

/// What the census compiles is what a turn then sends: the same system blocks and the same tools, byte for
/// byte, and the turn's request is the census's plus its message.
#[tokio::test]
async fn a_fresh_census_is_the_first_request_a_turn_sends() {
    let r = rig(vec![Scripted::text("Aye.")]);
    let (_, ahead) = census(&r.core, None, Some("cli"));
    turn(&r.core, None, "Where is the harbour?").await;
    let sent = r.fake.requests().remove(0);
    assert_eq!(sent.system, ahead.system, "the system blocks");
    assert_eq!(sent.tools, ahead.tools, "the tools");
}

/// A session's next request: its history in the messages, appended to its compilation.
#[tokio::test]
async fn a_sessions_next_request_counts_its_history() {
    let r = rig(vec![Scripted::text("Aye."), Scripted::text("Aye again.")]);
    let first = turn(&r.core, None, "Where is the harbour?").await;
    let sid = first.session_id;
    let (c, req) = census(&r.core, Some(&sid), None);
    assert_eq!(c.place, "session");
    assert_eq!(c.session_id.as_deref(), Some(sid.as_str()));
    assert_eq!(c.compile, "append");
    assert!(c.messages_tokens > 0, "{c:?}");
    assert_eq!(c.parts_sum(), c.total_tokens);
    assert_eq!(req.messages.len(), 2, "the question and the answer");
}

/// Tools are counted in the ledger by what was planned, with the last call's time.
#[tokio::test]
async fn a_tools_use_is_the_ledgers_calls_in_the_window() {
    let work = rig(vec![
        Scripted::tools("", &[("t1", "fs_list", json!({"path": "."}))]),
        Scripted::text("Done."),
    ]);
    let (before, _) = census(&work.core, None, None);
    assert!(before.use_unread.is_none(), "{:?}", before.use_unread);
    assert!(before
        .tools
        .iter()
        .all(|t| t.calls == 0 && t.last_call_ms.is_none()));
    turn(&work.core, None, "List the dock.").await;
    let (after, _) = census(&work.core, None, None);
    let list = after.tools.iter().find(|t| t.name == "fs_list").unwrap();
    assert_eq!(list.calls, 1, "{list:?}");
    assert!(list.last_call_ms.is_some());
    assert_eq!(
        after.tools.iter().map(|t| t.calls).sum::<u64>(),
        1,
        "no other tool was called"
    );
    let _ = &work.dir;
}

/// The answer is the same twice (but for its time) and its JSON has the same keys.
#[test]
fn the_answer_is_stable() {
    let r = rig(vec![]);
    let mut a = serde_json::to_value(census(&r.core, None, None).0).unwrap();
    let mut b = serde_json::to_value(census(&r.core, None, None).0).unwrap();
    for v in [&mut a, &mut b] {
        v["ms"] = json!(0);
        v["use_since_ms"] = json!(0);
    }
    assert_eq!(a, b);
    let keys: Vec<&str> = a.as_object().unwrap().keys().map(String::as_str).collect();
    for k in [
        "place",
        "class",
        "model",
        "compile",
        "system",
        "tools",
        "total_tokens",
        "tools_tokens",
    ] {
        assert!(keys.contains(&k), "{k} in {keys:?}");
    }
}

/// A place the census does not know, and a session that takes no turn, are refused with words.
#[test]
fn a_place_or_session_it_cannot_count_is_refused() {
    let r = rig(vec![]);
    let e = r
        .core
        .context_census(&ContextCensusParams {
            place: Some("moon".into()),
            ..ContextCensusParams::default()
        })
        .err()
        .unwrap()
        .to_string();
    assert!(
        e.contains("no place kind") && e.contains("cli, dm, task, harness"),
        "{e}"
    );
    let e = r
        .core
        .context_census(&ContextCensusParams {
            session_id: Some("ses_nobody".into()),
            ..ContextCensusParams::default()
        })
        .err()
        .unwrap()
        .to_string();
    assert!(e.contains("no session"), "{e}");
}

/// `exact` asks the provider to count and reads the counts back; a provider that cannot says why.
#[tokio::test]
async fn exact_asks_the_provider_to_count() {
    let r = rig(vec![]);
    let c = r
        .core
        .context_census_exact(ContextCensusParams {
            exact: true,
            ..ContextCensusParams::default()
        })
        .await
        .unwrap();
    let e = c.exact.expect("the fake provider counts by the estimate");
    assert_eq!(e.total, c.total_tokens);
    assert!(e.tools > 0 && e.system > 0);
    assert!(c.exact_error.is_none());
    let plain = r
        .core
        .context_census_exact(ContextCensusParams::default())
        .await
        .unwrap();
    assert!(plain.exact.is_none() && plain.exact_error.is_none());
}
