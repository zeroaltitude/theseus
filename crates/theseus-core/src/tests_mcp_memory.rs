//! The MCP connection through the whole core: an error's words and the status
//! reply carry the vault's values substituted; every memory method but
//! `history.read` (the server's `context_history`, `tests_mcp_history`) and
//! every approval stays refused; `memory.lookup`, which served the server's
//! old `memory_search` and `memory_get`, is no method now (issue 46). And
//! `rpc`, one request on a connection of a surface, which other tests share.

use std::sync::Arc;

use serde_json::{json, Value};
use theseus_protocol::method;

use crate::approval::Surface;
use crate::tests_imports::script;
use crate::tests_places::{rig_scripted, session, Rig};
use crate::Core;

fn rig(memory: bool) -> Rig {
    rig_scripted(move |c| c.mcp_server.memory = memory, script)
}

/// One request on an MCP connection (or `surface`'s): the result or the
/// error.
pub(crate) async fn rpc(
    core: &Arc<Core>,
    surface: Surface,
    method: &str,
    params: Value,
) -> Result<Value, theseus_protocol::RpcError> {
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
    let (ours, theirs) = tokio::io::duplex(256 * 1024);
    let (sr, sw) = tokio::io::split(theirs);
    let client = crate::approval::Client::new("mcp", surface);
    let srv = tokio::spawn(core.clone().serve_connection(sr, sw, client));
    let (cr, mut cw) = tokio::io::split(ours);
    let req = theseus_protocol::Request::new(theseus_protocol::Id::Num(1), method, params);
    let mut line = serde_json::to_string(&req).unwrap();
    line.push('\n');
    cw.write_all(line.as_bytes()).await.unwrap();
    let mut lines = BufReader::new(cr).lines();
    let out = loop {
        let l = lines.next_line().await.unwrap().unwrap();
        if let theseus_protocol::Message::Response(r) = serde_json::from_str(&l).unwrap() {
            break match (r.result, r.error) {
                (Some(v), _) => Ok(v),
                (None, e) => Err(e.unwrap()),
            };
        }
    };
    cw.shutdown().await.unwrap();
    drop(lines);
    let _ = srv.await;
    out
}

/// `history.read` on an MCP connection: the server's `context_history`, its one way to look back.
async fn ask(core: &Arc<Core>, params: Value) -> Result<Value, theseus_protocol::RpcError> {
    rpc(core, Surface::Mcp, method::HISTORY_READ, params).await
}

fn rows(core: &Core, kind: &str) -> Vec<Value> {
    core.store
        .ledger_tail::<crate::ledger::LedgerRow>(1000)
        .unwrap()
        .into_iter()
        .filter(|(_, r)| r.kind == kind)
        .map(|(_, r)| r.data)
        .collect()
}

/// An MCP error's words and `conversation_status`'s last reply carry the vault's values substituted too
/// (review G-R2 r2): an error that echoes what was asked, and the newest reply as the status reads it
/// (`mcp_server::mcp_last_reply` over the session's `session.history`, the MCP's own route).
#[tokio::test]
async fn an_mcp_error_and_the_status_reply_carry_the_vault_value_substituted() {
    let secret = "marsh-kelp-vault-value-77";
    let r = rig(true);
    r.core
        .secrets
        .hold("marsh_vault", crate::secrets::Secret::new(secret.into()));
    // An error that echoes the asked session.
    let e = ask(&r.core, json!({"session_id": secret, "client": "c"}))
        .await
        .unwrap_err();
    assert!(
        !e.message.contains(secret) && e.message.contains("[redacted:marsh_vault]"),
        "{}",
        e.message
    );
    // An error that echoes the handle asked to be opened.
    let e = ask(&r.core, json!({"open": secret, "client": "c"}))
        .await
        .unwrap_err();
    assert!(
        !e.message.contains(secret) && e.message.contains("[redacted:marsh_vault]"),
        "{}",
        e.message
    );
    // A decode error, which echoes the value it could not read (a cursor given as the vault's value).
    let e = ask(
        &r.core,
        json!({"place": "local", "before": secret, "client": "c"}),
    )
    .await
    .unwrap_err();
    assert_eq!(
        e.code,
        theseus_protocol::error_code::INVALID_PARAMS,
        "{}",
        e.message
    );
    assert!(
        !e.message.contains(secret) && e.message.contains("[redacted:marsh_vault]"),
        "{}",
        e.message
    );
    // The newest reply a session holds, read as conversation_status reads it.
    let sid = session(&r.core, None);
    let reply = crate::node::Node::assistant(
        &sid,
        "turn_x",
        0,
        crate::node::Body::AssistantMessage {
            blocks: vec![json!({"type": "text", "text": format!("the key is {secret}")})],
            model: "m".into(),
            provider: "p".into(),
            stop_reason: None,
            usage: Default::default(),
            cost_usd: None,
            catalog_version: None,
            request_id: None,
            correlation_id: None,
            compilation_id: None,
            request_digest: None,
            effort: None,
        },
    );
    r.core.store.append(&[reply.record().unwrap()]).unwrap();
    let h = rpc(
        &r.core,
        Surface::Mcp,
        method::SESSION_HISTORY,
        json!({"session_id": sid, "n": 40}),
    )
    .await
    .unwrap();
    let nodes = h["nodes"].as_array().cloned().unwrap_or_default();
    let last = crate::mcp_server::mcp_last_reply(&r.core, &nodes).expect("a reply");
    assert!(
        !last.contains(secret) && last.contains("[redacted:marsh_vault]"),
        "{last}"
    );
}

/// The MCP connection may call no other memory method, and none of the
/// owner's approvals counts.
#[tokio::test]
async fn the_mcp_connection_is_still_refused_every_other_memory_method_and_every_approval() {
    let r = rig(true);
    for m in [
        method::MEMORY_LABEL,
        method::MEMORY_SEARCH,
        method::MEMORY_RECALLS,
        method::IMPORT_SESSIONS,
        method::INDEX_QUERY,
    ] {
        assert!(!crate::mcp_server::allowed(m), "{m}");
        let e = rpc(
            &r.core,
            Surface::Mcp,
            m,
            json!({"query": "x", "session_id": "x"}),
        )
        .await
        .unwrap_err();
        assert!(e.message.contains("may not call"), "{m}: {e:?}");
    }
    assert!(!crate::mcp_server::allowed("memory.lookup"));
    for m in crate::mcp_server::JUDGED {
        let e = rpc(&r.core, Surface::Mcp, m, json!({})).await.expect_err(m);
        assert!(!e.message.is_empty(), "{m}");
    }
}

/// `memory.lookup` is gone with the MCP server's `memory_search` and `memory_get` (issue 46): the MCP
/// connection's door refuses it as a method it may not call, whatever `[mcp_server] memory` says, and any other
/// surface is told it is no method, as any unknown name is. No row is written.
#[tokio::test]
async fn memory_lookup_is_no_method_now() {
    let r = rig(true);
    let p = json!({"op": "get", "session": "ses_x", "client": "c"});
    let e = rpc(&r.core, Surface::Mcp, "memory.lookup", p.clone())
        .await
        .unwrap_err();
    assert!(e.message.contains("may not call"), "{e:?}");
    let e = rpc(&r.core, Surface::Cli, "memory.lookup", p)
        .await
        .unwrap_err();
    assert_eq!(
        e.code,
        theseus_protocol::error_code::METHOD_NOT_FOUND,
        "{e:?}"
    );
    assert!(rows(&r.core, "memory.read").is_empty());
}

/// `wake.create` on an MCP connection (review, issue 28): it is in neither `ALLOWED` nor `JUDGED`, so the
/// connection's door refuses it before it runs, and no wake is made. The same call on the CLI's socket is
/// anyone's (`rpc/wake_owner.rs`): it is made, in the owner's Home.
#[tokio::test]
async fn an_mcp_client_cannot_set_a_reminder_and_the_socket_can() {
    let r = rig(false);
    assert!(!crate::mcp_server::allowed(method::WAKE_CREATE));
    let call = json!({"when": "in 1h", "note": "Check the tide tables"});
    let before = r.core.kernel.pending_wakes().unwrap().len();
    let e = rpc(&r.core, Surface::Mcp, method::WAKE_CREATE, call.clone())
        .await
        .unwrap_err();
    assert!(e.message.contains("may not call"), "{e:?}");
    assert_eq!(r.core.kernel.pending_wakes().unwrap().len(), before);
    let made = rpc(&r.core, Surface::Cli, method::WAKE_CREATE, call)
        .await
        .unwrap();
    assert_eq!(made["home"], json!(true), "{made}");
    assert_eq!(r.core.kernel.pending_wakes().unwrap().len(), before + 1);
}
