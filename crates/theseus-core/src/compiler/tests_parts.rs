//! The parts the compiler's tests build requests from, the view's compile they run on (`on_view`), and the
//! clearing stubs' words. Pure compiles over invented sessions.

use serde_json::{json, Value};
use theseus_protocol::Usage;

use super::*;
use crate::node::{Body, ResultStatus};

pub(crate) fn spec() -> RequestSpec {
    RequestSpec {
        profile: "p".into(),
        provider: "anthropic".into(),
        model: "claude-opus-5".into(),
        max_tokens: 1000,
        system_text: "S".into(),
        context_text: String::new(),
        session_text: String::new(),
        persona_text: String::new(),
        context_files: vec![],
        persona: None,
        defer: None,
        tools: vec![
            json!({"name": "fs_read", "description": "d", "input_schema": {"type": "object"}}),
        ]
        .into(),
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
        memberships: vec![],
        guidance: vec![],
    }
}

pub(super) fn answer(blocks: Vec<Value>) -> Node {
    Node::assistant(
        "s",
        "t",
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

pub(super) fn result(id: &str, content: &str) -> Node {
    Node::tool_result(
        "s",
        None,
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
            parts: Vec::new(),
        },
    )
}

/// The turn the compiler's rule tests lay their nodes in.
pub(crate) const ONE_TURN: &str = "turn_rules";

/// A request as the view's compile makes it (`view::compile`, issue 44) of `input`'s nodes, every one of them
/// this turn's own (the compiler's rule tests hold rules within a request: what one turn sends), after an
/// empty front, with nothing beside. `input.current` is the turn's chain: a call after the first appends
/// to it, as a view turn's later calls do.
pub(crate) fn on_view(input: CompileInput<'_>) -> Compiled {
    let nodes: Vec<(u64, crate::stub::Stub)> = (input.nodes.iter())
        .map(|(p, s)| {
            let mut n = Node::clone(s);
            n.turn_id = Some(ONE_TURN.into());
            (*p, n.into())
        })
        .collect();
    let frozen = super::view::Frozen {
        turn_id: ONE_TURN.into(),
        compilation_id: input.current.map(|c| c.id.clone()),
        ..Default::default()
    };
    super::view::compile(
        CompileInput {
            nodes: &nodes,
            ..input
        },
        &frozen,
    )
}

/// Every `tool_use` in the request has its `tool_result` in the next
/// message, and every `tool_result` follows its `tool_use`.
pub(super) fn closed(c: &Compiled) -> Result<(), String> {
    let msgs = &c.request.messages;
    for (i, m) in msgs.iter().enumerate() {
        let blocks = m["content"].as_array().cloned().unwrap_or_default();
        let ids = |kind: &str, key: &str, bl: &[Value]| -> Vec<String> {
            bl.iter()
                .filter(|b| b["type"] == kind)
                .map(|b| b[key].as_str().unwrap_or_default().to_string())
                .collect()
        };
        for id in ids("tool_use", "id", &blocks) {
            let next = msgs
                .get(i + 1)
                .and_then(|n| n["content"].as_array().cloned());
            if !ids("tool_result", "tool_use_id", &next.unwrap_or_default()).contains(&id) {
                return Err(format!("call {id} has no result after it"));
            }
        }
        for id in ids("tool_result", "tool_use_id", &blocks) {
            let prev = i
                .checked_sub(1)
                .and_then(|p| msgs[p]["content"].as_array().cloned());
            if !ids("tool_use", "id", &prev.unwrap_or_default()).contains(&id) {
                return Err(format!("result {id} has no call before it"));
            }
        }
    }
    Ok(())
}

/// A program's stub names its kept output when there is one, else how to
/// get it again; any other tool's says to call it again.
#[test]
fn a_programs_stub_names_its_kept_output() {
    let mut r = result("c1", &"z".repeat(41_002));
    if let Body::ToolResult { meta, full_ref, .. } = &mut r.body {
        *meta = json!({"exit_code": 101});
        *full_ref = Some("/tmp/theseus-out/act_1.log".into());
    }
    let input = json!({"argv": ["cargo", "test"]});
    assert_eq!(
        clearing::stub("proc_run", &input, &r),
        "[Result cleared to save context: proc_run cargo test (exit 101, 41,002 characters); \
         the whole output is at /tmp/theseus-out/act_1.log.]"
    );
    if let Body::ToolResult { full_ref, .. } = &mut r.body {
        *full_ref = None;
    }
    assert_eq!(
        clearing::stub("proc_run", &input, &r),
        "[Result cleared to save context: proc_run cargo test (exit 101, 41,002 characters). \
         Run it again if you need its output.]"
    );
    let s = clearing::stub("web_fetch", &json!({"url": "https://example.test/a"}), &r);
    assert!(s.starts_with("[Result cleared to save context: web_fetch {\"url\":"));
    assert!(s.ends_with("(41,002 characters). Call it again if you need it.]"));
}
