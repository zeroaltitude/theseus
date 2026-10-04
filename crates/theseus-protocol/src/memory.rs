//! Recall's wire types (M6 §2.14, step 30a): a recall's manifest, which a
//! turn's `recall.shadow` row holds and `memory.search` answers, and
//! `memory.recalls`, a session's manifests. Each name carries `Recall` or
//! `Memory`, since the web apps' types share one namespace.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::index::{IndexSourceRank, IndexTimings};

/// `memory.search`: recall's pipeline over `query`, as a turn in
/// `session_id`'s place would run it, writing nothing. With no session, as
/// the CLI's: a private place.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct MemorySearchParams {
    pub query: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub session_id: Option<String>,
    /// Hits asked of the index (default 40, at most 100).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub k: Option<usize>,
}

/// `memory.recalls`: a session's recalls, newest last.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct MemoryRecallsParams {
    pub session_id: String,
    /// The newest this many (default 10, at most 200).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub limit: Option<usize>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct MemoryRecallsResult {
    pub session_id: String,
    /// The session's recalls in all.
    pub total: u64,
    pub recalls: Vec<RecallManifest>,
}

/// One recall: what the index offered, what the pack would admit, and why
/// each other candidate was dropped. A turn's `recall.shadow` row is one,
/// and so is `memory.search`'s answer.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct RecallManifest {
    /// `rcl_<id>`.
    pub recall_id: String,
    /// `shadow` (a turn's: nothing reaches the model) or `search`
    /// (`memory.search`).
    pub mode: String,
    /// The science and its parameters' digest (`baseline@<digest>`).
    pub science: String,
    /// `ran`, `deadline` (the index did not answer in time; the turn went
    /// on), or `unavailable` (no index answered: `why` says why).
    pub outcome: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub why: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub session_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub turn_id: Option<String>,
    /// Where the turn speaks: `private`, or `shared:<target>`.
    pub place: String,
    /// The query's length in characters, and its SHA-256's first 16 hex
    /// digits: the row keeps no copy of the text.
    pub query_chars: u64,
    pub query_digest: String,
    /// Only nodes written before this position were candidates.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub as_of: Option<u64>,
    /// The position the index held every node through, as it answered.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub indexed_through: Option<u64>,
    /// The hits the index answered with.
    pub candidates: u64,
    /// Each source's hits among them (`bm25`, `entity`, `vector`).
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub sources: BTreeMap<String, u64>,
    /// Sources that did not answer, and why (the model loading).
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub skipped: BTreeMap<String, String>,
    pub admitted: Vec<RecallItem>,
    pub dropped: Vec<RecallDrop>,
    /// The drops by reason.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub drops: BTreeMap<String, u64>,
    /// The pack's limit, and what the admitted items would take.
    pub budget_tokens: u64,
    pub used_tokens: u64,
    pub timings: RecallTimings,
}

/// An item the pack would admit: a reference, never a copy.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct RecallItem {
    pub node_id: String,
    pub chunk: u64,
    pub session_id: String,
    pub position: u64,
    pub kind: String,
    /// Its place in the science's order, from 1.
    pub rank: u64,
    pub fused: f64,
    /// Each source's rank and score.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub sources: BTreeMap<String, IndexSourceRank>,
    /// Its excerpt's tokens.
    pub tokens: u64,
    /// The excerpt itself: `memory.search` and `memory.recalls` give it; the
    /// row does not keep it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub text: Option<String>,
}

/// A candidate dropped, and why: `place`, `in_context`, `untrusted`,
/// `recursion`, `threshold`, or `budget`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct RecallDrop {
    pub node_id: String,
    pub chunk: u64,
    pub session_id: String,
    pub reason: String,
    pub fused: f64,
    /// A `budget` drop's tokens.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub tokens: u64,
}

fn is_zero(n: &u64) -> bool {
    *n == 0
}

/// Each stage's time, in milliseconds.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct RecallTimings {
    /// From the ask to the index's answer (or the deadline).
    pub index_ms: f64,
    /// The places, the filters, the rank, and the pack.
    pub pack_ms: f64,
    pub total_ms: f64,
    /// The deadline the index had.
    pub deadline_ms: u64,
    /// The index's own stages, as it answered.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub index: Option<IndexTimings>,
}
