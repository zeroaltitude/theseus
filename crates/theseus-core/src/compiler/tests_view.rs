//! The strategy `view` in the compiler (issue 44, `view.rs`): the wire order, frozen within a turn, the
//! earlier turns' thinking not carried, the window's end marked, and the clearing pass on its own numbers.

use serde_json::{json, Value};
use theseus_protocol::Usage;

use super::view::{self, Frozen};
use super::*;
use crate::node::{Body, ResultStatus};
use crate::provider::binding::unmarked;

fn spec() -> RequestSpec {
    RequestSpec {
        profile: "p".into(),
        provider: "anthropic".into(),
        model: "claude-opus-5".into(),
        max_tokens: 1000,
        system_text: "You are Theseus.".into(),
        context_text: String::new(),
        session_text: "Directory: /w".into(),
        persona_text: String::new(),
        card: None,
        cold: None,
        context_files: vec![],
        persona: None,
        tools: vec![
            json!({"name": "fs_read", "description": "d", "input_schema": {"type": "object"}}),
        ]
        .into(),
        effort: None,
        turn_effort: None,
        effort_refused: vec![],
        thinking_display: ThinkingDisplay::Summarized,
        refusal_fallbacks: false,
        fallback: None,
        first_party: true,
        cache_ttl: CacheTtl::FiveMinutes,
        conversation_ttl: CacheTtl::FiveMinutes,
        walk: None,
        defer: None,
        memberships: vec![],
        guidance: vec![],
        prefix: None,
        prefix_used: None,
    }
}

fn said(turn: &str, blocks: Vec<Value>) -> Node {
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

fn result(turn: &str, id: &str, content: &str) -> Node {
    Node::tool_result(
        "s",
        Some(turn),
        None,
        Body::ToolResult {
            tool_use_id: id.into(),
            tool: "fs.read".into(),
            status: ResultStatus::Ok,
            is_error: false,
            content: content.into(),
            correlation_id: None,
            bytes_total: 0,
            truncated: false,
            full_ref: None,
            duration_ms: None,
            late: false,
            meta: json!({}),
            image: None,
            external: None,
            payload: None,
        },
    )
}

fn thinking() -> Value {
    json!({"type": "thinking", "thinking": "", "signature": "sig"})
}

fn read(id: &str) -> Value {
    json!({"type": "tool_use", "id": id, "name": "fs_read", "input": {"path": format!("{id}.rs")}})
}

/// An earlier exchange: his ask, a call and its result of `size` characters, and the answer.
fn exchange(turn: &str, size: usize) -> Vec<Node> {
    vec![
        Node::user("s", Some(turn), "cli", &format!("ask {turn}")),
        said(turn, vec![thinking(), read(&format!("c{turn}"))]),
        result(turn, &format!("c{turn}"), &"x".repeat(size)),
        said(
            turn,
            vec![
                thinking(),
                json!({"type": "text", "text": format!("answer {turn}")}),
            ],
        ),
    ]
}

fn frozen(turn: &str, window: &[Node]) -> Frozen {
    Frozen {
        turn_id: turn.into(),
        compilation_id: None,
        front: vec![Frozen::block("s", turn, "who and where", "he keeps bees")],
        window: (window.iter().enumerate())
            .map(|(i, n)| (i as u64 + 1, n.clone().into()))
            .collect(),
        spoken: vec![],
        beside: Some(Frozen::block("s", turn, "beside", "waiting: one question")),
        memory_page: "# How your memory works".into(),
        record: None,
    }
}

fn run(f: &Frozen, own: &[Node], current: Option<&Compilation>) -> Compiled {
    let nodes: Vec<(u64, crate::stub::Stub)> = (own.iter().enumerate())
        .map(|(i, n)| (1000 + i as u64, n.clone().into()))
        .collect();
    let s = spec();
    view::compile(
        CompileInput {
            session_id: "s",
            current,
            nodes: &nodes,
            last_position: 2000,
            spec: &s,
            catalog: &Catalog::builtin(),
            force: None,
            window_override: None,
            blobs: None,
            hidden: &[],
            strip: None,
            overflowed: None,
            sources: &Default::default(),
            assembled: None,
            situation: &situation::Situation::Continuation,
            kept: None,
        },
        f,
    )
}

fn text_of(m: &Value) -> Vec<String> {
    (m["content"].as_array().into_iter().flatten())
        .map(|b| {
            b["text"]
                .as_str()
                .map_or_else(|| b["type"].to_string(), str::to_string)
        })
        .collect()
}

/// The wire order, the frozen turn and the thinking: the front heads the first message, the window's
/// earlier thinking is gone, its end is marked, the beside block stands before the message, and the turn's
/// second call begins with its first call's bytes, its own thinking kept.
#[test]
fn a_view_turn_lays_its_parts_in_order_and_holds_them_across_its_calls() {
    let window = exchange("t0", 10);
    let mut f = frozen("t1", &window);
    let mut own = vec![Node::user("s", Some("t1"), "cli", "now ask")];
    let first = run(&f, &own, None);
    assert!(first.new_compilation);
    assert_eq!(first.compilation.strategy, view::STRATEGY);
    let m = &first.request.messages;
    assert_eq!(text_of(&m[0]), ["who and where\n\nhe keeps bees", "ask t0"]);
    assert!(!serde_json::to_string(&m[1]).unwrap().contains("thinking"));
    let end = m[2]["content"].as_array().unwrap();
    assert_eq!(end[0]["type"], "tool_result");
    assert_eq!(text_of(&m[3]), ["answer t0"]);
    assert!(
        m[3]["content"][0].get("cache_control").is_some(),
        "the window's end is marked"
    );
    assert_eq!(
        text_of(&m[4]),
        ["beside\n\nwaiting: one question", "now ask"]
    );
    let system = serde_json::to_string(&first.request.system).unwrap();
    assert!(system.contains("# How your memory works"), "{system}");

    f.compilation_id = Some(first.compilation.id.clone());
    own.push(said("t1", vec![thinking(), read("c1")]));
    own.push(result("t1", "c1", "short"));
    let second = run(&f, &own, Some(&first.compilation));
    assert!(!second.new_compilation, "the turn's later call appends");
    let (a, b): (Vec<Value>, Vec<Value>) = (
        first.request.messages.iter().map(unmarked).collect(),
        second.request.messages.iter().map(unmarked).collect(),
    );
    assert_eq!(
        b[..a.len()],
        a[..],
        "the second call begins with the first's bytes"
    );
    assert_eq!(second.request.system, first.request.system);
    let own_call = serde_json::to_string(&second.request.messages[5]).unwrap();
    assert!(
        own_call.contains("\"thinking\""),
        "the turn's own thinking is kept"
    );
}

/// Past 80,000 tokens of results in the window, the oldest become receipts until the newest 40,000 stay
/// whole; the next turn carries the clearing, so it holds.
#[test]
fn the_window_clears_on_its_own_numbers_and_the_clearing_holds() {
    let mut window = Vec::new();
    for t in ["t0", "t1", "t2", "t3", "t4", "t5"] {
        window.extend(exchange(t, 60_000));
    }
    let f = frozen("t9", &window);
    let own = vec![Node::user("s", Some("t9"), "cli", "and now")];
    let first = run(&f, &own, None);
    let pass = first.cleared.expect("a pass ran");
    assert!(
        pass.results >= 1 && pass.tokens_after < pass.tokens_before,
        "{pass:?}"
    );
    let all = serde_json::to_string(&first.request.messages).unwrap();
    assert!(
        all.contains("[Result cleared to save context: fs_read ct0.rs"),
        "the oldest is a receipt"
    );
    assert!(
        !all.contains("[Result cleared to save context: fs_read ct5.rs"),
        "the newest stays whole"
    );
    // The next turn: a new compilation that carries what the last one cleared, with no pass of its own.
    let next = frozen("t10", &window);
    let later = run(
        &next,
        &[Node::user("s", Some("t10"), "cli", "more")],
        Some(&first.compilation),
    );
    assert!(later.new_compilation && later.cleared.is_none());
    let again = serde_json::to_string(&later.request.messages).unwrap();
    assert!(
        again.contains("[Result cleared to save context: fs_read ct0.rs"),
        "the clearing holds"
    );
}

/// An exchange the budgeter kept spoken shows its calls with their results as receipts, in that request
/// only: the compilation it persists records no clearing of them.
#[test]
fn a_window_exchange_kept_spoken_shows_its_results_as_receipts() {
    let window = exchange("t0", 5_000);
    let mut f = frozen("t1", &window);
    f.spoken = vec![window[2].id.clone()];
    let c = run(&f, &[Node::user("s", Some("t1"), "cli", "now")], None);
    let all = serde_json::to_string(&c.request.messages).unwrap();
    assert!(
        all.contains("[Result cleared to save context: fs_read ct0.rs"),
        "{all}"
    );
    assert!(c.cleared.is_none(), "no pass ran");
    assert!(
        clearing::of(&c.compilation).is_empty(),
        "nothing cleared is persisted"
    );
}
