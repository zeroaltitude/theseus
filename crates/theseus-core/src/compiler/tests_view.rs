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
        stripped: Default::default(),
        sessions: vec![],
        beside: Some(Frozen::block("s", turn, "beside", "waiting: one question")),
        memory_page: "# How your memory works".into(),
        record: None,
        ..Frozen::default()
    }
}

fn run(f: &Frozen, own: &[Node], current: Option<&Compilation>) -> Compiled {
    run_with(&spec(), f, own, current)
}

fn run_with(s: &RequestSpec, f: &Frozen, own: &[Node], current: Option<&Compilation>) -> Compiled {
    let nodes: Vec<(u64, crate::stub::Stub)> = (own.iter().enumerate())
        .map(|(i, n)| (1000 + i as u64, n.clone().into()))
        .collect();
    view::compile(
        CompileInput {
            session_id: "s",
            current,
            nodes: &nodes,
            last_position: 2000,
            spec: s,
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

/// A window exchange kept spoken that ended in a result with no answer: its receipt is made once from the
/// window alone, so the turn's own first answer never becomes its "after it you wrote", and the second call
/// of the turn sends the receipt byte for byte as the first did.
#[test]
fn a_receipt_is_made_from_the_window_alone_and_never_changes() {
    let mut window = exchange("t0", 5_000);
    window.truncate(3); // ask, call, result: no answer after it
    let f0 = frozen("t1", &window);
    let spoken = std::collections::HashSet::from([window[2].id.clone()]);
    let mut f = f0.clone();
    f.window = view::receipted(f0.window, &spoken);
    let mut own = vec![Node::user("s", Some("t1"), "cli", "now")];
    let first = run(&f, &own, None);
    let receipt = |c: &Compiled| serde_json::to_string(&c.request.messages[2]).unwrap();
    assert!(receipt(&first).contains("[Result cleared to save context: fs_read ct0.rs"));
    assert!(
        !receipt(&first).contains("After it you wrote"),
        "{}",
        receipt(&first)
    );
    f.compilation_id = Some(first.compilation.id.clone());
    own.push(said(
        "t1",
        vec![
            thinking(),
            json!({"type": "text", "text": "this turn's answer"}),
            read("c1"),
        ],
    ));
    own.push(result("t1", "c1", "short"));
    let second = run(&f, &own, Some(&first.compilation));
    assert_eq!(
        receipt(&second),
        receipt(&first),
        "the receipt never changes"
    );
    let (a, b): (Vec<Value>, Vec<Value>) = (
        first.request.messages.iter().map(unmarked).collect(),
        second.request.messages.iter().map(unmarked).collect(),
    );
    assert_eq!(b[..a.len()], a[..]);
}

/// A reset (a seal since, a refused image under the thinking, a forced recompile) starts a new chain: the
/// turn's own answers so far go without their thinking, and the cut answer a call replaces is not resent.
#[test]
fn a_reset_retires_the_turns_own_thinking() {
    let mut f = frozen("t1", &exchange("t0", 10));
    let mut own = vec![Node::user("s", Some("t1"), "cli", "now")];
    own.push(said("t1", vec![thinking(), read("c1")]));
    own.push(result("t1", "c1", "short"));
    let kept = run(&f, &own, None);
    assert!(serde_json::to_string(&kept.request.messages)
        .unwrap()
        .contains("\"thinking\""));
    f.stripped.insert(own[1].id.clone());
    let reset = run(&f, &own, None);
    let all = serde_json::to_string(&reset.request.messages).unwrap();
    assert!(!all.contains("\"thinking\""), "{all}");
    assert!(all.contains("tool_use"), "its calls stay, paired");
}

/// An effective spec that changed between two calls of a turn (a tool family `tools_find` loaded) starts a
/// new chain, as the transcript recompiles on a changed tools digest: a new compilation recording the new
/// tool set, the turn's answers so far without their thinking.
#[test]
fn a_tool_set_that_changed_mid_turn_starts_a_new_chain() {
    let mut f = frozen("t1", &exchange("t0", 10));
    let mut own = vec![Node::user("s", Some("t1"), "cli", "now")];
    let first = run(&f, &own, None);
    f.compilation_id = Some(first.compilation.id.clone());
    own.push(said("t1", vec![thinking(), read("c1")]));
    own.push(result("t1", "c1", "short"));
    let same = run(&f, &own, Some(&first.compilation));
    assert!(!same.new_compilation, "an unchanged spec appends");
    let mut more = spec();
    let mut tools = more.tools.to_vec();
    tools.push(json!({"name": "git_log", "description": "d", "input_schema": {"type": "object"}}));
    more.tools = tools.into();
    let changed = run_with(&more, &f, &own, Some(&first.compilation));
    assert!(
        changed.new_compilation,
        "a changed tool set starts a new chain"
    );
    assert_ne!(
        changed.compilation.manifest.tools_digest,
        first.compilation.manifest.tools_digest
    );
    let all = serde_json::to_string(&changed.request.messages).unwrap();
    assert!(
        !all.contains("\"thinking\""),
        "the turn's answer goes without its thinking: {all}"
    );
}

/// The thread card the chain began with is the one its later calls send, though the turn-start spec's
/// append would take the card recorded before; it is recorded with the compilation.
#[test]
fn the_chains_card_is_frozen_and_recorded() {
    let mut s = spec();
    s.card = Some(card::Card::new(
        "[Thread card: NOW]".into(),
        Some("[Thread card: BEFORE]".into()),
    ));
    let mut f = frozen("t1", &exchange("t0", 10));
    let mut own = vec![Node::user("s", Some("t1"), "cli", "now")];
    let first = run_with(&s, &f, &own, None);
    assert_eq!(first.card.as_deref(), Some("[Thread card: NOW]"));
    f.compilation_id = Some(first.compilation.id.clone());
    f.card = first.card.clone();
    own.push(said("t1", vec![thinking(), read("c1")]));
    own.push(result("t1", "c1", "short"));
    let second = run_with(&s, &f, &own, Some(&first.compilation));
    assert!(!second.new_compilation);
    let all = serde_json::to_string(&second.request.messages).unwrap();
    assert!(
        all.contains("Thread card: NOW") && !all.contains("Thread card: BEFORE"),
        "{all}"
    );
}

/// A turn that made room (`Frozen::room.keep`, `turn::view_step::room`) clears its own old results at a new
/// chain whatever the line: far under the clearing line, the older result becomes its stand-in and the
/// newest, after the last answer, stays whole; without the room, nothing is cleared.
#[test]
fn a_turn_that_made_room_clears_its_old_results_whatever_the_line() {
    let own = vec![
        Node::user("s", Some("t9"), "cli", "read both"),
        said("t9", vec![thinking(), read("old")]),
        result("t9", "old", &"o".repeat(5_000)),
        said("t9", vec![thinking(), read("new")]),
        result("t9", "new", &"n".repeat(5_000)),
    ];
    let mut f = frozen("t9", &[]);
    let plain = run(&f, &own, None);
    assert!(plain.cleared.is_none(), "under the line, no pass");
    f.room.keep = Some(0);
    let roomy = run(&f, &own, None);
    let pass = roomy.cleared.expect("a pass ran");
    assert_eq!(pass.results, 1, "{pass:?}");
    assert!(roomy.est_tokens < plain.est_tokens);
    let all = serde_json::to_string(&roomy.request.messages).unwrap();
    assert!(
        all.contains("[Result cleared to save context: fs_read old.rs"),
        "the older result is its stand-in"
    );
    assert!(all.contains(&"n".repeat(5_000)), "the newest stays whole");
}
