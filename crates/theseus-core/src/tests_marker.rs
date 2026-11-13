//! The `external` marker on a result node, once the hold is gone (R6, theseus-gyjyo.8): a fetched
//! page's result is still marked, and the marker is what the rest reads (Jev's security state, the
//! content pack's source URL, egress's "outside text"). Reading it writes nothing: no
//! `session.external_read` row, no hold on the session record, and a call that acts after the fetch
//! keeps its own posture.

use std::sync::Arc;

use serde_json::{json, Value};
use theseus_protocol::SessionKind;

use crate::config::WebToolsConfig;
use crate::node::{Body, Node, ResultStatus};
use crate::payload::Marked;
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

/// A job of `gh` (its output is a stranger's issue text) is marked outside text with no config
/// listing it, and marking holds nothing: the next call that acts runs on its own posture.
#[tokio::test]
async fn a_job_of_gh_is_marked_outside_text_and_holds_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("work");
    std::fs::create_dir_all(&root).unwrap();
    // A stand-in `gh`, named by file name as the real one is.
    let bin = root.join("gh");
    std::fs::write(&bin, "#!/bin/sh\necho 'a stranger wrote this issue'\n").unwrap();
    std::fs::set_permissions(&bin, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
    let mut cfg = Config::example();
    cfg.server.state_dir = dir.path().to_string_lossy().into_owned();
    cfg.tools.projects_dir = Some(root.canonicalize().unwrap().to_string_lossy().into_owned());
    cfg.tools.roots = vec![];
    cfg.tools.proc_sync_secs = 10;
    cfg.policy.enforcement = Posture::Notify;
    let model = Arc::new(FakeProvider::scripted(vec![
        Scripted::tools(
            "",
            &[(
                "g1",
                "proc_run",
                json!({"argv": [bin.to_string_lossy(), "issue", "view", "1"]}),
            )],
        ),
        Scripted::tools(
            "",
            &[("r1", "proc_run", json!({"argv": ["echo", "plainly"]}))],
        ),
        Scripted::text("Done."),
    ]));
    let store = Store::open(&dir.path().join("store")).unwrap();
    let core = Core::build(crate::rpc::Parts::for_tests(cfg, model, store)).unwrap();
    let rec = SessionRecord::new(SessionKind::Conversation, None);
    core.store.put_session(&rec.session_id, &rec).unwrap();
    let sid = rec.session_id;

    let res = turn(&core, &sid, "read the issue, then run").await;
    assert!(res.awaiting_confirm.is_none(), "nothing is held: {res:?}");
    let nodes: Vec<_> = core
        .store
        .session_nodes(&sid)
        .unwrap()
        .into_iter()
        .map(|(_, n)| n)
        .collect();
    let printed = nodes
        .iter()
        .find(|n| matches!(&n.body, Body::ToolResult { content, .. } if content.contains("a stranger wrote")))
        .expect("the stand-in's result");
    assert!(crate::external::is_outside(printed), "{printed:?}");
    let echoed = nodes
        .iter()
        .find(
            |n| matches!(&n.body, Body::ToolResult { content, .. } if content.contains("plainly")),
        )
        .expect("the echo's result");
    assert!(
        !crate::external::is_outside(echoed),
        "an unlisted program is not marked"
    );
    assert!(ledgered(&core, "session.external_read").is_empty());
}

/// A result marked outside text whose output was large enough to be held out of the request
/// (`[tools] large_payload_chars`) is still outside text. The marker and the payload are
/// independent: the payload decides what a compile *renders*, the marker decides what the gate,
/// the index and consolidation *count*, and the payload's bytes are reachable either way. Reading
/// the payload into the marker's own test — a `Body::ToolResult` pattern that names `payload` is a
/// condition, not a field — is how the marker gets narrowed by a mechanical pass without a single
/// test failing, so this one holds the two apart by name (theseus-czpp).
#[test]
fn a_marked_result_with_a_large_payload_is_still_outside_text() {
    fn result(session: &str, url: Option<&str>, payload: Option<Marked>) -> Node {
        Node::harness_body(
            "nod",
            session,
            "test",
            Body::ToolResult {
                tool_use_id: "tu_1".into(),
                tool: "http.fetch".into(),
                status: ResultStatus::Ok,
                is_error: false,
                content: "held out".into(),
                correlation_id: None,
                bytes_total: 8,
                truncated: false,
                full_ref: None,
                duration_ms: None,
                late: false,
                meta: Value::Null,
                image: None,
                external: url.map(|u| theseus_tools::External { url: u.into() }),
                payload: payload.map(Box::new),
            },
        )
    }
    // A page long enough to be held out: whoever wrote the page chose its length.
    let big = "a line of the stranger's page\n".repeat(8_000);
    let marked = Marked::of("http.fetch", &big, "/tmp/pay_1.txt");

    // The payload does not unmark the result.
    let held = result(
        "ses_a",
        Some("https://site.test/long"),
        Some(marked.clone()),
    );
    assert!(
        crate::external::is_outside(&held),
        "a large payload must not narrow the marker"
    );
    assert_eq!(
        crate::external::first(std::slice::from_ref(&held)),
        Some(crate::external::Outside {
            tool: "http.fetch".into(),
            url: "https://site.test/long".into(),
            at_ms: held.created_at_ms,
        }),
        "Jev's state names the large outside read"
    );
    // The control: a payload on an unmarked result is still not outside text, so the assertion
    // above is about the marker and not about every result that carries a payload.
    assert!(!crate::external::is_outside(&result(
        "ses_a",
        None,
        Some(marked.clone())
    )));

    // And the gate's own input: a session whose only outside read was a large one has read
    // outside text, so the `http.fetch` after it is judged (`judged`'s `holds`, Q12).
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open(&dir.path().join("store")).unwrap();
    let rec = SessionRecord::new(SessionKind::Conversation, None);
    store.put_session(&rec.session_id, &rec).unwrap();
    let node = result(
        &rec.session_id,
        Some("https://site.test/long"),
        Some(marked),
    );
    store.append(&[node.record().unwrap()]).unwrap();
    assert!(
        crate::external::read_any(&store, &rec.session_id),
        "a session whose only outside read was large has read outside text"
    );
}
