//! `node.list`, `node.reach` and `compilation.list`: a session's nodes as
//! clients render them, where a node was seen and who copied it, and the
//! compilations a session has (theseus-n4m step 12a, theseus-2qzx). Moved
//! out of `lib.rs` whole, and re-exported there, so every path is the same
//! (theseus-pf8a).

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::ReachFork;

/// One node as clients render it (a message, a tool call, a tool result).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct NodeInfo {
    pub node_id: String,
    /// `user_message`, `assistant_message`, `tool_call`, `tool_result`.
    pub kind: String,
    pub session_id: String,
    pub position: u64,
    pub at_unix_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub turn_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub loop_index: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub author: Option<String>,
    /// Text for display (user text; assistant text blocks; tool result content).
    #[serde(default)]
    pub text: String,
    /// Thinking summaries (assistant), when the provider returned them.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub thinking: String,
    /// Kind-specific fields: model/usage/cost (assistant), tool/input/gate
    /// (tool call), tool/status/is_error/bytes (tool result).
    #[serde(default)]
    #[cfg_attr(test, ts(type = "Record<string, unknown> | null"))]
    pub detail: Value,
    #[serde(default)]
    pub bytes: u64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct SessionRecompileParams {
    pub session_id: String,
    /// `fresh` (start over) or `transcript` (keep everything, thinking stripped).
    pub strategy: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct NodeListParams {
    #[serde(default)]
    pub session_id: Option<String>,
    #[serde(default)]
    pub kind: Option<String>,
    #[serde(default)]
    pub n: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct NodeListResult {
    pub nodes: Vec<NodeInfo>,
    pub total: u64,
}

/// `node.reach` (theseus-n4m, step 12a; design stage2 §2.11).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct NodeReachParams {
    /// A node's whole id, or its last 6 or more characters, or the cockpit's
    /// `msg·a1b2c3`, when one node ends so (theseus-glyw).
    pub node_id: String,
    /// How many generations of copies to follow: 3 by default, at most 16.
    /// 0 answers the node's own session only.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub max_generations: Option<u32>,
}

/// A compilation whose prefix (`includes`) holds a node.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct ReachCompilation {
    pub compilation_id: String,
    /// `transcript`, `fresh`, or `ring`.
    pub strategy: String,
    pub created_at_ms: u64,
}

/// Where a node was seen in its own session: the compilations that hold it,
/// and the loops whose context held it (a model call made after it was
/// written, whose compilation holds it or left it in the tail). A context is
/// a compilation or a loop. The first and last exposure are absent when it
/// was never seen.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct ReachExposure {
    pub compilations: Vec<ReachCompilation>,
    pub loops: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub first_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub last_ms: Option<u64>,
}

/// A copy of the node in another session, found by walking the edges into
/// it: generation 1 copies the node, generation 2 a copy, and so on.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct ReachDescendant {
    pub node_id: String,
    pub session_id: String,
    pub position: u64,
    pub generation: u32,
    /// The edge's kind: `derived_from`.
    pub via: String,
    /// The route that wrote the edge: `report` or `brief`.
    pub route: String,
    /// The node it copies, one generation up.
    pub from: String,
    #[serde(flatten)]
    pub exposure: ReachExposure,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct ReachTotals {
    /// Every compilation and loop, the node's and its copies'.
    pub contexts: u64,
    /// The sessions the node and its copies are in.
    pub sessions: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct NodeReachResult {
    pub node_id: String,
    pub session_id: String,
    pub position: u64,
    pub direct: ReachExposure,
    pub descendants: Vec<ReachDescendant>,
    /// The forks that took it by reference (theseus-2qzx), generation by
    /// generation; absent when none.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub forks: Vec<ReachFork>,
    pub totals: ReachTotals,
    /// The walk stopped short: a node at `max_generations` has copies of
    /// its own, or the walk reached its cap of copies.
    pub partial: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct CompilationListParams {
    #[serde(default)]
    pub session_id: Option<String>,
    #[serde(default)]
    pub n: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct CompilationInfo {
    pub compilation_id: String,
    pub session_id: String,
    pub created_at_ms: u64,
    pub trigger: String,
    pub strategy: String,
    pub as_of: u64,
    pub includes: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub derived_from: Option<String>,
    /// The manifest as stored (model, provider, digests, catalog version, strip_thinking).
    #[cfg_attr(test, ts(type = "Record<string, unknown>"))]
    pub manifest: Value,
    /// This is the session's current compilation.
    #[serde(default)]
    pub current: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct CompilationListResult {
    pub compilations: Vec<CompilationInfo>,
}
