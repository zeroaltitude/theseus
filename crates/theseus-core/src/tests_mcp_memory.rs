//! The MCP server's memory tools through the whole core (theseus-02i1h): the
//! one method `memory.lookup` on `Surface::Mcp`, answering only while
//! `[mcp_server] memory` is on. A hit carries its citation; a shared place's
//! nodes are never returned; veiled text stays veiled; `get` reads a node
//! with its neighbours or a session in order; a page has a cursor; every
//! other memory method and every approval stays refused; and each read is a
//! `memory.read` row that holds a digest and no words.

use std::sync::Arc;

use serde_json::{json, Value};
use theseus_protocol::method;

use crate::approval::Surface;
use crate::tests_memory_lookup::{episodes, import, index_over, script, HIDDEN, SHOWN};
use crate::tests_places::{rig_scripted, session, turn, Rig, LAB};
use crate::Core;

/// What the owner's own session said, and what a shared place's said.
const NATIVE: &str = "the ferry timetable decision was the seven o'clock boat";
const SHARED: &str = "the ferry timetable gossip from the shared channel";

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

async fn ask(core: &Arc<Core>, params: Value) -> Result<Value, theseus_protocol::RpcError> {
    rpc(core, Surface::Mcp, method::MEMORY_LOOKUP, params).await
}

async fn search(core: &Arc<Core>, extra: Value) -> Value {
    let mut p = json!({"op": "search", "client": "lantern-agent"});
    for (k, v) in extra.as_object().unwrap() {
        p[k] = v.clone();
    }
    ask(core, p).await.unwrap()
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

/// The owner's own session, a shared channel's session, and the imported
/// episodes, all behind one stand-in index.
struct Held {
    native: String,
    shared: String,
    imported: Vec<String>,
}

async fn held(r: &Rig) -> Held {
    let imported = import(&r.core, &episodes());
    let native = session(&r.core, None);
    turn(&r.core, &native, NATIVE).await;
    let shared = session(&r.core, Some(&format!("channel:{LAB}")));
    turn(&r.core, &shared, SHARED).await;
    let mut all = imported.clone();
    all.extend([native.clone(), shared.clone()]);
    r.core.runner.memory.set_ask(index_over(&r.core, all));
    Held {
        native,
        shared,
        imported,
    }
}

fn hits(v: &Value) -> &Vec<Value> {
    v["hits"].as_array().unwrap_or_else(|| panic!("{v}"))
}

/// A search returns a native and an imported hit, each with its citation
/// (the node's id and WAL position, its session, its as-of minute), its
/// origin and harness, and its sensitivity; the citation names the node as
/// the store holds it.
#[tokio::test]
async fn a_search_returns_native_and_imported_hits_with_citations() {
    let r = rig(true);
    let h = held(&r).await;
    let got = search(&r.core, json!({"words": "ferry parser", "limit": 25})).await;
    let native = hits(&got)
        .iter()
        .find(|x| x["citation"]["session_id"] == h.native.as_str() && x["text"] == NATIVE)
        .unwrap_or_else(|| panic!("{got}"));
    assert_eq!(native["origin"], "native");
    assert_eq!(native["harness"], "theseus");
    let imported = hits(&got)
        .iter()
        .find(|x| x["citation"]["session_id"] == h.imported[1].as_str())
        .unwrap_or_else(|| panic!("{got}"));
    assert_eq!(imported["origin"], "imported");
    assert_eq!(imported["harness"], "claude-cli");
    assert_eq!(imported["sensitivity"], "company-confidential");
    assert_eq!(imported["book"], "casebook");
    assert!(
        imported["text"].as_str().unwrap().contains(SHOWN),
        "{imported}"
    );
    for hit in [native, imported] {
        let c = &hit["citation"];
        let (pos, node) = r
            .core
            .store
            .get_node(c["node_id"].as_str().unwrap())
            .unwrap()
            .expect("the citation names a stored node");
        assert_eq!(c["position"].as_u64().unwrap(), pos, "{c}");
        assert_eq!(c["session_id"], node.session_id.as_str());
        let day = crate::judge::spend::local_day(node.created_at_ms);
        assert!(c["as_of"].as_str().unwrap().starts_with(&day), "{c}");
    }
}

/// A shared place's node is never returned: not by a search, not by `get`
/// of its id, not by `get` of its session.
#[tokio::test]
async fn a_shared_places_node_is_never_returned() {
    let r = rig(true);
    let h = held(&r).await;
    let got = search(&r.core, json!({"words": "ferry", "limit": 25})).await;
    assert!(
        hits(&got).iter().any(|x| x["text"] == NATIVE),
        "the private one is there: {got}"
    );
    let all = got.to_string();
    assert!(!all.contains(SHARED) && !all.contains(&h.shared), "{all}");
    // The shared session's own nodes by id and by session.
    let node = r
        .core
        .store
        .session_nodes(&h.shared)
        .unwrap()
        .into_iter()
        .find(|(_, n)| crate::recall::text_of(n) == SHARED)
        .map(|(_, n)| n)
        .unwrap();
    for p in [
        json!({"op": "get", "id": node.id}),
        json!({"op": "get", "session": h.shared}),
    ] {
        let e = ask(&r.core, p).await.unwrap_err();
        assert!(e.message.contains("no remembered node or session"), "{e:?}");
        assert!(!e.message.contains(SHARED), "{e:?}");
    }
}

/// A veiled hit shows its labels and never its text: not in a search, not
/// in a `get` of its node or its session.
#[tokio::test]
async fn a_veiled_hits_text_is_veiled() {
    let r = rig(true);
    let h = held(&r).await;
    let got = search(&r.core, json!({"words": "keeper parser", "limit": 25})).await;
    assert!(!got.to_string().contains(HIDDEN), "{got}");
    for (i, label) in [(0, "personal"), (2, "partner-confidential")] {
        let x = hits(&got)
            .iter()
            .find(|x| x["citation"]["session_id"] == h.imported[i].as_str())
            .unwrap_or_else(|| panic!("{got}"));
        assert_eq!(x["veiled"], true, "{x}");
        assert_eq!(x["sensitivity"], label);
        assert!(x["text"].as_str().unwrap().contains("veiled"), "{x}");
        let id = x["citation"]["node_id"].as_str().unwrap();
        let by_id = ask(&r.core, json!({"op": "get", "id": id})).await.unwrap();
        let by_session = ask(&r.core, json!({"op": "get", "session": h.imported[i]}))
            .await
            .unwrap();
        for v in [by_id, by_session] {
            assert!(!v.to_string().contains(HIDDEN), "{v}");
            assert!(hits(&v).iter().all(|x| x["veiled"] == true), "{v}");
        }
    }
    // An unveiled one's text shows.
    assert!(got.to_string().contains(SHOWN), "{got}");
}

/// `get` by a citation's id returns that node, flagged, with its session's
/// neighbours in order, a few on each side.
#[tokio::test]
async fn get_by_id_returns_the_node_and_its_neighbours() {
    let r = rig(true);
    let h = held(&r).await;
    turn(&r.core, &h.native, "second thing the owner said").await;
    turn(&r.core, &h.native, "third thing the owner said").await;
    let got = search(&r.core, json!({"words": "ferry", "limit": 25})).await;
    let id = hits(&got).iter().find(|x| x["text"] == NATIVE).unwrap()["citation"]["node_id"]
        .as_str()
        .unwrap()
        .to_string();
    let v = ask(
        &r.core,
        json!({"op": "get", "id": id, "before": 0, "after": 2}),
    )
    .await
    .unwrap();
    let texts: Vec<&str> = hits(&v)
        .iter()
        .map(|x| x["text"].as_str().unwrap())
        .collect();
    assert_eq!(texts[0], NATIVE, "{v}");
    assert_eq!(hits(&v).len(), 3, "{v}");
    assert_eq!(hits(&v)[0]["target"], true);
    assert!(
        hits(&v)[1..].iter().all(|x| x.get("target").is_none()),
        "{v}"
    );
    let pos: Vec<u64> = hits(&v)
        .iter()
        .map(|x| x["citation"]["position"].as_u64().unwrap())
        .collect();
    assert!(pos.windows(2).all(|w| w[0] < w[1]), "{pos:?}");
    // The default is two on each side, and only this session's.
    let v = ask(&r.core, json!({"op": "get", "id": id})).await.unwrap();
    assert!(hits(&v).len() >= 3 && hits(&v).len() <= 5, "{v}");
    assert!(hits(&v)
        .iter()
        .all(|x| x["citation"]["session_id"] == h.native.as_str()));
    // An id nothing holds, and both an id and a session.
    let e = ask(&r.core, json!({"op": "get", "id": "nod_nothing"}))
        .await
        .unwrap_err();
    assert!(e.message.contains("no remembered node"), "{e:?}");
    let e = ask(&r.core, json!({"op": "get", "id": id, "session": h.native}))
        .await
        .unwrap_err();
    assert!(e.message.contains("an id"), "{e:?}");
}

/// A page has a cursor: the next page begins where the last ended, with
/// no hit twice; the last page has none. A session pages the same way.
#[tokio::test]
async fn paging_follows_the_cursor() {
    let r = rig(true);
    let h = held(&r).await;
    let first = search(&r.core, json!({"words": "parser ferry", "limit": 2})).await;
    assert_eq!(hits(&first).len(), 2, "{first}");
    let next = first["next"].as_u64().unwrap_or_else(|| panic!("{first}"));
    assert_eq!(next, 2);
    let second = search(
        &r.core,
        json!({"words": "parser ferry", "limit": 2, "cursor": next}),
    )
    .await;
    let key = |v: &Value| -> Vec<String> {
        hits(v)
            .iter()
            .map(|x| x["citation"]["node_id"].as_str().unwrap().to_string())
            .collect()
    };
    assert!(!hits(&second).is_empty(), "{second}");
    assert!(key(&first).iter().all(|k| !key(&second).contains(k)));
    let mut cursor = 0;
    let mut seen = 0;
    loop {
        let page = search(
            &r.core,
            json!({"words": "parser ferry", "limit": 3, "cursor": cursor}),
        )
        .await;
        seen += hits(&page).len();
        match page["next"].as_u64() {
            Some(n) => cursor = n,
            None => break,
        }
    }
    assert_eq!(seen as u64, first["total"].as_u64().unwrap());
    // A session's messages, in order, a page at a time.
    let p1 = ask(
        &r.core,
        json!({"op": "get", "session": h.native, "limit": 1}),
    )
    .await
    .unwrap();
    assert_eq!(hits(&p1).len(), 1, "{p1}");
    assert_eq!(hits(&p1)[0]["text"], NATIVE);
    let p2 = ask(
        &r.core,
        json!({"op": "get", "session": h.native, "limit": 1, "cursor": p1["next"]}),
    )
    .await
    .unwrap();
    assert_eq!(hits(&p2)[0]["text"], "Done.", "{p2}");
}

/// With `[mcp_server] memory` off, both tools answer the refusal and read
/// nothing; any other surface is refused whatever the key says.
#[tokio::test]
async fn memory_off_answers_the_refusal_and_other_surfaces_are_refused() {
    let r = rig(false);
    held(&r).await;
    for p in [
        json!({"op": "search", "words": "ferry"}),
        json!({"op": "get", "session": "ses_x"}),
    ] {
        let e = ask(&r.core, p).await.unwrap_err();
        assert_eq!(
            e.message,
            "memory is off for MCP clients (`[mcp_server] memory`)"
        );
    }
    assert!(rows(&r.core, "memory.read").is_empty());
    let on = rig(true);
    held(&on).await;
    for surface in [Surface::Cli, Surface::Discord] {
        let e = rpc(
            &on.core,
            surface,
            method::MEMORY_LOOKUP,
            json!({"op": "search", "words": "ferry"}),
        )
        .await
        .unwrap_err();
        assert!(e.message.contains("the MCP server's"), "{e:?}");
    }
}

/// The MCP connection may call no other memory method, and none of the
/// owner's approvals counts.
#[tokio::test]
async fn the_mcp_connection_is_still_refused_every_other_memory_method_and_every_approval() {
    let r = rig(true);
    for m in [
        method::MEMORY_LABEL,
        method::MEMORY_CONSOLIDATE,
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
    assert!(crate::mcp_server::allowed(method::MEMORY_LOOKUP));
    for m in crate::mcp_server::JUDGED {
        let e = rpc(&r.core, Surface::Mcp, m, json!({})).await.expect_err(m);
        assert!(!e.message.is_empty(), "{m}");
    }
}

/// Each call that reads is one `memory.read` row: the principal `mcp`, the
/// tool, the client, a digest of the words, how many hits: never the words.
#[tokio::test]
async fn each_read_is_a_memory_read_row_with_a_digest_and_no_words() {
    let r = rig(true);
    let h = held(&r).await;
    let got = search(&r.core, json!({"words": "ferry parser", "limit": 25})).await;
    ask(&r.core, json!({"op": "get", "session": h.native}))
        .await
        .unwrap();
    let written = rows(&r.core, "memory.read");
    assert_eq!(written.len(), 2, "{written:?}");
    assert_eq!(written[0]["principal"], "mcp");
    assert_eq!(written[0]["tool"], "memory_search");
    assert_eq!(written[0]["client"], "lantern-agent");
    assert_eq!(
        written[0]["hits"].as_u64().unwrap(),
        hits(&got).len() as u64
    );
    assert_eq!(written[0]["ok"], true);
    assert_eq!(written[1]["tool"], "memory_get");
    assert_eq!(written[1]["hits"], 2);
    let digest = written[0]["digest"].as_str().unwrap();
    assert_eq!(digest.len(), 16);
    for row in &written {
        let all = row.to_string();
        assert!(!all.contains("ferry") && !all.contains("parser"), "{all}");
    }
    // The same words, the same digest.
    search(&r.core, json!({"words": " ferry parser "})).await;
    let again = rows(&r.core, "memory.read");
    assert_eq!(again[2]["digest"], digest);
}
