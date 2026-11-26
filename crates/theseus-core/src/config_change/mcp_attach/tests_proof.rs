//! An MCP server's card with the owner's second proof, end to end on a scratch
//! daemon with a stand-in authenticator and a stand-in server: the trial, the
//! card, the code, the server loaded live, then taken away again; and an
//! agent's process or a job's process refused with a good code and an open
//! window both in hand.

use serde_json::{json, Value};
use theseus_protocol::{method, RpcError};

use super::tests::{rig, wc, Rig, FILE};
use crate::approval::{Client, Surface};
use crate::owner_proof::Proofs;
use crate::peer_proc::Behind;
use crate::tests_m3::{rpc_as, surface};

fn owner() -> Client {
    surface("sock#1", Surface::Cli).behind(Behind::Owner)
}

async fn yes(r: &Rig, client: Client, corr: &str, code: Option<&str>) -> Result<Value, RpcError> {
    rpc_as(
        &r.core,
        client,
        method::ACTION_CONFIRM,
        json!({"correlation_id": corr, "approve": true, "code": code}),
    )
    .await
}

/// Attach with a code (a window opens), detach inside it on the yes alone.
#[tokio::test]
async fn a_server_is_attached_on_a_code_and_removed_inside_the_window() {
    let r = rig();
    let secret = r.core.proofs.enrol_stand_in();
    let sid = r.session();
    // Given a secret: the code is asked (issue 57).
    let mut call = wc();
    call["env"] = json!({"WC_KEY": "wc_key"});
    r.turn(&sid, "mcp_attach", call).await;
    let q = r.asked().remove(0);
    assert!(q.reason.contains("wc-server"), "{}", q.reason);
    // The yes alone is refused; nothing was written, started for good or loaded.
    let e = yes(&r, owner(), &q.correlation_id, None).await.unwrap_err();
    assert!(e.message.contains("second proof"), "{e:?}");
    assert_eq!(r.file_text(), FILE);
    assert!(!r.core.mcp.is_loaded("wc"));
    // A good code writes the table and loads the server live.
    yes(
        &r,
        owner(),
        &q.correlation_id,
        Some(&Proofs::stand_in_code(&secret, 0)),
    )
    .await
    .unwrap();
    assert!(
        r.file_text().contains("[mcp.servers.wc]"),
        "{}",
        r.file_text()
    );
    assert!(r.core.mcp.is_loaded("wc"));
    assert_eq!(r.core.proofs.status().windows.len(), 1);
    // Taking it away needs no code (issue 57): its card's yes alone, and the
    // window is not drawn on.
    let sid = r.session();
    r.turn(&sid, "mcp_detach", json!({"name": "wc"})).await;
    let d = r.asked().remove(0);
    assert!(!d.needs_proof, "{d:?}");
    yes(&r, owner(), &d.correlation_id, None).await.unwrap();
    assert!(!r.file_text().contains("[mcp.servers.wc]"));
    assert!(!r.core.mcp.is_loaded("wc"));
    assert_eq!(r.rows("owner.proof.window_used").len(), 0);
}

/// An agent's process and a job's process approve neither card, with the
/// owner's good code in hand and the owner's own window open.
#[tokio::test]
async fn an_agents_and_a_jobs_process_approve_neither_an_attach_nor_a_detach() {
    let r = rig();
    let secret = r.core.proofs.open_stand_in_window("cli");
    let sid = r.session();
    r.turn(&sid, "mcp_attach", wc()).await;
    let attach = r.asked().remove(0).correlation_id;
    let sid = r.session();
    r.turn(&sid, "mcp_detach", json!({"name": "base"})).await;
    let detach = r
        .asked()
        .into_iter()
        .find(|c| c.correlation_id != attach)
        .expect("the detach's card")
        .correlation_id;
    let windows = r.core.proofs.status().windows;
    for who in [
        Behind::Agent("claude".into()),
        Behind::Agent(crate::peer_proc::JOB.into()),
    ] {
        for q in [&attach, &detach] {
            for with in [None, Some(Proofs::stand_in_code(&secret, 1))] {
                let client = surface("sock#9", Surface::Cli).behind(who.clone());
                let e = yes(&r, client, q, with.as_deref()).await.unwrap_err();
                assert!(
                    e.message.contains("agent's process") || e.message.contains("only the owner"),
                    "{who:?}: {e:?}"
                );
            }
        }
    }
    assert_eq!(r.file_text(), FILE);
    assert!(!r.core.mcp.is_loaded("wc"));
    assert_eq!(r.asked().len(), 2, "both still wait");
    assert_eq!(r.core.proofs.status().windows, windows);
    // The owner's own hand still can: the window is the owner's.
    yes(&r, owner(), &attach, None).await.unwrap();
    assert!(r.core.mcp.is_loaded("wc"));
}
