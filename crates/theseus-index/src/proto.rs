//! The tender's protocol (M6 §2.2): JSON-RPC 2.0 over NDJSON on
//! `<index>/sock`, mode 0600, with the core its only client. The envelope is
//! `theseus-protocol`'s; these shapes move there in the wire-in (roadmap row
//! 51), as the design's `IndexStatus` and the tender's request and response
//! types.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

pub mod method {
    /// BM25, entity, and vector hits, fused, as of a position.
    pub const QUERY: &str = "index.query";
    /// The tender's health: health's `index` block.
    pub const STATUS: &str = "index.status";
    /// Drop the index and build it again from the WAL.
    pub const REBUILD: &str = "index.rebuild";
    /// The nodes nearest a node, by the 768-d vector (the memory pass's gate).
    pub const NEIGHBOURS: &str = "index.neighbours";
    /// Vectors for texts (consolidation's clustering, the exam).
    pub const EMBED: &str = "index.embed";
    /// Start loading the model, and answer at once (the core, as a turn
    /// begins).
    pub const WARM: &str = "index.warm";
}

fn default_k() -> usize {
    10
}

fn default_embed_wait_ms() -> u64 {
    30_000
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueryParams {
    pub text: String,
    /// Hits wanted, at most 100.
    #[serde(default = "default_k")]
    pub k: usize,
    /// Only nodes written before this position answer, so a replay never
    /// sees its own future.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub as_of: Option<u64>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub exclude_sessions: Vec<String>,
    #[serde(default, skip_serializing_if = "Filters::is_empty")]
    pub filters: Filters,
    /// The sources to rank and fuse (`bm25`, `entity`, `vector`); when
    /// empty, BM25 and entities, and vectors in `hybrid` mode.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub sources: Vec<String>,
    /// How long the vector source may wait for the model to load. 0 (the
    /// core's): a model not loaded is sent to load, and this query answers
    /// without vectors, saying so in `skipped`.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub wait_ms: u64,
}

fn is_zero(n: &u64) -> bool {
    *n == 0
}

impl QueryParams {
    pub fn new(text: &str) -> Self {
        Self {
            text: text.to_string(),
            k: default_k(),
            as_of: None,
            exclude_sessions: Vec::new(),
            filters: Filters::default(),
            sources: Vec::new(),
            wait_ms: 0,
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Filters {
    /// Only these sessions.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub sessions: Vec<String>,
    /// Only these kinds (`user_message`, `tool_result`, …).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub kinds: Vec<String>,
    /// Only external text (`true`), or none of it (`false`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub external: Option<bool>,
}

impl Filters {
    pub fn is_empty(&self) -> bool {
        self.sessions.is_empty() && self.kinds.is_empty() && self.external.is_none()
    }
}

/// One source's rank (from 1) and raw score for a hit.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SourceRank {
    pub rank: usize,
    pub score: f64,
}

/// A chunk that answered.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Hit {
    pub node_id: String,
    pub chunk: u64,
    pub session_id: String,
    pub position: u64,
    pub kind: String,
    pub origin: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub author: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub place: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool: Option<String>,
    pub time_ms: u64,
    pub external: bool,
    /// The chunk's text.
    pub text: String,
    /// The query's entities this chunk holds.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub entities_matched: Vec<String>,
    /// Each source that ranked it.
    pub sources: BTreeMap<String, SourceRank>,
    /// Reciprocal rank fusion over those sources.
    pub fused: f64,
}

/// Each stage's time, in milliseconds.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Timings {
    pub bm25_ms: f64,
    pub entity_ms: f64,
    /// Embedding the query (the vector source's first stage).
    #[serde(default)]
    pub embed_ms: f64,
    /// The int8 scan and the 768-d re-score.
    #[serde(default)]
    pub vector_ms: f64,
    pub fuse_ms: f64,
    pub load_ms: f64,
    pub total_ms: f64,
}

/// How far the index is behind the WAL: the bytes after its cursor, and how
/// long it has been behind (0 when caught up).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Lag {
    pub bytes: u64,
    pub ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueryResult {
    pub hits: Vec<Hit>,
    /// The position the index holds every node through.
    pub indexed_through: u64,
    pub lag: Lag,
    pub timings: Timings,
    /// Sources asked for (or defaulted to) that did not answer, and why: the
    /// model loading, or no weights.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub skipped: BTreeMap<String, String>,
}

/// What every vector carries: the model and its files, the code that made
/// it, and the precision. A vector answers a query embedded under the same
/// space (model, files, dimensions); a changed stamp re-embeds.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Stamp {
    /// `<name>@<revision>`.
    pub model: String,
    /// `model.safetensors`' SHA-256.
    pub weights: String,
    /// `tokenizer.json`'s SHA-256.
    pub tokenizer: String,
    /// The engine and this crate's embedding code: `candle-0.11.0+embed.1`.
    pub engine: String,
    pub precision: String,
    /// The scanned cut and the full vector: `[256, 768]`.
    pub dims: [usize; 2],
}

impl Stamp {
    /// Vectors of one space compare with each other, whatever engine made
    /// them: same model, same files, same dimensions.
    pub fn same_space(&self, o: &Stamp) -> bool {
        self.model == o.model
            && self.weights == o.weights
            && self.tokenizer == o.tokenizer
            && self.dims == o.dims
    }
}

/// Nomic's task prefixes.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Task {
    /// `search_document: `, what the index embeds its chunks with.
    #[default]
    SearchDocument,
    /// `search_query: `, what a query is embedded with.
    SearchQuery,
    Clustering,
    Classification,
}

impl Task {
    pub fn prefix(self) -> &'static str {
        match self {
            Task::SearchDocument => "search_document: ",
            Task::SearchQuery => "search_query: ",
            Task::Clustering => "clustering: ",
            Task::Classification => "classification: ",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NeighboursParams {
    pub node_id: String,
    #[serde(default = "default_k")]
    pub k: usize,
    /// Only nodes written before this position.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub as_of: Option<u64>,
}

/// A node near another, by its best chunk.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Neighbour {
    pub node_id: String,
    pub chunk: u64,
    /// Cosine of the 768-d vectors.
    pub score: f64,
    pub position: u64,
    pub session_id: String,
    pub kind: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NeighboursResult {
    pub node_id: String,
    pub neighbours: Vec<Neighbour>,
    pub vector_ms: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmbedParams {
    /// At most 64, each cut at 512 tokens.
    pub texts: Vec<String>,
    #[serde(default)]
    pub task: Task,
    /// 768 (the default) or 256 (the Matryoshka cut, as scanned).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dims: Option<usize>,
    /// How long to wait for the model to load.
    #[serde(default = "default_embed_wait_ms")]
    pub wait_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmbedResult {
    /// Unit vectors, one per text.
    pub vectors: Vec<Vec<f32>>,
    pub dims: usize,
    pub stamp: Stamp,
    /// Each text's tokens, its prefix and `[CLS]`/`[SEP]` included.
    pub tokens: Vec<usize>,
    pub embed_ms: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WarmResult {
    /// The model's state after the call: `loaded`, `loading` (a load was
    /// started or is running), or why it cannot (`off`, `no_weights`,
    /// `refused`).
    pub model: String,
    pub mode: String,
}

/// A backfill's progress, in WAL bytes.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Backfill {
    pub done_bytes: u64,
    pub total_bytes: u64,
}

/// Health's `index` block.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct IndexStatus {
    /// `starting`, `backfilling`, `ready`, or `stalled` (the WAL could not be
    /// read past a frame: `last_error` says why). The core adds `down`.
    pub state: String,
    /// What answers: `hybrid` (BM25, entities, and vectors), or `bm25_only`
    /// (no weights, or weights refused: `vectors` says which).
    pub mode: String,
    /// The vector side.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vectors: Option<VectorStatus>,
    pub pid: u32,
    pub index_dir: String,
    pub wal_dir: String,
    /// The index holds every node through this position.
    pub position: u64,
    /// The cursor: the segment and offset after the last frame read.
    pub segment: u32,
    pub offset: u64,
    pub documents: u64,
    pub nodes: u64,
    pub lag: Lag,
    /// Why a ready tender has bytes after its cursor: a frame not yet whole
    /// at the WAL's end (one being written, or a torn tail that the core's
    /// next open cuts).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub waiting: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub backfill: Option<Backfill>,
    pub commits: u64,
    pub records_read: u64,
    pub nodes_indexed: u64,
    pub nodes_skipped: u64,
    pub undecodable: u64,
    pub rebuilds: u64,
    pub extractor: u32,
    pub schema: u32,
    pub rss_bytes: u64,
    pub started_at_ms: u64,
    pub last_commit_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_error: Option<String>,
    #[serde(default)]
    pub last_error_ms: u64,
}

/// Health's `index.vectors` block.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct VectorStatus {
    /// `off` (not configured), `no_weights`, `refused` (a file is not the
    /// pinned one), `unloaded`, `loading`, or `loaded`.
    pub model: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub weights_dir: Option<String>,
    /// The stamp new vectors get.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stamp: Option<Stamp>,
    /// Chunks the index holds, chunks with a vector that answers (this
    /// stamp's, or an older one of its space), and distinct texts waiting
    /// for this stamp's vector.
    pub chunks: u64,
    pub vectors: u64,
    pub pending: u64,
    /// While an older stamp's vectors answer, their stamps, and the chunks
    /// already re-embedded.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reembed: Option<Reembed>,
    pub backfill: EmbedStats,
    pub loads: u64,
    pub unloads: u64,
    /// The last load, from the call to a model ready to run.
    pub load_ms: f64,
    pub loaded_at_ms: u64,
    pub last_used_ms: u64,
    pub idle_unload_secs: u64,
    /// What candle's pools run with (`RAYON_NUM_THREADS`,
    /// `CANDLE_NUM_THREADS`): the tender's own, or the core's.
    pub threads: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_error: Option<String>,
    #[serde(default)]
    pub last_error_ms: u64,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Reembed {
    pub from: Vec<Stamp>,
    pub done: u64,
    pub total: u64,
}

/// What the embedding thread has done since the tender started.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct EmbedStats {
    /// Distinct texts embedded, in batches.
    pub texts: u64,
    pub batches: u64,
    /// Tokens embedded, padding not counted.
    pub tokens: u64,
    /// Texts cut at 512 tokens.
    pub truncated: u64,
    /// The embedding thread's wall time and CPU time in batches.
    pub wall_ms: u64,
    pub cpu_ms: u64,
    /// Texts that would not embed (left without a vector).
    pub failed: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RebuildResult {
    /// The rebuild was queued: the tender drops the index and backfills.
    pub accepted: bool,
}
