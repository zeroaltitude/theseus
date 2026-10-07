//! The import (theseus-0lrr.6): the operator's past history, written by an
//! outside pipeline as episode files (JSON Lines, format 1), turned into
//! imported sessions: closed and read-only, private under the place rule,
//! each message a node whose origin is the import and whose time is the
//! message's own.
//!
//! - **`import.episodes`**: one batch of an episode file's lines, as the
//!   client read them (`theseus import openclaw <file>...` streams a file a
//!   batch at a time), written in one frame. An episode already imported is
//!   skipped; the same id with another hash is rejected and named, never
//!   overwritten; a line that does not read is named by its number.
//! - **`import.erase`**: every session and node of a tag tombstoned (the
//!   payload replaced by an erasure marker, the record's id, origin and
//!   times kept), and the index told to forget them.
//! - **`import.list`**: each tag with its counts. A read.
//!
//! The import and the erase are the owner's acts, from a private place
//! (refused inside a job's process).

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// The episode format this build reads.
pub const EPISODE_FORMAT: u64 = 1;

/// One line of an episode file, with its number in the file (from 1).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct ImportLine {
    #[cfg_attr(test, ts(type = "number"))]
    pub line: u64,
    pub text: String,
}

/// `import.episodes`: one batch of a file's lines.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct ImportEpisodesParams {
    /// The file the lines came from, as the client names it: for the
    /// report and the ledger's row.
    pub file: String,
    pub lines: Vec<ImportLine>,
}

/// A line the import did not take, and why.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct ImportRejected {
    #[cfg_attr(test, ts(type = "number"))]
    pub line: u64,
    /// The episode's id, when the line read far enough to name one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub episode_id: Option<String>,
    pub why: String,
}

/// `import.episodes`' answer: what the batch did.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct ImportEpisodesResult {
    /// Lines read that were not blank.
    #[cfg_attr(test, ts(type = "number"))]
    pub read: u64,
    /// Episodes written as new imported sessions.
    #[cfg_attr(test, ts(type = "number"))]
    pub imported: u64,
    /// Episodes imported before, with the same hash.
    #[cfg_attr(test, ts(type = "number"))]
    pub skipped: u64,
    /// Lines that did not read, and episodes whose id was imported before
    /// with another hash.
    pub rejected: Vec<ImportRejected>,
    /// The nodes written: messages and summaries.
    #[cfg_attr(test, ts(type = "number"))]
    pub nodes: u64,
    /// The frames written: one per batch.
    #[cfg_attr(test, ts(type = "number"))]
    pub frames: u64,
    pub ms: f64,
}

/// `import.erase`: every session of a tag.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct ImportEraseParams {
    pub tag: String,
    /// Why, in a few words, for the receipt.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub why: Option<String>,
}

/// `import.erase`'s answer, its receipt.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct ImportEraseResult {
    pub tag: String,
    /// Sessions tombstoned now (an erased one is not counted again).
    #[cfg_attr(test, ts(type = "number"))]
    pub sessions: u64,
    /// Their nodes, each written again with an erasure marker.
    #[cfg_attr(test, ts(type = "number"))]
    pub nodes: u64,
    #[cfg_attr(test, ts(type = "number"))]
    pub frames: u64,
    /// What the index did: `forgot 120 nodes`, or why it was not asked and
    /// that its follower drops them as it reads the markers.
    pub index: String,
    pub ms: f64,
    /// The erased sessions' topic memberships taken away (theseus-anh3):
    /// the sessions whose lists were emptied.
    #[serde(default)]
    #[cfg_attr(test, ts(type = "number"))]
    pub memberships: u64,
    /// The topics the tag's `import.topics` made that nothing else uses,
    /// taken away.
    #[serde(default)]
    #[cfg_attr(test, ts(type = "number"))]
    pub topics: u64,
}

/// `import.topics` (theseus-anh3): a tag's imported sessions' topic labels
/// (`labels.topic`, slash paths) as ontology topics, a tree with a topic
/// for each prefix, and each session's topic memberships, origin `import`:
/// at most the kind's per-session count, the rest kept as labels. From the
/// stored labels, never a re-import; a second run changes nothing. The
/// owner's act, from a private place.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct ImportTopicsParams {
    pub tag: String,
}

/// What `import.topics` did.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct ImportTopicsResult {
    pub tag: String,
    /// The tag's sessions read (an erased one is passed over).
    #[cfg_attr(test, ts(type = "number"))]
    pub sessions: u64,
    /// The distinct labels they carry, and the topics those make: one for
    /// each label and each prefix of one.
    #[cfg_attr(test, ts(type = "number"))]
    pub labels: u64,
    #[cfg_attr(test, ts(type = "number"))]
    pub topics: u64,
    /// Topics declared now (the rest were held already).
    #[cfg_attr(test, ts(type = "number"))]
    pub made: u64,
    /// Sessions whose topic list was written now (the rest held it already).
    #[cfg_attr(test, ts(type = "number"))]
    pub joined: u64,
    /// The tag's sessions' memberships from the import, after the run.
    #[cfg_attr(test, ts(type = "number"))]
    pub memberships: u64,
    /// Sessions with more topics than the kind allows one: the first ones in
    /// the pipeline's order were taken, the rest stay labels.
    #[cfg_attr(test, ts(type = "number"))]
    pub capped: u64,
    /// Labels no topic can be made of (an empty part, deeper than the tree
    /// nests), left as labels.
    #[cfg_attr(test, ts(type = "number"))]
    pub unplaced: u64,
    #[cfg_attr(test, ts(type = "number"))]
    pub frames: u64,
    pub ms: f64,
}

/// One tag, as `import.list` shows it.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct ImportTagInfo {
    pub tag: String,
    /// Imported sessions, erased ones included.
    #[cfg_attr(test, ts(type = "number"))]
    pub sessions: u64,
    #[cfg_attr(test, ts(type = "number"))]
    pub nodes: u64,
    #[cfg_attr(test, ts(type = "number"))]
    pub erased: u64,
    /// Sessions by source (`openclaw-store`, `wiki`).
    #[cfg_attr(test, ts(type = "Record<string, number>"))]
    pub sources: BTreeMap<String, u64>,
    /// The span of the episodes' own times (unix ms).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional, type = "number"))]
    pub first_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional, type = "number"))]
    pub last_ms: Option<u64>,
    /// When its last batch was written (unix ms).
    #[cfg_attr(test, ts(type = "number"))]
    pub updated_ms: u64,
}

/// `import.list`'s answer.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct ImportListResult {
    pub tags: Vec<ImportTagInfo>,
}

/// Health's count of what an import wrote (theseus-revl): the sessions an
/// import holds, apart from the owner's own (`HealthResult.sessions`), and
/// the erased ones. Both are nothing when there is no import.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct HealthImported {
    /// Imported sessions still held: the tags' sessions less their erased.
    #[cfg_attr(test, ts(type = "number"))]
    pub sessions: u64,
    /// Imported sessions an `import.erase` tombstoned. Their keys remain.
    #[cfg_attr(test, ts(type = "number"))]
    pub erased: u64,
}
