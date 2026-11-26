//! The real offer, run by a real turn (issue 59): the model calls `mcp_sign_in_offer` twice in one
//! turn in the owner's direct messages; the first posts one card to the owner's DM and nowhere else,
//! the second posts none and says so, and nothing is begun: the issuer is never asked for a
//! registration or an authorization, and no tool a turn has is a sign-in's begin, agree or finish.

use std::sync::{Arc, Mutex};

use serde_json::json;
use theseus_mcp::fake::{self, Fake};
use theseus_mcp::oauth::fake::{AuthConfig, FakeAuth};

use super::{session, turn, Model};
use crate::config::mcp::McpOauth;
use crate::config::McpServerConfig;
use crate::node::Body;
use crate::policy::Posture;
use crate::provider::{ProviderRequest, Scripted};
use crate::store::Store;
use crate::{Config, Core};

const OWNER: &str = "100000000000000042";

/// The offer, called twice, each after the result of the one before; then done.
fn twice(req: &ProviderRequest) -> Scripted {
    let results = req
        .messages
        .iter()
        .filter(|m| {
            m["content"]
                .as_array()
                .is_some_and(|c| c.iter().any(|b| b["type"] == "tool_result"))
        })
        .count();
    match results {
        0 => Scripted::tools(
            "",
            &[("o1", "mcp_sign_in_offer", json!({"server": "docs"}))],
        ),
        1 => Scripted::tools(
            "",
            &[("o2", "mcp_sign_in_offer", json!({"server": "docs"}))],
        ),
        _ => Scripted::text("Offered."),
    }
}

async fn rig() -> (Arc<Core>, Arc<FakeAuth>, tempfile::TempDir) {
    let auth = FakeAuth::start(AuthConfig::default()).await;
    let server = Fake::new(fake::Config {
        oauth: Some(auth.clone()),
        name: "docs".into(),
        ..Default::default()
    });
    let addr = server.serve_http(0).await.unwrap();
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("work");
    std::fs::create_dir_all(&root).unwrap();
    let mut cfg = Config::example();
    cfg.server.state_dir = dir.path().to_string_lossy().into_owned();
    cfg.tools.projects_dir = Some(root.canonicalize().unwrap().to_string_lossy().into_owned());
    cfg.tools.roots = vec![];
    cfg.policy.enforcement = Posture::Notify;
    cfg.places.owner = Some(vec![format!("discord:{OWNER}")]);
    cfg.mcp.servers.insert(
        "docs".into(),
        McpServerConfig {
            command: vec![],
            env: Default::default(),
            url: Some(format!("http://{addr}/mcp")),
            auth_secret: None,
            oauth: Some(McpOauth::default()),
            read: vec![],
            sandbox: Default::default(),
            egress: vec![],
            frozen: None,
            external: true,
            enabled: true,
            start_timeout_secs: 10,
            call_timeout_secs: 30,
        },
    );
    let store = Store::open(&dir.path().join("store")).unwrap();
    let model = Arc::new(Model {
        script: Box::new(twice),
        requests: Mutex::default(),
    });
    let core = Core::build(crate::rpc::Parts::for_tests(cfg, model, store)).unwrap();
    (core, auth, dir)
}

#[tokio::test]
async fn the_real_offer_posts_one_card_to_the_owner_and_begins_nothing() {
    let (core, auth, _dir) = rig().await;
    let place = format!("discord:dm:{OWNER}");
    let sid = session(&core, Some(&place));
    turn(&core, &sid, "sign me in to docs, please").await;
    let results: Vec<String> = core
        .store
        .session_nodes(&sid)
        .unwrap()
        .into_iter()
        .filter_map(|(_, n)| match n.body {
            Body::ToolResult { content, .. } => Some(content),
            _ => None,
        })
        .collect();
    assert_eq!(results.len(), 2, "{results:?}");
    assert!(results[0].contains("Nothing has started"), "{}", results[0]);
    assert!(
        results[1].contains("no new one was posted"),
        "{}",
        results[1]
    );
    let cards: Vec<_> = core
        .outbox
        .open_for(crate::outbox::OPERATOR_TARGET)
        .into_iter()
        .filter(|a| crate::outbox::kind_of(a) == "mcp_sign_in")
        .collect();
    assert_eq!(cards.len(), 1, "one card, to the owner");
    assert!(
        core.outbox
            .open_for(&place)
            .iter()
            .all(|a| crate::outbox::kind_of(a) != "mcp_sign_in"),
        "no card in the conversation's own place"
    );
    assert_eq!(auth.seen().registrations, 0, "nothing registered");
    assert!(
        auth.seen()
            .paths
            .iter()
            .all(|p| !p.contains("authorize") && !p.contains("token")),
        "{:?}",
        auth.seen().paths
    );
    let tools: Vec<String> = core
        .tools
        .registry
        .all()
        .map(|t| t.name().to_string())
        .collect();
    for t in &tools {
        assert!(
            !t.starts_with("mcp.login")
                && !t.starts_with("mcp.logout")
                && !t.starts_with("google.auth")
                && t != "google.revoke"
                && t != "aws.confirm_alerts",
            "a turn's tool reaches a sign-in: {t}"
        );
    }
}
