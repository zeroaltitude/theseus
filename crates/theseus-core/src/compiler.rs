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
//! that changes the model, the provider, the system prompt, or the tool set, or
//! drops leading turns, is the one place thinking blocks are stripped from the
//! prefix (a boundary the provider documents as safe). A thinking block's
//! signature is for the provider and model that wrote it, and another provider
//! refuses it (400, "Invalid signature in thinking block": GLM's sent to
//! Anthropic, theseus-kol), so a model change strips them too; and an assistant
//! message another provider wrote never carries its thinking, wherever it sits.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::attach::Media;
use crate::catalog::{Catalog, ThinkingMode, TokenRates};
use crate::config::{CacheTtl, Effort, ThinkingDisplay};
use crate::node::{Body, Node};
use crate::provider::{tool_uses_in, Census, ProviderRequest, ID_TOKENS, MESSAGE_TOKENS};

pub const COMPILER_VERSION: u32 = 1;
/// 2 since 13c (theseus-ev1): the system goes out as two blocks, and a block
/// whose prefix could never reach the model's caching minimum gets no marker.
pub const RENDERER_VERSION: u32 = 2;
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
    /// The context files the system's second block carried, in order
    /// (theseus-58a). Their text is in that block, so `system_digest` covers
    /// it.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub context_files: Vec<ContextFileRef>,
    /// Where the requests' cache breakpoints go (theseus-ev1); absent in a
    /// manifest from before 13c.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache: Option<CacheLayout>,
}

/// The cache breakpoints of a compilation's requests (theseus-ev1). The
/// provider caches a request's prefix in the order tools, system, messages,
/// up to each breakpoint. Each system block gets one, so an edit to a
/// context file rewrites the second block and what follows while the tools
/// and the header still read from the cache; the top-level automatic one
/// follows the conversation. That is 3 of the provider's 4. Fixed for the
/// compilation's life: the blocks, the tools, and the model are in its
/// digests. The TTLs are not: they are each request's (`context.compiled`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CacheLayout {
    /// The catalog's `caches`: the provider reads repeated prefixes. Without
    /// it no request carries a breakpoint.
    pub caches: bool,
    /// The model's shortest cacheable prefix, from the catalog.
    pub min_tokens: u32,
    /// The system blocks, in order, with their breakpoints.
    pub blocks: Vec<BlockBreakpoint>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BlockBreakpoint {
    /// `header` (the persona, the tools note, the profile's `system`) or
    /// `context` (the context files).
    pub block: String,
    /// The prefix the breakpoint closes, the tools and the system through
    /// this block, in bytes of their JSON.
    pub prefix_bytes: u64,
    /// The block carries `cache_control`. Not when the provider does not
    /// cache, nor when the prefix is too short to reach `min_tokens`
    /// (`MIN_BYTES_PER_TOKEN`): then the provider would never cache it, and
    /// the breakpoint would take a slot for nothing. A marked breakpoint
    /// whose prefix still falls short is skipped by the provider without an
    /// error, and the usage shows no write for it.
    pub marked: bool,
}

/// Bytes a token takes at the fewest, for the minimum's check. A prefix
/// under `min_tokens` × this can never reach the minimum, so its breakpoint
/// is dropped; any other is placed. Claude's tokenizers read this JSON at
/// about 2.6 bytes a token (Sonnet 5.5) to 3.1 (Haiku 4.5), and prose at
/// about 4. The compiler's chars/4 estimate would not do: on Eddie's config
/// it put the header, tools included, at about 3,350 tokens, which Sonnet
/// 5.5 counted at 5,045 and Haiku 4.5 at just under its 4,096 minimum (the
/// cache2 lane's live check, 2026-10-01). Under chars/4 a header that grew
/// past Haiku's minimum would still lose its breakpoint. A breakpoint
/// dropped that would have cached costs a rewrite; one placed that cannot
/// costs nothing.
pub const MIN_BYTES_PER_TOKEN: u64 = 2;

impl CacheLayout {
    /// The breakpoints set, by name, the conversation's last.
    pub fn breakpoints(&self) -> Vec<&str> {
        let mut out: Vec<&str> = self
            .blocks
            .iter()
            .filter(|b| b.marked)
            .map(|b| b.block.as_str())
            .collect();
        if self.caches {
            out.push("conversation");
        }
        out
    }
}

/// A context file as the system block carried it (theseus-58a): the
/// protocol's type, since `context.compiled` carries it (theseus-0g4).
pub use theseus_protocol::ContextFileRef;

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
    /// The system's first block, the header every session of the profile
    /// shares: the built-in persona, the tools note, and the profile's own
    /// `system` (theseus-ev1).
    pub system_text: String,
    /// The system's second block: the context files, the system level's,
    /// then the persona's; empty without any.
    pub context_text: String,
    /// The context files `context_text` carries, for the manifest.
    pub context_files: Vec<ContextFileRef>,
    /// The persona in play when the spec was built (theseus-c48), whose
    /// files follow the system level's in `context_text`.
    pub persona: Option<String>,
    /// Wire tool definitions, sorted by name.
    pub tools: Vec<Value>,
    pub effort: Option<Effort>,
    pub thinking_display: ThinkingDisplay,
    pub refusal_fallbacks: bool,
    /// The provider is Anthropic's own API (server-side fallbacks exist only there).
    pub first_party: bool,
    /// The profile's cache TTL, on the system blocks' breakpoints.
    pub cache_ttl: CacheTtl,
    /// The TTL of the conversation's breakpoint, the top-level automatic one:
    /// the profile's, or 5 minutes in a task. Never longer than `cache_ttl`,
    /// so a longer-lived entry never follows a shorter one, as the provider
    /// requires.
    pub conversation_ttl: CacheTtl,
}

#[derive(Clone, Copy)]
pub struct CompileInput<'a> {
    pub session_id: &'a str,
    pub current: Option<&'a Compilation>,
    /// Every node of the session with its WAL position, in position order.
    pub nodes: &'a [(u64, Arc<Node>)],
    /// The store's last position (the as-of for a new compilation).
    pub last_position: u64,
    pub spec: &'a RequestSpec,
    pub catalog: &'a Catalog,
    pub force: Option<Recompile>,
    /// Tests: pretend the model's window is this many tokens.
    pub window_override: Option<u64>,
    /// Where image blocks get their bytes (theseus-9g2); `None` shows every
    /// image as a line saying its bytes are missing.
    pub blobs: Option<&'a crate::blobs::Blobs>,
    /// The images the provider refused in this session (theseus-0s4), which
    /// render as their line.
    pub hidden: &'a [crate::session::NotShown],
    /// A recompile that strips the prefix's thinking, and why, when nothing
    /// else triggers one: a refused image sat before an answer of the model's
    /// (`image_not_shown`), so the history changed under that answer's
    /// thinking (theseus-0s4).
    pub strip: Option<&'a str>,
}

/// How far the bytes part of an estimate may run low, in percent of itself
/// (theseus-f5hf). The ring allows for it: it rings when the counted part
/// plus the bytes part × (1 + this) passes the window less the output cap
/// and the headroom. The worst measured: in Eddie's DM an 11 KB tool result
/// read at 1.76 bytes a token against Sonnet 5.5's figure of 2.4, and the
/// request's new part came to ×1.36 its estimate.
pub const MARGIN_PERCENT: u64 = 40;

/// A request's size in tokens, as the compiler estimates it (theseus-f5hf).
/// From a compilation's second call on, most of it is the provider's own
/// count: the session's latest answer came from this compilation's last
/// request, whose input the provider counted, and the answer costs as
/// input what it cost as output. Only what was written since is estimated,
/// from its bytes at the catalog's figures. Before that call (a new
/// session, a recompile, the ring's candidates) all of it is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Estimate {
    /// What the provider will count: `counted` + `estimated`.
    pub tokens: u64,
    pub method: EstimateMethod,
    /// The provider's count of this compilation's last request (its input,
    /// cache reads, and cache writes), plus its answer's output tokens.
    pub counted: u64,
    /// The rest, from its bytes at the catalog's `bytes_per_token`.
    pub estimated: u64,
    /// What the ring checks: `counted`, plus `estimated` and its margin
    /// ([`MARGIN_PERCENT`]).
    pub upper: u64,
    /// The request's bytes as JSON, base64 image data left out: the estimate
    /// before theseus-f5hf was a fourth of it.
    pub bytes: u64,
    /// The whole request's bytes by class, so a row can be estimated again
    /// at other figures.
    pub census: Census,
}

impl Estimate {
    /// The estimate as `context.compiled` carries it: the protocol's one
    /// definition of its shape (theseus-0g4).
    pub fn summary(&self) -> theseus_protocol::EstimateSummary {
        let c = &self.census;
        theseus_protocol::EstimateSummary {
            tokens: self.tokens,
            method: match self.method {
                EstimateMethod::Counted => "counted",
                EstimateMethod::Bytes => "bytes",
            }
            .into(),
            counted: self.counted,
            estimated: self.estimated,
            upper: self.upper,
            bytes: self.bytes,
            census: theseus_protocol::CensusSummary {
                json: c.json,
                text: c.text,
                opaque: c.opaque,
                messages: c.messages,
                blocks: c.blocks,
                ids: c.ids,
            },
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EstimateMethod {
    /// Part of it is the provider's count.
    Counted,
    /// All of it is from bytes.
    Bytes,
}

/// Estimate `request`'s tokens at `rates` (theseus-f5hf). `counted` is the
/// provider's count of what the request repeats, and the index of its first
/// message after that (`counted_part`). Leaves `bytes` at 0.
pub fn estimate(
    request: &ProviderRequest,
    rates: TokenRates,
    counted: Option<(u64, usize)>,
) -> Estimate {
    let census = request.census();
    let (method, counted, estimated) = match counted {
        Some((n, from)) => {
            let rest = &request.messages[from..];
            // An image's size is not in the request: each new one counts
            // at the model's most, until the next call counts it.
            let images = (images_in(rest) * image_cap(&request.model)).min(request.image_tokens);
            // The answer's own message: its framing, and its calls' ids,
            // which its output tokens leave out.
            let calls = request.messages[from - 1]["content"]
                .as_array()
                .map_or(0, |b| tool_uses_in(b).len() as u64);
            let answer = MESSAGE_TOKENS + calls * ID_TOKENS;
            (
                EstimateMethod::Counted,
                n,
                Census::of_messages(rest).tokens(rates) + answer + images,
            )
        }
        None => (
            EstimateMethod::Bytes,
            0,
            census.tokens(rates) + request.image_tokens,
        ),
    };
    Estimate {
        tokens: counted + estimated,
        method,
        counted,
        estimated,
        upper: counted + estimated + (estimated * MARGIN_PERCENT).div_ceil(100),
        bytes: 0,
        census,
    }
}

/// The provider's count of what `request` repeats, and where its new part
/// begins (theseus-f5hf). The session's latest answer must come from a call
/// of compilation `c`. Then every node before it was in that call's request:
/// nothing is written mid-call (a turn writes its input before its first
/// call, and tool results and late results between calls). The provider
/// counted that request's input, and the answer re-enters as input at its
/// output tokens, thinking included (Eddie's DM: to within the 6 tokens of
/// framing of it and the message after it). The request's last assistant
/// message must end with the answer's last block.
pub fn counted_part(
    nodes: &[(u64, Arc<Node>)],
    c: &Compilation,
    request: &ProviderRequest,
) -> Option<(u64, usize)> {
    let answer = nodes
        .iter()
        .rev()
        .find(|(_, n)| matches!(n.body, Body::AssistantMessage { .. }))?;
    let Body::AssistantMessage {
        blocks,
        usage,
        compilation_id,
        ..
    } = &answer.1.body
    else {
        return None;
    };
    let input =
        usage.input_tokens + usage.cache_read_input_tokens + usage.cache_creation_input_tokens;
    if compilation_id.as_deref() != Some(c.id.as_str()) || input == 0 {
        return None;
    }
    let at = request
        .messages
        .iter()
        .rposition(|m| m["role"] == "assistant")?;
    let last = request.messages[at]["content"].as_array()?.last();
    (last.is_some() && last == blocks.last()).then_some((input + usage.output_tokens, at + 1))
}

/// Image blocks in `messages`, those inside tool results included.
fn images_in(messages: &[Value]) -> u64 {
    fn count(blocks: &Value) -> u64 {
        blocks.as_array().map_or(0, |bs| {
            bs.iter()
                .map(|b| match b.get("type").and_then(Value::as_str) {
                    Some("image") => 1,
                    Some("tool_result") => count(&b["content"]),
                    _ => 0,
                })
                .sum()
        })
    }
    messages.iter().map(|m| count(&m["content"])).sum()
}

/// The most tokens one image costs `model`: its tile cap.
fn image_cap(model: &str) -> u64 {
    crate::catalog::image_tokens(model, 1 << 20, 1 << 20)
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
    /// `estimate.tokens`.
    pub est_tokens: u64,
    /// How the request's size was estimated (theseus-f5hf).
    pub estimate: Estimate,
    pub digest: String,
    /// tool_use ids that had no recorded result and got a synthetic one.
    pub repairs: Vec<String>,
    /// Where the request's cache breakpoints went (theseus-ev1).
    pub cache: CacheLayout,
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

/// The system blocks' names and texts, in order; an empty one is left out.
fn system_blocks(spec: &RequestSpec) -> Vec<(&'static str, &str)> {
    [
        ("header", spec.system_text.as_str()),
        ("context", spec.context_text.as_str()),
    ]
    .into_iter()
    .filter(|(_, text)| !text.is_empty())
    .collect()
}

/// The digest of the system blocks' texts. A lone block's is its text's, as
/// before the split; two are joined by a NUL, which the old single block,
/// joined by blank lines, never was. So a compilation from before the split
/// recompiles once (`system_changed`) and drops its prefix's thinking, whose
/// signatures were made over the old system (theseus-ev1).
fn system_digest(spec: &RequestSpec) -> String {
    let texts: Vec<&str> = system_blocks(spec).into_iter().map(|(_, t)| t).collect();
    sha(&texts.join("\u{0}"))
}

/// Where the breakpoints go (theseus-ev1): on each system block whose
/// prefix, the tools and the system through it, can reach the model's
/// minimum (`MIN_BYTES_PER_TOKEN`), when the provider caches at all.
pub fn cache_layout(spec: &RequestSpec, catalog: &Catalog) -> CacheLayout {
    let entry = catalog.get(&spec.model);
    let caches = entry.is_none_or(|e| e.caches);
    let min_tokens = entry.map_or(0, |e| e.cache_min_tokens);
    let len = |v: &Value| serde_json::to_string(v).map_or(0, |s| s.len() as u64);
    let mut bytes = len(&Value::Array(spec.tools.clone()));
    let blocks = system_blocks(spec)
        .into_iter()
        .map(|(block, text)| {
            bytes += len(&json!({"type": "text", "text": text}));
            BlockBreakpoint {
                block: block.into(),
                prefix_bytes: bytes,
                marked: caches && bytes >= min_tokens as u64 * MIN_BYTES_PER_TOKEN,
            }
        })
        .collect();
    CacheLayout {
        caches,
        min_tokens,
        blocks,
    }
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
        system_digest: system_digest(spec),
        tools_digest: theseus_kernel::digest_json(&Value::Array(spec.tools.clone()))[..16]
            .to_string(),
        tools,
        catalog_version: catalog.version.clone(),
        context_window: window,
        strip_thinking: strip,
        context_files: spec.context_files.clone(),
        cache: Some(cache_layout(spec, catalog)),
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
                // Each invalidates the prefix's thinking blocks: a system or
                // tool change the context they were thought in, and a model
                // or provider change their signatures (theseus-kol). The
                // earlier model's reasoning goes; its text, calls, and
                // results stay.
                Some((t.join("+"), "transcript", true))
            }
        }
    };
    // A refused image that sat before an answer now renders as its line
    // (theseus-0s4): the history under that answer's thinking changed, so the
    // thinking goes, as for any other change under it.
    if let (None, Some(why), Some(_)) = (&decided, input.strip, input.current) {
        decided = Some((why.to_string(), "transcript", true));
    }

    let all_renderable: Vec<&(u64, Arc<Node>)> =
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

    let media = (input.blobs, input.hidden);
    let (mut request, mut prefix_n, mut tail_n, mut repairs) =
        render_request(spec, input.catalog, &compilation, input.nodes, media);
    let rates = entry.map_or_else(|| TokenRates::of(&spec.model), |e| e.bytes_per_token);
    let counted = counted_part(input.nodes, &compilation, &request);
    let mut est = estimate(&request, rates, counted);

    // 2. Overflow: drop leading turns (ring), cutting only before a user
    // message. It rings on the estimate's upper bound, so a request whose
    // bytes run denser than the catalog's figures still rings in time; it
    // keeps turns while the estimate itself is under 60 %, and that
    // candidate's bound, at most 84 %, does not ring again (theseus-f5hf).
    if let Some(w) = window {
        let budget = w
            .saturating_sub(spec.max_tokens as u64)
            .saturating_sub(4_096);
        if est.upper > budget {
            let target = budget * 6 / 10;
            let seq: Vec<&Node> = all_renderable.iter().map(|(_, n)| &**n).collect();
            let starts: Vec<usize> = seq
                .iter()
                .enumerate()
                .filter(|(_, n)| matches!(n.body, Body::UserMessage { .. }))
                .map(|(i, _)| i)
                .collect();
            for &cut in starts.iter().skip(1) {
                let includes: Vec<String> = seq[cut..].iter().map(|n| n.id.clone()).collect();
                let candidate = make("overflow".into(), "ring", true, includes);
                let (r, p, t, rep) =
                    render_request(spec, input.catalog, &candidate, input.nodes, media);
                let e = estimate(&r, rates, None);
                let last = cut == *starts.last().unwrap();
                if e.tokens <= target || last {
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
    est.bytes = request.json_bytes();
    Compiled {
        request,
        compilation,
        new_compilation,
        trigger,
        prefix_nodes: prefix_n,
        tail_nodes: tail_n,
        messages,
        est_tokens: est.tokens,
        estimate: est,
        digest,
        repairs,
        cache: cache_layout(spec, input.catalog),
    }
}

/// Render a compilation plus its tail into a provider request. `media` is
/// where image blocks get their bytes, and the images the provider refused
/// in the session, which render as their line (theseus-0s4).
pub fn render_request(
    spec: &RequestSpec,
    catalog: &Catalog,
    c: &Compilation,
    nodes: &[(u64, Arc<Node>)],
    (blobs, hidden): (Option<&crate::blobs::Blobs>, &[crate::session::NotShown]),
) -> (ProviderRequest, usize, usize, Vec<String>) {
    let included: HashSet<&str> = c.includes.iter().map(String::as_str).collect();
    let prefix: Vec<&Node> = nodes
        .iter()
        .filter(|(pos, n)| *pos <= c.as_of && included.contains(n.id.as_str()) && renderable(n))
        .map(|(_, n)| &**n)
        .collect();
    let tail: Vec<&Node> = nodes
        .iter()
        .filter(|(pos, n)| *pos > c.as_of && renderable(n))
        .map(|(_, n)| &**n)
        .collect();
    let entry = catalog.get(&spec.model);
    // The compilation's model decides how its images show (theseus-9g2).
    let media = Media {
        vision: entry.is_some_and(|e| e.vision),
        model: &spec.model,
        blobs,
        hidden,
    };
    let (messages, repairs, image_tokens) = render_messages(
        &prefix,
        &tail,
        c.manifest.strip_thinking,
        &spec.provider,
        &media,
    );

    let mut betas = Vec::new();
    let mut extra = std::collections::BTreeMap::new();
    let thinking = match entry.map(|e| e.thinking) {
        Some(ThinkingMode::Always) | Some(ThinkingMode::Adaptive) => {
            let always = entry.map(|e| e.thinking) == Some(ThinkingMode::Always);
            let display = match spec.thinking_display {
                ThinkingDisplay::Updates if always => {
                    betas.push(BETA_THINKING_UPDATES.to_string());
                    "updates"
                }
                ThinkingDisplay::Omitted => "omitted",
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
    // The system in its blocks, each with its breakpoint where the layout
    // puts one, at the profile's TTL; the conversation's breakpoint is the
    // top-level automatic one. A longer-lived entry never follows a shorter
    // one (the provider's order rule), whatever the spec says.
    let layout = cache_layout(spec, catalog);
    let system = system_blocks(spec)
        .into_iter()
        .zip(&layout.blocks)
        .map(|((_, text), b)| {
            let mut block = json!({"type": "text", "text": text});
            if b.marked {
                block["cache_control"] = spec.cache_ttl.marker();
            }
            block
        })
        .collect();
    let conversation_ttl = spec.conversation_ttl.min(spec.cache_ttl);
    let req = ProviderRequest {
        model: spec.model.clone(),
        max_tokens: spec.max_tokens,
        system,
        messages,
        tools: spec.tools.clone(),
        thinking,
        output_config,
        cache_control: layout.caches.then(|| conversation_ttl.marker()),
        betas,
        extra,
        image_tokens,
    };
    (req, prefix.len(), tail.len(), repairs)
}

fn is_thinking(b: &Value) -> bool {
    matches!(
        b.get("type").and_then(Value::as_str),
        Some("thinking") | Some("redacted_thinking")
    )
}

fn tool_result_block(r: &Node, media: &Media, tokens: &mut u64) -> Value {
    match &r.body {
        Body::ToolResult {
            tool_use_id,
            content,
            is_error,
            image,
            ..
        } => {
            if *is_error {
                json!({"type": "tool_result", "tool_use_id": tool_use_id, "content": content, "is_error": true})
            } else if let Some(img) = image {
                // An image the tool returned (theseus-9g2).
                let content = crate::attach::tool_content(content, img, media, tokens);
                json!({"type": "tool_result", "tool_use_id": tool_use_id, "content": content})
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
/// `tool_result` blocks first; consecutive same-role messages merge. An
/// assistant message whose provider is not `provider`, the request's, keeps
/// no thinking block: its signature is one only its own provider can verify
/// (theseus-kol). Also returns the repaired calls and the tokens the images
/// are estimated at.
pub fn render_messages(
    prefix: &[&Node],
    tail: &[&Node],
    strip_prefix_thinking: bool,
    provider: &str,
    media: &Media,
) -> (Vec<Value>, Vec<String>, u64) {
    let mut image_tokens = 0u64;
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
            Body::UserMessage { text, attachments } => {
                // Each attachment is its own block (an image, two), before
                // the typed text (theseus-9g2). A message of attachments
                // alone has no text block; one without attachments renders
                // as it always did.
                let mut blocks: Vec<Value> = attachments
                    .iter()
                    .flat_map(|a| {
                        crate::attach::blocks(a, n.author.as_deref(), media, &mut image_tokens)
                    })
                    .collect();
                if !text.is_empty() || attachments.is_empty() {
                    blocks.push(json!({"type": "text", "text": text}));
                }
                push(&mut out, "user", blocks)
            }
            Body::AssistantMessage {
                blocks,
                provider: wrote,
                ..
            } => {
                let strip = (in_prefix && strip_prefix_thinking) || wrote != provider;
                let bl: Vec<Value> = if strip {
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
                            Some(r) => rb.push(tool_result_block(r, media, &mut image_tokens)),
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
    (msgs, repairs, image_tokens)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::node::{Body, ResultStatus};
    use theseus_protocol::Usage;

    fn spec(model: &str, system: &str) -> RequestSpec {
        RequestSpec {
            profile: "p".into(),
            provider: "anthropic".into(),
            model: model.into(),
            max_tokens: 1000,
            system_text: system.into(),
            context_text: String::new(),
            context_files: vec![],
            persona: None,
            tools: vec![
                json!({"name": "fs_read", "description": "d", "input_schema": {"type": "object"}}),
            ],
            effort: Some(Effort::High),
            thinking_display: ThinkingDisplay::Summarized,
            refusal_fallbacks: true,
            first_party: true,
            cache_ttl: CacheTtl::FiveMinutes,
            conversation_ttl: CacheTtl::FiveMinutes,
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
                image: None,
                external: None,
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
        let nodes: Vec<(u64, Arc<Node>)> = nodes
            .iter()
            .map(|(p, n)| (*p, Arc::new(n.clone())))
            .collect();
        compile(CompileInput {
            session_id: "s",
            current,
            nodes: &nodes,
            last_position: last,
            spec,
            catalog: &Catalog::builtin(),
            force: None,
            window_override: None,
            blobs: None,
            hidden: &[],
            strip: None,
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
        // A header this short is under Opus 5's caching minimum, so the only
        // breakpoint is the conversation's (theseus-ev1).
        assert!(c1.request.system[0].get("cache_control").is_none());
        assert_eq!(c1.request.cache_control, Some(json!({"type": "ephemeral"})));

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
    fn a_system_or_model_change_recompiles_and_strips_the_prefixs_thinking() {
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

        // A thinking block's signature is for the model that wrote it
        // (theseus-kol): a model change strips them as a system change does.
        let sp3 = spec("claude-sonnet-5", "A");
        let c3 = run(&nodes, Some(&c1.compilation), &sp3, 3);
        assert_eq!(c3.trigger.as_deref(), Some("model_changed"));
        assert!(c3.compilation.manifest.strip_thinking);
        assert_eq!(
            c3.request.messages[1]["content"],
            json!([{"type": "text", "text": "hello"}])
        );
        assert!(
            !c3.request.extra.contains_key("fallbacks"),
            "sonnet 5 has no fallbacks row"
        );
    }

    /// An assistant message another provider wrote: GLM's, with z.ai's
    /// signature, as the history a turn on Anthropic replays (theseus-kol).
    fn glm_said(blocks: Vec<Value>) -> Node {
        let mut n = assistant(blocks);
        if let Body::AssistantMessage {
            model, provider, ..
        } = &mut n.body
        {
            *model = "glm-5.3-flash".into();
            *provider = "zai".into();
        }
        n
    }

    fn glm_thinking() -> Value {
        json!({"type": "thinking", "thinking": "I should run it.", "signature": "zai-signature"})
    }

    /// The history's assistant message came from provider X (GLM, `zai`), and
    /// the request goes to Y (Anthropic): it carries none of X's thinking
    /// blocks with their signatures, wherever the message sits, in a new
    /// compilation's prefix or in an old one's tail, a tool loop's last
    /// message included (the 400 of theseus-kol). The same provider's keep
    /// theirs, byte for byte.
    #[test]
    fn a_thinking_block_goes_back_only_to_the_provider_that_wrote_it() {
        let call = json!({"type": "tool_use", "id": "t1", "name": "proc_run", "input": {"argv": ["sleep", "25"]}});
        let nodes = vec![
            (1, Node::user("s", None, "cli", "run sleep 25")),
            (2, glm_said(vec![glm_thinking(), call.clone()])),
            (
                3,
                result("t1", "Still running", ResultStatus::Background, false),
            ),
        ];
        let no_thinking = |c: &Compiled| {
            c.request.messages.iter().all(|m| {
                m["content"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .all(|b| !is_thinking(b))
            }) && !serde_json::to_string(&c.request.messages)
                .unwrap()
                .contains("zai-signature")
        };

        // A new compilation, made for Anthropic: nothing strips its prefix,
        // and still GLM's thinking stays home.
        let sonnet = spec("claude-sonnet-5-5", "A");
        let c = run(&nodes, None, &sonnet, 3);
        assert!(!c.compilation.manifest.strip_thinking);
        assert!(no_thinking(&c), "{:?}", c.request.messages);
        assert_eq!(c.request.messages[1]["content"], json!([call]));

        // The same history, on GLM's own provider: its thinking goes back.
        let mut glm = spec("glm-5.3-flash", "A");
        glm.provider = "zai".into();
        let g = run(&nodes, None, &glm, 3);
        assert_eq!(g.request.messages[1]["content"][0], glm_thinking());

        // A GLM message in the tail of a compilation made for Anthropic
        // (one that was never replaced): stripped there too.
        let c1 = run(&nodes[..1], None, &sonnet, 1);
        let c2 = run(&nodes, Some(&c1.compilation), &sonnet, 3);
        assert_eq!(c2.decision(), "append");
        assert!(no_thinking(&c2), "{:?}", c2.request.messages);

        // And when the operator switches the live profile mid-session, the
        // next turn's recompile strips every earlier thinking block.
        let s1 = run(&nodes, None, &glm, 3);
        let switched = run(&nodes, Some(&s1.compilation), &sonnet, 3);
        assert_eq!(switched.trigger.as_deref(), Some("model_changed"));
        assert!(switched.compilation.manifest.strip_thinking);
        assert!(no_thinking(&switched));
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
        let nodes: Vec<(u64, Arc<Node>)> =
            nodes.into_iter().map(|(p, n)| (p, Arc::new(n))).collect();
        let c = compile(CompileInput {
            session_id: "s",
            current: None,
            nodes: &nodes,
            last_position: 12,
            spec: &sp,
            catalog: &Catalog::builtin(),
            force: Some(Recompile::Fresh),
            window_override: None,
            blobs: None,
            hidden: &[],
            strip: None,
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
            blobs: None,
            hidden: &[],
            strip: None,
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

    /// A spec whose header and context blocks have these many characters.
    fn two_blocks(model: &str, header: usize, context: usize) -> RequestSpec {
        let mut s = spec(model, &"h".repeat(header));
        s.context_text = "c".repeat(context);
        s
    }

    fn hi() -> Vec<(u64, Node)> {
        vec![(1, Node::user("s", None, "web", "hi"))]
    }

    /// Each system block's `cache_control`, then the top-level one.
    fn marks(c: &Compiled) -> Vec<Value> {
        c.request
            .system
            .iter()
            .map(|b| b.get("cache_control").cloned().unwrap_or(Value::Null))
            .chain([c.request.cache_control.clone().unwrap_or(Value::Null)])
            .collect()
    }

    /// The system goes out in two blocks, the header and the context files,
    /// each with its breakpoint, and the conversation's is the top-level one:
    /// 3 of the provider's 4 (theseus-ev1). An edit to a context file changes
    /// the second block only: the header's bytes, and with the tools before
    /// them its cache entry, stay.
    #[test]
    fn the_system_is_two_blocks_each_with_a_breakpoint() {
        let sp = two_blocks("claude-opus-5", 4_000, 2_000);
        let c = run(&hi(), None, &sp, 1);
        let sys = &c.request.system;
        assert_eq!(sys.len(), 2);
        assert_eq!(sys[0]["text"], sp.system_text);
        assert_eq!(sys[1]["text"], sp.context_text);
        let five = json!({"type": "ephemeral"});
        assert_eq!(marks(&c), [five.clone(), five.clone(), five]);
        assert_eq!(c.cache.breakpoints(), ["header", "context", "conversation"]);
        let m = c.compilation.manifest.cache.clone().unwrap();
        assert_eq!(m, c.cache);
        assert!(m.caches && m.blocks.iter().all(|b| b.marked));
        assert_eq!(m.min_tokens, 512);
        // Each prefix is the tools and the system through its block, in bytes.
        let len = |v: &Value| serde_json::to_string(v).unwrap().len() as u64;
        let tools = len(&Value::Array(sp.tools.clone()));
        let header = len(&json!({"type": "text", "text": sp.system_text}));
        let context = len(&json!({"type": "text", "text": sp.context_text}));
        assert_eq!(m.blocks[0].prefix_bytes, tools + header);
        assert_eq!(m.blocks[1].prefix_bytes, tools + header + context);

        // An edited context file recompiles; the header's bytes do not move.
        let mut edited = sp;
        edited.context_text.push_str(" and one more line");
        let c2 = run(&hi(), Some(&c.compilation), &edited, 1);
        assert_eq!(c2.trigger.as_deref(), Some("system_changed"));
        assert_eq!(
            serde_json::to_string(&c2.request.system[0]).unwrap(),
            serde_json::to_string(&c.request.system[0]).unwrap()
        );
        assert_ne!(c2.request.system[1], c.request.system[1]);
        assert_eq!(c2.request.tools, c.request.tools);
    }

    /// Without context files the system is one block, with the digest and
    /// the bytes it had before the split, so those sessions do not recompile.
    /// With them the digest is new, so a compilation from before the split
    /// recompiles once and drops its prefix's thinking, whose signatures were
    /// made over the old single block (theseus-ev1).
    #[test]
    fn one_block_keeps_its_old_digest_and_two_recompile_once() {
        // A header over the minimum, as every real one is but on Haiku 4.5.
        let header = format!("You are Theseus. {}", "h".repeat(4_000));
        let one = spec("claude-opus-5", &header);
        let m = manifest_for(&one, &Catalog::builtin(), None, false);
        assert_eq!(m.system_digest, sha(&header));
        let c = run(&hi(), None, &one, 1);
        assert_eq!(
            c.request.system,
            vec![json!({"type": "text", "text": header, "cache_control": {"type": "ephemeral"}})]
        );
        // The old layout: one block, the files after a blank line.
        let mut two = one;
        two.context_text = "## A context file".into();
        let mut old = c.compilation;
        old.manifest.system_digest = sha(&format!("{}\n\n{}", two.system_text, two.context_text));
        old.manifest.cache = None;
        let nodes = vec![
            (1, Node::user("s", None, "web", "hi")),
            (
                2,
                assistant(vec![thinking(), json!({"type": "text", "text": "hello"})]),
            ),
            (3, Node::user("s", None, "web", "again")),
        ];
        let c2 = run(&nodes, Some(&old), &two, 3);
        assert_eq!(c2.trigger.as_deref(), Some("system_changed"));
        assert!(c2.compilation.manifest.strip_thinking);
        assert_eq!(c2.request.system.len(), 2);
    }

    /// A breakpoint whose prefix can never reach the model's minimum gets
    /// none: the provider would not cache it, and it would take a slot
    /// (theseus-ev1). A token takes 2 bytes at the fewest, so on Haiku 4.5
    /// (4,096 tokens) a prefix under 8,192 bytes is surely short: this
    /// ~4,100-byte header gets none, and the ~24,000-byte context block one.
    /// The manifest says which is which. On Opus 5, whose minimum is 512
    /// (1,024 bytes), both are marked.
    #[test]
    fn a_prefix_under_the_models_minimum_gets_no_breakpoint() {
        let sp = two_blocks("claude-haiku-4-5", 4_000, 20_000);
        let c = run(&hi(), None, &sp, 1);
        let five = json!({"type": "ephemeral"});
        assert_eq!(marks(&c), [Value::Null, five.clone(), five.clone()]);
        let m = c.compilation.manifest.cache.clone().unwrap();
        assert_eq!(m.min_tokens, 4_096);
        let header = &m.blocks[0];
        assert_eq!((header.block.as_str(), header.marked), ("header", false));
        assert!((4_000..8_192).contains(&header.prefix_bytes), "{header:?}");
        let context = &m.blocks[1];
        assert_eq!((context.block.as_str(), context.marked), ("context", true));
        assert!(context.prefix_bytes >= 24_000, "{context:?}");
        assert_eq!(c.cache.breakpoints(), ["context", "conversation"]);
        let stored = serde_json::to_value(&c.compilation.manifest).unwrap();
        assert_eq!(stored["cache"]["blocks"][0]["marked"], false, "{stored}");

        // The bound exactly: a header prefix of 8,192 bytes is marked on
        // Haiku, and one of 8,191 is not.
        let len = |v: &Value| serde_json::to_string(v).unwrap().len();
        let base = len(&Value::Array(sp.tools.clone())) + len(&json!({"type": "text", "text": ""}));
        for (bytes, marked) in [(8_192, true), (8_191, false)] {
            let at = spec("claude-haiku-4-5", &"h".repeat(bytes - base));
            let b = cache_layout(&at, &Catalog::builtin()).blocks[0].clone();
            assert_eq!((b.prefix_bytes, b.marked), (bytes as u64, marked));
        }

        // Without context files, Haiku's only breakpoint is the conversation's.
        let alone = spec("claude-haiku-4-5", &"h".repeat(4_000));
        let c = run(&hi(), None, &alone, 1);
        assert_eq!(marks(&c), [Value::Null, five.clone()]);

        let mut opus = sp;
        opus.model = "claude-opus-5".into();
        let c = run(&hi(), None, &opus, 1);
        assert_eq!(marks(&c), [five.clone(), five.clone(), five]);
    }

    /// The TTL on the wire (theseus-ev1). `5m` is the API's default, so its
    /// markers name none: the bytes of every request before 13c. `1h` is on
    /// every breakpoint, the top-level automatic one included. A task keeps
    /// its own conversation at 5 minutes after the header's 1-hour entries,
    /// the order the provider allows, and no spec puts a longer-lived entry
    /// after a shorter one. A TTL is in no digest, so changing it does not
    /// recompile.
    #[test]
    fn the_ttl_goes_on_every_breakpoint_longest_first() {
        let five = json!({"type": "ephemeral"});
        let hour = json!({"type": "ephemeral", "ttl": "1h"});
        let mut sp = two_blocks("claude-opus-5", 4_000, 2_000);
        let c = run(&hi(), None, &sp, 1);
        assert_eq!(marks(&c), [five.clone(), five.clone(), five.clone()]);
        assert_eq!(c.request.body()["cache_control"], five);

        sp.cache_ttl = CacheTtl::OneHour;
        sp.conversation_ttl = CacheTtl::OneHour;
        let c = run(&hi(), None, &sp, 1);
        assert_eq!(marks(&c), [hour.clone(), hour.clone(), hour.clone()]);
        assert_eq!(c.request.body()["cache_control"], hour);
        assert_eq!(c.request.body()["system"][0]["cache_control"]["ttl"], "1h");

        // A task on a 1-hour profile: the header's entries for an hour, its
        // own conversation's for 5 minutes.
        sp.conversation_ttl = CacheTtl::FiveMinutes;
        let c = run(&hi(), None, &sp, 1);
        assert_eq!(marks(&c), [hour.clone(), hour, five.clone()]);

        // Never a 1-hour entry after a 5-minute one.
        sp.cache_ttl = CacheTtl::FiveMinutes;
        sp.conversation_ttl = CacheTtl::OneHour;
        let c = run(&hi(), None, &sp, 1);
        assert_eq!(marks(&c), [five.clone(), five.clone(), five]);

        // A TTL change keeps the compilation, and so the prefix's thinking.
        let mut longer = sp;
        longer.cache_ttl = CacheTtl::OneHour;
        longer.conversation_ttl = CacheTtl::OneHour;
        let again = run(&hi(), Some(&c.compilation), &longer, 1);
        assert!(!again.new_compilation);
    }

    /// A model whose provider does not cache gets no breakpoints at all, and
    /// its manifest says so (theseus-ev1). Every built-in model caches, so
    /// it takes a config's `caches = false`.
    #[test]
    fn a_model_that_does_not_cache_gets_no_breakpoints() {
        let mut rows = std::collections::BTreeMap::new();
        rows.insert(
            "claude-opus-5".to_string(),
            crate::catalog::CatalogRow {
                caches: Some(false),
                ..Default::default()
            },
        );
        let catalog = Catalog::with_overrides(&rows);
        let sp = two_blocks("claude-opus-5", 4_000, 2_000);
        let nodes: Vec<(u64, Arc<Node>)> =
            hi().into_iter().map(|(p, n)| (p, Arc::new(n))).collect();
        let c = compile(CompileInput {
            session_id: "s",
            current: None,
            nodes: &nodes,
            last_position: 1,
            spec: &sp,
            catalog: &catalog,
            force: None,
            window_override: None,
            blobs: None,
            hidden: &[],
            strip: None,
        });
        assert_eq!(c.request.system.len(), 2);
        assert_eq!(marks(&c), [Value::Null, Value::Null, Value::Null]);
        assert!(c.request.body().get("cache_control").is_none());
        let m = c.compilation.manifest.cache.unwrap();
        assert!(!m.caches && m.blocks.iter().all(|b| !b.marked));
        assert!(c.cache.breakpoints().is_empty());
    }

    /// A compilation stored before 13c has no `cache` in its manifest. It
    /// reads as none, and with the same one-block system the next turn
    /// appends to it, rendering the same bytes.
    #[test]
    fn a_manifest_from_before_the_cache_layout_reads_as_none() {
        let sp = spec("claude-opus-5", "You are Theseus.");
        let c = run(&hi(), None, &sp, 1);
        let mut v = serde_json::to_value(&c.compilation).unwrap();
        assert!(v["manifest"]["cache"].is_object());
        v["manifest"].as_object_mut().unwrap().remove("cache");
        let old: Compilation = serde_json::from_value(v).unwrap();
        assert!(old.manifest.cache.is_none());
        let mut nodes = hi();
        nodes.push((2, Node::user("s", None, "web", "again")));
        let c2 = run(&nodes, Some(&old), &sp, 2);
        assert!(!c2.new_compilation, "{:?}", c2.trigger);
        assert_eq!(c2.request.system, c.request.system);
    }

    // ------------------------------------------------- the estimate (theseus-f5hf)

    /// `run` with a window.
    fn run_in(
        nodes: &[(u64, Node)],
        current: Option<&Compilation>,
        spec: &RequestSpec,
        last: u64,
        window: u64,
    ) -> Compiled {
        let nodes: Vec<(u64, Arc<Node>)> = nodes
            .iter()
            .map(|(p, n)| (*p, Arc::new(n.clone())))
            .collect();
        compile(CompileInput {
            session_id: "s",
            current,
            nodes: &nodes,
            last_position: last,
            spec,
            catalog: &Catalog::builtin(),
            force: None,
            window_override: Some(window),
            blobs: None,
            hidden: &[],
            strip: None,
        })
    }

    /// An answer a call of compilation `cmp` returned, which the provider
    /// counted: `input` tokens in (a cache write, as a first call's is),
    /// `output` out.
    fn answered(blocks: Vec<Value>, cmp: &str, input: u64, output: u64) -> Node {
        let mut n = assistant(blocks);
        if let Body::AssistantMessage {
            usage,
            compilation_id,
            ..
        } = &mut n.body
        {
            *usage = Usage {
                input_tokens: 4,
                cache_creation_input_tokens: input - 4,
                output_tokens: output,
                ..Default::default()
            };
            *compilation_id = Some(cmp.into());
        }
        n
    }

    fn call(id: &str) -> Value {
        json!({"type": "tool_use", "id": id, "name": "fs_read", "input": {"path": "/x"}})
    }

    /// `n` bytes of a tool's JSON output.
    fn json_output(n: usize) -> String {
        let row = r#"{"id":1234,"name":"node_17","kind":"file","size":4096,"tags":["a","b"]},"#;
        row.repeat(n / row.len() + 1)[..n].to_string()
    }

    /// A conversation heavy in tool results on Sonnet 5.5. chars/4 puts it
    /// under the ring's budget, while its count at the recorded ratio for
    /// Theseus's JSON (×1.52 of chars/4, the cache2 lane's first request)
    /// passes the window itself: the provider would refuse it. The estimate
    /// reads the JSON at the catalog's figure, and the ring rings.
    #[test]
    fn the_ring_rings_for_json_that_chars4_reads_as_fitting() {
        let sp = spec("claude-sonnet-5-5", "S");
        let out = json_output(88_000);
        let mut nodes = Vec::new();
        for i in 0..3u64 {
            let id = format!("t{i}");
            nodes.push((3 * i + 1, Node::user("s", None, "web", "read the next")));
            nodes.push((3 * i + 2, assistant(vec![call(&id)])));
            nodes.push((3 * i + 3, result(&id, &out, ResultStatus::Ok, false)));
        }
        let window = 100_000;
        let budget = window - 1_000 - 4_096;
        // The request as it stands, with no window to ring at.
        let whole = run(&nodes, None, &sp, 9);
        let chars4 = whole.estimate.bytes / 4;
        assert!(chars4 < budget, "chars/4 {chars4} rings by itself");
        assert!(
            chars4 * 152 / 100 > window,
            "the recorded ratio's count fits"
        );
        assert_eq!(whole.estimate.method, EstimateMethod::Bytes);
        assert!(whole.est_tokens > chars4, "{:?}", whole.estimate);
        assert!(
            whole.estimate.upper > chars4 * 152 / 100,
            "{:?}",
            whole.estimate
        );

        let c = run_in(&nodes, None, &sp, 9, window);
        assert_eq!(c.trigger.as_deref(), Some("overflow"));
        assert_eq!(c.compilation.strategy, "ring");
        assert!(c.est_tokens <= budget * 6 / 10, "{:?}", c.estimate);
        assert!(
            c.estimate.upper <= budget,
            "the kept turns do not ring again"
        );
    }

    /// From a compilation's second call on, the estimate adds what is new
    /// to the provider's own count of the last request and its answer.
    #[test]
    fn a_compilations_second_call_adds_to_the_providers_count() {
        let sp = spec("claude-sonnet-5-5", "S");
        let mut nodes = vec![(1, Node::user("s", None, "web", "read /x"))];
        let c1 = run(&nodes, None, &sp, 1);
        assert_eq!(c1.estimate.method, EstimateMethod::Bytes);
        assert_eq!(c1.estimate.counted, 0);
        let id = c1.compilation.id.clone();
        // The provider counted 5,204 tokens in and 88 out; then 11,142 bytes
        // of tool output.
        nodes.push((2, answered(vec![call("t1")], &id, 5_204, 88)));
        nodes.push((
            3,
            result("t1", &json_output(11_142), ResultStatus::Ok, false),
        ));
        let c2 = run(&nodes, Some(&c1.compilation), &sp, 3);
        assert!(!c2.new_compilation);
        let e = c2.estimate;
        assert_eq!(e.method, EstimateMethod::Counted);
        assert_eq!(e.counted, 5_204 + 88);
        // The new part: the output at 2.4 bytes a token, the result's
        // message and block, two ids (the call's and the result's), and the
        // answer's message.
        assert_eq!(e.estimated, 4_643 + 3 + 1 + 2 * 15 + 3);
        assert_eq!(e.tokens, e.counted + e.estimated);
        assert_eq!(
            e.upper,
            e.counted + e.estimated + (e.estimated * 40).div_ceil(100)
        );
        assert_eq!(c2.est_tokens, e.tokens);

        // An answer of another compilation, or one with no count, is not
        // counted on: the estimate is all bytes again.
        let mut other = nodes.clone();
        other[1].1 = answered(vec![call("t1")], "cmp_other", 5_204, 88);
        let c = run(&other, Some(&c1.compilation), &sp, 3);
        assert_eq!(c.estimate.method, EstimateMethod::Bytes);
        let mut uncounted = nodes.clone();
        uncounted[1].1 = assistant(vec![call("t1")]);
        let c = run(&uncounted, Some(&c1.compilation), &sp, 3);
        assert_eq!(c.estimate.method, EstimateMethod::Bytes);
        // A recompile makes a new compilation, which no answer is of yet.
        let mut sp = sp;
        sp.system_text = "S2".into();
        let c = run(&nodes, Some(&c1.compilation), &sp, 3);
        assert!(c.new_compilation);
        assert_eq!(c.estimate.method, EstimateMethod::Bytes);
    }

    /// Recorded counts: each loop of Eddie's DM on Sonnet 5.5 (2026-10-01),
    /// as the provider counted it, against the estimate from the loop
    /// before's count. Each row: the last request's count and its answer's
    /// output tokens and calls; then the bytes of tool output, or of a
    /// message, the request added; and the provider's count of it. The
    /// estimate is never under the count by more than its margin, so the
    /// bound the ring checks is never under it.
    #[test]
    fn the_estimate_is_within_its_margin_of_recorded_counts() {
        // (count, output, calls, tool output bytes, message bytes, next count)
        let dm: [(u64, u64, usize, usize, usize, u64); 19] = [
            (4_633, 88, 1, 11_142, 0, 11_092),
            (11_092, 695, 0, 0, 84, 11_819),
            (11_819, 125, 2, 18_848, 0, 19_914),
            (19_914, 287, 3, 38_499, 0, 34_542),
            (34_542, 4_176, 0, 0, 105, 38_754),
            (38_754, 432, 3, 228, 0, 39_380),
            (39_380, 233, 1, 2_141, 0, 40_527),
            (40_527, 336, 3, 3_421, 0, 42_789),
            (42_789, 425, 2, 3_555, 0, 44_868),
            (44_868, 505, 1, 7_515, 0, 48_200),
            (48_200, 1_060, 0, 0, 66, 49_286),
            (49_286, 126, 1, 2_141, 0, 50_383),
            (50_383, 310, 1, 526, 0, 51_001),
            (51_001, 197, 1, 2_437, 0, 52_114),
            (52_114, 282, 1, 1_811, 0, 53_070),
            (53_070, 558, 3, 4_658, 0, 55_728),
            (55_728, 429, 1, 2_416, 0, 57_283),
            (57_283, 722, 0, 0, 92, 58_036),
            (58_036, 343, 1, 833, 0, 58_726),
        ];
        let sp = spec("claude-sonnet-5-5", "S");
        let first = run(&hi(), None, &sp, 1);
        let mut worst: f64 = 1.0;
        for (count, output, calls, out_bytes, msg_bytes, next) in dm {
            let ids: Vec<String> = (0..calls).map(|k| format!("t{k}")).collect();
            let mut nodes = hi();
            let blocks: Vec<Value> = if calls == 0 {
                vec![json!({"type": "text", "text": "a"})]
            } else {
                ids.iter().map(|id| call(id)).collect()
            };
            nodes.push((2, answered(blocks, &first.compilation.id, count, output)));
            let mut p = 3;
            for id in &ids {
                let share = out_bytes / calls;
                nodes.push((p, result(id, &json_output(share), ResultStatus::Ok, false)));
                p += 1;
            }
            if msg_bytes > 0 {
                nodes.push((p, Node::user("s", None, "web", &"m".repeat(msg_bytes))));
                p += 1;
            }
            let c = run(&nodes, Some(&first.compilation), &sp, p);
            let e = c.estimate;
            assert_eq!(e.method, EstimateMethod::Counted);
            assert!(e.upper >= next, "{e:?} against {next}");
            worst = worst.min(e.tokens as f64 / next as f64);
        }
        // At its worst, an 11 KB tool output read at 1.76 bytes a token, the
        // estimate is 15 % low; the ring's bound covers it.
        assert!(worst > 0.84, "{worst}");
    }

    /// GLM's tokenizer reads Theseus's requests at about four bytes a token
    /// (×1.02 of chars/4, the cache2 lane), so its figures leave a GLM
    /// conversation unrung where Sonnet 5.5's would ring the same bytes.
    #[test]
    fn a_glm_conversation_is_not_rung_early() {
        let window = 200_000;
        let mut glm = spec("glm-5.3-flash", "S");
        glm.provider = "zai".into();
        // A conversation that chars/4 puts at 65 % of the budget.
        let budget = window - 1_000 - 4_096;
        let text = "word ".repeat((budget * 65 / 100 * 4 / 6) as usize / 5);
        let mut nodes = Vec::new();
        for i in 0..6u64 {
            nodes.push((i * 2 + 1, Node::user("s", None, "web", &text)));
            nodes.push((
                i * 2 + 2,
                assistant(vec![json!({"type": "text", "text": "ok"})]),
            ));
        }
        nodes.pop();
        let c = run_in(&nodes, None, &glm, 11, window);
        let chars4 = c.estimate.bytes / 4;
        assert!(chars4 > budget * 64 / 100, "{chars4}");
        assert_eq!(
            c.trigger.as_deref(),
            Some("new_session"),
            "{:?}",
            c.estimate
        );
        assert!(c.est_tokens < chars4 * 115 / 100, "{:?}", c.estimate);
        // Counted, GLM's own count of 90 % of the budget does not ring.
        let first = run_in(&hi(), None, &glm, 1, window);
        let mut counted = hi();
        counted.push((
            2,
            answered(
                vec![json!({"type": "text", "text": "ok"})],
                &first.compilation.id,
                budget * 90 / 100,
                2,
            ),
        ));
        counted.push((3, Node::user("s", None, "web", "and then?")));
        let c = run_in(&counted, Some(&first.compilation), &glm, 3, window);
        assert_eq!(c.estimate.method, EstimateMethod::Counted);
        assert!(!c.new_compilation, "{:?}", c.estimate);
    }
}
