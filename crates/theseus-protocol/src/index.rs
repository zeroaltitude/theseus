//! The index tender's protocol (M6 §2.2; roadmap row 51): JSON-RPC 2.0 over
//! NDJSON on `<state>/index/sock`, mode 0600, with the core its only client.
//! The tender (`theseus-index`) answers these, and the core forwards
//! `index.status` and `index.query` to it for every other client (`method`'s
//! `INDEX_STATUS` and `INDEX_QUERY`). Each name carries `Index`, since the web
//! apps' types share one namespace; `theseus-index` names them without it.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// The tender's methods, on its own socket.
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
    /// Nodes, or the chunks holding texts, leave the index now, and their
    /// vectors leave every vector file (the core's forget and redaction).
    pub const FORGET: &str = "index.forget";
}

fn default_k() -> usize {
    10
}

fn default_embed_wait_ms() -> u64 {
    30_000
}

fn is_zero(n: &u64) -> bool {
    *n == 0
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct IndexQueryParams {
    pub text: String,
    /// Hits wanted, at most 100.
    #[serde(default = "default_k")]
    pub k: usize,
    /// Only nodes written before this position answer, so a replay never
    /// sees its own future.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub as_of: Option<u64>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub exclude_sessions: Vec<String>,
    #[serde(default, skip_serializing_if = "IndexFilters::is_empty")]
    pub filters: IndexFilters,
    /// The sources to rank and fuse (`bm25`, `entity`, `vector`); when
    /// empty, BM25 and entities, and vectors in `hybrid` mode.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub sources: Vec<String>,
    /// How long the vector source may wait for the model to load. 0 (the
    /// core's): a model not loaded is sent to load, and this query answers
    /// without vectors, saying so in `skipped`.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub wait_ms: u64,
    /// Weights for the fusion, by source (`bm25`, `entity`, `vector`); a
    /// source not named takes the tender's default ([`IndexWeights`]).
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub weights: BTreeMap<String, f64>,
}

impl IndexQueryParams {
    pub fn new(text: &str) -> Self {
        Self {
            text: text.to_string(),
            k: default_k(),
            as_of: None,
            exclude_sessions: Vec::new(),
            filters: IndexFilters::default(),
            sources: Vec::new(),
            wait_ms: 0,
            weights: BTreeMap::new(),
        }
    }
}

/// Each source's weight in the fusion, `Σ w_s / (60 + rank_s)` (theseus-jz8):
/// the tender's defaults, which a query's `weights` override source by
/// source.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct IndexWeights {
    pub bm25: f64,
    pub entity: f64,
    pub vector: f64,
}

impl IndexWeights {
    /// Every source alike: reciprocal rank fusion as M6 §2.2 first gave it.
    pub const EQUAL: IndexWeights = IndexWeights {
        bm25: 1.0,
        entity: 1.0,
        vector: 1.0,
    };

    /// A source's weight; 0 for one this tender does not rank.
    pub fn get(&self, source: &str) -> f64 {
        match source {
            "bm25" => self.bm25,
            "entity" => self.entity,
            "vector" => self.vector,
            _ => 0.0,
        }
    }

    /// These weights with `overrides` in their place, each a known source
    /// and a finite number, 0 or more.
    pub fn with(&self, overrides: &BTreeMap<String, f64>) -> Result<IndexWeights, String> {
        let mut w = *self;
        for (source, &x) in overrides {
            if !x.is_finite() || x < 0.0 {
                return Err(format!(
                    "the weight {x} for {source}: a finite number, 0 or more"
                ));
            }
            match source.as_str() {
                "bm25" => w.bm25 = x,
                "entity" => w.entity = x,
                "vector" => w.vector = x,
                o => return Err(format!("no source {o:?} to weigh: bm25, entity, vector")),
            }
        }
        Ok(w)
    }

    /// `bm25=1,entity=1,vector=3`, over these weights.
    pub fn parse_over(&self, s: &str) -> Result<IndexWeights, String> {
        let mut o = BTreeMap::new();
        for part in s.split(',').map(str::trim).filter(|p| !p.is_empty()) {
            let (k, v) = part
                .split_once('=')
                .ok_or_else(|| format!("{part:?}: source=weight"))?;
            let v: f64 = v.trim().parse().map_err(|e| format!("{part:?}: {e}"))?;
            o.insert(k.trim().to_string(), v);
        }
        self.with(&o)
    }

    /// The weights of `sources`, by name.
    pub fn of(&self, sources: &[&str]) -> BTreeMap<String, f64> {
        sources
            .iter()
            .map(|s| (s.to_string(), self.get(s)))
            .collect()
    }
}

impl Default for IndexWeights {
    /// The tender's defaults: vectors 6, BM25 and entities 1 (theseus-jz8).
    /// Chosen on exam-v2's held-in items by a rule fixed before the grid ran
    /// (the most items with all their gold in the top 6, recall's budget),
    /// over vector weights 1, 1.5, 2, 3, 4, and 6: 23 of 34 items, against 19
    /// at equal weights. The held-out half judged it once, after.
    fn default() -> Self {
        IndexWeights {
            bm25: 1.0,
            entity: 1.0,
            vector: 6.0,
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct IndexFilters {
    /// Only these sessions.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub sessions: Vec<String>,
    /// Only these kinds (`user_message`, `tool_result`, …).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub kinds: Vec<String>,
    /// Only external text (`true`), or none of it (`false`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub external: Option<bool>,
}

impl IndexFilters {
    pub fn is_empty(&self) -> bool {
        self.sessions.is_empty() && self.kinds.is_empty() && self.external.is_none()
    }
}

/// One source's rank (from 1) and raw score for a hit.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct IndexSourceRank {
    pub rank: usize,
    pub score: f64,
}

/// A chunk that answered.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct IndexHit {
    pub node_id: String,
    pub chunk: u64,
    pub session_id: String,
    pub position: u64,
    pub kind: String,
    pub origin: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub author: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub place: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub tool: Option<String>,
    pub time_ms: u64,
    pub external: bool,
    /// The chunk's text.
    pub text: String,
    /// The query's entities this chunk holds.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub entities_matched: Vec<String>,
    /// Each source that ranked it.
    pub sources: BTreeMap<String, IndexSourceRank>,
    /// Reciprocal rank fusion over those sources.
    pub fused: f64,
}

/// Each stage's time, in milliseconds.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct IndexTimings {
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
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct IndexLag {
    pub bytes: u64,
    pub ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct IndexQueryResult {
    pub hits: Vec<IndexHit>,
    /// The position the index holds every node through.
    pub indexed_through: u64,
    pub lag: IndexLag,
    pub timings: IndexTimings,
    /// Sources asked for (or defaulted to) that did not answer, and why: the
    /// model loading, or no weights.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub skipped: BTreeMap<String, String>,
    /// Each fused source's weight: the query's, or the tender's default.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub weights: BTreeMap<String, f64>,
}

/// What every vector carries: the model and its files, the code that made
/// it, and the precision. A vector answers a query embedded under the same
/// space (model, files, dimensions); a changed stamp re-embeds.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct IndexStamp {
    /// `<name>@<revision>`.
    pub model: String,
    /// `model.safetensors`' SHA-256.
    pub weights: String,
    /// `tokenizer.json`'s SHA-256.
    pub tokenizer: String,
    /// The engine and the tender's embedding code: `candle-0.11.0+embed.1`.
    pub engine: String,
    pub precision: String,
    /// The scanned cut and the full vector: `[256, 768]`.
    pub dims: [usize; 2],
}

impl IndexStamp {
    /// Vectors of one space compare with each other, whatever engine made
    /// them: same model, same files, same dimensions.
    pub fn same_space(&self, o: &IndexStamp) -> bool {
        self.model == o.model
            && self.weights == o.weights
            && self.tokenizer == o.tokenizer
            && self.dims == o.dims
    }
}

/// Nomic's task prefixes.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum IndexEmbedTask {
    /// `search_document: `, what the index embeds its chunks with.
    #[default]
    SearchDocument,
    /// `search_query: `, what a query is embedded with.
    SearchQuery,
    Clustering,
    Classification,
}

impl IndexEmbedTask {
    pub fn prefix(self) -> &'static str {
        match self {
            IndexEmbedTask::SearchDocument => "search_document: ",
            IndexEmbedTask::SearchQuery => "search_query: ",
            IndexEmbedTask::Clustering => "clustering: ",
            IndexEmbedTask::Classification => "classification: ",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct IndexNeighboursParams {
    pub node_id: String,
    #[serde(default = "default_k")]
    pub k: usize,
    /// Only nodes written before this position.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub as_of: Option<u64>,
}

/// A node near another, by its best chunk.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct IndexNeighbour {
    pub node_id: String,
    pub chunk: u64,
    /// Cosine of the 768-d vectors.
    pub score: f64,
    pub position: u64,
    pub session_id: String,
    pub kind: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct IndexNeighboursResult {
    pub node_id: String,
    pub neighbours: Vec<IndexNeighbour>,
    pub vector_ms: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct IndexEmbedParams {
    /// At most 64, each cut at 512 tokens.
    pub texts: Vec<String>,
    #[serde(default)]
    pub task: IndexEmbedTask,
    /// 768 (the default) or 256 (the Matryoshka cut, as scanned).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub dims: Option<usize>,
    /// How long to wait for the model to load.
    #[serde(default = "default_embed_wait_ms")]
    pub wait_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct IndexEmbedResult {
    /// Unit vectors, one per text.
    pub vectors: Vec<Vec<f32>>,
    pub dims: usize,
    pub stamp: IndexStamp,
    /// Each text's tokens, its prefix and `[CLS]`/`[SEP]` included.
    pub tokens: Vec<usize>,
    pub embed_ms: f64,
}

/// `index.forget` (theseus-64x): what must leave the index now. The core's
/// removal paths call it: an operator's forget (a `Suppression`) and a
/// redaction (§5.6) name nodes; a text found copied where the lineage walk
/// did not reach can be named as a hit gave it.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct IndexForgetParams {
    /// Nodes that leave the index whole.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub nodes: Vec<String>,
    /// Chunk texts, exactly as a hit's `text` gives them: every chunk that
    /// holds one leaves the index.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub texts: Vec<String>,
}

/// A chunk the index still holds.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct IndexChunkRef {
    pub node_id: String,
    pub chunk: u64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct IndexForgetResult {
    /// Nodes that left the index whole (those it held).
    pub nodes: u64,
    /// Chunks that left it: the nodes' and the texts'.
    pub chunks: u64,
    /// Records dropped from the vector files: the texts that left, and
    /// every other dead record.
    pub vectors_dropped: u64,
    /// Chunks not forgotten that hold the same text as one that left, so its
    /// vector stays: name them too if the text must go.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub still_held: Vec<IndexChunkRef>,
    /// The vector files rewritten, and their bytes before and after.
    pub files: u64,
    pub bytes_before: u64,
    pub bytes_after: u64,
    pub ms: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct IndexWarmResult {
    /// The model's state after the call: `loaded`, `loading` (a load was
    /// started or is running), or why it cannot (`off`, `no_weights`,
    /// `refused`).
    pub model: String,
    pub mode: String,
}

/// A backfill's progress, in WAL bytes.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct IndexBackfill {
    pub done_bytes: u64,
    pub total_bytes: u64,
}

/// The tender's own status, as it answers `index.status` on its socket.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct IndexStatus {
    /// `starting`, `backfilling`, `ready`, or `stalled` (the WAL could not be
    /// read past a frame: `last_error` says why). The core adds `down`
    /// ([`IndexHealth`]).
    pub state: String,
    /// What answers: `hybrid` (BM25, entities, and vectors), or `bm25_only`
    /// (no weights, or weights refused: `vectors` says which).
    pub mode: String,
    /// The vector side.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub vectors: Option<IndexVectorStatus>,
    /// The fusion's default weights, by source.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub weights: Option<IndexWeights>,
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
    pub lag: IndexLag,
    /// Why a ready tender has bytes after its cursor: a frame not yet whole
    /// at the WAL's end (one being written, or a torn tail that the core's
    /// next open cuts).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub waiting: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub backfill: Option<IndexBackfill>,
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
    #[cfg_attr(test, ts(optional))]
    pub last_error: Option<String>,
    #[serde(default)]
    pub last_error_ms: u64,
}

/// The tender's vector side: `IndexStatus`'s `vectors`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct IndexVectorStatus {
    /// `off` (not configured), `no_weights`, `refused` (a file is not the
    /// pinned one), `unloaded`, `loading`, or `loaded`.
    pub model: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub weights_dir: Option<String>,
    /// The stamp new vectors get.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub stamp: Option<IndexStamp>,
    /// Chunks the index holds, chunks with a vector that answers (this
    /// stamp's, or an older one of its space), and distinct texts waiting
    /// for this stamp's vector.
    pub chunks: u64,
    pub vectors: u64,
    pub pending: u64,
    /// Records in the vector files this space answers from, and the dead
    /// among them: texts no chunk holds, which answer nothing and go at the
    /// next compaction (a quarter of a file dead, a rebuild, a forget).
    #[serde(default)]
    pub records: u64,
    #[serde(default)]
    pub dead: u64,
    #[serde(default)]
    pub compactions: IndexCompactions,
    /// While an older stamp's vectors answer, their stamps, and the chunks
    /// already re-embedded.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub reembed: Option<IndexReembed>,
    pub backfill: IndexEmbedStats,
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
    #[cfg_attr(test, ts(optional))]
    pub last_error: Option<String>,
    #[serde(default)]
    pub last_error_ms: u64,
}

/// The vector files' compactions since the tender started (theseus-64x).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct IndexCompactions {
    pub count: u64,
    /// Records dropped, in all.
    pub dropped: u64,
    /// The last one: when, why (`dead`, `rebuild`, `forget`), how long, and
    /// the files' bytes before and after.
    pub last_at_ms: u64,
    pub last_why: String,
    pub last_ms: f64,
    pub last_bytes_before: u64,
    pub last_bytes_after: u64,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct IndexReembed {
    pub from: Vec<IndexStamp>,
    pub done: u64,
    pub total: u64,
}

/// What the embedding thread has done since the tender started.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct IndexEmbedStats {
    /// Distinct texts embedded, in batches.
    pub texts: u64,
    pub batches: u64,
    /// Tokens embedded, padding not counted.
    pub tokens: u64,
    /// Texts past 512 tokens, embedded in windows.
    #[serde(default)]
    pub windowed: u64,
    /// Texts cut at the windows' cap (about 4,000 tokens).
    pub truncated: u64,
    /// The embedding thread's wall time and CPU time in batches.
    pub wall_ms: u64,
    pub cpu_ms: u64,
    /// Texts that would not embed (left without a vector).
    pub failed: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct IndexRebuildResult {
    /// The rebuild was queued: the tender drops the index and backfills.
    pub accepted: bool,
}

/// Health's `index` block, and the core's answer to `index.status` (roadmap
/// row 51): the tender as the core supervises it, and the tender's own status
/// when it answers.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct IndexHealth {
    /// In a word: `off` (`[index] enabled = false`), `starting` (before its
    /// supervisor starts a tender, 2 s after the daemon serves), `down` (no
    /// tender answers: `why` says why, and `tender` when it starts again), or
    /// the tender's own state (`starting`, `backfilling`, `ready`, `stalled`).
    pub state: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub why: Option<String>,
    /// The tender process, as the core supervises it: also in `children`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub tender: Option<crate::TenderStatus>,
    /// The tender's own `index.status`, when it answered.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub status: Option<IndexStatus>,
}
