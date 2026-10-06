//! The calls a later turn answers (theseus-8pei): a confirmed call's run,
//! in the continuation, and a background job's late result, each traced in
//! the turn that answered it, through whole cores whose telemetry posts to
//! a receiver.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use serde_json::json;
use theseus_protocol::{SessionKind, Span, TurnSubmitResult};

use super::tests::{flushed, last_metrics, pipeline, point_with, points_of, tuning, Receiver};
use crate::bus::EventSink;
use crate::policy::Posture;
use crate::provider::{
    DeltaSink, FakeProvider, Provider, ProviderFuture, ProviderRequest, Scripted,
};
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
    rig_with(Arc::new(FakeProvider::scripted(script)), endpoint, tweak)
}

/// `rig`, with this provider.
fn rig_with(fake: Arc<dyn Provider>, endpoint: &str, tweak: impl FnOnce(&mut Config)) -> Rig {
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

/// A provider that answers as `inner`, its call number `held` (from 0)
/// waiting until `release` is notified.
struct Held {
    inner: FakeProvider,
    held: usize,
    calls: AtomicUsize,
    release: Arc<tokio::sync::Notify>,
}

impl Provider for Held {
    fn name(&self) -> &str {
        "fake"
    }
    fn stream_message<'a>(
        &'a self,
        req: &'a ProviderRequest,
        on_delta: DeltaSink<'a>,
    ) -> ProviderFuture<'a> {
        Box::pin(async move {
            if self.calls.fetch_add(1, Ordering::SeqCst) == self.held {
                self.release.notified().await;
            }
            self.inner.stream_message(req, on_delta).await
        })
    }
}

/// A late result that settles while a turn of its execution still runs is
/// taken by that turn's `finish` (theseus-kxyc), the only place it counts:
/// one `tool proc_run` span at the trace's top level, `late`, `ok`, with the
/// job's run, and one `theseus.tool.calls` point. The model's answer to the
/// `background` placeholder is held until the job's completion is queued on
/// the execution, which a task drains from the spool as the daemon's
/// heartbeat does, so the turn still runs when it lands on any machine.
#[tokio::test]
async fn a_late_result_taken_as_its_turn_finishes_is_traced_there() {
    let rx = Receiver::start(vec![]).await;
    let release = Arc::new(tokio::sync::Notify::new());
    let model = Held {
        inner: FakeProvider::scripted(vec![
            sleeper("t1", "1.3"),
            Scripted::text("It ran meanwhile."),
        ]),
        held: 1,
        calls: AtomicUsize::new(0),
        release: release.clone(),
    };
    let r = rig_with(Arc::new(model), &rx.endpoint(), |c| {
        c.tools.proc_sync_secs = 1;
        c.policy.tools.insert("proc.run".into(), Posture::Open);
    });
    let drain = {
        let core = r.core.clone();
        tokio::spawn(async move {
            for _ in 0..600 {
                tokio::time::sleep(Duration::from_millis(50)).await;
                core.heartbeat("test");
                let job = core.kernel.actions().unwrap();
                let job = job.iter().find(|a| a.tool == "proc.run");
                let queued = job.is_some_and(|a| {
                    let e = core.kernel.execution(&a.execution_id).unwrap();
                    e.is_some_and(|e| e.queued_results.contains(&a.correlation_id))
                });
                if queued {
                    release.notify_one();
                    return true;
                }
            }
            release.notify_one();
            false
        })
    };
    let first = turn(&r.core, "run the tide script").await;
    assert!(drain.await.unwrap(), "the job's completion was queued");
    assert_eq!(first.output, "It ran meanwhile.");
    let trace = first.trace.as_ref().unwrap();
    let spans = spans_named(trace, "tool proc_run");
    assert_eq!(
        spans.len(),
        2,
        "its placeholder and its late result: {trace:#?}"
    );
    assert_eq!(spans[0].1.attrs["result"], "background");
    let (parent, span) = spans[1];
    assert_eq!(parent, trace.name, "at the trace's top level");
    for (_, c) in spans_named(trace, "continuation") {
        assert!(c.children.iter().all(|s| s.kind != "tool"), "{c:#?}");
    }
    assert_eq!(span.attrs["late"], json!(true));
    assert_eq!(span.attrs["result"], "ok");
    assert_eq!(span.attrs["tool"], "proc.run");
    assert_eq!(span.attrs["tool_use_id"], "t1");
    let run = span.attrs["run_ms"].as_u64().expect("the job's run");
    assert!((1_300..30_000).contains(&run), "the job's run: {run} ms");

    flushed(r.core.telemetry()).await;
    let metrics = last_metrics(&rx.got());
    let calls = points_of(&metrics, "theseus.tool.calls");
    assert_eq!(calls.len(), 1, "one series, `ok`: {calls:#?}");
    let ok = [
        ("theseus.tool.name", "proc.run"),
        ("theseus.tool.outcome", "ok"),
    ];
    assert_eq!(
        point_with(&metrics, "theseus.tool.calls", &ok)["asInt"],
        "1"
    );
    let took = point_with(&metrics, "theseus.tool.duration_ms", &ok);
    assert!(
        took["min"].as_f64().unwrap() >= 1_300.0,
        "timed by its run: {took}"
    );
}

/// Calls [A, B, C], A asking (theseus-6xwq): the continuation answers A,
/// then runs B and C as a response's calls run (`run_fresh`), and each is
/// traced under the continuation, named and `tool_use_id`'d for its own
/// call, in order, A alone and the two reads together; each counts once.
#[tokio::test]
async fn a_continuations_fresh_calls_are_traced_each_for_its_own() {
    let rx = Receiver::start(vec![]).await;
    let r = rig(
        vec![
            Scripted::tools(
                "",
                &[
                    ("a1", "proc_run", json!({"argv": ["sh", "-c", "echo tide"]})),
                    ("b1", "fs_read", json!({"path": "one.txt"})),
                    ("c1", "fs_read", json!({"path": "two.txt"})),
                ],
            ),
            Scripted::text("All three answered."),
        ],
        &rx.endpoint(),
        |c| {
            c.policy.enforcement = Posture::Open;
            c.policy.tools.insert("proc.run".into(), Posture::Approve);
        },
    );
    let work = r._dir.path().join("work");
    std::fs::write(work.join("one.txt"), "one\n").unwrap();
    std::fs::write(work.join("two.txt"), "two\n").unwrap();
    let first = turn(&r.core, "run it, then read both").await;
    let cont = approved(&r.core, &first).await;
    assert_eq!(cont.output, "All three answered.");
    let trace = cont.trace.as_ref().unwrap();
    let mut tools = Vec::new();
    for name in ["tool proc_run", "tool fs_read"] {
        tools.extend(spans_named(trace, name));
    }
    tools.sort_by_key(|(_, s)| s.attrs["tool_use_id"].as_str().map(str::to_string));
    let seen: Vec<(&str, &str, &str, &str)> = tools
        .iter()
        .map(|(parent, s)| {
            (
                *parent,
                s.name.as_str(),
                s.attrs["tool_use_id"].as_str().unwrap(),
                s.attrs["result"].as_str().unwrap(),
            )
        })
        .collect();
    assert_eq!(
        seen,
        vec![
            ("continuation", "tool proc_run", "a1", "ok"),
            ("tools", "tool fs_read", "b1", "ok"),
            ("tools", "tool fs_read", "c1", "ok"),
        ],
        "{trace:#?}"
    );
    // In order: the reads ran after A, under one `tools` span that is the
    // continuation's.
    let groups = spans_named(trace, "tools");
    assert_eq!(groups.len(), 1, "{trace:#?}");
    assert_eq!(groups[0].0, "continuation");
    assert_eq!(groups[0].1.attrs["calls"], 2);
    let a_end = tools[0].1.end_us.unwrap();
    assert!(groups[0].1.start_us >= a_end, "{trace:#?}");
    let cont_span = spans_named(trace, "continuation");
    let kids: Vec<&str> = cont_span[0]
        .1
        .children
        .iter()
        .map(|c| c.name.as_str())
        .collect();
    assert_eq!(kids, ["tool proc_run", "tools"], "{trace:#?}");

    flushed(r.core.telemetry()).await;
    let metrics = last_metrics(&rx.got());
    for (tool, n) in [("proc.run", "1"), ("fs.read", "2")] {
        let with = [("theseus.tool.name", tool), ("theseus.tool.outcome", "ok")];
        let p = point_with(&metrics, "theseus.tool.calls", &with);
        assert_eq!(p["asInt"], n, "{tool}");
    }
    assert_eq!(points_of(&metrics, "theseus.tool.calls").len(), 2);
}
