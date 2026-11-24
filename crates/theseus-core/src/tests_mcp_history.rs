//! The MCP server's `context_history` through the whole core (issue 46): the one method `history.read` on
//! `Surface::Mcp`, answering only while `[mcp_server] memory` is on, the history tool's own read with no turn.
//! Each test mirrors one of `tests_mcp_memory`'s for the old memory tools: the vault's values substituted in a
//! success and in an error; the surface refusal; the off setting; erased material never read; the audit row's
//! shape (a digest, never the words). And what the design adds: the anchor, and no turn's allowance.

use std::sync::Arc;

use serde_json::{json, Value};
use theseus_protocol::method;

use crate::approval::Surface;
use crate::tests_imports::{index_over, script};
use crate::tests_mcp_memory::rpc;
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
    for surface in [Surface::Web, Surface::Discord] {
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
            "history.read is the terminal's and the MCP server's: it reads the owner's memory, and this \
             surface may not",
            "{e:?}"
        );
    }
    assert!(rows(&on.core, "memory.read").is_empty());
    assert!(crate::mcp_server::allowed(method::HISTORY_READ));
}

/// The owner's terminal reads `history.read` (issue 44: `theseus history` with a search), whatever
/// `[mcp_server] memory` says, and its row names it `cli`; a web or chat surface is still refused, and an MCP
/// client with memory off still answers the refusal.
#[tokio::test]
async fn the_terminal_reads_history_and_a_web_or_chat_surface_is_still_refused() {
    let r = rig(false);
    let native = session(&r.core, None);
    turn(&r.core, &native, "the gull at the pier").await;
    let v = rpc(
        &r.core,
        Surface::Cli,
        method::HISTORY_READ,
        json!({"from": "2000-01-01"}),
    )
    .await
    .unwrap_or_else(|e| panic!("{e:?}"));
    assert!(v.to_string().contains("the gull at the pier"), "{v}");
    let written = rows(&r.core, "memory.read");
    assert_eq!(written.len(), 1, "{written:?}");
    assert_eq!(written[0]["principal"], "cli");
    assert_eq!(written[0]["tool"], "context_history");
    for surface in [Surface::Web, Surface::Discord] {
        let e = rpc(
            &r.core,
            surface,
            method::HISTORY_READ,
            json!({"from": "2000-01-01"}),
        )
        .await
        .unwrap_err();
        assert!(e.message.contains("this surface may not"), "{e:?}");
    }
    let e = ask(&r.core, json!({"from": "2000-01-01"}))
        .await
        .unwrap_err();
    assert_eq!(
        e.message,
        "memory is off for MCP clients (`[mcp_server] memory`)"
    );
}

/// A terminal's read is masked as an MCP one: a vault value in a line comes back substituted.
#[tokio::test]
async fn a_terminal_reads_reply_is_masked_as_an_mcp_one() {
    let r = rig(false);
    r.core
        .secrets
        .hold("marsh_vault", crate::secrets::Secret::new(SECRET.into()));
    let native = session(&r.core, None);
    turn(&r.core, &native, &format!("the ferry note: {SECRET}")).await;
    let v = rpc(
        &r.core,
        Surface::Cli,
        method::HISTORY_READ,
        json!({"from": "2000-01-01"}),
    )
    .await
    .unwrap_or_else(|e| panic!("{e:?}"));
    let said = v.to_string();
    assert!(!said.contains(SECRET), "{said}");
    assert!(said.contains("[redacted:marsh_vault]"), "{said}");
}

/// An erased import is read as nothing, with a control: before the erase, the import's session opens by its
/// id (an MCP caller's direct read of it) and shows its words; after the erase of that same import, the same
/// open shows none of them. A words search a stale index hit of the erased import comes first in still
/// answers the live native exchange: the erased hit takes no place in it (mirrors
/// `an_erased_import_is_read_as_nothing_by_session_or_by_words`). An import is on no exchange timeline, so no
/// page reaches it before or after.
#[tokio::test]
async fn an_erased_import_is_read_as_nothing() {
    use crate::tests_imports::{episode, import};
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
                outside: true,
                labels: Some(s()),
            }],
            end: s(),
            next: Some(cur()),
            open: Some(HistoryOpened {
                handle: s(),
                text: s(),
                outside: true,
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
        ("line.labels", &l["labels"]),
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
    // The same call, the same digest; a refused one (a bare page with no anchor) is a row too, not answered.
    history(&r.core, asked).await;
    ask(&r.core, json!({})).await.unwrap_err();
    let again = rows(&r.core, "memory.read");
    assert_eq!(again[2]["digest"], digest);
    assert_eq!((again.len(), &again[3]["ok"]), (4, &json!(false)));
}

/// The anchor: a bare page names a place or the client's conversation, or is refused in words; a words search
/// with neither reads every place (the owner's one rule: only a bare look-back stays in one place); `session_id`
/// reads where that conversation speaks, the same page as its place.
#[tokio::test]
async fn a_page_needs_a_place_or_a_conversation() {
    let r = rig(true);
    let e = ask(&r.core, json!({})).await.unwrap_err();
    assert_eq!(
        e.message,
        crate::history_tool::mcp::NO_ANCHOR,
        "{}",
        e.message
    );
    let native = session(&r.core, None);
    turn(&r.core, &native, "the first ferry note").await;
    r.core
        .runner
        .memory
        .set_ask(index_over(&r.core, vec![native.clone()]));
    let found = history(&r.core, json!({"words": "ferry"})).await;
    assert!(found.to_string().contains("first ferry note"), "{found}");
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

/// An outside agent is told when what it reads holds another person's words, as the model's result is marked
/// External (D6a round 2): the guest's line on a page and its open say `outside`; the owner's line does not.
#[tokio::test]
async fn a_line_and_an_open_that_hold_another_persons_words_say_outside() {
    use crate::exchange::{
        exchange_record, mark_record, touch, Exchange, Opener, OwnerMark, Peer, Root, OWNER,
    };
    use crate::node::Node;
    const PLACE: &str = "discord:channel:77";
    let r = rig(true);
    let sid = session(&r.core, None);
    let mut recs = Vec::new();
    for (i, person, words) in [
        (1u64, OWNER, "the owner's ferry question"),
        (2, "discord:7", "a guest's ferry words"),
    ] {
        let turn = format!("turn_mcp_{i}");
        let at = 1_000_000 + i * 1_000;
        let ask = Node::user(&sid, Some(&turn), person, words);
        let mark = OwnerMark {
            node: ask.id.clone(),
            in_person: person == OWNER,
            owner_handle: None,
            peer: if person == OWNER {
                Peer::Person
            } else {
                Peer::Transport
            },
            agent: None,
            surface: "cli".into(),
            place: PLACE.into(),
            arrived_session: sid.clone(),
            ran_session: sid.clone(),
            author: (person != OWNER).then(|| person.to_string()),
            at_ms: at,
        };
        let x = Exchange {
            turn_id: turn.clone(),
            session_id: sid.clone(),
            place: PLACE.into(),
            private: false,
            opened_by: Opener::Message,
            person: Some(person.into()),
            start_ms: at,
            end_ms: Some(at + 500),
            ask: Some(ask.id.clone()),
            reply: None,
            messages: vec![ask.id.clone()],
            stop: Some("end_turn".into()),
            rebuilt: false,
        };
        recs.push(ask.record().unwrap());
        recs.push(mark_record(&mark).unwrap());
        recs.push(exchange_record(&x).unwrap());
        recs.extend(touch(Root::Place(PLACE), &turn, &sid, at));
    }
    r.core.store.append(&recs).unwrap();
    let page = history(&r.core, json!({"place": PLACE})).await;
    let got = lines(&page);
    let line = |w: &str| {
        got.iter()
            .find(|l| l["words"].as_str().unwrap().contains(w))
            .unwrap_or_else(|| panic!("{w}: {page}"))
    };
    assert_eq!(line("guest's")["outside"], true, "{page}");
    assert_eq!(line("owner's")["outside"], false, "{page}");
    let handle = line("guest's")["handle"].as_str().unwrap().to_string();
    let opened = history(&r.core, json!({"open": handle})).await;
    assert_eq!(opened["open"]["outside"], true, "{opened}");
}

/// An exchange of `place` in `sid`: its ask said by `person` (marked), its reply `said`, its touch written.
pub(crate) fn exchange_said(
    core: &Core,
    sid: &str,
    (turn, at): (&str, u64),
    (place, person): (&str, &str),
    (words, said): (&str, &str),
) {
    use crate::exchange::{
        exchange_record, mark_record, touch, Exchange, Opener, OwnerMark, Peer, Root, OWNER,
    };
    use crate::node::{Body, Node};
    let ask = Node::user(sid, Some(turn), person, words);
    let answer = Node::assistant(
        sid,
        turn,
        0,
        Body::AssistantMessage {
            blocks: vec![json!({"type": "text", "text": said})],
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
    let mark = OwnerMark {
        node: ask.id.clone(),
        in_person: person == OWNER,
        owner_handle: None,
        peer: if person == OWNER {
            Peer::Person
        } else {
            Peer::Transport
        },
        agent: None,
        surface: "cli".into(),
        place: place.into(),
        arrived_session: sid.into(),
        ran_session: sid.into(),
        author: (person != OWNER).then(|| person.to_string()),
        at_ms: at,
    };
    let x = Exchange {
        turn_id: turn.into(),
        session_id: sid.into(),
        place: place.into(),
        private: false,
        opened_by: Opener::Message,
        person: Some(person.into()),
        start_ms: at,
        end_ms: Some(at + 500),
        ask: Some(ask.id.clone()),
        reply: Some(answer.id.clone()),
        messages: vec![ask.id.clone()],
        stop: Some("end_turn".into()),
        rebuilt: false,
    };
    let mut recs = vec![
        ask.record().unwrap(),
        answer.record().unwrap(),
        mark_record(&mark).unwrap(),
        exchange_record(&x).unwrap(),
    ];
    recs.extend(touch(Root::Place(place), turn, sid, at));
    core.store.append(&recs).unwrap();
}

/// A reply-only session page holds its source turn's input provenance for an outside agent too (D6 round 3,
/// A): a guest's long reply alone on the page (the guest's message on the page before) says `outside`; the
/// owner's same shape does not.
#[tokio::test]
async fn a_reply_only_session_page_says_outside_for_a_guests_turn() {
    let r = rig(true);
    let long = "the guest asked whether the ferry runs at dawn, and I said it does. ".repeat(600);
    let (guest, owner) = (session(&r.core, None), session(&r.core, None));
    exchange_said(
        &r.core,
        &guest,
        ("turn_guest_1", 1_000_000),
        ("discord:channel:77", "discord:7"),
        ("does the ferry run at dawn?", &long),
    );
    exchange_said(
        &r.core,
        &owner,
        ("turn_owner_1", 1_000_000),
        ("discord:dm:42", crate::exchange::OWNER),
        ("does the ferry run at dawn?", &long),
    );
    let page = history(&r.core, json!({"open": guest})).await;
    let text = page["open"]["text"].as_str().unwrap();
    assert!(
        text.contains("ferry runs at dawn") && !text.contains("does the ferry run at dawn?"),
        "{page}"
    );
    assert_eq!(page["open"]["outside"], true, "{page}");
    let page = history(&r.core, json!({"open": owner})).await;
    assert_eq!(page["open"]["outside"], false, "{page}");
}

/// A stand-in index that answers `stale` hits no longer on record (in `sid`, ranked first), then `sid`'s own
/// nodes.
fn stale_then_live(core: &Arc<Core>, sid: String, stale: usize) -> crate::recall::Ask {
    use theseus_protocol::index::{IndexHit, IndexQueryResult, IndexSourceRank};
    let store = core.store.clone();
    let hit =
        |node_id: String, session_id: String, position: u64, text: String, rank: usize| IndexHit {
            text,
            node_id,
            chunk: 0,
            session_id,
            position,
            kind: "user_message".into(),
            origin: "operator".into(),
            author: None,
            place: None,
            tool: None,
            time_ms: 0,
            external: false,
            entities_matched: vec![],
            entities: vec![],
            sources: [("bm25".to_string(), IndexSourceRank { rank, score: 1.0 })].into(),
            fused: 1.0 / (rank as f64 + 1.0),
        };
    Arc::new(move |_| -> crate::recall::AskFuture {
        let mut hits: Vec<IndexHit> = (0..stale)
            .map(|i| {
                hit(
                    format!("nd_gone_{i:04}"),
                    sid.clone(),
                    i as u64,
                    format!("the ferry note, number {i}"),
                    i + 1,
                )
            })
            .collect();
        for (k, (position, n)) in store.session_nodes(&sid).unwrap().into_iter().enumerate() {
            hits.push(hit(
                n.id.clone(),
                sid.clone(),
                position,
                crate::recall::text_of(&n),
                stale + k + 1,
            ));
        }
        Box::pin(async move {
            Ok(IndexQueryResult {
                hits,
                indexed_through: 0,
                lag: Default::default(),
                timings: Default::default(),
                skipped: Default::default(),
                weights: Default::default(),
            })
        })
    })
}

/// Stale hits that fill the search's bound end the page "may be incomplete", never "exhausted" (D6 round 3,
/// F): 120 hits no longer on record, ranked above the one live exchange, fill the words search; the live
/// exchange below them may be lost, so the page says the search may be incomplete.
#[tokio::test]
async fn stale_hits_that_fill_the_bound_end_the_search_incomplete() {
    let r = rig(true);
    let sid = session(&r.core, None);
    turn(&r.core, &sid, "the ferry note that is still here").await;
    let place = place_of(&r.core, &sid);
    r.core
        .runner
        .memory
        .set_ask(stale_then_live(&r.core, sid.clone(), 120));
    let v = history(&r.core, json!({"place": place, "words": "ferry note"})).await;
    assert_eq!(v["end"], "search_cut", "{v}");
}

/// A words search whose vector search fails (or is late) answers from the word sources alone, `words_only`
/// (recall-fallback): the history read takes their hits as an answer, not as an index that did not answer
/// (retargeted from `memory.lookup`'s `words_only_hits_are_an_answer` when that tool went, D6 step 6).
#[tokio::test]
async fn words_only_hits_are_an_answer() {
    let r = rig(true);
    let native = session(&r.core, None);
    turn(&r.core, &native, "the ferry note about the ledger").await;
    // A distractor outside the span asked for: an exchange of the same place in March 2026.
    let place = place_of(&r.core, &native);
    let march = crate::wake::parse_at("2026-03-03T18:00:00Z").unwrap();
    exchange_said(
        &r.core,
        &native,
        ("turn_old_ledger", march),
        (&place, crate::exchange::OWNER),
        ("the old ledger of March", "Noted."),
    );
    let words = index_over(&r.core, vec![native.clone()]);
    r.core.runner.memory.set_ask(Arc::new(
        move |p: theseus_protocol::index::IndexQueryParams| -> crate::recall::AskFuture {
            if p.sources.iter().any(|s| s == "vector") {
                return Box::pin(async { Err("the query would not embed".to_string()) });
            }
            words(p)
        },
    ));
    let yesterday = crate::judge::spend::local_day(theseus_protocol::now_unix_ms() - 86_400_000);
    let found = history(
        &r.core,
        json!({"session_id": native, "words": "ledger", "from": yesterday}),
    )
    .await;
    assert!(
        lines(&found)
            .iter()
            .any(|l| l["words"].as_str().unwrap().contains("ferry note")),
        "{found}"
    );
    assert!(
        !found.to_string().contains("old ledger"),
        "the distractor outside the span is absent: {found}"
    );
}

/// An imported episode whose words are `says`, in `tag`, numbered `n`.
fn tagged(n: usize, tag: &str, at: &str, says: &str) -> Value {
    let mut v = crate::tests_imports::episode(n, "public", at, says);
    v["import_tag"] = json!(tag);
    v["hash"] = json!(crate::import::episode::hash_of(&v));
    v
}

/// The handles of a page's lines.
fn handles(v: &Value) -> Vec<String> {
    lines(v)
        .iter()
        .map(|l| l["handle"].as_str().unwrap().to_string())
        .collect()
}

/// Imported conversations are found by words through the history read (D6 step 6, round 2: the plan's
/// parity step 4, `place: "import"`): a hit names the session to open, its message's own date and its book;
/// the same search after the import is erased finds nothing (`memory.lookup`'s erased-import test by words,
/// with its positive control).
#[tokio::test]
async fn imported_words_are_found_and_once_erased_are_nothing() {
    use crate::tests_imports::{episodes, import, SHOWN};
    let r = rig(true);
    let ids = import(&r.core, &episodes());
    r.core
        .runner
        .memory
        .set_ask(index_over(&r.core, ids.clone()));
    let found = history(&r.core, json!({"place": "import", "words": "parser"})).await;
    let hit = lines(&found)
        .iter()
        .find(|l| l["handle"] == json!(ids[1]) && l["words"].as_str().unwrap().contains(SHOWN))
        .unwrap_or_else(|| panic!("the imported message is found: {found}"));
    assert_eq!(
        hit["at_ms"],
        json!(crate::wake::parse_at("2026-03-10T18:00:00Z").unwrap()),
        "its message's own date: {hit}"
    );
    assert_eq!(hit["place"], json!("import:casebook"), "its book: {hit}");
    // The session it names opens.
    let opened = history(&r.core, json!({"open": ids[1]})).await;
    assert!(
        opened["open"]["text"].as_str().unwrap().contains(SHOWN),
        "{opened}"
    );
    crate::import::write::erase(&r.core.store, "kelp-2026", Some("a test"), "cli").unwrap();
    let after = history(&r.core, json!({"place": "import", "words": "parser"})).await;
    assert!(
        lines(&after).is_empty(),
        "an erased import is nothing: {after}"
    );
    assert!(!after.to_string().contains("tide-table"), "{after}");
}

/// Stale hits ranked first, below the bound, cost no live one: an erased import's hits come first, and the
/// pages that follow give every live item exactly once (`memory.lookup`'s stale-first paging test).
#[tokio::test]
async fn stale_imported_hits_ranked_first_cost_no_live_one_across_pages() {
    use crate::tests_imports::import;
    let r = rig(true);
    let at = "2026-03-10T18:00:00Z";
    let old: Vec<Value> = (0..3)
        .map(|n| tagged(n, "old-2025", at, "the kelp gauge"))
        .collect();
    let live: Vec<Value> = (10..22)
        .map(|n| tagged(n, "kelp-2026", at, "the kelp gauge"))
        .collect();
    let gone = import(&r.core, &old);
    let kept = import(&r.core, &live);
    crate::import::write::erase(&r.core.store, "old-2025", Some("a test"), "cli").unwrap();
    let mut all = gone.clone();
    all.extend(kept.clone());
    r.core.runner.memory.set_ask(index_over(&r.core, all));
    let (mut seen, mut before, mut pages) = (Vec::<String>::new(), None::<Value>, 0);
    let mut items: Vec<(String, String)> = Vec::new();
    loop {
        let mut args = json!({"place": "import", "words": "gauge"});
        if let Some(b) = &before {
            args["before"] = b.clone();
        }
        let page = history(&r.core, args).await;
        pages += 1;
        for l in lines(&page) {
            let item = (l["handle"].to_string(), l["words"].to_string());
            assert!(!items.contains(&item), "an item twice: {item:?}");
            items.push(item);
        }
        seen.extend(handles(&page));
        match page["end"].as_str() {
            Some("more") => before = Some(page["next"].clone()),
            end => {
                assert_eq!(
                    end,
                    Some("history_exhausted"),
                    "the last page says that is all: {page}"
                );
                break;
            }
        }
        assert!(pages < 20, "the pages end: {page}");
    }
    assert!(pages > 1, "more than one page");
    let mut sessions = seen.clone();
    sessions.sort();
    sessions.dedup();
    let mut want = kept.clone();
    want.sort();
    assert_eq!(
        sessions, want,
        "every live import, and no erased one: {seen:?}"
    );
    assert!(gone.iter().all(|g| !seen.contains(g)), "{seen:?}");
}

/// Words kept to a span: the September episode, outside March, is absent (`memory.lookup`'s month test).
#[tokio::test]
async fn imported_words_keep_to_their_span() {
    use crate::tests_imports::{episodes, import};
    let r = rig(true);
    let ids = import(&r.core, &episodes());
    r.core
        .runner
        .memory
        .set_ask(index_over(&r.core, ids.clone()));
    let march = history(
        &r.core,
        json!({"place": "import", "words": "Message", "when": "March 2026"}),
    )
    .await;
    let got = handles(&march);
    assert!(got.contains(&ids[1]) && got.contains(&ids[3]), "{march}");
    assert!(
        !got.contains(&ids[4]),
        "the September episode is outside March: {march}"
    );
}

/// A volatile value read back through the history tool is never a bare fact (M6 §2.11): an imported session
/// opened by its id, and the same message found by words, carry `recall::render::volatile_at`'s mark, dated by
/// the message's own day; the stable message is unmarked (`memory.lookup`'s volatile test).
#[tokio::test]
async fn a_volatile_value_comes_back_as_of_the_day_it_was_true() {
    use crate::tests_imports::import;
    const MOVES: &str = "the gauge pipeline currently fails 3 tests";
    const STANDS: &str = "the gauge pipeline reads a fixed-width column";
    let at = |d: u32| format!("2026-03-{d:02}T18:00:00Z");
    let msg = |idx: u32, text: &str| {
        json!({"idx": idx, "time": at(10 + idx), "author": "wren", "integrity": "operator",
               "text": text, "unit": format!("u-v-{idx}"), "sha256": "ab".repeat(32)})
    };
    let mut ep = crate::tests_imports::episode(40, "public", &at(10), "the gauge pipeline");
    ep["as_of"] = json!({"start": at(10), "end": at(11)});
    ep["messages"] = json!([msg(0, MOVES), msg(1, STANDS)]);
    ep["hash"] = json!(crate::import::episode::hash_of(&ep));
    let r = rig(true);
    let ids = import(&r.core, &[ep]);
    let day = crate::judge::spend::local_day(crate::wake::parse_at(&at(10)).unwrap());
    let mark = format!("volatile: as of {day}, unverified");
    let opened = history(&r.core, json!({"open": ids[0]})).await;
    let text = opened["open"]["text"].as_str().unwrap();
    assert!(text.contains(MOVES) && text.contains(STANDS), "{text}");
    assert_eq!(text.matches("volatile: as of").count(), 1, "{text}");
    assert!(text.contains(&mark), "{text}");
    r.core
        .runner
        .memory
        .set_ask(index_over(&r.core, ids.clone()));
    let found = history(&r.core, json!({"place": "import", "words": "gauge"})).await;
    let moves = lines(&found)
        .iter()
        .find(|l| l["words"].as_str().unwrap().contains(MOVES))
        .unwrap_or_else(|| panic!("{found}"));
    assert!(moves["words"].as_str().unwrap().contains(&mark), "{moves}");
    let stands = lines(&found)
        .iter()
        .find(|l| l["words"].as_str().unwrap().contains(STANDS))
        .unwrap_or_else(|| panic!("{found}"));
    assert!(
        !stands["words"].as_str().unwrap().contains("volatile"),
        "{stands}"
    );
}

/// `index_over`, honouring the session filter a search narrowed to, as the real index does.
fn index_filtered(core: &Arc<Core>, sessions: Vec<String>) -> crate::recall::Ask {
    let all = index_over(core, sessions);
    Arc::new(
        move |p: theseus_protocol::index::IndexQueryParams| -> crate::recall::AskFuture {
            let only = p.filters.sessions.clone();
            let asked = all(p);
            Box::pin(async move {
                let mut r = asked.await?;
                if !only.is_empty() {
                    r.hits.retain(|h| only.contains(&h.session_id));
                }
                Ok(r)
            })
        },
    )
}

/// An imported episode of `messages` (each `(integrity, author, text)`), in March 2026.
fn spoken_by(n: usize, messages: &[(&str, &str, &str)]) -> Value {
    let at = "2026-03-03T18:00:00Z";
    let mut v = crate::tests_imports::episode(n, "personal", at, "the kelp survey");
    v["messages"] = json!(messages
        .iter()
        .enumerate()
        .map(|(i, (integrity, author, text))| json!({"idx": i, "time": at, "author": author,
            "integrity": integrity, "text": text, "unit": format!("u-{n}-{i}"), "sha256": "ab".repeat(32)}))
        .collect::<Vec<_>>());
    v["hash"] = json!(crate::import::episode::hash_of(&v));
    v
}

/// The search's outside mark is the session reader's rule (D6 step 6, round 3, B2): an imported message of
/// outside text is outside; an agent's message in an episode that holds outside text is outside too (it may
/// derive from it); the operator's own and an agent's beside the operator alone are not.
#[tokio::test]
async fn an_import_hit_is_outside_by_the_session_readers_rule() {
    use crate::tests_imports::import;
    let r = rig(true);
    let ids = import(
        &r.core,
        &[
            spoken_by(
                51,
                &[
                    (
                        "outside",
                        "a stranger",
                        "a stranger's note on the kelp beds",
                    ),
                    (
                        "agent",
                        "main",
                        "Noted: the kelp beds are thinning, the note says.",
                    ),
                ],
            ),
            spoken_by(
                52,
                &[
                    ("operator", "wren", "count the kelp beds"),
                    ("agent", "main", "Counted: eleven kelp beds."),
                ],
            ),
        ],
    );
    r.core
        .runner
        .memory
        .set_ask(index_over(&r.core, ids.clone()));
    let found = history(&r.core, json!({"place": "import", "words": "kelp"})).await;
    let mark = |says: &str| {
        lines(&found)
            .iter()
            .find(|l| l["words"].as_str().unwrap().contains(says))
            .unwrap_or_else(|| panic!("{says}: {found}"))["outside"]
            .as_bool()
            .unwrap()
    };
    assert!(mark("a stranger's note"), "outside text: {found}");
    assert!(
        mark("are thinning"),
        "an agent beside outside text: {found}"
    );
    assert!(!mark("count the kelp beds"), "the operator's own: {found}");
    assert!(
        !mark("eleven kelp beds"),
        "an agent beside the operator alone: {found}"
    );
}

/// An unqualified import search ranks over every import (D6 step 6, round 3, B3): an episode older than the
/// newest 500 is found. A book whose catalog is larger than the page the search narrows to says the search is
/// incomplete, never that it is all.
#[tokio::test]
async fn an_import_older_than_the_newest_500_is_found_and_a_cut_book_says_so() {
    use crate::tests_imports::import;
    let r = rig(true);
    let mut eps = vec![tagged(
        0,
        "kelp-2026",
        "2020-01-01T00:00:00Z",
        "the oldest OWL-LEDGER",
    )];
    eps.extend(
        (1..=500).map(|n| tagged(n, "kelp-2026", "2026-03-10T18:00:00Z", "a newer kelp note")),
    );
    let ids = import(&r.core, &eps);
    r.core
        .runner
        .memory
        .set_ask(index_filtered(&r.core, ids.clone()));
    let found = history(&r.core, json!({"place": "import", "words": "OWL-LEDGER"})).await;
    assert!(
        handles(&found).contains(&ids[0]),
        "the oldest episode is found: {}",
        found["end"]
    );
    // The book's search is narrowed to the catalog's newest 500: its last page says the search is incomplete.
    let mut before: Option<Value> = None;
    for _ in 0..20 {
        let mut args = json!({"place": "import:casebook", "words": "OWL-LEDGER"});
        if let Some(b) = &before {
            args["before"] = b.clone();
        }
        let page = history(&r.core, args).await;
        match page["end"].as_str() {
            Some("more") => before = Some(page["next"].clone()),
            end => {
                assert_eq!(
                    end,
                    Some("search_cut"),
                    "the book's cut is said, never that it is all"
                );
                return;
            }
        }
    }
    panic!("the book's pages end");
}

/// A native page marks every message-derived field it shows (D6 step 6, round 3, B4): a stable ask and a
/// volatile reply mark the reply, by the reply's own day; an ask's mark is dated by its own message, not by its
/// exchange's start.
#[tokio::test]
async fn a_native_page_marks_a_volatile_reply_and_dates_each_mark_by_its_source() {
    const MOVES: &str = "the gauge pipeline currently fails 3 tests";
    const PLACE: &str = "channel:gauges";
    let r = rig(true);
    let sid = session(&r.core, None);
    let now = theseus_protocol::now_unix_ms();
    let today = crate::judge::spend::local_day(now);
    exchange_said(
        &r.core,
        &sid,
        ("turn_gauge_1", now),
        (PLACE, crate::exchange::OWNER),
        ("what is the gauge status?", MOVES),
    );
    let page = history(&r.core, json!({ "place": PLACE })).await;
    let line = &lines(&page)[0];
    let reply = line["reply_begins"].as_str().unwrap();
    assert!(reply.contains(MOVES), "{line}");
    assert!(
        reply.contains(&format!("volatile: as of {today}, unverified")),
        "the reply's mark: {line}"
    );
    assert!(
        !line["words"].as_str().unwrap().contains("volatile"),
        "the stable ask: {line}"
    );
    // An exchange that started in March, its message written today: the mark is the message's own day.
    let march = crate::wake::parse_at("2026-03-03T18:00:00Z").unwrap();
    let other = "channel:gauges-two";
    exchange_said(
        &r.core,
        &sid,
        ("turn_gauge_2", march),
        (other, crate::exchange::OWNER),
        (MOVES, "Noted."),
    );
    let page = history(&r.core, json!({ "place": other })).await;
    let words = lines(&page)[0]["words"].as_str().unwrap().to_string();
    assert!(
        words.contains(&format!("volatile: as of {today}, unverified")),
        "dated by the message, not by the exchange's start: {words}"
    );
}

/// The import place takes no `label`, as the native page refuses a bad pair (D6 step 6, round 3).
#[tokio::test]
async fn an_import_place_takes_no_label() {
    let r = rig(true);
    let got = ask(
        &r.core,
        json!({"place": "import", "words": "kelp", "label": "topic:kelp"}),
    )
    .await;
    assert!(got.is_err(), "{got:?}");
}

/// A forged import cursor far past any page is answered, not a panic or a wrapped offset (D6 step 6, round 3).
#[tokio::test]
async fn a_forged_import_cursor_is_answered() {
    use crate::tests_imports::{episodes, import};
    let r = rig(true);
    let ids = import(&r.core, &episodes());
    r.core.runner.memory.set_ask(index_over(&r.core, ids));
    let got = history(
        &r.core,
        json!({"place": "import", "words": "parser",
               "before": {"at_ms": u64::MAX, "turn_id": "import"}}),
    )
    .await;
    assert!(lines(&got).is_empty(), "{got}");
}

/// A span with no place and no conversation reads every place (issue 44): each line names its own place; a
/// bare page with neither is still refused.
#[tokio::test]
async fn history_read_with_a_span_and_no_anchor_reads_every_place() {
    let r = rig(true);
    let here = session(&r.core, None);
    let there = session(&r.core, Some("channel:5"));
    turn(&r.core, &here, "the gull at the pier").await;
    turn(&r.core, &there, "the heron on the reef").await;
    let (a, b) = (place_of(&r.core, &here), place_of(&r.core, &there));
    assert_ne!(a, b, "two places");
    let v = history(&r.core, json!({"from": "2000-01-01"})).await;
    let got = lines(&v);
    assert_eq!(got.len(), 2, "{v}");
    assert!(got[0]["words"].as_str().unwrap().contains("heron"), "{v}");
    assert_eq!(got[0]["place"], b.as_str(), "{v}");
    assert_eq!(got[1]["place"], a.as_str(), "{v}");
    assert_eq!(v["end"], "span_start", "{v}");
}

/// A fork's page of every place by date holds nothing an ancestor said at or after its point, whatever place
/// it reads; another place's exchange is on it.
#[tokio::test]
async fn an_mcp_forks_span_page_holds_nothing_past_its_point_in_any_place() {
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
    let elsewhere = session(&r.core, Some("channel:5"));
    turn(&r.core, &elsewhere, "the heron on the reef").await;
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
    let v = history(&r.core, json!({"session_id": fork, "from": "2000-01-01"})).await;
    let said = v.to_string();
    assert!(said.contains("first tide note"), "{v}");
    assert!(said.contains("heron on the reef"), "another place: {v}");
    assert!(!said.contains("second tide note"), "past the point: {v}");
}

/// KEY: an old caller's `thing` is refused at the daemon in words that name `label`, before any history is read
/// (no `memory.read` row), whether the call names a place or a date, on the MCP connection and at the terminal;
/// never answered as a different search. The vault's values are substituted in the refusal.
#[tokio::test]
async fn history_read_refuses_thing_in_words_before_reading() {
    let r = rig(true);
    r.core
        .secrets
        .hold("marsh_vault", crate::secrets::Secret::new(SECRET.into()));
    let here = session(&r.core, None);
    turn(&r.core, &here, "the gull at the pier").await;
    let place = place_of(&r.core, &here);
    let before = rows(&r.core, "memory.read").len();
    for surface in [Surface::Mcp, Surface::Cli] {
        for params in [
            json!({"place": place, "thing": "topic:kelp"}),
            json!({"from": "2000-01-01", "thing": SECRET}),
        ] {
            let e = rpc(&r.core, surface, method::HISTORY_READ, params.clone())
                .await
                .unwrap_err();
            assert_eq!(
                e.code,
                theseus_protocol::error_code::INVALID_PARAMS,
                "{params}: {}",
                e.message
            );
            assert!(
                e.message.contains("thing is now label"),
                "{params}: {}",
                e.message
            );
            assert!(!e.message.contains(SECRET), "{}", e.message);
        }
    }
    assert_eq!(
        rows(&r.core, "memory.read").len(),
        before,
        "no history was read"
    );
}
