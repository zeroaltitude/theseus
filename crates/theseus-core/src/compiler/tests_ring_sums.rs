//! The ring's guess by segment sums (theseus-0l3g): over generated
//! sessions, the cut it picks is the one the walk over every cut picked
//! before it, the earliest whose own render is under the target, and a long
//! session's ring renders a few candidates and decodes the kept turns, not
//! the session.

use proptest::prelude::*;
use serde_json::{json, Value};
use theseus_protocol::Usage;

use super::*;
use crate::node::{Body, ResultStatus};

fn spec() -> RequestSpec {
    RequestSpec {
        profile: "p".into(),
        provider: "anthropic".into(),
        model: "claude-opus-5".into(),
        max_tokens: 1000,
        system_text: "S".into(),
        context_text: String::new(),
        session_text: String::new(),
        persona_text: String::new(),
        card: None,
        cold: None,
        context_files: vec![],
        persona: None,
        tools: vec![
            json!({"name": "fs_read", "description": "d", "input_schema": {"type": "object"}}),
        ],
        effort: Some(Effort::High),
        turn_effort: None,
        effort_refused: vec![],
        thinking_display: ThinkingDisplay::Summarized,
        refusal_fallbacks: true,
        fallback: None,
        first_party: true,
        cache_ttl: CacheTtl::FiveMinutes,
        conversation_ttl: CacheTtl::FiveMinutes,
        walk: None,
        defer: None,
        memberships: vec![],
        guidance: vec![],
    }
}

fn answer(turn: &str, blocks: Vec<Value>) -> Node {
    Node::assistant(
        "s",
        turn,
        0,
        Body::AssistantMessage {
            blocks,
            model: "claude-opus-5".into(),
            provider: "anthropic".into(),
            stop_reason: None,
            usage: Usage::default(),
            cost_usd: None,
            catalog_version: None,
            request_id: None,
            correlation_id: None,
            compilation_id: None,
            request_digest: None,
            effort: None,
        },
    )
}

fn result(id: &str, content: String, late: bool) -> Node {
    Node::tool_result(
        "s",
        None,
        None,
        Body::ToolResult {
            tool_use_id: id.into(),
            tool: "fs.read".into(),
            status: ResultStatus::Ok,
            is_error: false,
            content,
            correlation_id: None,
            bytes_total: 0,
            truncated: false,
            full_ref: None,
            duration_ms: None,
            late,
            meta: json!({}),
            image: None,
            external: None,
        },
    )
}

/// One exchange's shape: its message's length, its calls' results'
/// lengths, whether its answer thinks (stripped by the ring), whether a late
/// result follows it, and whether it ends unanswered (the next message then
/// merges with it).
#[derive(Debug, Clone)]
struct Exchange {
    said: usize,
    results: Vec<usize>,
    thinks: bool,
    late: bool,
    unanswered: bool,
}

fn exchange() -> impl Strategy<Value = Exchange> {
    (
        1usize..400,
        prop::collection::vec(1usize..1_900, 0..3),
        any::<bool>(),
        prop::bool::weighted(0.1),
        prop::bool::weighted(0.1),
    )
        .prop_map(|(said, results, thinks, late, unanswered)| Exchange {
            said,
            results,
            thinks,
            late,
            unanswered,
        })
}

/// The session's nodes, `(position, node)` from 1.
fn session(xs: &[Exchange]) -> Vec<(u64, Node)> {
    let mut nodes = Vec::new();
    for (k, x) in xs.iter().enumerate() {
        let turn = format!("t{k}");
        let mut m = Node::user(
            "s",
            Some(&turn),
            "web",
            &format!("m{k} {}", "w".repeat(x.said)),
        );
        m.turn_id = Some(turn.clone());
        nodes.push(m);
        if x.unanswered {
            continue;
        }
        for (j, size) in x.results.iter().enumerate() {
            let id = format!("c{k}_{j}");
            let mut blocks = vec![];
            if x.thinks {
                blocks.push(json!({"type": "thinking", "thinking": "t".repeat(300), "signature": "s".repeat(200)}));
            }
            blocks.push(json!({"type": "tool_use", "id": id, "name": "fs_read", "input": {"path": format!("f{k}")}}));
            nodes.push(answer(&turn, blocks));
            nodes.push(result(&id, "r".repeat(*size), false));
        }
        nodes.push(answer(
            &turn,
            vec![json!({"type": "text", "text": format!("done {k}")})],
        ));
        if x.late {
            nodes.push(result(&format!("c{k}_late"), "l".repeat(90), true));
        }
    }
    nodes
        .into_iter()
        .enumerate()
        .map(|(i, n)| (i as u64 + 1, n))
        .collect()
}

fn stubs(nodes: &[(u64, Node)]) -> Vec<(u64, crate::stub::Stub)> {
    nodes.iter().map(|(p, n)| (*p, n.clone().into())).collect()
}

fn compile_in(nodes: &[(u64, crate::stub::Stub)], window: u64) -> Compiled {
    let last = nodes.last().map_or(0, |(p, _)| *p);
    compile(CompileInput {
        session_id: "s",
        current: None,
        nodes,
        last_position: last,
        spec: &spec(),
        catalog: &Catalog::builtin(),
        force: None,
        window_override: Some(window),
        blobs: None,
        hidden: &[],
        strip: None,
        overflowed: None,
        sources: &Default::default(),
        signals: None,
        assembled: None,
        situation: &situation::Situation::Continuation,
        kept: None,
    })
}

/// The tokens of the ring's candidate that keeps every node from `from` on,
/// rendered whole, as the walk over every cut read it.
fn candidate(nodes: &[(u64, crate::stub::Stub)], made: &Compilation, from: usize) -> u64 {
    let mut c = made.clone();
    c.includes = nodes[from..]
        .iter()
        .filter(|(_, n)| renderable(n))
        .map(|(_, n)| n.id.clone())
        .collect();
    c.budget = None;
    let media = (None, &[][..], None, &Default::default());
    let r = render_request(&spec(), &Catalog::builtin(), &c, nodes, media);
    let rates = Catalog::builtin()
        .get("claude-opus-5")
        .map_or_else(|| TokenRates::of("claude-opus-5"), |e| e.bytes_per_token);
    estimate(&r.request, rates, None).tokens
}

/// The cut the walk over every cut picks: the earliest user message after
/// the first whose candidate is under the target, else the last.
fn walked(nodes: &[(u64, crate::stub::Stub)], made: &Compilation, target: u64) -> usize {
    let starts: Vec<usize> = nodes
        .iter()
        .enumerate()
        .filter(|(_, (_, n))| n.kind == crate::stub::Kind::UserMessage)
        .map(|(i, _)| i)
        .collect();
    for &cut in starts.iter().skip(1) {
        if candidate(nodes, made, cut) <= target {
            return cut;
        }
    }
    *starts.last().unwrap()
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(48))]

    #[test]
    fn the_rings_guess_picks_the_cut_the_walk_over_every_cut_picked(
        xs in prop::collection::vec(exchange(), 3..40),
        window in 6_000u64..30_000,
    ) {
        let nodes = stubs(&session(&xs));
        let c = compile_in(&nodes, window);
        prop_assume!(c.compilation.strategy == "ring");
        let budget = budget::request_budget(window, &spec());
        let target = budget * 6 / 10;
        let first = c.compilation.includes.first().cloned().unwrap_or_default();
        let picked = nodes.iter().position(|(_, n)| n.id == first).unwrap();
        prop_assert_eq!(picked, walked(&nodes, &c.compilation, target));
    }
}

/// After a start, a long session whose stored compilation rang appends and
/// rings again: the compile decodes the nodes past the stored cut and the
/// segments the guess walks, not the session (theseus-0l3g). The walk over
/// every cut decoded nearly all of it.
#[test]
fn a_long_sessions_ring_after_a_start_decodes_past_its_cut_not_the_session() {
    let xs: Vec<Exchange> = (0..2_400)
        .map(|k| Exchange {
            said: 40 + k % 7,
            results: vec![1_500],
            thinks: k % 3 == 0,
            late: false,
            unanswered: false,
        })
        .collect();
    let nodes = session(&xs);
    let before = nodes.len() - 400 * 4;
    // The compilation stored before the start, over the first 2,000 exchanges.
    let stored = compile_in(&stubs(&nodes[..before]), 200_000).compilation;
    assert_eq!(stored.strategy, "ring");
    // A start: every node a stub of its record, as a store reads them, so a
    // decode is counted.
    let cache = std::sync::Arc::new(crate::node_cache::NodeCache::default());
    let dir = tempfile::tempdir().unwrap();
    let wal = std::sync::Arc::new(
        theseus_store::WalStore::open(dir.path(), theseus_store::WalConfig::default()).unwrap(),
    );
    let read: Vec<(u64, crate::stub::Stub)> = nodes
        .iter()
        .map(|(p, n)| {
            let r = theseus_store::Record {
                position: *p,
                kind: theseus_store::kinds::NODE,
                schema: 0,
                key: Some(n.id.clone()),
                scope: Some("s".into()),
                at_unix_ms: 0,
                payload: serde_json::to_vec(n).unwrap(),
            };
            (*p, crate::stub::Stub::of_record(r, &cache, &wal).unwrap())
        })
        .collect();
    let last = read.last().map_or(0, |(p, _)| *p);
    let c = compile(CompileInput {
        session_id: "s",
        current: Some(&stored),
        nodes: &read,
        last_position: last,
        spec: &spec(),
        catalog: &Catalog::builtin(),
        force: None,
        window_override: Some(200_000),
        blobs: None,
        hidden: &[],
        strip: None,
        overflowed: None,
        sources: &Default::default(),
        signals: None,
        assembled: None,
        situation: &situation::Situation::Continuation,
        kept: None,
    });
    assert_eq!(c.trigger.as_deref(), Some("overflow"));
    let kept = c.compilation.includes.len();
    let past = stored.includes.len() + 400 * 4;
    let decoded = crate::stub::read(&read) as usize;
    assert!(
        decoded <= past + 20,
        "decoded {decoded} of {} nodes; {past} past the stored cut, {kept} kept",
        read.len()
    );
}
