//! A Gmail search's plan says its query, cut to eighty characters where no
//! known value is split: the gate record the call's node keeps holds the
//! value whole or not at all. The value is invented.

use std::sync::Arc;

use serde_json::json;
use theseus_google::fake::{Fake, REFRESH_TOKEN};

use super::tests::{board, config, session};
use crate::node::Body;
use crate::provider::{FakeProvider, Scripted};
use crate::store::Store;
use crate::test_support::turn;
use crate::Core;

const V: &str = "kqzv-0417-wjxp-9932-hmfb";

#[tokio::test]
async fn a_searchs_plan_summary_stops_before_a_known_value() {
    let fake = Fake::start().await;
    let dir = tempfile::tempdir().unwrap();
    let cfg = config(dir.path(), &fake, "file:/tmp/unused-google-refresh-token");
    cfg.validate().unwrap();
    let store = Store::open(&dir.path().join("store")).unwrap();
    let q = format!("{}{V} newer_than:7d", "x".repeat(70));
    let model = Arc::new(FakeProvider::scripted(vec![
        Scripted::tools("", &[("s1", "gmail_search", json!({"q": q}))]),
        Scripted::text("Done."),
    ]));
    let core = Core::build(crate::rpc::Parts {
        secrets: board(Some(REFRESH_TOKEN)),
        scrubber: Arc::new(crate::scrub::Scrubber::with_values(vec![(
            V.into(),
            "cut_test".into(),
        )])),
        ..crate::rpc::Parts::for_tests(cfg, model, store)
    })
    .unwrap();
    let sid = session(&core);
    turn(&core, &sid, "search my mail").await;
    let summary = core
        .store
        .session_nodes(&sid)
        .unwrap()
        .into_iter()
        .find_map(|(_, n)| match n.body {
            Body::ToolCall {
                tool_use_id,
                gate: Some(g),
                ..
            } if tool_use_id == "s1" => g.plan.map(|p| p.summary),
            _ => None,
        })
        .expect("the call's gate record keeps its plan");
    assert_eq!(summary, format!("search Gmail: {}", "x".repeat(70)));
}
