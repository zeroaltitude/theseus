//! Recall's wire types (M6 §2.14, step 30a): a recall's manifest, which a
//! turn's `recall.shadow` row holds and `memory.search` answers, and
//! `memory.recalls`, a session's manifests; and (30b) the `BudgetReport`
//! and `memory.label`. Each name carries `Recall`, `Memory`, or (the
//! design's name) `Budget`, since the web apps' types share one namespace.

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
    /// The arm whose science and sources the search runs (`baseline`,
    /// `+synthesis`); `baseline` when absent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub arm: Option<String>,
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
    /// The session's arm (M6 30b): `baseline` for recall in front of the
    /// model, or the control's `none` with `baseline` in shadow.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub arm: Option<String>,
    /// The pack's budget: its limit, what it used, and each item it dropped
    /// for the budget (§2.11: never silently thinner).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub budget: Option<BudgetReport>,
    /// Jev's live rerank (M6 32d), when the turn waited on one: whether
    /// its order was used, or why recall's own stood.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub rerank: Option<RecallRerank>,
}

/// What a live rerank did to a recall (M6 32d): the turn waited at most
/// `[memory] rerank_wait_ms` from its start for `rerank.v1`'s answer.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct RecallRerank {
    /// `jdg_…`, the rerank's judgment; none when nothing was sent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub judgment: Option<String>,
    /// Jev's order is what the pack admitted from.
    pub applied: bool,
    /// Why recall's own order stood: `timeout` (the wait ended first; the
    /// answer, when it comes, is recorded `late`), `breaker_open`,
    /// `budget`, `nothing_eligible`, or the call's fallback (`model_drift`,
    /// a failure's class, a skip's reason).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub why: Option<String>,
    /// How long the turn waited, from the rerank's start.
    pub waited_ms: f64,
    /// The wait's bound.
    #[cfg_attr(test, ts(type = "number"))]
    pub wait_ms: u64,
}

/// What a compilation, or a recall's pack, fitted into its limit and what it
/// left out (M6 30b; §2.8, §2.11): every drop with its reason, tokens, and
/// tier, the ring's cut as a range, and an overage when even the kept part
/// did not fit. A compilation's is stored with it (`budget`); a recall's
/// rides in its manifest.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct BudgetReport {
    /// The tokens it had: a request's, the model's window less the output
    /// cap and a margin; a pack's, `[memory] recall_budget_tokens`. 0: no
    /// window is known.
    pub limit_tokens: u64,
    /// The tokens it used, as estimated.
    pub used_tokens: u64,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub dropped: Vec<BudgetDrop>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub overage: Option<BudgetOverage>,
}

/// One thing a budget left out.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct BudgetDrop {
    /// A node left out alone (a recall's item).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub node_id: Option<String>,
    /// A run of nodes left out (the ring's cut).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub range: Option<BudgetRange>,
    /// `budget` (a recall's), `overflow` (the ring's).
    pub reason: String,
    pub tokens: u64,
    /// `recall`, `ring`, or (30c) `compaction`.
    pub tier: String,
}

/// A run of a session's nodes, first to last in order.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct BudgetRange {
    pub first: String,
    pub last: String,
    pub nodes: u64,
}

/// What did not fit even after every drop: a named outcome, never a
/// thinner prompt.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct BudgetOverage {
    /// The tokens over the limit.
    pub tokens: u64,
    pub why: String,
}

/// `memory.label` (M6 30b, §2.14): an operator's label on a node. `wrong`
/// and `stale` keep the node out of recall (`labeled_wrong`), and `useful`
/// lets it back; `should_have` says recall missed it.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct MemoryLabelParams {
    pub node_id: String,
    /// `useful`, `wrong`, `stale`, or `should_have`.
    pub label: String,
    /// The recall that offered it, when one did.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub recall_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub note: Option<String>,
}

/// The labels `memory.label` takes.
pub const MEMORY_LABELS: [&str; 4] = ["useful", "wrong", "stale", "should_have"];

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct MemoryLabelResult {
    pub node_id: String,
    pub label: String,
    /// Whether recall now leaves the node out.
    pub excluded: bool,
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

/// `memory.consolidate` (M6 31b): consolidation now. A dry run lists the
/// clusters it would synthesize, and writes nothing.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct MemoryConsolidateParams {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub dry_run: Option<bool>,
}

/// What a consolidation did, cluster by cluster.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct MemoryConsolidateResult {
    pub dry_run: bool,
    /// The recall rows read (`recall.shadow`, `recall.ran`).
    pub recalls: u64,
    pub clusters: Vec<SynthesisReport>,
    /// Components and clusters left out, by reason (`size`,
    /// `not_a_source`, `synthesized`, `external`, `profiles_disagree`, …).
    pub skipped: std::collections::BTreeMap<String, u64>,
    /// The local day's spend, this run's included, and its limit.
    pub spent_today_usd: f64,
    pub limit_usd: f64,
    /// Why the run stopped before its last cluster, if it did.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub stopped: Option<String>,
}

/// One cluster's synthesis, or what a dry run would ask.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct SynthesisReport {
    /// The digest of its sources' ids.
    pub cluster: String,
    pub sources: Vec<String>,
    /// The distinct turns that admitted its weakest pair together.
    pub turns: u64,
    pub profile: String,
    /// `would_propose` (a dry run), `supported`, `unchecked`, `rejected`, or
    /// `failed`.
    pub outcome: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub synthesis_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub text: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub why: Option<String>,
    #[serde(default)]
    pub cost_usd: f64,
}
