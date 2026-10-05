//! Cancels as a metric (theseus-qdk5; M4 §2.11): `theseus.cancel`, by
//! backend and state, counted where health counts them (`cancel::Stops`),
//! through a whole core whose telemetry posts to a receiver.

use std::os::unix::process::CommandExt;
use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde_json::json;
use theseus_protocol::SessionKind;

use super::tests::{attrs_of, flushed, last_metrics, pipeline, points_of, tuning, Receiver};
use crate::bus::EventSink;
use crate::config::WebToolsConfig;
use crate::policy::Posture;
use crate::provider::{
    DeltaSink, FakeProvider, Provider, ProviderFuture, ProviderRequest, Scripted,
};
use crate::session::SessionRecord;
use crate::store::Store;
use crate::turn::TurnRequest;
use crate::web::tests::{serve, web};
use crate::{Config, Core};

/// A stand-in model: asked to fetch, it fetches a page that never comes;
/// asked to run, it runs a job that outlives `proc_sync_secs`; once a result
/// has come back, it answers.
struct Model {
    url: String,
}

impl Provider for Model {
    fn name(&self) -> &str {
        "fake"
    }
    fn stream_message<'a>(
        &'a self,
        req: &'a ProviderRequest,
        on_delta: DeltaSink<'a>,
    ) -> ProviderFuture<'a> {
        Box::pin(async move {
            let answered = req.messages.iter().any(|m| {
                m["content"]
                    .as_array()
                    .is_some_and(|b| b.iter().any(|b| b["type"] == "tool_result"))
            });
            let asked = serde_json::to_string(&req.messages).unwrap_or_default();
            let next = if answered {
                Scripted::text("done")
            } else if asked.contains("fetch the page") {
                Scripted::tools("", &[("tu_hang", "http_fetch", json!({ "url": self.url }))])
            } else {
                let argv = json!({"argv": ["sh", "-c", "sleep 3; echo tide"]});
                Scripted::tools("", &[("tu_tide", "proc_run", argv)])
            };
            FakeProvider::scripted(vec![next])
                .stream_message(req, on_delta)
                .await
        })
    }
}

fn core_in(dir: &Path, port: u16, endpoint: &str) -> Arc<Core> {
    let root = dir.join("work");
    std::fs::create_dir_all(&root).unwrap();
    let mut cfg = Config::example();
    cfg.server.state_dir = dir.to_string_lossy().into_owned();
    cfg.tools.projects_dir = Some(root.canonicalize().unwrap().to_string_lossy().into_owned());
    cfg.tools.roots = vec![];
    cfg.tools.proc_sync_secs = 1;
    cfg.policy.enforcement = Posture::Notify;
    let store = Store::open(&dir.join("store")).unwrap();
    let model = Arc::new(Model {
        url: format!("http://site.test:{port}/hang"),
    });
    Core::build(crate::rpc::Parts {
        toollets: web(port, WebToolsConfig::default(), true).tools(),
        telemetry: Some(pipeline(endpoint, None, tuning())),
        ..crate::rpc::Parts::for_tests(cfg, model, store)
    })
    .unwrap()
}

/// A turn in a new session.
async fn turn(core: &Arc<Core>, input: &str) -> anyhow::Result<theseus_protocol::TurnSubmitResult> {
    let rec = SessionRecord::new(SessionKind::Conversation, None);
    core.store.put_session(&rec.session_id, &rec).unwrap();
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
}

/// The action of the first call of `tool` in `state`, within 20 s.
async fn action_of(
    core: &Core,
    tool: &str,
    state: theseus_kernel::ActionState,
) -> theseus_kernel::Action {
    let t0 = Instant::now();
    loop {
        let found = core.kernel.actions().unwrap().into_iter();
        if let Some(a) = found
            .into_iter()
            .find(|a| a.tool == tool && a.state == state)
        {
            return a;
        }
        assert!(
            t0.elapsed() < Duration::from_secs(20),
            "no {tool} {state:?}"
        );
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

/// An async tool's cancel (its task aborted) and a job's (its wrapper's
/// group stopped, as a wrapper from before 18a is) are two `theseus.cancel`
/// series of 1, `async`/`verified` and `l0`/`verified`, the same as
/// health's `cancels`.
#[tokio::test]
async fn each_cancel_is_counted_by_its_backend_and_state_as_health_counts_it() {
    let rx = Receiver::start(vec![]).await;
    let server = serve().await;
    let dir = tempfile::tempdir().unwrap();
    let core = core_in(dir.path(), server.port, &rx.endpoint());

    // The fetch: in flight, then cancelled.
    let fetching = {
        let core = core.clone();
        tokio::spawn(async move { turn(&core, "fetch the page").await })
    };
    let fetch = action_of(&core, "http.fetch", theseus_kernel::ActionState::Dispatched).await;
    tokio::time::sleep(Duration::from_millis(100)).await;
    let (_, _, verdicts) = core
        .cancel_execution_judged(&fetch.execution_id, "test")
        .await
        .unwrap();
    assert_eq!(verdicts[0].verified_by, "task", "{verdicts:?}");
    let _ = tokio::time::timeout(Duration::from_secs(10), fetching)
        .await
        .expect("the turn ended");

    // The job: in the background, its wrapper a process whose group the
    // stop ends, as the in-process launcher leaves none.
    let ran = turn(&core, "run the tide script").await.unwrap();
    assert_eq!(ran.output, "done");
    let job = action_of(&core, "proc.run", theseus_kernel::ActionState::Dispatched).await;
    let root = dir.path().join("work");
    std::fs::write(root.join("job-wrapper"), "while :; do sleep 0.05; done\n").unwrap();
    let mut wrapper = std::process::Command::new("sh")
        .args(["job-wrapper", "--correlation-id", &job.correlation_id])
        .current_dir(&root)
        .process_group(0)
        .spawn()
        .unwrap();
    core.spool
        .write_pid(&job.correlation_id, wrapper.id())
        .unwrap();
    let t0 = Instant::now();
    while !theseus_kernel::job::wrapper_alive(wrapper.id(), &job.correlation_id) {
        assert!(
            t0.elapsed() < Duration::from_secs(5),
            "the wrapper never read as one"
        );
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    let (_, _, verdicts) = core
        .cancel_execution_judged(&job.execution_id, "test")
        .await
        .unwrap();
    let _ = wrapper.wait();
    assert_eq!(
        (verdicts[0].state.as_str(), verdicts[0].verified_by.as_str()),
        ("termination_verified", "group"),
        "{verdicts:?}"
    );

    let health: Vec<(String, String, u64)> = core
        .health()
        .cancels
        .into_iter()
        .map(|c| (c.backend, c.state, c.n))
        .collect();
    let want = |b: &str, s: &str| (b.to_string(), s.to_string(), 1);
    assert_eq!(
        health,
        vec![want("async", "verified"), want("l0", "verified")]
    );
    flushed(core.telemetry()).await;
    let metrics = last_metrics(&rx.got());
    let mut points: Vec<(String, String, u64)> = points_of(&metrics, "theseus.cancel")
        .into_iter()
        .map(|p| {
            let a = attrs_of(p);
            let n = p["asInt"].as_str().unwrap().parse().unwrap();
            (
                a["theseus.cancel.backend"].clone(),
                a["theseus.cancel.state"].clone(),
                n,
            )
        })
        .collect();
    points.sort();
    assert_eq!(points, health, "the metric is health's count");
}

/// A job the stop cannot reach (no wrapper: the in-process launcher leaves
/// none) is counted as health counts it, `inproc`/`unsupported`: the metric
/// takes each cancel's own state.
#[tokio::test]
async fn a_cancel_nothing_reaches_is_counted_unsupported() {
    let rx = Receiver::start(vec![]).await;
    let server = serve().await;
    let dir = tempfile::tempdir().unwrap();
    let core = core_in(dir.path(), server.port, &rx.endpoint());
    turn(&core, "run the tide script").await.unwrap();
    let job = action_of(&core, "proc.run", theseus_kernel::ActionState::Dispatched).await;
    let (_, _, verdicts) = core
        .cancel_execution_judged(&job.execution_id, "test")
        .await
        .unwrap();
    assert_eq!(verdicts[0].state, "unsupported", "{verdicts:?}");
    let health: Vec<_> = core.health().cancels;
    assert_eq!(health.len(), 1, "{health:?}");
    flushed(core.telemetry()).await;
    let metrics = last_metrics(&rx.got());
    let points = points_of(&metrics, "theseus.cancel");
    assert_eq!(points.len(), 1, "{points:#?}");
    let a = attrs_of(points[0]);
    assert_eq!(
        (
            a["theseus.cancel.backend"].as_str(),
            a["theseus.cancel.state"].as_str(),
            points[0]["asInt"].as_str()
        ),
        (
            health[0].backend.as_str(),
            health[0].state.as_str(),
            Some("1")
        )
    );
    assert_eq!(health[0].state, "unsupported");
}
