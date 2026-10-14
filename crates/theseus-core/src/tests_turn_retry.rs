//! A retried turn answers the message that waits, and a continuation that made
//! no model call posts no reply (theseus-posww):
//! - a message with only a recall's note or a compaction's summary after it
//!   still waits (each is written by the turn that failed, before its call),
//!   so its retry calls the model and answers;
//! - a continuation that finds nothing new posts nothing to the place, unless
//!   a person waits on it (the session is in a run of failed turns): then one
//!   line says why nothing was done.

use std::path::Path;
use std::sync::Arc;

use serde_json::Value;
use theseus_protocol::{SessionKind, TurnSubmitResult};

use crate::bus::EventSink;
use crate::config::MemoryMode;
use crate::node::{Body, Node};
use crate::provider::{FakeProvider, ProviderError, Scripted};
use crate::session::{Failing, SessionRecord};
use crate::store::Store;
use crate::tests_recall::index_of;
use crate::turn::TurnRequest;
use crate::{Config, Core};

const HERON: &str = "Remember: the grey heron nests by the old weir at Millbrook.";
const PLACE: &str = "discord:dm:7";

fn build(dir: &Path, model: Arc<FakeProvider>) -> Arc<Core> {
    let mut cfg = Config::example();
    cfg.server.state_dir = dir.to_string_lossy().into_owned();
    cfg.tools.roots = vec![];
    cfg.memory.mode = MemoryMode::Canary;
    cfg.memory.canary_fraction = 1.0;
    let store = Store::open(&dir.join("store")).unwrap();
    Core::build(crate::rpc::Parts::for_tests(cfg, model, store)).unwrap()
}

/// A conversation bound to a DM, with `said` already written to it.
fn session(core: &Core, said: &[&str], bound: bool) -> String {
    let rec = SessionRecord::new(SessionKind::Conversation, None);
    core.store.put_session(&rec.session_id, &rec).unwrap();
    for s in said {
        let n = Node::user(&rec.session_id, None, "test", s);
        core.store.append(&[n.record().unwrap()]).unwrap();
    }
    if bound {
        core.outbox.bind_place("dm:7", &rec.session_id).unwrap();
        core.runner.place_rule.bind_one(crate::places::BoundPlace {
            target: PLACE.into(),
            name: "DM".into(),
            private: false,
            ..Default::default()
        });
    }
    rec.session_id
}

async fn run(core: &Arc<Core>, sid: &str, input: Option<&str>) -> anyhow::Result<TurnSubmitResult> {
    let rec: SessionRecord = core.store.get_session(sid).unwrap().unwrap();
    let (live, _) = core.live_profile();
    let target = core.runner.resolve_target(&live, None, None, None)?;
    core.runner
        .run(TurnRequest {
            prompt: None,
            surface: None,
            session: rec,
            input: input.map(str::to_string),
            target,
            sink: EventSink::new(core.bus.clone(), sid, None),
            author: input.map_or("harness", |_| "test").into(),
            recompile: None,
            attachments: vec![],
            arrived: None,
            reply_to: None,
        })
        .await
}

/// The bodies of the place's posts of `kind` not yet delivered.
fn posts(core: &Core, kind: &str) -> Vec<Value> {
    core.outbox
        .open_for(PLACE)
        .iter()
        .filter(|a| crate::outbox::kind_of(a) == kind)
        .map(|a| crate::outbox::body_of(a).clone())
        .collect()
}

/// A continuation of the session with nothing new for it: the execution is
/// woken, as the driver's retry of a failed turn wakes it.
async fn nudged(core: &Arc<Core>, sid: &str) -> TurnSubmitResult {
    let rec: SessionRecord = core.store.get_session(sid).unwrap().unwrap();
    let exec = rec.execution_id.unwrap();
    core.kernel.wake(&exec, "test").unwrap();
    core.continue_execution(&exec).await.unwrap().unwrap()
}

fn overloaded() -> Scripted {
    Scripted::Fail(ProviderError::Overloaded {
        message: "overloaded_error".into(),
    })
}

/// The owner's 2026-10-11 case: a turn recalls, then fails at its provider
/// call; the recall's note is the last node, after the message. The retry
/// must call the model and answer the message, not end `nothing_new`.
#[tokio::test]
async fn a_retry_answers_the_message_a_recall_note_follows() {
    let dir = tempfile::tempdir().unwrap();
    let model = Arc::new(FakeProvider::scripted(vec![
        overloaded(),
        Scripted::text("By the old weir."),
    ]));
    let core = build(dir.path(), model.clone());
    let src = session(&core, &[HERON], false);
    core.runner.memory.set_ask(index_of(&core, vec![src]));
    let sid = session(&core, &[], true);
    run(&core, &sid, Some("Where does the grey heron nest?"))
        .await
        .expect_err("the call fails");
    let nodes = core.store.session_nodes(&sid).unwrap();
    assert!(
        matches!(nodes.last().unwrap().1.body, Body::Recall { .. }),
        "the recall's note is the last node"
    );
    let rec: SessionRecord = core.store.get_session(&sid).unwrap().unwrap();
    let exec = rec.execution_id.clone().unwrap();
    let retry = core.continue_execution(&exec).await.unwrap().unwrap();
    assert!(retry.continuation);
    assert_eq!(retry.stop_reason, "no_tool_calls");
    assert_eq!(
        (retry.loops, retry.output.as_str()),
        (1, "By the old weir.")
    );
    assert_eq!(model.requests().len(), 2, "the retry called the model");
}

/// A compaction inside the turn that failed (theseus-3ki0) writes its summary
/// after the message: the summary is no reply either, and the retry answers.
#[tokio::test]
async fn a_retry_answers_the_message_a_summary_follows() {
    let dir = tempfile::tempdir().unwrap();
    let model = Arc::new(FakeProvider::scripted(vec![
        overloaded(),
        Scripted::text("By the old weir."),
    ]));
    let core = build(dir.path(), model.clone());
    let sid = session(&core, &["The herons came back in March."], true);
    run(&core, &sid, Some("Where does the grey heron nest?"))
        .await
        .expect_err("the call fails");
    let first = core.store.session_nodes(&sid).unwrap()[0].0;
    let summary = Node::summary(
        &sid,
        "t-compaction",
        Body::Summary {
            first,
            last: first,
            nodes: 1,
            text: "The herons came back in March.".into(),
            profile: "session".into(),
            model: "fake".into(),
            cost_usd: None,
            header: "[Summary of 1 earlier message]".into(),
        },
    );
    core.store.append(&[summary.record().unwrap()]).unwrap();
    let nodes = core.store.session_nodes(&sid).unwrap();
    assert!(
        matches!(nodes.last().unwrap().1.body, Body::Summary { .. }),
        "the summary is the last node"
    );
    let rec: SessionRecord = core.store.get_session(&sid).unwrap().unwrap();
    let exec = rec.execution_id.clone().unwrap();
    let retry = core.continue_execution(&exec).await.unwrap().unwrap();
    assert_eq!(
        (
            retry.stop_reason.as_str(),
            retry.loops,
            retry.output.as_str()
        ),
        ("no_tool_calls", 1, "By the old weir.")
    );
    assert_eq!(model.requests().len(), 2, "the retry called the model");
}

/// A continuation that finds nothing new posts nothing where no one waits.
#[tokio::test]
async fn a_continuation_with_nothing_new_posts_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let model = Arc::new(FakeProvider::scripted(vec![Scripted::text("Four.")]));
    let core = build(dir.path(), model.clone());
    let sid = session(&core, &[], true);
    run(&core, &sid, Some("What is two and two?"))
        .await
        .unwrap();
    assert_eq!(posts(&core, "reply").len(), 1);
    let again = nudged(&core, &sid).await;
    assert_eq!(
        (again.stop_reason.as_str(), again.loops),
        ("nothing_new", 0)
    );
    assert_eq!(posts(&core, "reply").len(), 1, "no footer-only reply");
    assert!(posts(&core, "notice").is_empty());
    assert_eq!(model.requests().len(), 1);
}

/// The retry of a failed turn that finds nothing to do says so, once, in a
/// line: a person is waiting on it.
#[tokio::test]
async fn a_retry_with_nothing_to_do_says_why_in_one_line() {
    let dir = tempfile::tempdir().unwrap();
    let model = Arc::new(FakeProvider::scripted(vec![Scripted::text("Four.")]));
    let core = build(dir.path(), model.clone());
    let sid = session(&core, &[], true);
    run(&core, &sid, Some("What is two and two?"))
        .await
        .unwrap();
    // A run of failed turns, as a failure after that answer would leave it.
    let (run_of_failures, ..) = Failing::after(None, "invalid_request", false, true, 7, true);
    core.store
        .update_session(&sid, |r| {
            r.failing = Some(run_of_failures);
            Ok(vec![])
        })
        .unwrap();
    let again = nudged(&core, &sid).await;
    assert_eq!(again.stop_reason, "nothing_new");
    assert_eq!(posts(&core, "reply").len(), 1, "no footer-only reply");
    let said = posts(&core, "notice");
    assert_eq!(said.len(), 1, "{said:?}");
    let text = said[0]["text"].as_str().unwrap();
    assert!(text.starts_with("Nothing to retry: "), "{text}");
    assert!(text.contains("invalid_request"), "{text}");
    assert_eq!(text.lines().count(), 1, "one line: {text}");
}
