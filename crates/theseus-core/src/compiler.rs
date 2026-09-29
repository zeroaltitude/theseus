//! The context compiler, simplest form (spec §4.4, §4.4a, Part II P5).
//!
//! A session's context is a **compilation** (a prefix: an ordered selection of
//! node ids, frozen with a manifest) plus an **append tail** (every node the
//! session wrote after the compilation's as-of position, in order, unselected).
//! Each loop asks one question: append, or recompile? M3 answers it with
//! deterministic triggers only (no Jev): a new session, a model or provider
//! change, a system prompt change, a tool set change, overflow of the context
//! window, or an operator's manual request.
//!
//! The renderer is deterministic and append-only: the same nodes and the same
//! manifest produce the same bytes, and a later request's messages begin with
//! the earlier request's messages unchanged. That is what keeps the provider's
//! prompt cache warm and what the preserved-thinking rules require. A recompile
//! that changes the system prompt or tool set, or drops leading turns, is the
//! one place thinking blocks are stripped from the prefix (a boundary the
//! provider documents as safe); a model change keeps them (the provider drops
//! what the new model cannot read, unbilled).

use std::collections::{HashMap, HashSet};

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::catalog::{Catalog, ThinkingMode};
use crate::node::{Body, Node, ResultStatus};
use crate::provider::{tool_uses_in, ProviderRequest};

pub const COMPILER_VERSION: u32 = 1;
pub const RENDERER_VERSION: u32 = 1;
pub const COMPILATION_SCHEMA: u16 = 1;

pub const BETA_THINKING_UPDATES: &str = "thinking-display-updates-2026-08-18";
pub const BETA_FALLBACKS: &str = "server-side-fallback-2026-07-01";

/// What a compilation froze: everything that, if it changed, would change the
/// prefix bytes or invalidate thinking blocks.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Manifest {
    pub compiler_version: u32,
    pub renderer_version: u32,
    pub profile: String,
    pub provider: String,
    pub model: String,
    pub system_digest: String,
    pub tools_digest: String,
    /// Wire names of the tools offered, sorted.
    #[serde(default)]
    pub tools: Vec<String>,
    pub catalog_version: String,
    #[serde(default)]
    pub context_window: Option<u64>,
    /// The prefix is rendered without thinking blocks.
    #[serde(default)]
    pub strip_thinking: bool,
    /// The context files the system block carried, in order (theseus-58a).
    /// Their text is in the system block, so `system_digest` covers it.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub context_files: Vec<ContextFileRef>,
}

/// A context file as the system block carried it (theseus-58a).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContextFileRef {
    /// The file, `~` expanded.
    pub path: String,
    /// The first 16 hex digits of the SHA-256 of the text included; absent
    /// when the file could not be read.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub digest: Option<String>,
    /// Bytes of the file the block carries.
    #[serde(default)]
    pub bytes: u64,
    /// The file was longer than the cap, and the block carries its start.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub cut: bool,
    /// Why the file could not be read: `not found`, `permission denied`, …
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub missing: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Compilation {
    pub id: String,
    pub schema: u16,
    pub session_id: String,
    pub created_at_ms: u64,
    /// Why it was made: `new_session`, `model_changed`, `system_changed`,
    /// `tools_changed` (joined with `+`), `overflow`, `manual_fresh`, `manual_transcript`.
    pub trigger: String,
    /// `transcript` (everything so far), `fresh` (nothing), `ring` (leading turns dropped).
    pub strategy: String,
    /// WAL position the selection was made at: later nodes are the tail.
    pub as_of: u64,
    /// The prefix: node ids, in order.
    pub includes: Vec<String>,
    #[serde(default)]
    pub derived_from: Option<String>,
    pub manifest: Manifest,
}

/// An operator's request to recompile.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Recompile {
    /// Keep the whole transcript as the prefix (thinking stripped).
    Transcript,
    /// Start over: the model sees nothing before this point.
    Fresh,
}

/// How requests are built, fixed for the life of a compilation.
#[derive(Debug, Clone)]
pub struct RequestSpec {
    pub profile: String,
    pub provider: String,
    pub model: String,
    pub max_tokens: u32,
    /// The whole system block, context files included.
    pub system_text: String,
    /// The context files `system_text` carries, for the manifest.
    pub context_files: Vec<ContextFileRef>,
    /// Wire tool definitions, sorted by name.
    pub tools: Vec<Value>,
    pub effort: Option<String>,
    /// `summarized`, `omitted`, or `updates`.
    pub thinking_display: String,
    pub refusal_fallbacks: bool,
    /// The provider is Anthropic's own API (server-side fallbacks exist only there).
    pub first_party: bool,
}

pub struct CompileInput<'a> {
    pub session_id: &'a str,
    pub current: Option<&'a Compilation>,
    /// Every node of the session with its WAL position, in position order.
    pub nodes: &'a [(u64, Node)],
    /// The store's last position (the as-of for a new compilation).
    pub last_position: u64,
    pub spec: &'a RequestSpec,
    pub catalog: &'a Catalog,
    pub force: Option<Recompile>,
    /// Tests: pretend the model's window is this many tokens.
    pub window_override: Option<u64>,
}

#[derive(Debug, Clone)]
pub struct Compiled {
    pub request: ProviderRequest,
    pub compilation: Compilation,
    /// `compilation` was made by this call and must be persisted.
    pub new_compilation: bool,
    pub trigger: Option<String>,
    pub prefix_nodes: usize,
    pub tail_nodes: usize,
    pub messages: usize,
    pub est_tokens: u64,
    pub digest: String,
    /// tool_use ids that had no recorded result and got a synthetic one.
    pub repairs: Vec<String>,
}

impl Compiled {
    pub fn decision(&self) -> &'static str {
        if self.new_compilation {
            "recompile"
        } else {
            "append"
        }
    }
}

fn sha(s: &str) -> String {
    use sha2::{Digest, Sha256};
    hex::encode(Sha256::digest(s.as_bytes()))[..16].to_string()
}

pub fn manifest_for(
    spec: &RequestSpec,
    catalog: &Catalog,
    window: Option<u64>,
    strip: bool,
) -> Manifest {
    let mut tools: Vec<String> = spec
        .tools
        .iter()
        .filter_map(|t| t.get("name").and_then(Value::as_str).map(str::to_string))
        .collect();
    tools.sort();
    Manifest {
        compiler_version: COMPILER_VERSION,
        renderer_version: RENDERER_VERSION,
        profile: spec.profile.clone(),
        provider: spec.provider.clone(),
        model: spec.model.clone(),
        system_digest: sha(&spec.system_text),
        tools_digest: theseus_kernel::digest_json(&Value::Array(spec.tools.clone()))[..16]
            .to_string(),
        tools,
        catalog_version: catalog.version.clone(),
        context_window: window,
        strip_thinking: strip,
        context_files: spec.context_files.clone(),
    }
}

fn renderable(n: &Node) -> bool {
    !matches!(n.body, Body::ToolCall { .. })
}

pub fn compile(input: CompileInput<'_>) -> Compiled {
    let spec = input.spec;
    let entry = input.catalog.get(&spec.model);
    let window = input.window_override.or(entry.map(|e| e.context_window));
    let now_manifest = manifest_for(spec, input.catalog, window, false);

    // 1. Append or recompile?
    let mut decided: Option<(String, &'static str, bool)> = match (input.current, input.force) {
        // Fresh keeps only the current exchange, so what it keeps was produced
        // against a longer conversation: its thinking is stripped, as for ring.
        (_, Some(Recompile::Fresh)) => Some(("manual_fresh".into(), "fresh", true)),
        (_, Some(Recompile::Transcript)) => Some(("manual_transcript".into(), "transcript", true)),
        (None, None) => Some(("new_session".into(), "transcript", false)),
        (Some(c), None) => {
            let m = &c.manifest;
            let mut t: Vec<&str> = Vec::new();
            if m.provider != now_manifest.provider || m.model != now_manifest.model {
                t.push("model_changed");
            }
            if m.system_digest != now_manifest.system_digest {
                t.push("system_changed");
            }
            if m.tools_digest != now_manifest.tools_digest {
                t.push("tools_changed");
            }
            if t.is_empty() {
                None
            } else {
                // A system or tool change invalidates earlier thinking blocks;
                // a model change alone does not (the provider drops unreadable
                // blocks itself). A prefix that was stripped stays stripped.
                let strip = t.iter().any(|x| *x != "model_changed") || m.strip_thinking;
                Some((t.join("+"), "transcript", strip))
            }
        }
    };

    let all_renderable: Vec<&(u64, Node)> =
        input.nodes.iter().filter(|(_, n)| renderable(n)).collect();
    let make = |trigger: String, strategy: &str, strip: bool, includes: Vec<String>| Compilation {
        id: crate::new_id("cmp"),
        schema: COMPILATION_SCHEMA,
        session_id: input.session_id.into(),
        created_at_ms: theseus_protocol::now_unix_ms(),
        trigger,
        strategy: strategy.into(),
        as_of: input.last_position,
        includes,
        derived_from: input.current.map(|c| c.id.clone()),
        manifest: Manifest {
            strip_thinking: strip,
            ..now_manifest.clone()
        },
    };

    let mut compilation = match decided.take() {
        None => input
            .current
            .cloned()
            .expect("append requires a current compilation"),
        Some((trigger, strategy, strip)) => {
            let includes = if strategy == "fresh" {
                // From the latest operator message on: the request that
                // triggered the recompile, plus any tool traffic it started.
                let from = all_renderable
                    .iter()
                    .rposition(|(_, n)| matches!(n.body, Body::UserMessage { .. }))
                    .unwrap_or(all_renderable.len());
                all_renderable[from..]
                    .iter()
                    .map(|(_, n)| n.id.clone())
                    .collect()
            } else {
                all_renderable.iter().map(|(_, n)| n.id.clone()).collect()
            };
            make(trigger, strategy, strip, includes)
        }
    };
    let mut new_compilation = input
        .current
        .map(|c| c.id != compilation.id)
        .unwrap_or(true);
    let mut trigger = new_compilation.then(|| compilation.trigger.clone());

    let (mut request, mut prefix_n, mut tail_n, mut repairs) =
        render_request(spec, input.catalog, &compilation, input.nodes);
    let mut est = request.estimate_tokens();

    // 2. Overflow: drop leading turns (ring), cutting only before a user message.
    if let Some(w) = window {
        let budget = w
            .saturating_sub(spec.max_tokens as u64)
            .saturating_sub(4_096);
        if est > budget {
            let target = budget * 6 / 10;
            let seq: Vec<&Node> = all_renderable.iter().map(|(_, n)| n).collect();
            let starts: Vec<usize> = seq
                .iter()
                .enumerate()
                .filter(|(_, n)| matches!(n.body, Body::UserMessage { .. }))
                .map(|(i, _)| i)
                .collect();
            for &cut in starts.iter().skip(1) {
                let includes: Vec<String> = seq[cut..].iter().map(|n| n.id.clone()).collect();
                let candidate = make("overflow".into(), "ring", true, includes);
                let (r, p, t, rep) = render_request(spec, input.catalog, &candidate, input.nodes);
                let e = r.estimate_tokens();
                let last = cut == *starts.last().unwrap();
                if e <= target || last {
                    compilation = candidate;
                    request = r;
                    prefix_n = p;
                    tail_n = t;
                    repairs = rep;
                    est = e;
                    new_compilation = true;
                    trigger = Some("overflow".into());
                    break;
                }
            }
        }
    }

    let messages = request.messages.len();
    let digest = request.digest();
    Compiled {
        request,
        compilation,
        new_compilation,
        trigger,
        prefix_nodes: prefix_n,
        tail_nodes: tail_n,
        messages,
        est_tokens: est,
        digest,
        repairs,
    }
}

/// Render a compilation plus its tail into a provider request.
pub fn render_request(
    spec: &RequestSpec,
    catalog: &Catalog,
    c: &Compilation,
    nodes: &[(u64, Node)],
) -> (ProviderRequest, usize, usize, Vec<String>) {
    let included: HashSet<&str> = c.includes.iter().map(String::as_str).collect();
    let prefix: Vec<&Node> = nodes
        .iter()
        .filter(|(pos, n)| *pos <= c.as_of && included.contains(n.id.as_str()) && renderable(n))
        .map(|(_, n)| n)
        .collect();
    let tail: Vec<&Node> = nodes
        .iter()
        .filter(|(pos, n)| *pos > c.as_of && renderable(n))
        .map(|(_, n)| n)
        .collect();
    let (messages, repairs) = render_messages(&prefix, &tail, c.manifest.strip_thinking);

    let entry = catalog.get(&spec.model);
    let mut betas = Vec::new();
    let mut extra = std::collections::BTreeMap::new();
    let thinking = match entry.map(|e| e.thinking) {
        Some(ThinkingMode::Always) | Some(ThinkingMode::Adaptive) => {
            let always = entry.map(|e| e.thinking) == Some(ThinkingMode::Always);
            let display = match spec.thinking_display.as_str() {
                "updates" if always => {
                    betas.push(BETA_THINKING_UPDATES.to_string());
                    "updates"
                }
                "omitted" => "omitted",
                _ => "summarized",
            };
            Some(json!({"type": "adaptive", "display": display}))
        }
        _ => None,
    };
    let output_config = match (entry.map(|e| e.effort), &spec.effort) {
        (Some(true), Some(e)) => Some(json!({"effort": e})),
        _ => None,
    };
    if spec.refusal_fallbacks
        && spec.first_party
        && entry.map(|e| e.refusal_fallbacks).unwrap_or(false)
    {
        betas.push(BETA_FALLBACKS.to_string());
        extra.insert("fallbacks".to_string(), Value::String("default".into()));
    }
    let system = if spec.system_text.is_empty() {
        Vec::new()
    } else {
        vec![
            json!({"type": "text", "text": spec.system_text, "cache_control": {"type": "ephemeral"}}),
        ]
    };
    let req = ProviderRequest {
        model: spec.model.clone(),
        max_tokens: spec.max_tokens,
        system,
        messages,
        tools: spec.tools.clone(),
        thinking,
        output_config,
        cache_control: Some(json!({"type": "ephemeral"})),
        betas,
        extra,
    };
    (req, prefix.len(), tail.len(), repairs)
}

fn is_thinking(b: &Value) -> bool {
    matches!(
        b.get("type").and_then(Value::as_str),
        Some("thinking") | Some("redacted_thinking")
    )
}

fn tool_result_block(r: &Node) -> Value {
    match &r.body {
        Body::ToolResult {
            tool_use_id,
            content,
            is_error,
            ..
        } => {
            if *is_error {
                json!({"type": "tool_result", "tool_use_id": tool_use_id, "content": content, "is_error": true})
            } else {
                json!({"type": "tool_result", "tool_use_id": tool_use_id, "content": content})
            }
        }
        _ => json!({}),
    }
}

/// A background job's real result, delivered as text after its placeholder
/// was already answered.
fn late_result_text(r: &Node) -> String {
    match &r.body {
        Body::ToolResult {
            tool_use_id,
            tool,
            status,
            content,
            meta,
            ..
        } => {
            let exit = meta
                .get("exit_code")
                .and_then(Value::as_i64)
                .map(|c| format!(", exit code {c}"))
                .unwrap_or_default();
            format!(
                "[Background result for your earlier {tool} call ({tool_use_id}): status {}{exit}]\n{content}",
                status.as_str()
            )
        }
        _ => String::new(),
    }
}

/// Nodes → provider messages. Tool results are placed in one user message
/// right after the assistant message whose `tool_use` blocks they answer,
/// `tool_result` blocks first; consecutive same-role messages merge.
pub fn render_messages(
    prefix: &[&Node],
    tail: &[&Node],
    strip_prefix_thinking: bool,
) -> (Vec<Value>, Vec<String>) {
    let mut results: HashMap<&str, &Node> = HashMap::new();
    for n in prefix.iter().chain(tail.iter()) {
        if let Body::ToolResult {
            tool_use_id,
            late: false,
            ..
        } = &n.body
        {
            results.insert(tool_use_id.as_str(), n);
        }
    }
    let mut out: Vec<(String, Vec<Value>)> = Vec::new();
    let mut repairs = Vec::new();
    fn push(out: &mut Vec<(String, Vec<Value>)>, role: &str, blocks: Vec<Value>) {
        if blocks.is_empty() {
            return;
        }
        match out.last_mut() {
            Some((r, b)) if r == role => b.extend(blocks),
            _ => out.push((role.to_string(), blocks)),
        }
    }
    let items = prefix
        .iter()
        .map(|n| (*n, true))
        .chain(tail.iter().map(|n| (*n, false)));
    for (n, in_prefix) in items {
        match &n.body {
            Body::UserMessage { text } => push(
                &mut out,
                "user",
                vec![json!({"type": "text", "text": text})],
            ),
            Body::AssistantMessage { blocks, .. } => {
                let bl: Vec<Value> = if in_prefix && strip_prefix_thinking {
                    blocks.iter().filter(|b| !is_thinking(b)).cloned().collect()
                } else {
                    blocks.clone()
                };
                if bl.is_empty() {
                    continue;
                }
                let uses = tool_uses_in(&bl);
                push(&mut out, "assistant", bl);
                if !uses.is_empty() {
                    let mut rb = Vec::new();
                    for u in uses {
                        match results.get(u.id.as_str()) {
                            Some(r) => rb.push(tool_result_block(r)),
                            None => {
                                repairs.push(u.id.clone());
                                rb.push(json!({
                                    "type": "tool_result",
                                    "tool_use_id": u.id,
                                    "content": "No result was recorded for this call (the harness was interrupted before it ran).",
                                    "is_error": true,
                                }));
                            }
                        }
                    }
                    push(&mut out, "user", rb);
                }
            }
            Body::ToolResult { late: true, .. } => push(
                &mut out,
                "user",
                vec![json!({"type": "text", "text": late_result_text(n)})],
            ),
            _ => {}
        }
    }
    let msgs = out
        .into_iter()
        .map(|(role, content)| json!({"role": role, "content": content}))
        .collect();
    (msgs, repairs)
}

/// The result status a background placeholder carries.
pub fn is_placeholder(n: &Node) -> bool {
    matches!(
        n.body,
        Body::ToolResult {
            status: ResultStatus::Background,
            late: false,
            ..
        }
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::node::Body;
    use theseus_protocol::Usage;

    fn spec(model: &str, system: &str) -> RequestSpec {
        RequestSpec {
            profile: "p".into(),
            provider: "anthropic".into(),
            model: model.into(),
            max_tokens: 1000,
            system_text: system.into(),
            context_files: vec![],
            tools: vec![
                json!({"name": "fs_read", "description": "d", "input_schema": {"type": "object"}}),
            ],
            effort: Some("high".into()),
            thinking_display: "summarized".into(),
            refusal_fallbacks: true,
            first_party: true,
        }
    }

    /// The tools digest the replaced sorted-key serializer gave (at 8a1e41d).
    /// If it changed, every stored manifest would read as a tool change, and
    /// each session's next turn would recompile and strip its thinking.
    #[test]
    fn the_tools_digest_matches_the_one_stored_manifests_carry() {
        let mut s = spec("m", "sys");
        s.tools = vec![
            json!({"name": "fs_read", "description": "Read a file", "input_schema": {"type": "object", "required": ["path"], "properties": {"path": {"type": "string"}, "offset": {"type": "integer", "minimum": 0}}}}),
            json!({"name": "proc_run", "input_schema": {"properties": {"argv": {"items": {"type": "string"}, "type": "array"}}, "type": "object"}, "description": "Run é ✓"}),
        ];
        let m = manifest_for(&s, &Catalog::builtin(), None, false);
        assert_eq!(m.tools_digest, "46f4e6a0c66ccbb0");
    }

    fn assistant(blocks: Vec<Value>) -> Node {
        Node::assistant(
            "s",
            "t",
            0,
            Body::AssistantMessage {
                blocks,
                model: "m".into(),
                provider: "anthropic".into(),
                stop_reason: None,
                usage: Usage::default(),
                cost_usd: None,
                catalog_version: None,
                request_id: None,
                correlation_id: None,
                compilation_id: None,
                request_digest: None,
            },
        )
    }

    fn result(id: &str, content: &str, status: ResultStatus, late: bool) -> Node {
        Node::tool_result(
            "s",
            None,
            None,
            Body::ToolResult {
                tool_use_id: id.into(),
                tool: "fs.read".into(),
                status,
                is_error: matches!(status, ResultStatus::Error | ResultStatus::Declined),
                content: content.into(),
                correlation_id: None,
                bytes_total: 0,
                truncated: false,
                full_ref: None,
                duration_ms: None,
                late,
                meta: json!({"exit_code": 0}),
            },
        )
    }

    fn thinking() -> Value {
        json!({"type": "thinking", "thinking": "", "signature": "sig"})
    }

    fn run(
        nodes: &[(u64, Node)],
        current: Option<&Compilation>,
        spec: &RequestSpec,
        last: u64,
    ) -> Compiled {
        compile(CompileInput {
            session_id: "s",
            current,
            nodes,
            last_position: last,
            spec,
            catalog: &Catalog::builtin(),
            force: None,
            window_override: None,
        })
    }

    #[test]
    fn appending_keeps_every_earlier_byte_and_tool_results_follow_their_call() {
        let sp = spec("claude-opus-5", "You are Theseus.");
        let mut nodes = vec![(1, Node::user("s", None, "web", "read /x"))];
        let c1 = run(&nodes, None, &sp, 1);
        assert!(c1.new_compilation);
        assert_eq!(c1.trigger.as_deref(), Some("new_session"));
        assert_eq!(c1.request.thinking.as_ref().unwrap()["type"], "adaptive");
        assert_eq!(c1.request.output_config.as_ref().unwrap()["effort"], "high");
        assert_eq!(c1.request.extra["fallbacks"], "default");
        assert!(c1.request.betas.contains(&BETA_FALLBACKS.to_string()));
        assert_eq!(c1.request.system[0]["cache_control"]["type"], "ephemeral");

        nodes.push((
            2,
            assistant(vec![
                thinking(),
                json!({"type": "text", "text": "Reading."}),
                json!({"type": "tool_use", "id": "t1", "name": "fs_read", "input": {"path": "/x"}}),
                json!({"type": "tool_use", "id": "t2", "name": "fs_read", "input": {"path": "/y"}}),
            ]),
        ));
        nodes.push((3, result("t2", "Y", ResultStatus::Ok, false)));
        nodes.push((4, result("t1", "X", ResultStatus::Error, false)));
        let c2 = run(&nodes, Some(&c1.compilation), &sp, 4);
        assert!(!c2.new_compilation);
        assert_eq!(c2.decision(), "append");
        assert_eq!(&c2.request.messages[..1], &c1.request.messages[..]);
        assert_eq!(c2.request.messages.len(), 3);
        // Results in one user message, in tool_use order, error flagged.
        let rb = &c2.request.messages[2]["content"];
        assert_eq!(rb[0]["tool_use_id"], "t1");
        assert_eq!(rb[0]["is_error"], true);
        assert_eq!(rb[1]["tool_use_id"], "t2");
        assert!(rb[1].get("is_error").is_none());
        // The thinking block replays verbatim.
        assert_eq!(c2.request.messages[1]["content"][0], thinking());

        nodes.push((5, assistant(vec![json!({"type": "text", "text": "Done."})])));
        nodes.push((6, Node::user("s", None, "web", "thanks")));
        let c3 = run(&nodes, Some(&c1.compilation), &sp, 6);
        assert_eq!(&c3.request.messages[..3], &c2.request.messages[..]);
        assert_eq!(c3.request.system, c2.request.system);
        assert_eq!(c3.request.tools, c2.request.tools);
        assert!(c3.repairs.is_empty());
    }

    #[test]
    fn system_change_recompiles_and_strips_prefix_thinking_but_model_change_keeps_it() {
        let sp = spec("claude-opus-5", "A");
        let nodes = vec![
            (1, Node::user("s", None, "web", "hi")),
            (
                2,
                assistant(vec![thinking(), json!({"type": "text", "text": "hello"})]),
            ),
            (3, Node::user("s", None, "web", "again")),
        ];
        let c1 = run(&nodes[..1], None, &sp, 1);
        let sp2 = spec("claude-opus-5", "B");
        let c2 = run(&nodes, Some(&c1.compilation), &sp2, 3);
        assert!(c2.new_compilation);
        assert_eq!(c2.trigger.as_deref(), Some("system_changed"));
        assert!(c2.compilation.manifest.strip_thinking);
        assert_eq!(
            c2.compilation.derived_from.as_deref(),
            Some(c1.compilation.id.as_str())
        );
        assert_eq!(
            c2.request.messages[1]["content"].as_array().unwrap().len(),
            1
        );

        let sp3 = spec("claude-sonnet-5", "A");
        let c3 = run(&nodes, Some(&c1.compilation), &sp3, 3);
        assert_eq!(c3.trigger.as_deref(), Some("model_changed"));
        assert!(!c3.compilation.manifest.strip_thinking);
        assert_eq!(c3.request.messages[1]["content"][0], thinking());
        assert!(
            !c3.request.extra.contains_key("fallbacks"),
            "sonnet 5 has no fallbacks row"
        );
    }

    #[test]
    fn late_results_render_as_text_and_missing_results_are_repaired() {
        let sp = spec("glm-5.3-flash", "");
        let nodes = vec![
            (1, Node::user("s", None, "web", "build it")),
            (
                2,
                assistant(vec![
                    json!({"type": "tool_use", "id": "t1", "name": "proc_run", "input": {}}),
                ]),
            ),
            (
                3,
                result("t1", "Still running", ResultStatus::Background, false),
            ),
            (
                4,
                assistant(vec![json!({"type": "text", "text": "Started."})]),
            ),
            (5, result("t1", "BUILD OK", ResultStatus::Ok, true)),
            (
                6,
                assistant(vec![
                    json!({"type": "tool_use", "id": "t9", "name": "fs_read", "input": {}}),
                ]),
            ),
        ];
        let c = run(&nodes, None, &sp, 6);
        assert!(c.request.thinking.is_none(), "GLM takes no thinking param");
        assert!(c.request.output_config.is_none());
        assert!(c.request.system.is_empty());
        let m = &c.request.messages;
        assert_eq!(m[2]["content"][0]["content"], "Still running");
        let late = m[4]["content"][0]["text"].as_str().unwrap();
        assert!(
            late.contains("Background result")
                && late.contains("BUILD OK")
                && late.contains("exit code 0")
        );
        assert_eq!(c.repairs, vec!["t9".to_string()]);
        assert_eq!(m.last().unwrap()["content"][0]["is_error"], true);
    }

    #[test]
    fn fresh_drops_the_past_and_overflow_rings_at_a_user_boundary() {
        let sp = spec("claude-opus-5", "S");
        let big = "word ".repeat(2_000);
        let mut nodes = Vec::new();
        for i in 0..6u64 {
            nodes.push((
                i * 2 + 1,
                Node::user("s", None, "web", &format!("q{i} {big}")),
            ));
            nodes.push((
                i * 2 + 2,
                assistant(vec![
                    thinking(),
                    json!({"type": "text", "text": format!("a{i}")}),
                ]),
            ));
        }
        let c = compile(CompileInput {
            session_id: "s",
            current: None,
            nodes: &nodes,
            last_position: 12,
            spec: &sp,
            catalog: &Catalog::builtin(),
            force: Some(Recompile::Fresh),
            window_override: None,
        });
        assert_eq!(c.compilation.strategy, "fresh");
        assert!(c.compilation.manifest.strip_thinking);
        assert_eq!(
            c.request.messages.len(),
            2,
            "only the latest exchange survives"
        );
        assert!(c.request.messages[0]["content"][0]["text"]
            .as_str()
            .unwrap()
            .starts_with("q5"));
        assert!(c.request.messages[1]["content"]
            .as_array()
            .unwrap()
            .iter()
            .all(|b| !is_thinking(b)));

        let c = compile(CompileInput {
            session_id: "s",
            current: None,
            nodes: &nodes,
            last_position: 12,
            spec: &sp,
            catalog: &Catalog::builtin(),
            force: None,
            window_override: Some(13_000),
        });
        assert_eq!(c.trigger.as_deref(), Some("overflow"));
        assert_eq!(c.compilation.strategy, "ring");
        assert!(c.compilation.manifest.strip_thinking);
        assert_eq!(
            c.request.messages[0]["role"], "user",
            "ring cuts before a user message"
        );
        assert!(c.request.messages.len() < 12);
        assert!(c.request.messages.iter().all(|m| m["content"]
            .as_array()
            .unwrap()
            .iter()
            .all(|b| !is_thinking(b))));
    }
}
