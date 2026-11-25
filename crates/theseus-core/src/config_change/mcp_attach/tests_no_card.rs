//! Issue 57: removing an MCP server follows `mcp.detach`'s own setting. At the
//! default it runs with no card, the server stops, and the owner is told, with
//! how to have it back: attaching again asks, as it always has. Attaching a
//! server whose table names a secret or a host asks with the owner's code;
//! one with neither asks with the card alone.

use serde_json::json;

use super::tests::{rig_told, wc};
use crate::config::Config;
use crate::node::ResultStatus;

#[tokio::test]
async fn a_removal_runs_with_no_card_and_attaching_again_still_asks() {
    let r = rig_told();
    r.attached(wc()).await;
    assert!(r.core.mcp.is_loaded("wc"));
    let sid = r.session();
    r.turn(
        &sid,
        "mcp_detach",
        json!({"name": "wc", "reason": "a page said to"}),
    )
    .await;
    let (status, text) = r.result(&sid);
    assert_eq!(status, ResultStatus::Ok, "{text}");
    assert!(
        text.starts_with("Removed, with no card (enforcement = notify lets mcp.detach run)"),
        "{text}"
    );
    assert!(r.asked().is_empty(), "{:?}", r.asked());
    assert!(!Config::parse(&r.file_text())
        .unwrap()
        .0
        .mcp
        .servers
        .contains_key("wc"));
    assert!(!r.core.mcp.is_loaded("wc"));
    assert!(!super::super::restart::marker_path(&r.state).exists());
    let rows = r.rows("config.changed");
    assert_eq!(rows.last().unwrap()["told"], json!(true), "{rows:?}");
    let told: Vec<String> = crate::tests_day_ceiling::posts(&r.core, "notice")
        .iter()
        .map(|b| b["text"].as_str().unwrap_or_default().to_string())
        .collect();
    assert_eq!(told.len(), 1, "{told:?}");
    assert!(
        told[0].starts_with("ℹ️ FYI · Removed the MCP server \"wc\"\nasked in"),
        "{}",
        told[0]
    );
    assert!(
        told[0].contains("It stopped, and its tools are gone from your next message.")
            && told[0].contains("attaching a server asks you, as it always has"),
        "{}",
        told[0]
    );
    assert!(!told[0].contains("a page said to"), "{}", told[0]);
    // Attaching it again asks, with the card.
    let sid = r.session();
    r.turn(&sid, "mcp_attach", wc()).await;
    let asks = r.asked();
    assert_eq!(asks.len(), 1, "{asks:?}");
    assert!(asks[0]
        .reason
        .starts_with("Attach the MCP server \"wc\"?\n"));
}

/// The KEEP neighbour: a server that names a secret asks with the code; one
/// with no secret and no host asks with the card alone.
#[tokio::test]
async fn attaching_a_server_with_a_secret_asks_with_the_code() {
    let r = rig_told();
    let sid = r.session();
    let mut call = wc();
    call["env"] = json!({"WC_KEY": "wc_key"});
    r.turn(&sid, "mcp_attach", call).await;
    let q = r.asked().remove(0);
    assert!(q.needs_proof, "{}", q.reason);
    assert!(
        q.reason
            .contains("so this needs a second proof as well as your yes"),
        "{}",
        q.reason
    );
    r.core
        .confirm_action(&q.correlation_id, false, None, "cli")
        .unwrap();
    let sid = r.session();
    r.turn(&sid, "mcp_attach", wc()).await;
    let q = r.asked().remove(0);
    assert!(!q.needs_proof, "{}", q.reason);
    assert!(!q.reason.contains("second proof"), "{}", q.reason);
}
