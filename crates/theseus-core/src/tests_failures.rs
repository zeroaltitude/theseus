//! A failure that will not pass is not retried forever (theseus-ljr), through
//! the whole core with a scripted stand-in provider:
//! - a 400 every time: one retry, then the execution waits on input, with one
//!   notice that says the next message retries, and the next message does;
//! - a 529: the driver's retries go on while it lasts, with one notice, and the
//!   turn answers once the provider recovers;
//! - an internal fault that recurs: one retry, then the execution waits too.
//!
//! The driver's backoff itself is timed against a real daemon
//! (`theseusd/tests/failures.rs`).

use std::path::Path;
use std::sync::Arc;

use serde_json::{json, Value};
use theseus_kernel::{ExecState, Wake};
use theseus_protocol::{SessionKind, TurnSubmitResult};

use crate::bus::EventSink;
use crate::provider::{FakeProvider, ProviderError, Scripted};
use crate::session::{SessionRecord, Then};
use crate::store::Store;
use crate::turn::{TurnError, TurnRequest};
use crate::{Config, Core};

struct Rig {
    core: Arc<Core>,
    fake: Arc<FakeProvider>,
    _dir: tempfile::TempDir,
}

fn config(root: &Path, state: &Path) -> Config {
    let mut cfg = Config::example();
    cfg.server.state_dir = state.to_string_lossy().into_owned();
    cfg.tools.projects_dir = Some(root.to_string_lossy().into_owned());
    cfg.tools.roots = vec![];
    cfg
}

fn rig(script: Vec<Scripted>) -> Rig {
    rig_with(script, |_| {})
}

fn rig_with(script: Vec<Scripted>, tweak: impl FnOnce(&mut Config)) -> Rig {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("work");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(root.join("harbor.txt"), "the tide turns at four\n").unwrap();
    let mut cfg = config(&root.canonicalize().unwrap(), dir.path());
    tweak(&mut cfg);
    let store = Store::open(&dir.path().join("store")).unwrap();
    let fake = Arc::new(FakeProvider::scripted(script));
    let core = Core::build(crate::rpc::Parts::for_tests(cfg, fake.clone(), store)).unwrap();
    Rig {
        core,
        fake,
        _dir: dir,
    }
}

/// A conversation bound to a place, so that its notices are outbox posts.
fn bound_session(core: &Core) -> String {
    let rec = SessionRecord::new(SessionKind::Conversation, None);
    core.store.put_session(&rec.session_id, &rec).unwrap();
    core.outbox.bind_place("dm:7", &rec.session_id).unwrap();
    // A DM's person is its owner once the binding binds it (the place rule).
    core.runner.place_rule.bind_one(crate::places::BoundPlace {
        target: "discord:dm:7".into(),
        name: "DM".into(),
        private: false,
        ..Default::default()
    });
    rec.session_id
}

async fn turn(core: &Arc<Core>, sid: &str, input: &str) -> anyhow::Result<TurnSubmitResult> {
    let rec: SessionRecord = core.store.get_session(sid).unwrap().unwrap();
    let (live, _) = core.live_profile();
    let target = core.runner.resolve_target(&live, None, None, None)?;
    let sink = EventSink::new(core.bus.clone(), sid, None);
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
}

/// The failed-turn notices the session's place has been sent.
fn notices(core: &Core) -> Vec<Value> {
    core.outbox
        .open_for("discord:dm:7")
        .iter()
        .filter(|a| crate::outbox::kind_of(a) == "failed")
        .map(|a| crate::outbox::body_of(a).clone())
        .collect()
}

/// The ledger rows of `kind`, oldest first.
fn rows(core: &Core, kind: &str) -> Vec<Value> {
    let rows: Vec<(u64, crate::ledger::LedgerRow)> = core.store.ledger_tail(500).unwrap();
    rows.into_iter()
        .filter(|(_, r)| r.kind == kind)
        .map(|(_, r)| r.data)
        .collect()
}

fn thens(core: &Core) -> Vec<String> {
    rows(core, "turn.next")
        .iter()
        .map(|r| r["then"].as_str().unwrap().to_string())
        .collect()
}

fn failing(core: &Core, sid: &str) -> Option<crate::session::Failing> {
    core.store
        .get_session::<SessionRecord>(sid)
        .unwrap()
        .unwrap()
        .failing
}

fn bad_request() -> Scripted {
    Scripted::Fail(ProviderError::InvalidRequest {
        status: 400,
        message: "invalid_request_error: model: the model claude-lighthouse-9 is not served".into(),
    })
}

fn overloaded() -> Scripted {
    Scripted::Fail(ProviderError::Overloaded {
        message: "overloaded_error: Overloaded".into(),
    })
}

/// A 400 every time. The input turn's failure gets one retry, which fails
/// too: the execution then waits on input, and the place gets one notice,
/// which says the next message retries. Before theseus-ljr the driver
/// retried it every few minutes forever, with a notice each time.
#[tokio::test]
async fn a_failure_that_will_not_pass_is_retried_once_then_waits_on_the_next_message() {
    let r = rig(vec![
        bad_request(),
        bad_request(),
        Scripted::text("Back on course."),
    ]);
    let sid = bound_session(&r.core);
    let err = turn(&r.core, &sid, "plot the harbor tides")
        .await
        .expect_err("the provider refuses it");
    assert_eq!(
        err.downcast_ref::<TurnError>().unwrap().class,
        "invalid_request"
    );
    let rec: SessionRecord = r.core.store.get_session(&sid).unwrap().unwrap();
    let exec = rec.execution_id.clone().unwrap();
    let e = r.core.kernel.execution(&exec).unwrap().unwrap();
    assert_eq!(
        (e.state, e.resume_pending),
        (ExecState::Queued, true),
        "one retry is queued"
    );
    assert!(notices(&r.core).is_empty(), "the retry is silent");

    let retry = r.core.continue_execution(&exec).await;
    assert!(retry.is_err(), "the retry fails the same way");
    let e = r.core.kernel.execution(&exec).unwrap().unwrap();
    assert_eq!(
        (e.state, e.wake.clone(), e.resume_pending),
        (ExecState::Waiting, Some(Wake::Input), false),
        "parked on input: nothing for the driver"
    );
    let n = notices(&r.core);
    assert_eq!(n.len(), 1, "one notice for the run: {n:?}");
    assert_eq!(
        (&n[0]["class"], &n[0]["then"], &n[0]["turns"]),
        (&json!("invalid_request"), &json!("park"), &json!(2))
    );
    assert!(
        n[0]["error"]
            .as_str()
            .unwrap()
            .contains("claude-lighthouse-9 is not served"),
        "it says what failed: {}",
        n[0]
    );
    assert_eq!(thens(&r.core), ["retry", "park"]);
    assert_eq!(rows(&r.core, "turn.failed").len(), 2);
    assert_eq!(r.fake.requests().len(), 2);
    let run = failing(&r.core, &sid).unwrap();
    assert_eq!((run.turns, run.lasting, run.parked), (2, 2, true));
    let not_ready = r.core.continue_execution(&exec).await.unwrap_err();
    assert!(
        format!("{not_ready:#}").contains("not ready for a continuation turn"),
        "a parked execution is not the driver's: {not_ready:#}"
    );
    assert_eq!(r.fake.requests().len(), 2);

    // The next message retries it.
    let res = turn(&r.core, &sid, "try the tides again").await.unwrap();
    assert_eq!(res.output, "Back on course.");
    assert_eq!(r.fake.requests().len(), 3);
    assert_eq!(failing(&r.core, &sid), None, "the answer ended the run");
    assert_eq!(notices(&r.core).len(), 1);
}

/// A 529 three times, then an answer: every retry is queued for the driver,
/// whose backoff spaces them, and only the run's first failure posts a
/// notice, which says it retries with backoff.
#[tokio::test]
async fn a_transient_failure_keeps_its_retries_and_answers_once_the_provider_recovers() {
    let r = rig(vec![
        overloaded(),
        overloaded(),
        overloaded(),
        Scripted::text("The tide turns at four."),
    ]);
    let sid = bound_session(&r.core);
    assert!(turn(&r.core, &sid, "when does the tide turn?")
        .await
        .is_err());
    let rec: SessionRecord = r.core.store.get_session(&sid).unwrap().unwrap();
    let exec = rec.execution_id.clone().unwrap();
    for attempt in 2..=3 {
        let e = r.core.kernel.execution(&exec).unwrap().unwrap();
        assert_eq!(
            (e.state, e.resume_pending),
            (ExecState::Queued, true),
            "attempt {attempt} is queued"
        );
        assert!(r.core.continue_execution(&exec).await.is_err());
    }
    let n = notices(&r.core);
    assert_eq!(n.len(), 1, "{n:?}");
    assert_eq!(
        (&n[0]["then"], &n[0]["turns"]),
        (&json!("backoff"), &json!(1))
    );
    let answered = r.core.continue_execution(&exec).await.unwrap().unwrap();
    assert_eq!(answered.output, "The tide turns at four.");
    assert_eq!(thens(&r.core), ["backoff"; 3]);
    assert_eq!(failing(&r.core, &sid), None);
    assert_eq!(notices(&r.core).len(), 1);
    assert_eq!(r.fake.requests().len(), 4);
}

/// An internal fault (a frame the store would not write) after the model
/// asked for a read: the fault gets one retry, and when that faults too the
/// execution waits on input, with the run's one notice. Before theseus-ljr
/// a fault that recurred was retried forever.
#[tokio::test]
async fn an_internal_fault_that_recurs_is_retried_once_then_waits() {
    let r = rig(vec![Scripted::tools(
        "Reading it.",
        &[("t1", "fs_read", json!({"path": "harbor.txt"}))],
    )]);
    let fs_frame = |records: &[theseus_store::NewRecord]| {
        records.iter().any(|r| {
            r.kind == theseus_store::kinds::ACTION
                && serde_json::from_slice::<Value>(&r.payload)
                    .is_ok_and(|a| a["tool"].as_str().is_some_and(|t| t.starts_with("fs")))
        })
    };
    let sid = bound_session(&r.core);
    r.core.store.fail_turn_frame(fs_frame);
    let err = turn(&r.core, &sid, "what does harbor.txt say?")
        .await
        .expect_err("the turn faults");
    assert!(format!("{err:#}").contains("an injected fault"), "{err:#}");
    let rec: SessionRecord = r.core.store.get_session(&sid).unwrap().unwrap();
    let exec = rec.execution_id.clone().unwrap();
    let e = r.core.kernel.execution(&exec).unwrap().unwrap();
    assert_eq!((e.state, e.resume_pending), (ExecState::Queued, true));
    assert_eq!(thens(&r.core), ["retry"]);
    assert!(notices(&r.core).is_empty());

    r.core.store.fail_turn_frame(fs_frame);
    assert!(
        r.core.continue_execution(&exec).await.is_err(),
        "it faults again"
    );
    let e = r.core.kernel.execution(&exec).unwrap().unwrap();
    assert_eq!(
        (e.state, e.resume_pending),
        (ExecState::Waiting, false),
        "parked: {e:?}"
    );
    assert_eq!(thens(&r.core), ["retry", "park"]);
    let n = notices(&r.core);
    assert_eq!(n.len(), 1, "{n:?}");
    assert_eq!(n[0]["then"], "park");
    assert_eq!(failing(&r.core, &sid).unwrap().class, "internal");
    // The rule's own name for what follows a park.
    assert_eq!(Then::Park.as_str(), "park");
}

/// `[model.retries]` (theseus-7gir.21): a call that fails with a class that
/// passes with time is made again inside its turn, at most `transient`
/// times, each wait a span on the turn's trace; past them the turn fails as
/// before, to the driver's backoff. A class that will not pass is never made
/// again there.
#[tokio::test]
async fn a_transient_failure_is_retried_inside_its_turn_as_the_config_allows() {
    let retrying = |transient, script| {
        rig_with(script, |c| {
            c.model.retries = crate::config::Retries {
                transient,
                backoff_ms: 5,
                backoff_max_ms: 5,
                ..Default::default()
            }
        })
    };
    // Two 529s, then the answer: two retries, one turn.
    let r = retrying(2, vec![overloaded(), overloaded(), Scripted::text("Done.")]);
    let sid = bound_session(&r.core);
    let res = turn(&r.core, &sid, "say done").await.expect("it answers");
    assert_eq!(res.output, "Done.");
    assert_eq!(r.fake.requests.lock().unwrap().len(), 3);
    assert_eq!(rows(&r.core, "provider.error").len(), 2);
    assert!(thens(&r.core).is_empty(), "no turn failed");
    let trace = res.trace.expect("the turn's trace");
    let spans = trace.children.iter().flat_map(|l| &l.children);
    assert_eq!(spans.filter(|s| s.name == "retry").count(), 2);
    // Past `transient`, the turn fails to the driver's backoff, as before.
    let r = retrying(1, vec![overloaded(), overloaded(), Scripted::text("Done.")]);
    let sid = bound_session(&r.core);
    turn(&r.core, &sid, "say done").await.expect_err("it fails");
    assert_eq!(r.fake.requests.lock().unwrap().len(), 2);
    assert_eq!(thens(&r.core), ["backoff"]);
    // A 400 is never made again inside the turn.
    let r = retrying(2, vec![bad_request(), Scripted::text("Done.")]);
    let sid = bound_session(&r.core);
    turn(&r.core, &sid, "say done").await.expect_err("it fails");
    assert_eq!(r.fake.requests.lock().unwrap().len(), 1);
}
