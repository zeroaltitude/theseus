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

/// With `[mcp_server] memory` off the call answers the same refusal as the memory tools, and writes no row;
/// another surface is refused as the MCP server's (mirrors
/// `memory_off_answers_the_refusal_and_other_surfaces_are_refused`).
#[tokio::test]
async fn memory_off_answers_the_refusal_and_other_surfaces_are_refused() {
    let r = rig(false);
    let e = ask(&r.core, json!({"place": "local"})).await.unwrap_err();
    assert_eq!(
        e.message,
        "memory is off for MCP clients (`[mcp_server] memory`)"
    );
    assert!(rows(&r.core, "memory.read").is_empty());
    let on = rig(true);
    for surface in [Surface::Cli, Surface::Discord] {
        let e = rpc(
            &on.core,
            surface,
            method::HISTORY_READ,
            json!({"place": "local"}),
        )
        .await
        .unwrap_err();
        assert_eq!(
            e.message,
            "history.read is the MCP server's: it reads the owner's memory for another agent",
            "{e:?}"
        );
    }
    assert!(crate::mcp_server::allowed(method::HISTORY_READ));
}

/// An erased import is never read: its words are on no page, a words search finds nothing of it, and its
/// node opens nothing (mirrors `an_erased_import_is_read_as_nothing_by_session_or_by_words`).
#[tokio::test]
async fn an_erased_import_is_read_as_nothing() {
    use crate::tests_memory_lookup::{episode, import};
    const GONE: &str = "the lighthouse keeper's ledger";
    let r = rig(true);
    let imported = import(
        &r.core,
        &[episode(9, "public", "2026-03-03T18:00:00Z", GONE)],
    );
    crate::import::write::erase(&r.core.store, "kelp-2026", Some("a test"), "cli").unwrap();
    let native = session(&r.core, None);
    turn(&r.core, &native, "the ferry note about the ledger").await;
    let mut all = imported.clone();
    all.push(native.clone());
    r.core.runner.memory.set_ask(index_over(&r.core, all));
    for p in [
        json!({"place": place_of(&r.core, &native)}),
        json!({"session_id": native, "words": "lighthouse keeper ledger"}),
    ] {
        let v = history(&r.core, p).await;
        assert!(!v.to_string().contains("lighthouse"), "{v}");
    }
    let node = (r
        .core
        .store
        .session_nodes(&imported[0])
        .unwrap()
        .into_iter())
    .map(|(_, n)| n.id)
    .next()
    .expect("its tombstone");
    let e = ask(&r.core, json!({"open": node})).await.unwrap_err();
    assert!(!e.message.contains("lighthouse"), "{}", e.message);
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

/// Each call is one `memory.read` row: the principal `mcp`, the tool `context_history`, the client, a digest
/// of what was asked, how many lines answered (an open is one): never the words (mirrors
/// `each_read_is_a_memory_read_row_with_a_digest_and_no_words`).
#[tokio::test]
async fn each_read_is_a_memory_read_row_with_a_digest_and_no_words() {
    let r = rig(true);
    let native = session(&r.core, None);
    turn(&r.core, &native, "the ferry parser note").await;
    r.core
        .runner
        .memory
        .set_ask(index_over(&r.core, vec![native.clone()]));
    let asked = json!({"session_id": native, "words": "ferry parser"});
    let page = history(&r.core, asked.clone()).await;
    let handle = lines(&page)[0]["handle"].as_str().unwrap().to_string();
    history(&r.core, json!({"open": handle})).await;
    let written = rows(&r.core, "memory.read");
    assert_eq!(written.len(), 2, "{written:?}");
    assert_eq!(written[0]["principal"], "mcp");
    assert_eq!(written[0]["tool"], "context_history");
    assert_eq!(written[0]["client"], "lantern-agent");
    assert_eq!(
        written[0]["hits"].as_u64().unwrap(),
        lines(&page).len() as u64
    );
    assert_eq!(written[0]["ok"], true);
    assert_eq!(
        (&written[1]["tool"], &written[1]["hits"]),
        (&json!("context_history"), &json!(1))
    );
    let digest = written[0]["digest"].as_str().unwrap();
    assert_eq!(digest.len(), 16);
    for row in &written {
        let all = row.to_string();
        assert!(!all.contains("ferry") && !all.contains("parser"), "{all}");
    }
    // The same call, the same digest; a refused one is a row too, not answered.
    history(&r.core, asked).await;
    ask(&r.core, json!({"words": "ferry"})).await.unwrap_err();
    let again = rows(&r.core, "memory.read");
    assert_eq!(again[2]["digest"], digest);
    assert_eq!((again.len(), &again[3]["ok"]), (4, &json!(false)));
}

/// The anchor: a page or a words search names a place or the client's conversation, or is refused in words;
/// `session_id` reads where that conversation speaks, the same page as its place.
#[tokio::test]
async fn a_page_needs_a_place_or_a_conversation() {
    let r = rig(true);
    for p in [json!({}), json!({"words": "ferry"})] {
        let e = ask(&r.core, p).await.unwrap_err();
        assert_eq!(
            e.message,
            crate::history_tool::mcp::NO_ANCHOR,
            "{}",
            e.message
        );
    }
    let native = session(&r.core, None);
    turn(&r.core, &native, "the first ferry note").await;
    let place = place_of(&r.core, &native);
    let by_place = history(&r.core, json!({"place": place})).await;
    let by_session = history(&r.core, json!({"session_id": native})).await;
    assert_eq!(
        by_place["lines"], by_session["lines"],
        "{by_place} / {by_session}"
    );
    assert_eq!(by_session["place"], place.as_str());
}

/// No turn, no turn's allowance: nothing is in view, so a bare page starts at the newest exchange, and calls
/// past a turn's five looks are pages as the first was, never "allowance used".
#[tokio::test]
async fn no_turn_means_the_newest_first_and_no_allowance() {
    let r = rig(true);
    let native = session(&r.core, None);
    turn(&r.core, &native, "the first ferry note").await;
    turn(&r.core, &native, "the second ferry note").await;
    let place = place_of(&r.core, &native);
    for _ in 0..(crate::view::history::LOOKS_PER_TURN + 2) {
        let v = history(&r.core, json!({"place": place})).await;
        let got = lines(&v);
        assert_eq!(got.len(), 2, "{v}");
        assert!(got[0]["words"].as_str().unwrap().contains("second"), "{v}");
        assert_eq!(v["end"], "history_exhausted", "{v}");
        assert!(v.get("next").is_none(), "{v}");
    }
}
