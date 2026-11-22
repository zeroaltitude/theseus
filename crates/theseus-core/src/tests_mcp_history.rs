//! The MCP server's `context_history` through the whole core (issue 46): the one method `history.read` on
//! `Surface::Mcp`, answering only while `[mcp_server] memory` is on, the history tool's own read with no turn.
//! Each test mirrors one of `tests_mcp_memory`'s for the old memory tools: the vault's values substituted in a
//! success and in an error; the surface refusal; the off setting; erased material never read; the audit row's
//! shape (a digest, never the words). And what the design adds: the anchor, and no turn's allowance.

use std::sync::Arc;

use serde_json::{json, Value};
use theseus_protocol::method;

use crate::approval::Surface;
use crate::tests_mcp_memory::rpc;
use crate::tests_memory_lookup::{index_over, script};
use crate::tests_places::{rig_scripted, session, turn, Rig};
use crate::Core;

const SECRET: &str = "marsh-kelp-vault-value-77";

fn rig(memory: bool) -> Rig {
    rig_scripted(move |c| c.mcp_server.memory = memory, script)
}

/// `history.read` on an MCP connection, as the server's `context_history` asks it.
async fn ask(core: &Arc<Core>, mut params: Value) -> Result<Value, theseus_protocol::RpcError> {
    params["client"] = json!("lantern-agent");
    rpc(core, Surface::Mcp, method::HISTORY_READ, params).await
}

async fn history(core: &Arc<Core>, params: Value) -> Value {
    ask(core, params).await.unwrap_or_else(|e| panic!("{e:?}"))
}

/// Where `sid`'s first exchange was, as its record names it: the place a page reads.
fn place_of(core: &Core, sid: &str) -> String {
    let turn = (core.store.session_nodes(sid).unwrap().into_iter())
        .find_map(|(_, n)| n.turn_id)
        .expect("a turn");
    crate::exchange::exchange(&core.store, &turn)
        .unwrap()
        .expect("its exchange")
        .place
}

fn lines(v: &Value) -> &Vec<Value> {
    v["lines"].as_array().unwrap_or_else(|| panic!("{v}"))
}

/// A value the vault holds is substituted in every string of a reply: plain and in its base64 and percent
/// encodings, on a bare page, on a words page and in an open (mirrors
/// `a_vault_value_is_substituted_in_every_mcp_memory_reply`).
#[tokio::test]
async fn a_vault_value_is_substituted_in_every_history_reply() {
    use base64::Engine as _;
    let b64 = base64::engine::general_purpose::STANDARD.encode(SECRET);
    let enc = SECRET.replace('-', "%2D");
    let said = format!("plain {SECRET} b64 {b64} pct {enc}");
    let r = rig(true);
    r.core
        .secrets
        .hold("marsh_vault", crate::secrets::Secret::new(SECRET.into()));
    let native = session(&r.core, None);
    turn(&r.core, &native, &format!("the ferry note: {said}")).await;
    r.core
        .runner
        .memory
        .set_ask(index_over(&r.core, vec![native.clone()]));
    let raw_gone = |v: &Value| {
        let t = v.to_string();
        for raw in [SECRET, b64.as_str(), enc.as_str()] {
            assert!(!t.contains(raw), "{raw} came back: {t}");
        }
    };
    let clean = |v: &Value| {
        raw_gone(v);
        assert!(v.to_string().contains("[redacted:marsh_vault]"), "{v}");
    };
    let page = history(&r.core, json!({"place": place_of(&r.core, &native)})).await;
    clean(&page);
    let handle = lines(&page)[0]["handle"].as_str().unwrap().to_string();
    let opened = history(&r.core, json!({"open": handle})).await;
    clean(&opened);
    assert!(
        opened["open"]["text"]
            .as_str()
            .unwrap()
            .contains("the ferry note"),
        "{opened}"
    );
    let found = history(
        &r.core,
        json!({"session_id": native, "words": "ferry note"}),
    )
    .await;
    clean(&found);
}

/// An error's words carry the vault's values substituted: the anchor's refusal that echoes the session asked,
/// and a decode error that echoes the value it could not read (mirrors
/// `an_mcp_error_and_the_status_reply_carry_the_vault_value_substituted`).
#[tokio::test]
async fn a_history_error_carries_the_vault_value_substituted() {
    let r = rig(true);
    r.core
        .secrets
        .hold("marsh_vault", crate::secrets::Secret::new(SECRET.into()));
    let e = ask(&r.core, json!({"session_id": SECRET}))
        .await
        .unwrap_err();
    assert!(e.message.starts_with("no session "), "{}", e.message);
    assert!(
        !e.message.contains(SECRET) && e.message.contains("[redacted:marsh_vault]"),
        "{}",
        e.message
    );
    // `before` given as the vault's value, a string where a cursor is decoded.
    let e = ask(&r.core, json!({"place": "local", "before": SECRET}))
        .await
        .unwrap_err();
    assert_eq!(
        e.code,
        theseus_protocol::error_code::INVALID_PARAMS,
        "{}",
        e.message
    );
    assert!(
        !e.message.contains(SECRET) && e.message.contains("[redacted:marsh_vault]"),
        "{}",
        e.message
    );
}
