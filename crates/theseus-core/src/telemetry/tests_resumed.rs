//! The calls a later turn answers (theseus-8pei): a confirmed call's run,
//! in the continuation, and a background job's late result, each traced in
//! the turn that answered it, through whole cores whose telemetry posts to
//! a receiver.

use std::sync::Arc;
use std::time::Duration;

use serde_json::json;
use theseus_protocol::{SessionKind, Span, TurnSubmitResult};

use super::tests::{flushed, last_metrics, pipeline, point_with, points_of, tuning, Receiver};
use crate::bus::EventSink;
use crate::policy::Posture;
use crate::provider::{FakeProvider, Scripted};
use crate::session::SessionRecord;
use crate::turn::TurnRequest;
use crate::{Config, Core};

struct Rig {
    core: Arc<Core>,
    _dir: tempfile::TempDir,
}

/// A core whose provider follows `script` (then "fake reply"), whose tools
/// work in a scratch folder, and whose telemetry posts to `endpoint`; `tweak`
/// sets the rest of its config.
fn rig(script: Vec<Scripted>, endpoint: &str, tweak: impl FnOnce(&mut Config)) -> Rig {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("work");
    std::fs::create_dir_all(&root).unwrap();
    let root = root.canonicalize().unwrap();
    let mut c = Config::example();
    c.server.state_dir = dir.path().to_string_lossy().into_owned();
    c.tools.projects_dir = Some(root.to_string_lossy().into_owned());
    c.tools.roots = vec![];
    tweak(&mut c);
    let store = crate::store::Store::open(&dir.path().join("store")).unwrap();
    let fake = Arc::new(FakeProvider::scripted(script));
    let core = Core::build(crate::rpc::Parts {
        telemetry: Some(pipeline(endpoint, None, tuning())),
        ..crate::rpc::Parts::for_tests(c, fake, store)
    })
    .unwrap();
    Rig { core, _dir: dir }
}

/// One input turn in a new session, counted in telemetry as `turn.submit`
/// counts it.
async fn turn(core: &Arc<Core>, input: &str) -> TurnSubmitResult {
    let rec = SessionRecord::new(SessionKind::Conversation, None);
    core.store.put_session(&rec.session_id, &rec).unwrap();
    let (live, _) = core.live_profile();
    let target = core.runner.resolve_target(&live, None, None, None).unwrap();
    let sink = EventSink::new(core.bus.clone(), &rec.session_id, None);
    let res = core
        .runner
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
        .unwrap();
    core.telemetry().record_turn(&res);
    res
}

/// The turn's call waits for the operator: approve it, and run the turn it
/// resumes.
pub(crate) async fn approved(core: &Arc<Core>, res: &TurnSubmitResult) -> TurnSubmitResult {
    assert_eq!(res.stop_reason, "awaiting_confirm", "{res:?}");
    let pending = core.pending_confirms(&res.session_id).unwrap();
    core.confirm_action(&pending[0].correlation_id, true, None, "test")
        .unwrap();
    let exec = res.execution_id.clone().unwrap();
    core.continue_execution(&exec).await.unwrap().unwrap()
}

/// Drain the spool until the job's completion has queued the execution.
async fn wait_queued(core: &Core, exec: &str) {
    for _ in 0..200 {
        tokio::time::sleep(Duration::from_millis(50)).await;
        core.heartbeat("test");
        if core.kernel.execution(exec).unwrap().unwrap().state.as_str() == "queued" {
            return;
        }
    }
    panic!("the job's completion never queued the execution");
}

/// Every span named `name` in the tree, with its parent's name.
pub(crate) fn spans_named<'a>(root: &'a Span, name: &str) -> Vec<(&'a str, &'a Span)> {
    fn walk<'a>(s: &'a Span, name: &str, out: &mut Vec<(&'a str, &'a Span)>) {
        for c in &s.children {
            if c.name == name {
                out.push((s.name.as_str(), c));
            }
            walk(c, name, out);
        }
    }
    let mut out = Vec::new();
    walk(root, name, &mut out);
    out
}

fn sleeper(id: &str, secs: &str) -> Scripted {
    Scripted::tools(
        "",
        &[(
            id,
            "proc_run",
            json!({"argv": ["sh", "-c", format!("sleep {secs}; echo tide")]}),
        )],
    )
}

/// A proc.run that waits for the operator, approved: the turn it resumes
/// holds the call's span under its continuation's, its result `ok`, timed by
/// its run; the proposing turn's span still says it waited.
#[tokio::test]
async fn a_confirmed_calls_run_is_traced_in_its_continuation() {
    let rx = Receiver::start(vec![]).await;
    let r = rig(
        vec![sleeper("t1", "0.4"), Scripted::text("The tide is in.")],
        &rx.endpoint(),
        |c| c.policy.enforcement = Posture::Approve,
    );
    let first = turn(&r.core, "run the tide script").await;
    let asked = spans_named(first.trace.as_ref().unwrap(), "tool proc_run");
    assert_eq!(asked.len(), 1);
    assert_eq!(asked[0].1.attrs["result"], "awaiting_confirm");

    let cont = approved(&r.core, &first).await;
    assert_eq!(cont.output, "The tide is in.");
    let trace = cont.trace.as_ref().unwrap();
    let ran = spans_named(trace, "tool proc_run");
    assert_eq!(ran.len(), 1, "{trace:#?}");
    let (parent, span) = ran[0];
    assert_eq!(parent, "continuation");
    for (k, v) in [
        ("tool", "proc.run"),
        ("family", "proc"),
        ("backend", "job"),
        ("result", "ok"),
        ("tool_use_id", "t1"),
    ] {
        assert_eq!(span.attrs[k], json!(v), "{k}");
    }
    let ms = span.duration_us() / 1000;
    assert!(
        (400..30_000).contains(&ms),
        "timed by its run: {ms} ms, {span:?}"
    );
}

/// A proc.run that outlives `proc_sync_secs`: its late result, taken by the
/// next turn, is one `tool proc_run` span there, `late`, with the job's run.
#[tokio::test]
async fn a_late_result_is_traced_once_with_its_jobs_run() {
    let rx = Receiver::start(vec![]).await;
    let r = rig(
        vec![sleeper("t1", "1.6"), Scripted::text("It runs on.")],
        &rx.endpoint(),
        |c| {
            c.tools.proc_sync_secs = 1;
            c.policy.tools.insert("proc.run".into(), Posture::Open);
        },
    );
    let first = turn(&r.core, "run the long tide script").await;
    let placed = spans_named(first.trace.as_ref().unwrap(), "tool proc_run");
    assert_eq!(placed.len(), 1, "the call that answered `background`");
    assert_eq!(placed[0].1.attrs["result"], "background");
    let exec = first.execution_id.clone().unwrap();
    wait_queued(&r.core, &exec).await;
    let next = r.core.continue_execution(&exec).await.unwrap().unwrap();
    let trace = next.trace.as_ref().unwrap();
    let late = spans_named(trace, "tool proc_run");
    assert_eq!(late.len(), 1, "{trace:#?}");
    let (parent, span) = late[0];
    assert_eq!(parent, "continuation");
    assert_eq!(span.attrs["late"], json!(true));
    assert_eq!(span.attrs["result"], "ok");
    assert_eq!(span.attrs["tool"], "proc.run");
    let run = span.attrs["run_ms"].as_u64().expect("the job's run");
    assert!((1_600..30_000).contains(&run), "the job's run: {run} ms");
    assert_eq!(span.duration_us(), 0, "a point at its absorption");
}

/// Counted once, at its answer (theseus-8pei): a confirmed call and a
/// background job, across their turns, are one `theseus.tool.calls` point
/// each, `ok`, and the duration holds their runs; nothing is counted
/// `awaiting_confirm` or `background`.
#[tokio::test]
async fn a_confirmed_call_and_a_background_job_are_counted_once_by_their_runs() {
    let rx = Receiver::start(vec![]).await;
    let r = rig(
        vec![
            sleeper("t1", "0.4"),
            Scripted::text("The tide is in."),
            sleeper("t2", "1.6"),
            Scripted::text("It runs on."),
        ],
        &rx.endpoint(),
        |c| {
            c.policy.enforcement = Posture::Approve;
            c.tools.proc_sync_secs = 1;
        },
    );
    let first = turn(&r.core, "run the tide script").await;
    approved(&r.core, &first).await;
    let second = turn(&r.core, "run the long tide script").await;
    let cont = approved(&r.core, &second).await;
    let placed = spans_named(cont.trace.as_ref().unwrap(), "tool proc_run");
    assert_eq!(placed[0].1.attrs["result"], "background", "{placed:?}");
    let exec = second.execution_id.clone().unwrap();
    wait_queued(&r.core, &exec).await;
    r.core.continue_execution(&exec).await.unwrap().unwrap();

    flushed(r.core.telemetry()).await;
    let metrics = last_metrics(&rx.got());
    let calls = points_of(&metrics, "theseus.tool.calls");
    assert_eq!(calls.len(), 1, "one series, `ok`: {calls:#?}");
    let ok = [
        ("theseus.tool.name", "proc.run"),
        ("theseus.tool.outcome", "ok"),
        ("theseus.outcome", "complete"),
    ];
    assert_eq!(
        point_with(&metrics, "theseus.tool.calls", &ok)["asInt"],
        "2"
    );
    let took = point_with(&metrics, "theseus.tool.duration_ms", &ok);
    assert_eq!(took["count"], "2");
    let sum = took["sum"].as_f64().unwrap();
    assert!((2_000.0..60_000.0).contains(&sum), "their runs: {sum} ms");
    let min = took["min"].as_f64().unwrap();
    assert!(min >= 400.0, "each timed by its run: {took}");
}
