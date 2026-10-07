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

/// `import.sessions` (theseus-7n3e): the imported episodes, filtered,
/// counted by facet, a page at a time, for the cockpit's context explorer.
/// `session.list` leaves imported sessions out (theseus-7087); this is
/// their list. A read: it writes nothing.
///
/// Every filter is optional and they combine: `topic` takes a topic and
/// everything under it (a slash path, cut at a slash); `q` is words over
/// the title, the place's name and the topics, case folded; `from_ms` and
/// `to_ms` keep the episodes whose span meets them.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct ImportSessionsParams {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub tag: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub source: Option<String>,
    /// A place kind: `dm`, `slack-channel`, `cli`, ...
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub place: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub sensitivity: Option<String>,
    /// A book hint (`diary`, `casebook`, ...).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub book: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub topic: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub q: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional, type = "number"))]
    pub from_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional, type = "number"))]
    pub to_ms: Option<u64>,
    /// Count the erased ones too (their text is gone).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub erased: bool,
    /// `newest` (by the span's end; the default), `oldest`, or `longest`
    /// (most messages).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub sort: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional, type = "number"))]
    pub offset: Option<u64>,
    /// The page's size: 50 by default, at most 500.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional, type = "number"))]
    pub limit: Option<u64>,
    /// Each row of the page with its summary's text.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub summaries: bool,
    /// Only these sessions (a recalled note's, a deep link's): their rows and
    /// labels in one read. Empty: every session.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub ids: Vec<String>,
}

/// One imported episode, as `import.sessions` lists it: its provenance and
/// labels from its session's record. The labels are the pipeline's recorded
/// facts, never enforcement.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct ImportedEpisode {
    pub session_id: String,
    pub episode_id: String,
    pub tag: String,
    pub source: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub agent: Option<String>,
    /// Where it happened, as the pipeline names it: its kind, and its name
    /// when the source records one.
    pub place_kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub place_name: Option<String>,
    /// Its span (unix ms): the as-of time.
    #[cfg_attr(test, ts(type = "number"))]
    pub start_ms: u64,
    #[cfg_attr(test, ts(type = "number"))]
    pub end_ms: u64,
    /// `personal`, `company-confidential`, `partner-confidential`, `public`.
    pub sensitivity: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub partner: Option<String>,
    #[serde(default)]
    pub topics: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub book: Option<String>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub credential_redacted: bool,
    /// The pipeline's triage: its category and keep score.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub triage: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub keep: Option<f64>,
    pub messages: u32,
    /// A summary node was written.
    pub summary: bool,
    /// The first words of its summary (or its first message). Absent when
    /// withheld (`ImportSessionsResult.withheld`) or erased.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub title: Option<String>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub erased: bool,
    /// The episode file it was read from, as the client named it, and its line.
    pub file: String,
    #[cfg_attr(test, ts(type = "number"))]
    pub line: u64,
    #[cfg_attr(test, ts(type = "number"))]
    pub imported_at_ms: u64,
    /// With `summaries`: the summary's text, and how many messages it cites.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub summary_text: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub cites: Option<u32>,
}

/// A facet's value and the episodes that have it.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct ImportFacet {
    pub value: String,
    #[cfg_attr(test, ts(type = "number"))]
    pub count: u64,
}

/// Each facet's values, counted over the episodes every other filter keeps
/// (a facet's own filter aside, so its other values stay in view), most
/// first.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct ImportFacets {
    pub tags: Vec<ImportFacet>,
    pub sources: Vec<ImportFacet>,
    pub places: Vec<ImportFacet>,
    pub sensitivities: Vec<ImportFacet>,
    pub books: Vec<ImportFacet>,
    /// Every topic path and each of its ancestors, an episode counted once
    /// under each, by path.
    pub topics: Vec<ImportFacet>,
    /// The span's start by month (`2026-03`), oldest first.
    pub months: Vec<ImportFacet>,
}

/// `import.sessions`' answer.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct ImportSessionsResult {
    /// The episodes every filter keeps.
    #[cfg_attr(test, ts(type = "number"))]
    pub total: u64,
    /// Every imported episode held (erased ones too).
    #[cfg_attr(test, ts(type = "number"))]
    pub all: u64,
    #[cfg_attr(test, ts(type = "number"))]
    pub offset: u64,
    pub episodes: Vec<ImportedEpisode>,
    pub facets: ImportFacets,
    /// Why the text (titles, summaries) was left out: an imported session is
    /// the owner's own history, so its text goes only to a private place
    /// (the CLI, the web UI). Absent when it was given.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub withheld: Option<String>,
    /// The projection answered from: the import's counts it was built at,
    /// which an import batch or an erase changes.
    pub version: String,
    /// What building it took (ms), when this answer built it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub built_ms: Option<f64>,
    pub ms: f64,
}
