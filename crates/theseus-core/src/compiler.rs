//! The context compiler's shared pieces (spec §4.4, §4.4a, Part II P5): the compilation record and its
//! manifest, the request spec, the estimate, the render and the spec preparation. Every request is the view's
//! (`compiler::view`, issue 44): the old way of building one from a session's transcript, deciding append
//! or recompile, is gone, and a compilation it made still decodes (a session may still point at one).
//!
//! The renderer is deterministic: the same nodes and the same manifest produce the same bytes, and within a
//! chain a later request's messages begin with the earlier request's unchanged. That is what keeps the
//! provider's prompt cache warm and what the preserved-thinking rules require. A thinking block's signature is
//! for the provider and model that wrote it, and another provider refuses it (400, "Invalid signature in
//! thinking block": GLM's sent to Anthropic, theseus-kol), so an assistant message another provider or model
//! wrote never carries its thinking, wherever it sits.

use std::sync::Arc;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::attach::Media;
use crate::catalog::{Catalog, ThinkingMode, TokenRates};
use crate::config::{CacheTtl, Effort, ThinkingDisplay};
use crate::node::{Body, Node};
use crate::provider::{tool_uses_in, Census, ProviderRequest, ID_TOKENS, MESSAGE_TOKENS};
use theseus_protocol::memory::{BudgetDrop, BudgetReport};

mod budget;
use budget::{budget_report, request_budget};
pub use layers::{cache_layout, BlockBreakpoint, CacheLayout, MIN_BYTES_PER_TOKEN};
use layers::{system_blocks, system_digest};
pub mod cache;
pub(crate) mod clearing;
pub mod effort;
pub mod layers;
pub(crate) mod phases;
pub mod session_block;
pub mod summary;
#[cfg(test)]
mod tests_clear;
#[cfg(test)]
mod tests_layers;
#[cfg(test)]
pub(crate) mod tests_parts;
#[cfg(test)]
mod tests_search_refs;
#[cfg(test)]
mod tests_view;
pub use clearing::Pass;
pub mod situation;
pub mod view;
pub use effort::{output_config, BETA_TURN_EFFORT};
pub(crate) mod pairing;
mod render;
mod server_calls;
#[cfg(test)]
use render::is_thinking;
pub use render::{render_messages, Around, Messages};

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
    /// The retired prefix library's header the third layer was (theseus-rr15, format 41): an old
    /// compilation's, still read; a new one records none.
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
    /// Why it was made: a view chain's (`compiler::view`); an old store also holds the transcript's
    /// (`new_session`, `model_changed`, `system_changed`, `tools_changed`, `overflow`, `manual_fresh`,
    /// `manual_transcript`, ...), which nothing makes now.
    pub trigger: String,
    /// `view` (a view turn's chain), or `fork` (the record a new fork opens with: its inherited run, by
    /// reference; the fork's own turns compile views). An old store also holds `transcript` (the old
    /// builder's, and a fork's record before 2026-11-22), `fresh`, `ring`, `compaction` and `cold_resume`,
    /// which nothing makes now; they still decode.
    pub strategy: String,
    /// WAL position the selection was made at: later nodes are the tail.
    pub as_of: u64,
    /// The prefix: node ids, in order.
    pub includes: Vec<String>,
    #[serde(default)]
    pub derived_from: Option<String>,
    pub manifest: Manifest,
    /// What it fitted into the model's window and what it left out (M6 30b,
    /// §2.11): a clearing pass's drops, a recall's drops, an overage.
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
    /// A new chain within the turn (the provider dropped a block whose prefix changed): the whole history
    /// again, the earlier thinking stripped.
    pub force: bool,
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
    /// What the compile is for, as the compile step tells it (M6 35a).
    pub situation: &'a situation::Situation,
}

/// A request that passed the model's window, as the provider said it
/// (theseus-9p88): it refused the prompt ("prompt is too long"), or cut the
/// answer at the window (`model_context_window_exceeded`). Either way the
/// estimate and the catalog's window were wrong about it, so the next
/// compile measures by the provider's numbers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Overflowed {
    /// The provider's count of the request's prompt, when it gave one.
    pub counted: Option<u64>,
    /// Theseus's estimate of the same request (`Estimate::tokens`).
    pub estimated: u64,
    /// The window the provider measured against: the refusal's maximum,
    /// or a cut answer's prompt plus its output. A compile measures by it
    /// when it is below the catalog's.
    pub maximum: Option<u64>,
    /// The answer cut at the window that the next call replaces: it is not
    /// rendered, nor are its calls.
    pub retrying: Option<String>,
}

/// How far the bytes part of an estimate may run low, in percent of itself
/// (theseus-f5hf). The overage allows for it: a request is over when the
/// counted part plus the bytes part × (1 + this) passes the window less the
/// output cap and the headroom. The worst measured: in the owner's DM an 11 KB tool result
/// read at 1.76 bytes a token against Sonnet 5.5's figure of 2.4, and the
/// request's new part came to ×1.36 its estimate.
pub const MARGIN_PERCENT: u64 = 40;

/// A request's size in tokens, as the compiler estimates it (theseus-f5hf).
/// From a compilation's second call on, most of it is the provider's own
/// count: the session's latest answer came from this compilation's last
/// request, whose input the provider counted, and the answer costs as
/// input what it cost as output. Only what was written since is estimated,
/// from its bytes at the catalog's figures. Before that call (a new
/// session, a recompile) all of it is.
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
    /// What the overage checks: `counted`, plus `estimated` and its margin
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
    /// The situation it settled in (M6 35a): what its request may admit.
    pub situation: situation::Situation,
    /// The clearing pass this compile made (theseus-3ki0), for its row.
    pub cleared: Option<Pass>,
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
        prefix: None,
    }
}

/// Whether a node renders into a request: a call node does not, since its
/// reply carries the call (`node.reach` counts by the same rule).
pub(crate) fn renderable(n: &(impl crate::stub::Shaped + ?Sized)) -> bool {
    n.kind() != crate::stub::Kind::ToolCall
}

/// The spec preparation every strategy runs before it renders: the spec an append renders and the one a
/// recompile renders, with the walk's guidance composed into the third layer and the deferred tool set
/// resolved (`tools_defer::forks`). `None` for the first: the two are one spec.
pub(crate) fn prepared<'a>(
    input: &CompileInput<'a>,
) -> (
    Option<std::borrow::Cow<'a, RequestSpec>>,
    std::borrow::Cow<'a, RequestSpec>,
) {
    crate::tools_defer::forks(input)
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
    (blobs, hidden, retrying, sources): (
        Option<&crate::blobs::Blobs>,
        &[crate::session::NotShown],
        Option<&str>,
        &crate::recall::render::Sources,
    ),
) -> Rendered {
    // An assembled prefix's recall section is its, wherever it was written.
    let (prefix, tail) = situation::selected(c, nodes);
    let prefix = summary::summaries_first(prefix, c.recall_id.as_deref());
    let tail: Vec<&Node> = tail.into_iter().map(|(_, n)| n).collect();
    let entry = catalog.get(&spec.model);
    let fallback = spec.fallback.as_ref().map(|(m, r)| (&m[..], r.as_deref()));
    let media = media_of(spec, catalog, blobs, hidden);
    let (retrying, cleared) = (retrying.or(fallback.and_then(|f| f.1)), clearing::of(c));
    let strip = c.manifest.strip_thinking;
    // A replayed tool search stands only on a request that offers what it found (theseus-it6th).
    let offers = server_calls::Offers::of(&spec.tools);
    // The gate's call records, which render nothing: they prove which call each result answers.
    let records: Vec<&Node> = (nodes.iter())
        .filter(|(_, n)| !renderable(n))
        .map(|(_, n)| &**n)
        .collect();
    let mut rendered = render_messages(
        &prefix,
        &tail,
        strip,
        &spec.provider,
        &media,
        retrying,
        (sources, &cleared, &offers, &records),
    );

    let mut betas = Vec::new();
    let mut extra = std::collections::BTreeMap::new();
    // A fallback's request is one that model takes (theseus-7gir.18).
    let entry = fallback.map_or(entry, |f| catalog.get(f.0));
    let (thinking, thinking_beta) = thinking_config(entry, spec.thinking_display);
    betas.extend(thinking_beta);
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

/// What the request asks for in `thinking`: the model's own mode decides
/// whether it is sent at all, and the profile decides how the blocks are
/// shown. Only a model that always thinks takes `display: "updates"`, and
/// that display rides a beta, which comes back alongside the config for the
/// caller to put on the request.
fn thinking_config(
    entry: Option<&crate::catalog::CatalogEntry>,
    thinking_display: ThinkingDisplay,
) -> (Option<Value>, Option<String>) {
    let mut beta = None;
    let thinking = match entry.map(|e| e.thinking) {
        Some(ThinkingMode::Always) | Some(ThinkingMode::Adaptive) => {
            let always = entry.map(|e| e.thinking) == Some(ThinkingMode::Always);
            let display = match thinking_display {
                ThinkingDisplay::Updates if always => {
                    beta = Some(BETA_THINKING_UPDATES.to_string());
                    "updates"
                }
                ThinkingDisplay::Omitted => "omitted",
                _ => "summarized",
            };
            Some(json!({"type": "adaptive", "display": display}))
        }
        _ => None,
    };
    (thinking, beta)
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
