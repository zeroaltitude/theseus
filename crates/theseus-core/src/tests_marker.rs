//! The `external` marker on a result node, once the hold is gone (R6, theseus-gyjyo.8): a fetched
//! page's result is still marked, and the marker is what the rest reads (Jev's security state, the
//! content pack's source URL, egress's "outside text"). Reading it writes nothing: no
//! `session.external_read` row, no hold on the session record, and a call that acts after the fetch
//! keeps its own posture.

use std::sync::Arc;

use serde_json::{json, Value};
use theseus_protocol::SessionKind;

use crate::config::WebToolsConfig;
use crate::node::{Body, ResultStatus};
use crate::policy::Posture;
use crate::provider::{FakeProvider, Scripted};
use crate::session::SessionRecord;
use crate::store::Store;
use crate::test_support::turn;
use crate::web::tests::{serve, web};
use crate::{Config, Core};

fn ledgered(core: &Core, kind: &str) -> Vec<Value> {
    core.store
        .ledger_tail::<crate::ledger::LedgerRow>(100_000)
        .unwrap()
        .into_iter()
        .filter(|(_, r)| r.kind == kind)
        .map(|(_, r)| r.data)
        .collect()
}

/// A fetch, then a run, then the answer, with the posture the owner's config gives the writers.
#[tokio::test]
async fn a_fetched_page_keeps_its_marker_and_a_call_after_it_is_not_held() {
    let server = serve().await;
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("work");
    std::fs::create_dir_all(&root).unwrap();
    let mut cfg = Config::example();
    cfg.server.state_dir = dir.path().to_string_lossy().into_owned();
    cfg.tools.projects_dir = Some(root.canonicalize().unwrap().to_string_lossy().into_owned());
    cfg.tools.roots = vec![];
    cfg.tools.proc_sync_secs = 10;
    cfg.policy.enforcement = Posture::Notify;
    let url = format!("http://site.test:{}/page.html", server.port);
    let model = Arc::new(FakeProvider::scripted(vec![
        Scripted::tools("", &[("f1", "http_fetch", json!({ "url": url }))]),
        Scripted::tools("", &[("r1", "proc_run", json!({"argv": ["echo", "hi"]}))]),
        Scripted::text("Done."),
    ]));
    let store = Store::open(&dir.path().join("store")).unwrap();
    let core = Core::build(crate::rpc::Parts {
        toollets: web(server.port, WebToolsConfig::default(), true).tools(),
        ..crate::rpc::Parts::for_tests(cfg, model, store)
    })
    .unwrap();
    let rec = SessionRecord::new(SessionKind::Conversation, None);
    core.store.put_session(&rec.session_id, &rec).unwrap();
    let sid = rec.session_id;

    let res = turn(&core, &sid, "read it, then run").await;
    assert!(
        res.awaiting_confirm.is_none(),
        "the run is not held: {res:?}"
    );

    // The marker is on the fetch's result, with the URL it read.
    let nodes: Vec<_> = core
        .store
        .session_nodes(&sid)
        .unwrap()
        .into_iter()
        .map(|(_, n)| n)
        .collect();
    let fetched = nodes
        .iter()
        .find(|n| matches!(&n.body, Body::ToolResult { tool, status: ResultStatus::Ok, .. } if tool == "http.fetch"))
        .expect("a fetch result");
    assert!(crate::external::is_outside(fetched));
    let Body::ToolResult {
        external: Some(e), ..
    } = &fetched.body
    else {
        panic!("the fetch's result is marked")
    };
    assert_eq!(e.url, url);

    // Reading it holds nothing: no row, no field on the record.
    assert!(ledgered(&core, "session.external_read").is_empty());
    let rec = serde_json::to_value(
        core.store
            .get_session::<SessionRecord>(&sid)
            .unwrap()
            .unwrap(),
    )
    .unwrap();
    assert!(rec.get("external").is_none_or(Value::is_null), "{rec}");
}
