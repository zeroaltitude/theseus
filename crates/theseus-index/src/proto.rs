//! The tender's protocol (M6 §2.2): JSON-RPC 2.0 over NDJSON on
//! `<index>/sock`, mode 0600, with the core its only client. The envelope is
//! `theseus-protocol`'s; these shapes move there in the wire-in (roadmap row
//! 51), as the design's `IndexStatus` and the tender's request and response
//! types.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

pub mod method {
    /// BM25 and entity hits, fused, as of a position.
    pub const QUERY: &str = "index.query";
    /// The tender's health: health's `index` block.
    pub const STATUS: &str = "index.status";
    /// Drop the index and build it again from the WAL.
    pub const REBUILD: &str = "index.rebuild";
}

fn default_k() -> usize {
    10
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
    /// The sources to rank and fuse (`bm25`, `entity`); every one when empty.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub sources: Vec<String>,
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
    /// What answers: `bm25_only` until 29c's vectors.
    pub mode: String,
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RebuildResult {
    /// The rebuild was queued: the tender drops the index and backfills.
    pub accepted: bool,
}
