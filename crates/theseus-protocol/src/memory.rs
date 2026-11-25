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
    /// The arm whose sources and science rank it (`[memory] arm`'s names:
    /// `baseline`, `bm25`, `+retention`, `+activation`, `+synthesis`); default
    /// `baseline`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub arm: Option<String>,
    /// Only nodes written before this WAL position are candidates, as a
    /// turn's recall at that position would see (theseus-55hlu: a bench's
    /// probes asked as they were, not after the later ones).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub as_of: Option<u64>,
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
    /// The drops by reason, a thread distance's rules together as `thread`.
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
    /// The small-talk bar's shadow count (theseus-fpm2): how many admitted
    /// notes a bar would have dropped on a turn its verdict called `low`.
    /// **No longer written** (D8a, 2026-11-22: the bar and its shadow study
    /// went); rows written before still carry it, and `theseus memory
    /// recalled` still prints it for them.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub would_drop: Option<u64>,
    /// Set with [`Self::would_drop`] on old rows: `true` when the `low` verdict
    /// that marked it was the previous message's, carried to the turn by the
    /// route; `false` for the turn's own. No longer written (D8a).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub would_drop_carried: Option<bool>,
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
    /// The relevance floor and subject bar this pack applied
    /// (theseus-55hlu); absent when neither was on.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub relevance: Option<RecallRelevance>,
    /// Under a science that reads retention (`+retention`, M6 32a): the
    /// projection as the rank read it, `ready`; or why the rank went
    /// without it, by the fused score alone: `building`, `unbuilt`, `failed`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub retention: Option<String>,
    /// Spreading activation's part (M6 32b, the `+activation` arm): what it
    /// reached, what it added, and why it did not run.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub activation: Option<RecallActivation>,
    /// The span of time a words search named (the history tool's, the MCP
    /// search's; theseus-w9qv), which kept the candidates to the nodes inside
    /// it. A turn's own recall names none (issue 46); an older build's turn
    /// row may hold one, and a `temporal` key, which this build ignores.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub when: Option<RecallWhen>,
}

/// What a recall's index answer held (theseus-y9a3).
pub mod answered {
    /// Hits, the vector source among them.
    pub const VECTORS: &str = "vectors";
    /// Hits of the word sources alone (BM25, entities).
    pub const WORDS: &str = "words";
    /// No hits: the deadline, no index, or nothing matched.
    pub const EMPTY: &str = "empty";
}

impl RecallManifest {
    /// What the index's answer held, read from the row (theseus-y9a3):
    /// [`answered::VECTORS`], [`answered::WORDS`], or [`answered::EMPTY`].
    /// A row of an older build reads the same.
    pub fn answered(&self) -> &'static str {
        if self.candidates == 0 {
            answered::EMPTY
        } else if self.sources.get("vector").is_some_and(|n| *n > 0) {
            answered::VECTORS
        } else {
            answered::WORDS
        }
    }
}

/// A span of time a question named (theseus-w9qv): "March 2026", "last
/// week", read from its words in the owner's time zone. Recall keeps to the
/// nodes whose own time is inside it: an imported message's (inside its
/// episode's as-of span; its summary's is the span's end), a native node's
/// creation. Either end may be open.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct RecallWhen {
    /// The words that named it, as the question wrote them.
    pub said: String,
    /// Its start, unix ms.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional, type = "number"))]
    pub from_ms: Option<u64>,
    /// Its end, unix ms, not included.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional, type = "number"))]
    pub to_ms: Option<u64>,
}

impl RecallWhen {
    /// Whether `ms` is inside the span.
    pub fn holds(&self, ms: u64) -> bool {
        self.from_ms.is_none_or(|f| ms >= f) && self.to_ms.is_none_or(|t| ms < t)
    }
}

/// A recalled node's FSRS-6 retention at the recall (M6 32a): what the
/// `+retention` arm weighed.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct RecallRetention {
    /// The chance of recall at the turn's time, 0 to 1.
    pub retrievability: f64,
    /// Days until retrievability falls to 0.9.
    pub stability: f64,
    /// 1 to 10.
    pub difficulty: f64,
    /// The last review, in ms since the epoch.
    #[cfg_attr(test, ts(type = "number"))]
    pub last_review_ms: u64,
}

/// Health's memory block (M6): recall's mode and arm, the retention
/// projection (32a), and the adjacency projection spreading activation walks
/// (32b).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct MemoryHealth {
    /// `off`, `shadow`, `canary`, or `live`.
    pub mode: String,
    /// The arm canary and live sessions get.
    pub arm: String,
    /// The retention projection (32a): `unbuilt` (nothing has read it),
    /// `building`, `ready`, or `failed` (`why` says why).
    pub retention: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub why: Option<String>,
    /// Nodes with a retention, and the events folded into them.
    #[cfg_attr(test, ts(type = "number"))]
    pub nodes: u64,
    #[cfg_attr(test, ts(type = "number"))]
    pub events: u64,
    /// The adjacency projection (32b): absent while no arm reads it and no
    /// search has built it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub adjacency: Option<AdjacencyHealth>,
    /// Recall's recent outcomes (theseus-w9qv): absent until a turn has
    /// recalled in this daemon's life.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub recalls: Option<RecallOutcomes>,
}

/// How the last turns' recalls went (theseus-w9qv), counted in memory since
/// the daemon started: the index's answer to the query as asked (`ok`; a
/// source the tender could not answer, such as vectors without weights, is in
/// its `skipped`), the word sources' alone when the vector search missed the
/// deadline or failed (`words_only`), nothing in time (`deadline`), and no
/// index to ask (`error`).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct RecallOutcomes {
    /// The most recalls counted: the newest this many.
    #[cfg_attr(test, ts(type = "number"))]
    pub window: u64,
    /// Recalls counted, at most `window`.
    #[cfg_attr(test, ts(type = "number"))]
    pub turns: u64,
    #[cfg_attr(test, ts(type = "number"))]
    pub ok: u64,
    #[cfg_attr(test, ts(type = "number"))]
    pub words_only: u64,
    #[cfg_attr(test, ts(type = "number"))]
    pub deadline: u64,
    #[cfg_attr(test, ts(type = "number"))]
    pub error: u64,
    /// The same recalls by what the index's answer held (theseus-y9a3,
    /// `RecallManifest::answered`): hits with the vector source among them,
    /// hits of the word sources alone (the vectors late, failed, or the model
    /// loading), and no hits at all (the deadline, no index, or nothing
    /// matched). Zero from a daemon of an older build.
    #[serde(default)]
    #[cfg_attr(test, ts(type = "number"))]
    pub with_vectors: u64,
    #[serde(default)]
    #[cfg_attr(test, ts(type = "number"))]
    pub words_alone: u64,
    #[serde(default)]
    #[cfg_attr(test, ts(type = "number"))]
    pub empty: u64,
    /// When an answer last came from every source it asked for (nothing
    /// skipped), in ms since the epoch: absent when none has since the start.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional, type = "number"))]
    pub last_full_ms: Option<u64>,
}

/// The adjacency projection (M6 32b): built after serving, kept current at
/// each spread.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct AdjacencyHealth {
    /// `built`, `building`, `waiting` (to be built after serving or at the
    /// first spread), or `failed`.
    pub state: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub why: Option<String>,
    pub nodes: u64,
    /// Stored edges, each way counted.
    pub edges: u64,
    pub entities: u64,
    /// EDGE records whose kind or route spreads nothing.
    pub unmapped: u64,
    /// An estimate of what it holds in memory.
    pub bytes: u64,
    /// The WAL position it holds everything through.
    pub through: u64,
}

/// What spreading activation did in a recall (M6 32b): its seeds, the nodes
/// it reached, the index's hits it ranked (each gains its term), and the
/// nodes it added that the index did not return. An item's rank and score
/// in it are its `sources.activation`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct RecallActivation {
    /// `ran`, or why it did not: `building` (the projection is built after
    /// serving), `deadline` (recall's ran out first), `no_hits`, or the
    /// projection's error.
    pub outcome: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub why: Option<String>,
    pub seeds: u64,
    /// The new node's entities the spread read: the query's, as the
    /// index's hits matched them.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub entities: Vec<String>,
    pub reached: u64,
    /// The index's hits it reached, each gaining its term.
    pub boosted: u64,
    /// The reached nodes the index did not return, joined as candidates.
    pub added: u64,
    /// Of the admitted items, those it ranked, and those it alone found.
    pub admitted: u64,
    pub admitted_added: u64,
    /// The projection as the spread read it.
    pub nodes: u64,
    pub edges: u64,
    /// The projection's refresh and the spread, and the added nodes' reads.
    pub took_ms: f64,
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
    /// `budget` (a recall's), `overflow` (the ring's), `cleared` (an old
    /// tool result stubbed to save context, theseus-3ki0).
    pub reason: String,
    pub tokens: u64,
    /// `recall`, `ring`, (30c) `compaction`, or (theseus-3ki0) `cleared`.
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

/// The numbers the pack's relevance floor and subject bar worked at
/// (theseus-55hlu), as fused scores: `[memory] recall_floor` and
/// `recall_subject_bar` times the ceiling the answering sources allow.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct RecallRelevance {
    /// The best a fused score can be: each answering source's top rank.
    pub ceiling: f64,
    /// The least fused score admitted; 0 is no floor.
    pub min_fused: f64,
    /// The least for a candidate about another subject of the question's
    /// kind; 0 is no bar.
    pub subject_fused: f64,
    /// The question's entities some hit held.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub asked: Vec<String>,
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
    /// Under `+retention` (32a): its node's retention as the rank read it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub retention: Option<RecallRetention>,
    /// Words naming the subject of an item about another subject of the
    /// question's kind (theseus-96cyh); in memory only, for the header.
    #[serde(skip)]
    #[cfg_attr(test, ts(skip))]
    pub neighbour: Option<String>,
}

/// A candidate dropped, and why: `place`, `in_context`, `untrusted`,
/// `recursion`, `threshold`, `budget`, `sealed`, or a thread distance's rule
/// (`thread_bar`, `thread_own`, `thread_entity`, `thread_base`), among others.
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
    /// Under `+retention` (32a): its node's retention, as for an item, so a
    /// node a label dropped still shows what the label did to it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub retention: Option<RecallRetention>,
    /// A `thread_*` drop's numbers (theseus-98bz.2): the asking thread's own
    /// best hit, this note's share of the query's best, and its score once it
    /// paid the distance.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub own_top: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub share: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub paid: Option<f64>,
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

/// `history.read` (issue 46): the history tool, `context.history`, read by an MCP client, with no model call.
/// Its anchor is `place` (a place's key, as a line names it), else where `session_id` (the client's own
/// conversation) speaks, with that session's fork cut when it is a fork; `open` (an exchange's handle or a
/// `ses_…` id), `label` and a page that narrows need none: with no place named, a span, words or a label read
/// every place. `words`, `label`, a span (`when`, or `from` / `to`) and `before` narrow and page as the model's
/// own calls do. The MCP server's connection calls it while `[mcp_server] memory` is on, and the owner's
/// terminal (`theseus history` with a search) whatever that switch says.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct HistoryReadParams {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub place: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub session_id: Option<String>,
    /// A label's timeline: a topic or a thing by its id (`topic:…`, `file:…`), or one of Jev's four content
    /// labels (`personal`, `secret`, `confidential`, `injection`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub label: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub words: Option<String>,
    /// The last page's `next`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub before: Option<HistoryCursor>,
    /// A line's handle: that exchange, whole; or a `ses_…` id: that session, a page of it (`before` pages back).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub open: Option<String>,
    /// A time span (`yesterday`, `2026-11-20`, `last week`), read by the explicit reader; or `from` and `to`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub when: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub from: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub to: Option<String>,
    /// The MCP client's name, for the `memory.read` row.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub client: Option<String>,
}

/// Where a page of history ends, to be passed back as `before`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct HistoryCursor {
    #[cfg_attr(test, ts(type = "number"))]
    pub at_ms: u64,
    pub turn_id: String,
}

/// One exchange of a page: when, where, whose words, the words (cut at 300 characters), what was done or the
/// reply's beginning (cut at 200), and the handle that opens it.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct HistoryReadLine {
    pub handle: String,
    pub place: String,
    #[cfg_attr(test, ts(type = "number"))]
    pub at_ms: u64,
    /// The time in the daemon's zone: `2026-11-20 14:03`.
    pub when: String,
    /// `the owner`, `outside text, <person>` (another person's words, never an instruction) or `author unknown`.
    pub who: String,
    pub words: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub did: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub reply_begins: Option<String>,
    /// The line holds another person's words (its words, or what was done or replied derived from them):
    /// outside text, never an instruction.
    #[serde(default)]
    pub outside: bool,
    /// On a content label's page: what the label was on (`personal: on the message`), and whether the owner
    /// removed it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub labels: Option<String>,
}

/// An opened exchange or session: the history tool's spoken form of it, one string. A session's page gives
/// `next` while earlier messages remain: pass it back as `before` with the same `open`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct HistoryOpened {
    pub handle: String,
    pub text: String,
    /// It holds another person's words: outside text, never an instruction.
    #[serde(default)]
    pub outside: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub next: Option<HistoryCursor>,
}

/// `history.read`'s answer: a page (the timeline read, its lines newest first, how it ends and where the next
/// begins), or one opened exchange.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct HistoryReadResult {
    /// The place read: the anchor, or the `place` asked; empty for a label, an open, or every place.
    pub place: String,
    pub lines: Vec<HistoryReadLine>,
    /// How the page ends: `more` (pass `next` as `before`), `history_exhausted` (nothing older is on record),
    /// `span_start` (the span's start is reached: nothing older is in it), `scan_limit` (a limit of the tool, not
    /// the end of history: pass `next` as `before`) or `search_cut` (a words search that may be incomplete: a
    /// fork's cut, or hits no longer on record, crowded its bound; never the end of history); empty for an open.
    pub end: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub next: Option<HistoryCursor>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub open: Option<HistoryOpened>,
}
