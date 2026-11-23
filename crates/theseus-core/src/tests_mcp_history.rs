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

/// An erased import is read as nothing, with a control: before the erase, the import's session opens by its
/// id (an MCP caller's direct read of it) and shows its words; after the erase of that same import, the same
/// open shows none of them. A words search a stale index hit of the erased import comes first in still
/// answers the live native exchange: the erased hit takes no place in it (mirrors
/// `an_erased_import_is_read_as_nothing_by_session_or_by_words`). An import is on no exchange timeline, so no
/// page reaches it before or after.
#[tokio::test]
async fn an_erased_import_is_read_as_nothing() {
    use crate::tests_memory_lookup::{episode, import};
    const GONE: &str = "the lighthouse keeper's ledger";
    let r = rig(true);
    let imported = import(
        &r.core,
        &[episode(9, "public", "2026-03-03T18:00:00Z", GONE)],
    );
    let opened = history(&r.core, json!({"open": imported[0]})).await;
    assert!(
        opened["open"]["text"].as_str().unwrap().contains(GONE),
        "the control: before the erase it reads: {opened}"
    );
    crate::import::write::erase(&r.core.store, "kelp-2026", Some("a test"), "cli").unwrap();
    let after = ask(&r.core, json!({"open": imported[0]})).await;
    if let Ok(v) = &after {
        assert!(!v.to_string().contains("lighthouse"), "{v}");
    }
    let native = session(&r.core, None);
    turn(&r.core, &native, "the ferry note about the ledger").await;
    // The stale hit first, then the live one.
    let mut all = imported.clone();
    all.push(native.clone());
    r.core.runner.memory.set_ask(index_over(&r.core, all));
    let found = history(&r.core, json!({"session_id": native, "words": "ledger"})).await;
    assert!(!found.to_string().contains("lighthouse"), "{found}");
    assert!(
        lines(&found)
            .iter()
            .any(|l| l["words"].as_str().unwrap().contains("ferry note")),
        "the live exchange still answers: {found}"
    );
}

/// Every string of a reply passes the vault's substitution, not only what was said (review of D6b, §1): a
/// line's handle, place, time and words, the opened handle and text, the page's place and end, and each
/// cursor's turn id. Numbers pass as they are.
#[tokio::test]
async fn every_string_of_a_reply_is_substituted_field_by_field() {
    use theseus_protocol::memory::{
        HistoryCursor, HistoryOpened, HistoryReadLine, HistoryReadResult,
    };
    let r = rig(true);
    r.core
        .secrets
        .hold("marsh_vault", crate::secrets::Secret::new(SECRET.into()));
    let s = || format!("x {SECRET} y");
    let cur = || HistoryCursor {
        at_ms: 7,
        turn_id: s(),
    };
    let got = crate::history_tool::mcp::substituted(
        &r.core,
        HistoryReadResult {
            place: s(),
            lines: vec![HistoryReadLine {
                handle: s(),
                place: s(),
                at_ms: 5,
                when: s(),
                who: s(),
                words: s(),
                did: Some(s()),
                reply_begins: Some(s()),
            }],
            end: s(),
            next: Some(cur()),
            open: Some(HistoryOpened {
                handle: s(),
                text: s(),
                next: Some(cur()),
            }),
        },
    );
    let v = serde_json::to_value(&got).unwrap();
    let l = &v["lines"][0];
    let o = &v["open"];
    for (name, field) in [
        ("place", &v["place"]),
        ("end", &v["end"]),
        ("next.turn_id", &v["next"]["turn_id"]),
        ("line.handle", &l["handle"]),
        ("line.place", &l["place"]),
        ("line.when", &l["when"]),
        ("line.who", &l["who"]),
        ("line.words", &l["words"]),
        ("line.did", &l["did"]),
        ("line.reply_begins", &l["reply_begins"]),
        ("open.handle", &o["handle"]),
        ("open.text", &o["text"]),
        ("open.next.turn_id", &o["next"]["turn_id"]),
    ] {
        assert_eq!(field, &json!("x [redacted:marsh_vault] y"), "{name}: {v}");
    }
    assert_eq!(
        (&l["at_ms"], &v["next"]["at_ms"], &o["next"]["at_ms"]),
        (&json!(5), &json!(7), &json!(7))
    );
}

/// The same on the real path: a vault value that equals a turn's id leaves neither as a line's handle nor as an
/// open's; one that equals a line's time leaves not as its `when`. A handle the substitution changed no longer
/// navigates: that is the right failure.
#[tokio::test]
async fn a_vault_value_equal_to_a_handle_or_a_time_is_substituted() {
    let r = rig(true);
    let native = session(&r.core, None);
    turn(&r.core, &native, "the ferry note").await;
    let place = place_of(&r.core, &native);
    let page = history(&r.core, json!({"place": place})).await;
    let handle = lines(&page)[0]["handle"].as_str().unwrap().to_string();
    let when = lines(&page)[0]["when"].as_str().unwrap().to_string();
    r.core
        .secrets
        .hold("turn_vault", crate::secrets::Secret::new(handle.clone()));
    r.core
        .secrets
        .hold("time_vault", crate::secrets::Secret::new(when.clone()));
    let page = history(&r.core, json!({"place": place})).await;
    let l = &lines(&page)[0];
    assert!(!page.to_string().contains(&handle), "{page}");
    assert!(
        l["handle"]
            .as_str()
            .unwrap()
            .contains("[redacted:turn_vault]"),
        "{page}"
    );
    assert!(
        l["when"]
            .as_str()
            .unwrap()
            .contains("[redacted:time_vault]"),
        "{page}"
    );
    let opened = history(&r.core, json!({"open": handle})).await;
    assert!(!opened.to_string().contains(&handle), "{opened}");
    assert!(
        opened["open"]["handle"]
            .as_str()
            .unwrap()
            .contains("[redacted:turn_vault]"),
        "{opened}"
    );
}

/// A fork an MCP client made of its own conversation reads its own history as the model's call in it does
/// (review of D6b, §4): an ancestor's exchange before the fork's point is on its page, and the ones the point
/// falls in or that began after it are neither on its page nor opened by their handle; the source itself
/// still reads them all.
#[tokio::test]
async fn an_mcp_fork_reads_its_history_without_what_came_after_its_point() {
    use theseus_protocol::fork::SessionForkParams;
    let r = rig(true);
    let s = r
        .core
        .mcp_open(theseus_protocol::SessionOpenParams {
            label: Some("mcp lab".into()),
            ..Default::default()
        })
        .unwrap()
        .session_id;
    turn(&r.core, &s, "the first tide note").await;
    turn(&r.core, &s, "the second tide note").await;
    turn(&r.core, &s, "the third tide note").await;
    let point = (r.core.store.session_nodes(&s).unwrap().into_iter())
        .find(|(_, n)| {
            matches!(&n.body, crate::node::Body::UserMessage { text, .. } if text.contains("second"))
        })
        .expect("the second message")
        .1
        .id;
    let fork = r
        .core
        .session_fork(
            SessionForkParams {
                session: s.clone(),
                at: point,
                ..Default::default()
            },
            Surface::Mcp,
        )
        .unwrap()
        .session_id;
    let source = history(&r.core, json!({"session_id": s})).await;
    let third = (lines(&source).iter())
        .find(|l| l["words"].as_str().unwrap().contains("third"))
        .unwrap_or_else(|| panic!("the source reads its third: {source}"))["handle"]
        .as_str()
        .unwrap()
        .to_string();
    let forked = history(&r.core, json!({"session_id": fork})).await;
    let said = forked.to_string();
    assert!(
        said.contains("first tide note"),
        "before the point: {forked}"
    );
    assert!(
        !said.contains("second tide note") && !said.contains("third tide note"),
        "{forked}"
    );
    let e = ask(&r.core, json!({"session_id": fork, "open": third}))
        .await
        .unwrap_err();
    assert!(e.message.contains("can be opened here"), "{}", e.message);
    // Its words search holds none of them either.
    r.core
        .runner
        .memory
        .set_ask(index_over(&r.core, vec![s.clone()]));
    let found = history(&r.core, json!({"session_id": fork, "words": "tide note"})).await;
    assert!(!found.to_string().contains("third tide note"), "{found}");
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
