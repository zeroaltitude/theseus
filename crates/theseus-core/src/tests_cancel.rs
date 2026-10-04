//! A cancel reaches an async tool's task (M4 18a; design §2.3): an
//! `http.fetch` of a page that never comes is aborted mid-flight, and its call
//! settles cancelled, verified by its task; its result, the cancel's answer,
//! the ledger, and health all say so.

use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde_json::json;
use theseus_protocol::SessionKind;

use crate::bus::EventSink;
use crate::config::WebToolsConfig;
use crate::node::{Body, ResultStatus};
use crate::policy::Posture;
use crate::provider::{
    DeltaSink, FakeProvider, Provider, ProviderFuture, ProviderRequest, Scripted,
};
use crate::session::SessionRecord;
use crate::store::Store;
use crate::turn::TurnRequest;
use crate::web::tests::{serve, web};
use crate::{Config, Core};

/// A stand-in model: it fetches the page first, and once a result has come
/// back, it answers.
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
            let next = if answered {
                Scripted::text("done")
            } else {
                Scripted::tools("", &[("tu_hang", "http_fetch", json!({ "url": self.url }))])
            };
            FakeProvider::scripted(vec![next])
                .stream_message(req, on_delta)
                .await
        })
    }
}

fn core_in(dir: &Path, port: u16) -> Arc<Core> {
    let root = dir.join("work");
    std::fs::create_dir_all(&root).unwrap();
    let mut cfg = Config::example();
    cfg.server.state_dir = dir.to_string_lossy().into_owned();
    cfg.tools.projects_dir = Some(root.canonicalize().unwrap().to_string_lossy().into_owned());
    cfg.tools.roots = vec![];
    cfg.policy.enforcement = Posture::Notify;
    let store = Store::open(&dir.join("store")).unwrap();
    let model = Arc::new(Model {
        url: format!("http://site.test:{port}/hang"),
    });
    Core::build(crate::rpc::Parts {
        toollets: web(port, WebToolsConfig::default(), true).tools(),
        ..crate::rpc::Parts::for_tests(cfg, model, store)
    })
    .unwrap()
}

/// The fetch's result: its status, its text, and its meta.
fn fetch_result(core: &Core, sid: &str) -> (ResultStatus, String, serde_json::Value) {
    core.store
        .session_nodes(sid)
        .unwrap()
        .into_iter()
        .find_map(|(_, n)| match n.body {
            Body::ToolResult {
                tool,
                status,
                content,
                meta,
                ..
            } if tool == "http.fetch" => Some((status, content, meta)),
            _ => None,
        })
        .expect("the fetch's result")
}

/// The async tools' abort (18a): the cancel aborts the fetch's task, waits
/// for its handle to finish, and settles the call verified by its task, at
/// once and not when the page would have come. The turn's result for the
/// call says it was aborted and how that is known; the cancel answers with
/// the verdict; an `action.cancel_verified` row and health's count follow.
#[tokio::test]
async fn a_cancel_aborts_an_async_tools_task_and_verifies_it() {
    let server = serve().await;
    let dir = tempfile::tempdir().unwrap();
    let core = core_in(dir.path(), server.port);
    let rec = SessionRecord::new(SessionKind::Conversation, None);
    core.store.put_session(&rec.session_id, &rec).unwrap();
    let sid = rec.session_id.clone();
    let turn = {
        let core = core.clone();
        tokio::spawn(async move {
            let (live, _) = core.live_profile();
            let target = core.runner.resolve_target(&live, None, None, None).unwrap();
            let sink = EventSink::new(core.bus.clone(), &rec.session_id, None);
            core.runner
                .run(TurnRequest {
                    session: rec,
                    input: Some("fetch the page".into()),
                    target,
                    sink,
                    author: "test".into(),
                    recompile: None,
                    attachments: vec![],
                    arrived: None,
                    reply_to: None,
                })
                .await
        })
    };
    // The fetch is in flight: its action dispatched, its task waiting on the
    // page.
    let t0 = Instant::now();
    let fetch = loop {
        let found =
            core.kernel.actions().unwrap().into_iter().find(|a| {
                a.tool == "http.fetch" && a.state == theseus_kernel::ActionState::Dispatched
            });
        if let Some(a) = found {
            break a;
        }
        assert!(t0.elapsed() < Duration::from_secs(20), "no fetch in flight");
        tokio::time::sleep(Duration::from_millis(10)).await;
    };
    tokio::time::sleep(Duration::from_millis(100)).await;
    let t1 = Instant::now();
    let (_, killed, verdicts) = core
        .cancel_execution_judged(&fetch.execution_id, "test")
        .await
        .unwrap();
    assert_eq!(killed, vec![fetch.correlation_id.clone()]);
    assert_eq!(verdicts.len(), 1, "{verdicts:?}");
    let v = &verdicts[0];
    assert_eq!(
        (v.state.as_str(), v.verified_by.as_str(), v.words().as_str()),
        ("termination_verified", "task", "verified: task")
    );
    assert!(t1.elapsed() < Duration::from_secs(2), "{:?}", t1.elapsed());
    let a = core.kernel.action(&fetch.correlation_id).unwrap().unwrap();
    assert_eq!(
        a.cancel,
        Some(theseus_kernel::CancelState::TerminationVerified)
    );
    assert_eq!(
        a.verdict.as_ref().map(|v| v.verified_by),
        Some(theseus_kernel::VerifiedBy::Task)
    );
    // The turn ends at once, its call answered as aborted (its next model
    // call is refused: the execution is cancelled).
    let _ = tokio::time::timeout(Duration::from_secs(10), turn)
        .await
        .expect("the turn ended");
    let result = fetch_result(&core, &sid);
    assert_eq!(result.0, ResultStatus::Cancelled, "{result:?}");
    assert!(result.1.contains("verified: task"), "{}", result.1);
    assert_eq!(result.2["verified"], "verified: task");
    let rows: Vec<_> = core
        .store
        .ledger_tail::<crate::ledger::LedgerRow>(100_000)
        .unwrap()
        .into_iter()
        .filter(|(_, r)| r.kind == "action.cancel_verified")
        .map(|(_, r)| r.data)
        .collect();
    assert_eq!(rows.len(), 1, "{rows:?}");
    assert_eq!(rows[0]["verified_by"], "task");
    let counts = core.health().cancels;
    assert_eq!(counts.len(), 1, "{counts:?}");
    assert_eq!(
        (
            counts[0].backend.as_str(),
            counts[0].state.as_str(),
            counts[0].n
        ),
        ("async", "verified", 1)
    );
}
