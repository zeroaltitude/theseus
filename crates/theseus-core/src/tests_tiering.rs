//! Tiering through the whole core (M6 step 33, theseus-6fn.13; design
//! §2.10 and 33's rows in §3.1 and §3.2): the heat cache shared across turns
//! and readers, so a session's next turn decodes only its new nodes.

use std::sync::Arc;

use crate::config::MemoryMode;
use crate::tests_recall::{rig_with, session, turn};
use crate::Core;

/// The store's count of node decodes.
fn decodes(core: &Core) -> u64 {
    core.store.node_cache().decodes()
}

/// A session's nodes, counted without a decode (the records' keys).
fn count(core: &Arc<Core>, sid: &str) -> u64 {
    use theseus_store::Store as _;
    core.store
        .inner()
        .scan_scope(sid, 0, usize::MAX)
        .unwrap()
        .iter()
        .filter(|r| r.kind == theseus_store::kinds::NODE)
        .count() as u64
}

/// A long session's second turn decodes only its new nodes: every node a
/// reader decoded before is served from the cache by position, the turn's
/// first read of its transcript included. With the cache off, each turn
/// decodes the session whole again.
#[tokio::test]
async fn decodes_fall_on_a_long_session() {
    let mut per_turn = Vec::new();
    for mb in [crate::node_cache::DEFAULT_MB, 0] {
        let r = rig_with(MemoryMode::Off, |c| c.memory.node_cache_mb = mb);
        let c = &r.core;
        let said: Vec<String> = (0..60)
            .map(|i| format!("Note {i}: the lamp at the north pier was lit at dusk."))
            .collect();
        let said: Vec<&str> = said.iter().map(String::as_str).collect();
        let sid = session(c, None, &said);
        turn(c, &sid, "the first turn reads the session").await;
        let (n0, d0) = (count(c, &sid), decodes(c));
        turn(c, &sid, "the second turn").await;
        let (n1, d1) = (count(c, &sid), decodes(c));
        let new = n1 - n0;
        assert!(new >= 2, "the input and the answer: {new}");
        per_turn.push((d1 - d0, new, n1));
    }
    let (cached, new, _) = per_turn[0];
    assert_eq!(cached, new, "with the cache, only the turn's new nodes");
    let (off, _, all) = per_turn[1];
    assert!(off >= all, "without it, the session whole: {off} of {all}");
}

// ---------------------------------------------------------------- stubs

use serde_json::{json, Value};

use crate::catalog::{Catalog, CatalogRow};
use crate::compiler::{render_request, Compilation, Recompile, RequestSpec};
use crate::provider::Scripted;
use crate::session::SessionRecord;
use crate::stub::Stub;
use crate::tests_recall::{index_of, Rig};

const HERON: &str = "Remember: the grey heron nests by the old weir at Millbrook.";

/// The session's model's window in these tests, and its output cap: a few
/// long turns pass it, so the session rings or compacts.
fn small_window(c: &mut crate::Config, mb: u64, summary: &str) {
    c.memory.node_cache_mb = mb;
    c.memory.summary_profile = summary.into();
    c.catalog.insert(
        "claude-sonnet-5-5".into(),
        CatalogRow {
            context_window: Some(40_000),
            max_output_tokens: Some(2_000),
            ..Default::default()
        },
    );
}

/// A message of about `tokens` tokens of prose, its first word `tag`.
fn words(tag: &str, tokens: usize) -> String {
    let mut s = format!("{tag} ");
    while s.len() < tokens * 4 {
        s.push_str("the keeper logged the tide and the wind before the lamp was lit. ");
    }
    s
}

/// The rows of `kind`, oldest first.
fn rows(core: &Core, kind: &str) -> Vec<Value> {
    let rows: Vec<(u64, crate::ledger::LedgerRow)> = core.store.ledger_tail(5000).unwrap();
    rows.into_iter()
        .filter(|(_, r)| r.kind == kind)
        .map(|(_, r)| r.data)
        .collect()
}

/// A turn, with an operator's recompile when `recompile` says.
async fn turn_with(core: &Arc<Core>, sid: &str, input: &str, recompile: Option<Recompile>) {
    let rec: SessionRecord = core.store.get_session(sid).unwrap().unwrap();
    let (live, _) = core.live_profile();
    let target = core.runner.resolve_target(&live, None, None, None).unwrap();
    let sink = crate::bus::EventSink::new(core.bus.clone(), sid, None);
    core.runner
        .run(crate::turn::TurnRequest {
            prompt: None,
            session: rec,
            input: Some(input.into()),
            target,
            sink,
            author: "test".into(),
            recompile,
            attachments: vec![],
            arrived: None,
            reply_to: None,
        })
        .await
        .unwrap();
}

/// Ids (`ses_` and 32 hex digits) as the order they first appear in, and
/// clock times as `<time>`: two cores' runs of one script then compare.
fn normalized(s: &str) -> String {
    let b = s.as_bytes();
    let (mut out, mut seen) = (String::new(), Vec::<String>::new());
    let mut i = 0;
    while i < b.len() {
        let word = b[i..].iter().take_while(|c| c.is_ascii_lowercase()).count();
        let hex = |from: usize| {
            b[from..]
                .iter()
                .take_while(|c| c.is_ascii_hexdigit())
                .count()
        };
        let starts = i == 0 || !b[i - 1].is_ascii_alphanumeric();
        if starts && word > 0 && b.get(i + word) == Some(&b'_') && hex(i + word + 1) >= 32 {
            let end = i + word + 1 + 32;
            let id = &s[i..end];
            let n = seen.iter().position(|x| x == id).unwrap_or_else(|| {
                seen.push(id.to_string());
                seen.len() - 1
            });
            out.push_str(&format!("<id{n}>"));
            i = end;
            continue;
        }
        let date = |at: usize| {
            b.len() >= at + 10
                && b[at..at + 10].iter().enumerate().all(|(k, c)| {
                    if k == 4 || k == 7 {
                        *c == b'-'
                    } else {
                        c.is_ascii_digit()
                    }
                })
        };
        if date(i) {
            let mut end = i + 10;
            if s[end..].starts_with(' ') && s.len() >= end + 6 && s.as_bytes()[end + 3] == b':' {
                end += 6;
            }
            out.push_str("<time>");
            i = end;
            continue;
        }
        let ch = s[i..].chars().next().unwrap();
        out.push(ch);
        i += ch.len_utf8();
    }
    out
}

/// What the model was sent, each request's system, messages, and tools.
fn sent(r: &Rig) -> Vec<String> {
    r.model
        .requests()
        .iter()
        .map(|q| normalized(&serde_json::to_string(&(&q.system, &q.messages, &q.tools)).unwrap()))
        .collect()
}

/// One script through whole turns, recall live: a plain turn that recalls
/// session A's heron, a tool call, turns until the session compacts, and
/// two after. The core, its model, and session B.
async fn scenario(mb: u64) -> (Rig, String) {
    let r = rig_with(MemoryMode::Live, |c| small_window(c, mb, "session"));
    let c = &r.core;
    let a = session(c, None, &[HERON]);
    let b = session(c, None, &[]);
    c.runner.memory.set_ask(index_of(c, vec![a]));
    turn(c, &b, "Where does the grey heron nest?").await;
    r.model.script.lock().unwrap().extend([
        Scripted::tools("", &[("t1", "text_diff", json!({"a": "x\n", "b": "y\n"}))]),
        Scripted::text("Diffed."),
    ]);
    turn(c, &b, "diff these two").await;
    for k in 0..12 {
        turn(c, &b, &words(&format!("turn{k}"), 6_000)).await;
        if rows(c, "context.compiled")
            .iter()
            .any(|row| row["strategy"] == "compaction")
        {
            break;
        }
    }
    turn(c, &b, "And the heron again?").await;
    turn(c, &b, "One more.").await;
    (r, b)
}

/// Every request is byte for byte the same with the cache off
/// (`node_cache_mb = 0`), through whole turns: plain, a tool call, a
/// compaction, recall live, and the turns after it. And the turns after the
/// compaction left the summarized range stubs, decoding none of it.
#[tokio::test]
async fn every_request_is_the_same_with_the_cache_off() {
    let (on, b_on) = scenario(crate::node_cache::DEFAULT_MB).await;
    let (off, _) = scenario(0).await;
    let (a, b) = (sent(&on), sent(&off));
    assert!(a.len() >= 8, "{} requests", a.len());
    assert_eq!(a.len(), b.len());
    for (i, (x, y)) in a.iter().zip(&b).enumerate() {
        assert_eq!(x, y, "request {i} differs with the cache off");
    }
    let compiled = rows(&on.core, "context.compiled");
    assert!(
        compiled.iter().any(|r| r["strategy"] == "compaction"),
        "it compacted"
    );
    assert!(
        a.iter().any(|q| q.contains("grey heron nests")),
        "recall fed the model"
    );
    assert!(a.iter().any(|q| q.contains("text_diff")), "a tool call");
    let last = compiled.last().unwrap();
    assert!(
        last["stubs"].as_u64().unwrap_or(0) > 0,
        "the summarized range stays stubs: {last}"
    );
    let n = |k: &str| last[k].as_u64().unwrap_or(0);
    assert!(
        n("decoded") <= n("prefix_nodes") + n("tail_nodes"),
        "it reads past a stub only what it renders: {last}"
    );
    renders_the_same_from_stubs(&on.core, &b_on);
}

/// The session's current compilation, rendered from a transcript of stubs
/// and from one decoded whole: the same request, and the stubs left some
/// nodes undecoded.
fn renders_the_same_from_stubs(core: &Arc<Core>, sid: &str) {
    let rec: SessionRecord = core.store.get_session(sid).unwrap().unwrap();
    let c: Compilation = core
        .store
        .get_compilation(rec.compilation_id.as_deref().unwrap())
        .unwrap()
        .unwrap();
    let stubs = core.store.transcript(sid).unwrap();
    let whole: Vec<(u64, Stub)> = core
        .store
        .session_nodes(sid)
        .unwrap()
        .into_iter()
        .map(|(p, n)| (p, Stub::hydrated(p, Arc::new(n))))
        .collect();
    let spec = RequestSpec {
        profile: "p".into(),
        provider: "anthropic".into(),
        model: c.manifest.model.clone(),
        max_tokens: 1000,
        system_text: "s".into(),
        context_text: String::new(),
        context_files: vec![],
        persona: None,
        tools: vec![],
        effort: None,
        thinking_display: Default::default(),
        refusal_fallbacks: false,
        first_party: true,
        cache_ttl: Default::default(),
        conversation_ttl: Default::default(),
        walk: None,
        memberships: vec![],
        guidance: vec![],
    };
    let sources = core.runner.memory.read_sources(
        &core.store,
        whole
            .iter()
            .filter(|(_, n)| n.kind == crate::stub::Kind::Recall)
            .map(|(_, n)| &**n),
    );
    let catalog = Catalog::builtin();
    let media = (None, &[][..], None, &sources);
    let from_stubs = render_request(&spec, &catalog, &c, &stubs, media);
    let from_whole = render_request(&spec, &catalog, &c, &whole, media);
    assert_eq!(from_stubs.request.body(), from_whole.request.body());
    let left = stubs.iter().filter(|(_, n)| !n.is_hydrated()).count();
    assert!(
        left > 0,
        "a render past a compaction leaves the summarized range stubs"
    );
}

/// The debug check that a turn's kept transcript equals the store's holds
/// through appends, an operator's recompile, and a ring (with compaction
/// off): every read in these turns asserts it in a test build. After the
/// ring, the turns decode only their new nodes, and what the ring cut stays
/// stubs.
#[tokio::test]
async fn the_transcript_check_holds_through_a_ring_and_a_recompile() {
    let r = rig_with(MemoryMode::Off, |c| small_window(c, 0, "off"));
    let c = &r.core;
    let sid = session(c, None, &[]);
    turn_with(c, &sid, "hello", None).await;
    turn_with(c, &sid, "recompile, please", Some(Recompile::Transcript)).await;
    for k in 0..12 {
        turn_with(c, &sid, &words(&format!("turn{k}"), 6_000), None).await;
        if rows(c, "context.compiled")
            .iter()
            .any(|row| row["strategy"] == "ring")
        {
            break;
        }
    }
    turn_with(c, &sid, "after the ring", None).await;
    let compiled = rows(c, "context.compiled");
    assert!(compiled.iter().any(|r| r["trigger"] == "manual_transcript"));
    assert!(compiled.iter().any(|r| r["strategy"] == "ring"), "it rang");
    let last = compiled.last().unwrap();
    assert!(
        last["stubs"].as_u64().unwrap_or(0) > 0,
        "the ring's cut stays stubs: {last}"
    );
    let n = |k: &str| last[k].as_u64().unwrap_or(0);
    assert!(
        n("decoded") <= n("prefix_nodes") + n("tail_nodes"),
        "{last}"
    );
}

/// A stub rehydrates on reference: session A's heron, written before A's
/// compaction's floor (so in A's transcript a stub for good), is recalled
/// into B by its position, and renders the bytes the store holds, with the
/// cache on and off.
#[tokio::test]
async fn a_recall_source_from_before_a_floor_renders_the_same_bytes() {
    for mb in [crate::node_cache::DEFAULT_MB, 0] {
        let r = rig_with(MemoryMode::Live, |c| small_window(c, mb, "session"));
        let c = &r.core;
        let a = session(c, None, &[HERON]);
        let b = session(c, None, &[]);
        for k in 0..12 {
            turn(c, &a, &words(&format!("turn{k}"), 6_000)).await;
            if rows(c, "context.compiled")
                .iter()
                .any(|row| row["strategy"] == "compaction")
            {
                break;
            }
        }
        let nodes = c.store.transcript(&a).unwrap();
        let (_, last) = crate::compiler::compaction::floor(&nodes).expect("A compacted");
        let (pos, heron) = &nodes[0];
        assert!(*pos <= last, "the heron is before the floor");
        assert!(!heron.is_hydrated(), "and a stub there");
        let stored = c.store.session_nodes(&a).unwrap().remove(0).1;
        let read = c.store.node_at(*pos, &heron.id).unwrap().unwrap();
        assert_eq!(*read, stored, "by position, the node the store holds");
        assert_eq!(heron.body, stored.body, "the stub rehydrates to it");
        // B recalls it, by its position, and the model reads its bytes.
        let all = index_of(c, vec![a.clone()]);
        c.runner
            .memory
            .set_ask(Arc::new(move |p| -> crate::recall::AskFuture {
                let asked = all(p);
                Box::pin(async move {
                    let mut res = asked.await?;
                    res.hits.retain(|h| h.text.contains("grey heron"));
                    Ok(res)
                })
            }));
        let res = turn(c, &b, "Where does the grey heron nest?").await;
        assert_eq!(res.recalled, 1);
        let q = r.model.requests().pop().unwrap();
        let text = serde_json::to_string(&q.messages).unwrap();
        assert!(text.contains(&format!("\\\"{HERON}\\\"")), "{text}");
    }
}
