//! The layered request (theseus-h4a7, thread-context): what the system holds,
//! in the order the provider caches it, so what sessions share is the same
//! bytes in every request and is cached once. The four breakpoints:
//!
//! 1. **`header`**: the built-in persona, the assembly note, the precedence
//!    line, the tools note, the profile's `system`, and the owner's standing
//!    block for the session's place class (a shared place's carries its
//!    scope's view, as `standing` renders it).
//! 2. **`persona`**: every configured persona's context files, in the
//!    config's order (a persona's name). Empty, and unmarked, with none.
//! 3. **`project`**: the system-level context files, then the guidance of the
//!    session's categories (a thread's topics among them).
//! 4. the messages, under the top-level automatic breakpoint.
//!
//! **Nothing per request inside layers 1 to 3**: no date, session id, place
//! name or turn count. What is the session's own goes in the unmarked
//! `session` block after them (its directory, `session_block`) or in the
//! messages' tail, so two sessions on a profile and a place class render
//! layers 1 and 2 byte for byte, and layer 3 when they share categories. A
//! layer is marked only when its prefix can reach the model's caching
//! minimum. The layers' text is rendered here and nowhere else; the prefix
//! library (row T13) builds on this module.

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use super::{session_block, sha, RequestSpec};
use crate::catalog::Catalog;

/// The layers' names, in a compilation's cache layout and its `context.compiled`.
pub const HEADER: &str = "header";
pub const PERSONA: &str = "persona";
pub const PROJECT: &str = "project";

/// The cache breakpoints of a compilation's requests (theseus-ev1). The
/// provider caches a request's prefix in the order tools, system, messages,
/// up to each breakpoint. Each of the three layers gets one, so an edit to a
/// project file rewrites the third layer and what follows while the tools,
/// the header and the persona still read from the cache; the top-level
/// automatic one follows the conversation: the provider's four. Fixed: for
/// its life, the blocks, the tools, and the model are in its digests. The
/// TTLs are not: they are each request's (`context.compiled`).
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
    /// `header`, `persona`, `project` (the layers), or `session` (its
    /// directory, aab7, which carries no breakpoint of its own). A
    /// compilation from before the layers names its second block `context`.
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
/// about 4. The compiler's chars/4 estimate would not do: on the owner's config
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

/// The system blocks' names and texts, in order; an empty one is left out.
pub(super) fn system_blocks(spec: &RequestSpec) -> Vec<(&'static str, &str)> {
    [
        (HEADER, spec.system_text.as_str()),
        (PERSONA, spec.persona_text.as_str()),
        (PROJECT, spec.context_text.as_str()),
        (session_block::NAME, spec.session_text.as_str()),
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
pub(crate) fn system_digest(spec: &RequestSpec) -> String {
    let texts: Vec<&str> = system_blocks(spec).into_iter().map(|(_, t)| t).collect();
    sha(&texts.join("\u{0}"))
}

/// Where the breakpoints go (theseus-ev1): on each layer whose prefix, the
/// tools and the system through it, can reach the model's minimum
/// (`MIN_BYTES_PER_TOKEN`), when the provider caches at all. The session's
/// block carries none: the conversation's breakpoint after it covers it.
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
                marked: caches
                    && block != session_block::NAME
                    && bytes >= min_tokens as u64 * MIN_BYTES_PER_TOKEN,
            }
        })
        .collect();
    CacheLayout {
        caches,
        min_tokens,
        blocks,
    }
}

/// Each system block's digest, by name: what a layer was, in the order they
/// are sent. Two sessions that share a layer share its digest.
pub fn layer_digests(spec: &RequestSpec) -> Vec<(String, String)> {
    system_blocks(spec)
        .into_iter()
        .map(|(name, text)| (name.to_string(), sha(text)))
        .collect()
}

/// The digests of the system blocks a request carries, named by the layout
/// that placed them: what was sent, guidance and all.
pub fn digests_of(layout: &CacheLayout, system: &[Value]) -> Vec<(String, String)> {
    layout
        .blocks
        .iter()
        .zip(system)
        .map(|(b, block)| {
            (
                b.block.clone(),
                sha(block["text"].as_str().unwrap_or_default()),
            )
        })
        .collect()
}
