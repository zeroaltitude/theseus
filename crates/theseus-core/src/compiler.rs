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

use std::sync::Arc;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::attach::Media;
use crate::catalog::{Catalog, ThinkingMode, TokenRates};
use crate::config::{CacheTtl, Effort, ThinkingDisplay};
use crate::node::{Body, Node};
use crate::prefix;
use crate::provider::{tool_uses_in, Census, ProviderRequest, ID_TOKENS, MESSAGE_TOKENS};
use theseus_protocol::memory::{BudgetDrop, BudgetReport};

mod budget;
use budget::{budget_report, request_budget};
pub use layers::{cache_layout, BlockBreakpoint, CacheLayout, MIN_BYTES_PER_TOKEN};
use layers::{system_blocks, system_digest};
pub mod cache;
pub mod card;
pub(crate) mod clearing;
pub mod cold;
pub mod compaction;
pub mod effort;
pub mod layers;
pub(crate) mod phases;
pub mod place_digest;
mod ring;
pub mod session_block;
#[cfg(test)]
mod tests_clear;
#[cfg(test)]
mod tests_cold;
#[cfg(test)]
mod tests_kept;
#[cfg(test)]
mod tests_layers;
#[cfg(test)]
mod tests_replay;
#[cfg(test)]
pub(crate) mod tests_ring;
#[cfg(test)]
mod tests_ring_sums;
pub use clearing::Pass;
pub mod situation;
pub use effort::{output_config, BETA_TURN_EFFORT};
pub mod kept;
mod render;
mod server_calls;
pub mod stubs;
#[cfg(test)]
use render::is_thinking;
pub use render::{render_messages, Messages};

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
    /// Every membership the compile walk used (theseus-8kk.1): its kind,
    /// category, origin, and as-of. While the session appends, the walk
    /// takes these again; a recompile takes the current ones.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub memberships: Vec<theseus_ontology::MembershipUsed>,
    /// Each category's guidance the system's second block carried, in
    /// order, with its version and digest.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub guidance: Vec<theseus_ontology::GuidanceUsed>,
    /// The owner's standing records the second block listed (theseus-a7vi).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub standing: Vec<crate::standing::StandingUsed>,
    /// The tool families the request loaded (theseus-7m7w.2); None before
    /// deferral, or with it off. An append takes these again.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub loaded: Option<Vec<String>>,
    /// The prefix library's header the third layer is (theseus-rr15, format 41).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prefix: Option<crate::prefix::PrefixUse>,
    // A manifest from 19a to the place rule (COMPILATION schema 4) also has
    // `audience`, `readers`, `integrity`, and `withheld`: they read, and are
    // left unread (COMPILATION schema 5, theseus-nbsh).
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
    /// `transcript` (everything so far), `fresh` (nothing), `ring` (leading
    /// turns dropped), `compaction` (a summary of them in their place, 30c).
    pub strategy: String,
    /// WAL position the selection was made at: later nodes are the tail.
    pub as_of: u64,
    /// The prefix: node ids, in order.
    pub includes: Vec<String>,
    #[serde(default)]
    pub derived_from: Option<String>,
    pub manifest: Manifest,
    /// What it fitted into the model's window and what it left out (M6 30b,
    /// §2.11): the ring's cut as a range, a recall's drops, an overage.
    /// Absent in a compilation from before it (COMPILATION's layout 7).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub budget: Option<BudgetReport>,
    /// An assembled prefix's recall section (M6 30c): the `Recall` node it
    /// renders first, whatever its position. Absent in a compilation from
    /// before it (COMPILATION's layout 8), and in one not assembled.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recall_id: Option<String>,
    /// The situation it was made in (M6 35a). Absent in a compilation from
    /// before it (COMPILATION's layout 9).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub situation: Option<situation::Situation>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub inherits: Option<theseus_protocol::Inherits>,
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
    /// The second layer: every configured persona's context files; empty
    /// without any (`layers`).
    pub persona_text: String,
    /// The third layer: the system level's context files, then the guidance
    /// of the session's categories; empty without any.
    pub context_text: String,
    /// A thread's card, at the head of its messages (`card`).
    pub card: Option<card::Card>,
    /// A cold resume of a thread (`cold`): this compile rewrites it small.
    pub cold: Option<cold::Cold>,
    /// The third block: the session's own (`session_block`); maybe empty.
    pub session_text: String,
    /// The context files `context_text` carries, for the manifest.
    pub context_files: Vec<ContextFileRef>,
    /// The persona in play when the spec was built (theseus-c48).
    pub persona: Option<String>,
    /// Wire tool definitions, sorted by name.
    pub tools: crate::tool_set::Tools,
    /// The families the request loads, the tools above being the whole catalog
    /// (`tools_defer`); `compile` resolves it. None: every tool loads.
    pub defer: Option<crate::tools_defer::Deferral>,
    pub effort: Option<Effort>,
    /// The turn's own, as a per-message effort (`effort`, theseus-o719).
    pub turn_effort: Option<Effort>,
    /// Models this daemon found to refuse per-message effort (theseus-posww).
    pub effort_refused: Vec<String>,
    pub thinking_display: ThinkingDisplay,
    pub refusal_fallbacks: bool,
    /// A refusal's fallback for the rest of the turn (theseus-7gir.18): the
    /// model its requests name, and the refused answer they leave out (none
    /// for a fallback a timeout made: nothing was answered to leave out).
    pub fallback: Option<(String, Option<String>)>,
    /// The provider is Anthropic's own API (server-side fallbacks exist only there).
    pub first_party: bool,
    /// The profile's cache TTL, on the system blocks' breakpoints.
    pub cache_ttl: CacheTtl,
    /// The conversation breakpoint's TTL: the profile's, or 5 minutes in a task;
    /// never longer than `cache_ttl` (the provider requires it).
    pub conversation_ttl: CacheTtl,
    /// The ontology's compile walk, fixed for the turn (theseus-8kk.1); none
    /// while the ontology holds no category. `compile` renders its guidance
    /// after the context files.
    pub walk: Option<Arc<crate::ontology::Walk>>,
    /// What the walk used, once `compile` has rendered it: the manifest's.
    pub memberships: Vec<theseus_ontology::MembershipUsed>,
    pub guidance: Vec<theseus_ontology::GuidanceUsed>,
    /// The prefix library's plan for the third layer (`prefix::applied`), and what it applied.
    pub prefix: Option<Arc<crate::prefix::Plan>>,
    pub prefix_used: Option<crate::prefix::PrefixUse>,
}

#[derive(Clone, Copy)]
pub struct CompileInput<'a> {
    pub session_id: &'a str,
    pub current: Option<&'a Compilation>,
    /// Every node of the session with its WAL position, in position order.
    pub nodes: &'a [(u64, crate::stub::Stub)],
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
    /// The provider said this turn's last request passed the window
    /// (theseus-9p88): the compilation rings, whatever the estimate says.
    pub overflowed: Option<&'a Overflowed>,
    /// The sources the session's `Recall` nodes render (M6 30b).
    pub sources: &'a crate::recall::render::Sources,
    /// CONTINUE's candidate signals' thresholds and the clock (M5 25b);
    /// `None` reads none.
    pub signals: Option<crate::signals::SignalsAt>,
    /// The assembled strategy's recall section (M6 30c): a new compilation
    /// renders this `Recall` node first in its prefix (`recall_id`).
    pub assembled: Option<&'a str>,
    /// What the compile is for, as the compile step tells it (M6 35a).
    pub situation: &'a situation::Situation,
    /// Where the session's render is kept across turns, and the caller's
    /// view of its nodes (theseus-0l3g; `kept`): none renders every node.
    pub kept: Option<(&'a kept::Kept, u64)>,
}

/// A request that passed the model's window, as the provider said it
/// (theseus-9p88): it refused the prompt ("prompt is too long"), or cut the
/// answer at the window (`model_context_window_exceeded`). Either way the
/// estimate and the catalog's window were wrong about it, so the next
/// compilation rings by the provider's numbers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Overflowed {
    /// The provider's count of the request's prompt, when it gave one.
    pub counted: Option<u64>,
    /// Theseus's estimate of the same request (`Estimate::tokens`).
    pub estimated: u64,
    /// The window the provider measured against: the refusal's maximum,
    /// or a cut answer's prompt plus its output. The ring uses it when it
    /// is below the catalog's.
    pub maximum: Option<u64>,
    /// The answer cut at the window that the next call replaces: it is not
    /// rendered, nor are its calls.
    pub retrying: Option<String>,
}

impl Overflowed {
    /// `tokens` from a bytes estimate, raised by how far the provider's
    /// count of the request that overflowed ran over its estimate.
    fn scale(&self, tokens: u64) -> u64 {
        match self.counted {
            Some(n) if self.estimated > 0 && n > self.estimated => {
                (tokens as u128 * n as u128).div_ceil(self.estimated as u128) as u64
            }
            _ => tokens,
        }
    }
}

/// How far the bytes part of an estimate may run low, in percent of itself
/// (theseus-f5hf). The ring allows for it: it rings when the counted part
/// plus the bytes part × (1 + this) passes the window less the output cap
/// and the headroom. The worst measured: in the owner's DM an 11 KB tool result
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
            // at the model's most, until the next call counts it. A new PDF
            // (theseus-c9l6) counts as every image and PDF the request
            // carries, the safe side, until the next call counts it.
            let images = match crate::attach::media_in(rest) {
                (images, 0) => (images * image_cap(&request.model)).min(request.image_tokens),
                _ => request.image_tokens,
            };
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
/// output tokens, thinking included (the owner's DM: to within the 6 tokens of
/// framing of it and the message after it). The request's last assistant
/// message must end with the answer's last block.
pub fn counted_part(
    nodes: &[(u64, crate::stub::Stub)],
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

/// The most tokens one image costs `model`: its tile cap.
fn image_cap(model: &str) -> u64 {
    crate::catalog::image_tokens(model, 1 << 20, 1 << 20)
}

#[derive(Debug, Clone)]
pub struct Compiled {
    /// What it fitted and left out (M6 30b): every compilation's, append or
    /// not; a new one's is stored with it.
    pub budget: BudgetReport,
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
    /// The context files the request carries as their headers alone: a
    /// shared place's that are not marked public (the place rule).
    pub withheld: u64,
    /// CONTINUE's candidate signals that fired, and the sizes beside them
    /// (M5 25b). They decide nothing here.
    pub signals: crate::signals::Signals,
    /// The situation it settled in (M6 35a): what its request may admit.
    pub situation: situation::Situation,
    /// The clearing pass this compile made (theseus-3ki0), for its row.
    pub cleared: Option<Pass>,
    /// The thread card this request renders, when it has one: a new
    /// compilation's is stored with it (`thread.card.<compilation id>`).
    pub card: Option<String>,
    /// The prefix library's pick this compile took, for its frame (theseus-rr15).
    pub prefix: Option<Arc<crate::prefix::Picked>>,
}

/// A rendered request.
pub struct Rendered {
    pub request: ProviderRequest,
    pub prefix_nodes: usize,
    pub tail_nodes: usize,
    pub repairs: Vec<String>,
    /// The first message that carries the tail.
    pub tail_from: usize,
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

pub(crate) fn sha(s: &str) -> String {
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
        system_digest: system_digest(spec),
        tools_digest: spec.tools.digest().to_string(),
        tools,
        catalog_version: catalog.version.clone(),
        context_window: window,
        strip_thinking: strip,
        context_files: spec.context_files.clone(),
        cache: Some(cache_layout(spec, catalog)),
        memberships: spec.memberships.clone(),
        guidance: spec.guidance.clone(),
        standing: vec![],
        loaded: crate::tools_defer::loaded_of(spec),
        prefix: spec.prefix_used.clone(),
    }
}

/// Whether a node renders into a request: a call node does not, since its
/// reply carries the call (`node.reach` counts by the same rule).
pub(crate) fn renderable(n: &(impl crate::stub::Shaped + ?Sized)) -> bool {
    n.kind() != crate::stub::Kind::ToolCall
}

/// Compile a loop's request. With an ontology walk, the spec an append
/// renders carries the guidance of the memberships the current manifest
/// recorded, and the one a recompile renders that of the current ones
/// (theseus-8kk.1): so a membership change waits for the next recompile,
/// and a guidance edit in play changes the system block, which recompiles.
pub fn compile(input: CompileInput<'_>) -> Compiled {
    let (appended, recompiled) = crate::tools_defer::forks(&input);
    if input.spec.card.is_some() {
        let appended = appended.unwrap_or_else(|| recompiled.clone());
        let (a, r) = card::split(appended, recompiled);
        return compile_with(input, &a, &r);
    }
    let appended = appended.as_deref().unwrap_or(&*recompiled);
    compile_with(input, appended, &recompiled)
}

/// `compile`, with the spec an append renders and the one a recompile does.
#[expect(clippy::too_many_lines, reason = "shape budget: split it")]
fn compile_with(
    input: CompileInput<'_>,
    appended: &RequestSpec,
    recompiled: &RequestSpec,
) -> Compiled {
    let mut ph = phases::Phases::new("compile");
    // The prefix library's header in the third layer (theseus-rr15).
    let (a, r) = (
        prefix::applied(appended),
        (!std::ptr::eq(appended, recompiled)).then(|| prefix::applied(recompiled)),
    );
    let (appended, recompiled) = (&*a, r.as_deref().unwrap_or(&*a));
    let spec = appended;
    let entry = input.catalog.get(&spec.model);
    let window = input.window_override.or(entry.map(|e| e.context_window));
    // The provider's word on the window beats the catalog's when it is
    // smaller (theseus-9p88): a catalog that overstates a window would let
    // every request past it.
    let window = match (window, input.overflowed.and_then(|o| o.maximum)) {
        (Some(w), Some(m)) => Some(w.min(m)),
        (w, m) => w.or(m),
    };
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
                // A thread resumed cold: the cache is gone, so the rewrite
                // refreshes its card and may make it small (`cold`).
                input
                    .spec
                    .cold
                    .map(|_| ("cold_resume".to_string(), "transcript", true))
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
    // A range the session moved to a thread stays out of a recompile, one
    // line in its place (theseus-q9b85); a frozen compilation that holds
    // what has moved, or lacks what came back, recompiles once.
    let moves = crate::moved::read(input.nodes);
    if let (None, Some(c)) = (&decided, input.current) {
        if moves.stale(c) {
            decided = Some(("moved".into(), "transcript", true));
        }
    }
    // A recompile, or a ring, renders the walk's current memberships.
    let now_manifest = if std::ptr::eq(appended, recompiled) {
        now_manifest
    } else if decided.is_some() {
        manifest_for(recompiled, input.catalog, window, false)
    } else {
        now_manifest
    };
    let mut spec = if decided.is_some() {
        recompiled
    } else {
        appended
    };
    // Past a compaction, its summary stands for its range (30c).
    let all_renderable: Vec<_> = compaction::visible(input.nodes)
        .into_iter()
        .filter(|(_, n)| moves.shows(n))
        .collect();
    let make =
        |trigger: String, strategy: &str, strip: bool, includes: Vec<String>, base: &Manifest| {
            Compilation {
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
                    ..base.clone()
                },
                budget: None,
                recall_id: input.assembled.map(str::to_string),
                situation: None,
                inherits: None,
            }
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
                    .rposition(|(_, n)| n.kind == crate::stub::Kind::UserMessage)
                    .unwrap_or(all_renderable.len());
                all_renderable[from..]
                    .iter()
                    .map(|(_, n)| n.id.clone())
                    .collect()
            } else {
                all_renderable.iter().map(|(_, n)| n.id.clone()).collect()
            };
            make(trigger, strategy, strip, includes, &now_manifest)
        }
    };
    let mut new_compilation = input
        .current
        .map(|c| c.id != compilation.id)
        .unwrap_or(true);
    let mut trigger = new_compilation.then(|| compilation.trigger.clone());

    let media = (
        input.blobs,
        input.hidden,
        input.overflowed.and_then(|o| o.retrying.as_deref()),
        input.sources,
    );
    ph.mark("decide");
    let held = input.kept.map(|(k, view)| (k, input.session_id, view));
    let mut rendered = render_kept(spec, input.catalog, &compilation, input.nodes, media, held);
    ph.mark("render");
    let rates = entry.map_or_else(|| TokenRates::of(&spec.model), |e| e.bytes_per_token);
    let counted = counted_part(input.nodes, &compilation, &rendered.request);
    let mut est = estimate(&rendered.request, rates, counted);
    ph.mark("estimate");
    let mut ring_drop: Option<BudgetDrop> = None;

    // 2. Old results cleared (`clearing`), then 3. overflow (`ring`): each
    // a new compilation, made from the recompile's spec and manifest.
    let mut cleared = None;
    let same = std::ptr::eq(spec, recompiled);
    let base = || match same {
        true => now_manifest.clone(),
        false => manifest_for(recompiled, input.catalog, window, false),
    };
    if let Some(cold) = spec
        .cold
        .filter(|c| new_compilation && est.tokens > c.working_tokens)
    {
        let b = base();
        let r = ring::Ring {
            recompiled,
            base: &b,
            renderable: &all_renderable,
            est: &est,
            budget: 0,
            rates,
            carried: compilation.budget.as_ref(),
        };
        if let Some(rung) = cold::resume(&input, &r, cold, &make) {
            ring_drop = Some(rung.cut).filter(|c| c.range.is_some());
            (compilation, rendered, est) = (rung.compilation, rung.rendered, rung.est);
            (spec, trigger) = (recompiled, Some("cold_resume".into()));
        }
    }
    if let Some(w) = window {
        let budget = request_budget(w, spec);
        let (b, seen) = (base(), (&compilation, &rendered.request, &est));
        let at = clearing::At {
            spec: recompiled,
            base: &b,
            budget,
            rates,
            new: new_compilation,
        };
        if let Some(m) = clearing::clear(&input, &at, seen, &make) {
            (compilation, rendered, est) = (m.compilation, m.rendered, m.est);
            (cleared, spec, new_compilation) = (Some(m.pass), recompiled, true);
            trigger = Some(compilation.trigger.clone());
        }
        ph.mark("clear");
        if est.upper > budget || input.overflowed.is_some() {
            let base = base();
            let (renderable, carried) = (&all_renderable, compilation.budget.as_ref());
            let r = ring::Ring {
                recompiled,
                base: &base,
                renderable,
                est: &est,
                budget,
                rates,
                carried,
            };
            if let Some(rung) = ring::ring(&input, &r, &make) {
                // A pass the ring's cut left nothing of is not one.
                cleared = cleared.filter(|_| rung.compilation.budget.is_some());
                ring_drop = Some(rung.cut);
                (compilation, rendered, est) = (rung.compilation, rung.rendered, rung.est);
                (spec, new_compilation) = (recompiled, true);
                trigger = Some("overflow".into());
            }
            ph.mark("ring");
        }
    }

    let withheld = spec
        .context_files
        .iter()
        .filter(|f| f.withheld.is_some())
        .count();
    let Rendered {
        request,
        prefix_nodes,
        tail_nodes,
        repairs,
        tail_from,
    } = rendered;
    let messages = request.messages.len();
    let digest = request.digest();
    est.bytes = request.json_bytes();
    // The results it renders cleared stay in its report (theseus-3ki0).
    let drops = clearing::drops(compilation.budget.as_ref()).cloned();
    let limit = window.map(|w| request_budget(w, spec));
    let budget = budget_report(limit, &est, drops.chain(ring_drop));
    let first = input.current.is_none();
    let situation = situation::settle(input.situation, first, new_compilation, trigger.as_deref());
    if new_compilation {
        compilation.budget = Some(budget.clone());
        compilation.situation = Some(situation.clone());
    }
    let signals = input.signals.map_or_else(Default::default, |at| {
        let seen = crate::signals::Seen {
            nodes: input.nodes,
            compilation: &compilation,
            request: &request,
            tail_from,
            tokens: est.tokens,
            window,
            rates,
        };
        crate::signals::read(&at, &seen)
    });
    ph.mark("report");
    ph.log(input.nodes, &format!(", {} messages", messages));
    Compiled {
        budget,
        request,
        compilation,
        new_compilation,
        trigger,
        prefix_nodes,
        tail_nodes,
        messages,
        est_tokens: est.tokens,
        estimate: est,
        digest,
        repairs,
        cache: cache_layout(spec, input.catalog),
        withheld: withheld as u64,
        signals,
        situation,
        cleared,
        card: spec.card.as_ref().map(|c| c.shown.clone()),
        prefix: None,
    }
}

/// Render a compilation plus its tail into a provider request. `media` is
/// where image blocks get their bytes, the images the provider refused in
/// the session, which render as their line (theseus-0s4), and the answer cut
/// at the window that this request retries, which it leaves out
/// (theseus-9p88).
pub fn render_request(
    spec: &RequestSpec,
    catalog: &Catalog,
    c: &Compilation,
    nodes: &[(u64, crate::stub::Stub)],
    media: (
        Option<&crate::blobs::Blobs>,
        &[crate::session::NotShown],
        Option<&str>,
        &crate::recall::render::Sources,
    ),
) -> Rendered {
    render_kept(spec, catalog, c, nodes, media, None)
}

/// `render_request`, taking up the session's kept render (`kept`,
/// theseus-0l3g) when `held` names where it is: the session and the
/// caller's view of its nodes.
fn render_kept(
    spec: &RequestSpec,
    catalog: &Catalog,
    c: &Compilation,
    nodes: &[(u64, crate::stub::Stub)],
    (blobs, hidden, retrying, sources): (
        Option<&crate::blobs::Blobs>,
        &[crate::session::NotShown],
        Option<&str>,
        &crate::recall::render::Sources,
    ),
    held: Option<(&kept::Kept, &str, u64)>,
) -> Rendered {
    // An assembled prefix's recall section is its, wherever it was written.
    let (prefix, tail) = situation::selected(c, nodes);
    let prefix = compaction::summaries_first(prefix, c.recall_id.as_deref());
    let tail: Vec<&Node> = tail.into_iter().map(|(_, n)| n).collect();
    let entry = catalog.get(&spec.model);
    let fallback = spec.fallback.as_ref().map(|(m, r)| (&m[..], r.as_deref()));
    let media = media_of(spec, catalog, blobs, hidden);
    let (retrying, cleared) = (retrying.or(fallback.and_then(|f| f.1)), clearing::of(c));
    let strip = c.manifest.strip_thinking;
    let mut rendered = match held {
        Some((kept, session, view)) => kept::render(
            kept::Kept1 {
                kept,
                session,
                key: kept::Key::of(&c.id, strip, &spec.provider, &media, view),
            },
            &prefix,
            &tail,
            kept::Args {
                strip_prefix_thinking: strip,
                provider: &spec.provider,
                media: &media,
                retrying,
                sources,
                cleared: &cleared,
            },
        ),
        None => render_messages(
            &prefix,
            &tail,
            strip,
            &spec.provider,
            &media,
            retrying,
            (sources, &cleared),
        ),
    };
    card::attach(spec, &mut rendered);

    let mut betas = Vec::new();
    let mut extra = std::collections::BTreeMap::new();
    // A fallback's request is one that model takes (theseus-7gir.18).
    let entry = fallback.map_or(entry, |f| catalog.get(f.0));
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
    let output_config = output_config(entry, spec.effort);
    if server_fallbacks(spec, entry) {
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
    let mut req = ProviderRequest {
        model: fallback.map_or(spec.model.as_str(), |f| f.0).to_string(),
        max_tokens: spec.max_tokens,
        system,
        messages: rendered.messages,
        tools: spec.tools.to_vec(),
        thinking,
        output_config,
        cache_control: layout.caches.then(|| conversation_ttl.marker()),
        betas,
        extra,
        image_tokens: rendered.image_tokens,
    };
    let tail_from = effort::place(&mut req, &rendered.efforts, spec, entry, rendered.tail_from);
    Rendered {
        request: req,
        prefix_nodes: prefix.len(),
        tail_nodes: tail.len(),
        repairs: rendered.repairs,
        tail_from,
    }
}

/// How a request on `spec` shows its images and PDFs: the compilation's
/// model decides (theseus-9g2, theseus-c9l6).
pub(super) fn media_of<'a>(
    spec: &'a RequestSpec,
    catalog: &Catalog,
    blobs: Option<&'a crate::blobs::Blobs>,
    hidden: &'a [crate::session::NotShown],
) -> Media<'a> {
    let entry = catalog.get(&spec.model);
    Media {
        vision: entry.is_some_and(|e| e.vision),
        pdf: entry.is_some_and(|e| e.pdf),
        pdf_pages: entry.map_or(0, |e| crate::attach::page_limit(e.context_window)),
        model: &spec.model,
        also: spec.fallback.as_ref().map(|f| f.0.as_str()),
        blobs,
        hidden,
    }
}

/// The provider's own refusal fallback rides the request (`fallbacks:
/// "default"`): the profile asks for it, the provider is Anthropic's API, and
/// the model takes it; never on a fallback's request (theseus-7gir.18).
pub fn server_fallbacks(spec: &RequestSpec, entry: Option<&crate::catalog::CatalogEntry>) -> bool {
    let takes = entry.is_some_and(|e| e.refusal_fallbacks);
    spec.refusal_fallbacks && spec.first_party && spec.fallback.is_none() && takes
}

#[cfg(test)]
mod tests;
