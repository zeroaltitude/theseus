//! One long instruction (theseus-3ki0): old results cleared past the
//! clearing line; and the parts the compiler's tests build sessions from.
//! Pure compiles over invented sessions.

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
        card: None,
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
        prefix: None,
        prefix_used: None,
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

pub(super) fn call(id: &str, path: &str) -> Value {
    json!({"type": "tool_use", "id": id, "name": "fs_read", "input": {"path": path, "offset": 1, "limit": 400}})
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
        },
    )
}

pub(super) const INSTRUCTION: &str = "fix the build of the lighthouse crate until it is green";

/// One instruction, then `n` exchanges, each a call and its result of
/// `size(k)` characters: `(position, node)` from 1.
pub(super) fn one_turn(n: usize, size: impl Fn(usize) -> usize) -> Vec<(u64, Node)> {
    let mut nodes = vec![Node::user("s", None, "web", INSTRUCTION)];
    for k in 0..n {
        let id = format!("c{k}");
        nodes.push(answer(vec![
            json!({"type": "text", "text": format!("step {k}")}),
            call(&id, &format!("src/f{k}.rs")),
        ]));
        let body = format!("r{k} ") + &"x".repeat(size(k));
        nodes.push(result(&id, &body));
    }
    nodes.push(answer(vec![json!({"type": "text", "text": "done"})]));
    nodes
        .into_iter()
        .enumerate()
        .map(|(i, n)| (i as u64 + 1, n))
        .collect()
}

pub(super) fn compile_in(
    nodes: &[(u64, Node)],
    current: Option<&Compilation>,
    window: Option<u64>,
    overflowed: Option<&Overflowed>,
) -> Compiled {
    let nodes: Vec<(u64, crate::stub::Stub)> =
        nodes.iter().map(|(p, n)| (*p, n.clone().into())).collect();
    let last = nodes.last().map_or(0, |(p, _)| *p);
    compile(CompileInput {
        session_id: "s",
        current,
        nodes: &nodes,
        last_position: last,
        spec: &spec(),
        catalog: &Catalog::builtin(),
        force: false,
        window_override: window,
        blobs: None,
        hidden: &[],
        strip: None,
        overflowed,
        sources: &Default::default(),
        assembled: None,
        situation: &situation::Situation::Continuation,
    })
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

/// Past the clearing line, results older than the newest ones within
/// `KEEP_TOKENS` (and within what `CLEAR_TO` % leaves room for) and longer
/// than 2,000 characters become stubs, oldest first; newer and shorter ones
/// stay whole. The pass is one recompile, its stubs kept by the next append.
#[test]
fn clearing_stubs_only_old_large_results_and_each_says_how_to_get_it_back() {
    // 40 exchanges: the even ones 6,000 characters, the odd ones 1,500.
    let size = |k: usize| if k.is_multiple_of(2) { 6_000 } else { 1_500 };
    let nodes = one_turn(40, size);
    let window = 100_000;
    let c = compile_in(&nodes, None, Some(window), None);
    let budget = window - 1_000 - 4_096;
    let pass = c.cleared.expect("a pass");
    assert_eq!(c.trigger.as_deref(), Some("new_session+cleared"));
    assert!(pass.tokens_before > budget * 50 / 100, "{pass:?}");
    assert!(pass.tokens_after < pass.tokens_before);
    assert!(c.budget.overage.is_none());
    let cleared: Vec<&str> = c
        .budget
        .dropped
        .iter()
        .filter(|d| d.tier == "cleared")
        .map(|d| d.node_id.as_deref().unwrap())
        .collect();
    assert_eq!(cleared.len() as u64, pass.results);
    assert!(!cleared.is_empty());
    // Which ones, worked out again here: the tokens a result saves are its
    // characters less the stub's 200, at the ratio the drops carry; the keep
    // is the lower of `KEEP_TOKENS` and what `CLEAR_TO` % leaves beside the
    // rest of the request; the newest results within it stay whole.
    let drop = &c.budget.dropped[0];
    let per_byte = drop.tokens as f64 / (6_003 - 200) as f64;
    let tokens = |len: usize| (len.saturating_sub(200) as f64 * per_byte).ceil() as u64;
    let results: Vec<(&str, usize)> = nodes
        .iter()
        .filter_map(|(_, n)| match &n.body {
            Body::ToolResult { content, .. } => Some((n.id.as_str(), content.len())),
            _ => None,
        })
        .collect();
    let live: u64 = results.iter().map(|(_, len)| tokens(*len)).sum();
    let overhead = pass.tokens_before - live;
    let keep =
        clearing::KEEP_TOKENS.min((budget * clearing::CLEAR_TO / 100).saturating_sub(overhead));
    let (mut held, mut cutting, mut expected) = (0, false, Vec::new());
    for (k, (id, len)) in results.iter().enumerate().rev() {
        cutting = cutting || (k + 1 < results.len() && held + tokens(*len) > keep);
        match (cutting, *len > 2_000) {
            (true, true) => expected.push(*id),
            (false, _) => held += tokens(*len),
            _ => {}
        }
    }
    expected.reverse();
    assert_eq!(
        cleared, expected,
        "the oldest large results, the newest whole"
    );
    let kept_big = results
        .iter()
        .filter(|(id, len)| *len > 2_000 && !cleared.contains(id));
    assert!(
        kept_big.clone().count() >= 5,
        "a keep of tokens, not of 20 exchanges"
    );
    assert!(kept_big.map(|(_, len)| tokens(*len)).sum::<u64>() <= keep + tokens(6_003));
    // The stub says what it was and how to get it back, and what the model
    // wrote after it; the rest renders.
    let text = serde_json::to_string(&c.request.messages).unwrap();
    assert!(text.contains(
        "[Result cleared to save context: fs_read src/f0.rs, lines 1-400 (6,003 characters). \
         You read it, and your notes after it still stand; read again only the line range \
         you need. After it you wrote: \\\"step 1\\\".]"
    ));
    assert!(text.contains("r1 xxx"), "a short old result stays");
    assert!(text.contains("r38 xxx"), "a recent large result stays");
    assert!(!text.contains("r0 xxx"));
    closed(&c).unwrap();
    // The next append keeps the stubs: its request begins with this one's.
    let mut more = nodes;
    let p = more.last().unwrap().0;
    more.push((p + 1, Node::user("s", None, "web", "and now?")));
    let next = compile_in(&more, Some(&c.compilation), Some(window), None);
    assert_eq!(next.decision(), "append");
    assert!(next.cleared.is_none(), "no second pass under the line");
    assert_eq!(
        &next.request.messages[..c.request.messages.len() - 1],
        &c.request.messages[..c.request.messages.len() - 1]
    );
    assert_eq!(
        next.budget
            .dropped
            .iter()
            .filter(|d| d.tier == "cleared")
            .count(),
        cleared.len(),
        "its report names them too"
    );
}

/// Under the token line, no pass; and the newest exchange's results are
/// never cleared, however large.
#[test]
fn no_pass_under_the_line_or_without_old_results() {
    let nodes = one_turn(40, |_| 6_000);
    let c = compile_in(&nodes, None, None, None);
    assert!(c.cleared.is_none(), "1M: under the token line");
    // One exchange of a huge result, the newest: nothing older to clear.
    let mut few = one_turn(1, |_| 60_000);
    few.pop();
    let c = compile_in(&few, None, Some(40_000), None);
    assert!(
        c.cleared.is_none(),
        "the newest exchange's results stay whole"
    );
    let one = serde_json::to_string(&c.request.messages).unwrap();
    assert!(!one.contains("Result cleared"));
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
