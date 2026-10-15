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
use serde_json::Value;

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
    /// This batch's number in the client's run, from 1, and how many batches
    /// the client expects to send (an estimate when it cannot know: the
    /// daemon never shows a number past the total). The work board shows
    /// `batch 41 of 120` from them (theseus-jn17m); a client that sends
    /// neither is shown as running, without a count.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub batch: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub batches: Option<u32>,
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
    /// The sessions this batch created or appended to (theseus-pjmj7): what a
    /// sync passes on to its people step.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub touched: Vec<crate::import_sync::ImportTouched>,
    /// Episodes imported before whose messages grew: the stored ones are an
    /// exact prefix, and only the new ones were written (theseus-pjmj7).
    #[serde(default, skip_serializing_if = "is_zero")]
    #[cfg_attr(test, ts(type = "number", optional))]
    pub appended: u64,
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

fn is_zero(n: &u64) -> bool {
    *n == 0
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
    /// The people the tag's `import.people` made that nothing else uses,
    /// taken away (theseus-wy7y).
    #[serde(default)]
    #[cfg_attr(test, ts(type = "number"))]
    pub people: u64,
}

/// `import.people` (theseus-wy7y): a tag's imported sessions' people as the
/// ontology's: each message author who is a person (`person:<name>`), and
/// each DM's other party with its id, one person each, found by an exact
/// handle first; and each session a person spoke in or was the DM of, a
/// membership of origin `import`. From the stored records, deterministic,
/// no model call; a second run changes nothing. `dry_run` counts and writes
/// nothing. The owner's act, from a private place.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct ImportPeopleParams {
    pub tag: String,
    #[serde(default)]
    pub dry_run: bool,
    /// Propose people from the sessions' text (theseus-wy7y): a model
    /// extracts the candidates, Jev judges them, and each kept one is a
    /// proposal; nothing joins the ontology until the owner accepts. With
    /// `dry_run`, the sessions, tokens and projected cost, with no call.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    #[cfg_attr(test, ts(as = "Option<bool>", optional))]
    pub propose: bool,
    /// `propose`'s spend cap for this run, in dollars (5 when absent).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub cap_usd: Option<f64>,
}

/// What `import.people { propose }` did, or would do (theseus-wy7y).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct PeopleProposeReport {
    /// The tag's live imported sessions, and of those: done by an earlier
    /// run (before the tag's mark), skipped for no human-facing text, and
    /// read by this run (or, dry, that a run would read).
    #[cfg_attr(test, ts(type = "number"))]
    pub sessions: u64,
    #[cfg_attr(test, ts(type = "number"))]
    pub done_before: u64,
    #[cfg_attr(test, ts(type = "number"))]
    pub no_text: u64,
    #[cfg_attr(test, ts(type = "number"))]
    pub read: u64,
    /// The extractor's input, estimated, over the sessions read.
    #[cfg_attr(test, ts(type = "number"))]
    pub tokens: u64,
    /// The extractor's profile and model.
    pub profile: String,
    pub model: String,
    /// Dry: the projected cost of the sessions left (the extractor's price
    /// and Jev's); else what this run spent, both calls.
    pub projected_usd: f64,
    pub spent_usd: f64,
    pub cap_usd: f64,
    /// Candidates the extractor returned, those excluded before any call
    /// (the owner, personas, agents, `[people] not_people`, a rejected or
    /// held name), and Jev's judgments asked.
    #[cfg_attr(test, ts(type = "number"))]
    pub candidates: u64,
    #[cfg_attr(test, ts(type = "number"))]
    pub excluded: u64,
    #[cfg_attr(test, ts(type = "number"))]
    pub judged: u64,
    /// Extractions that failed (each booked at its reservation).
    #[cfg_attr(test, ts(type = "number"))]
    pub failed: u64,
    /// Sessions left after a stop (the cap, or the daemon's), and how to go
    /// on from the tag's mark.
    #[cfg_attr(test, ts(type = "number"))]
    pub left: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub stopped: Option<String>,
}

/// What `import.people` did, or would do.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct ImportPeopleResult {
    pub tag: String,
    pub dry_run: bool,
    /// Live imported sessions read.
    #[cfg_attr(test, ts(type = "number"))]
    pub sessions: u64,
    /// Distinct people found, and of those, the ones held already (by an
    /// exact handle, or made by this tag's run before) and the ones made.
    #[cfg_attr(test, ts(type = "number"))]
    pub people: u64,
    #[cfg_attr(test, ts(type = "number"))]
    pub held: u64,
    #[cfg_attr(test, ts(type = "number"))]
    pub made: u64,
    /// Sessions whose person list was written, and the memberships of
    /// origin `import` they hold.
    #[cfg_attr(test, ts(type = "number"))]
    pub joined: u64,
    #[cfg_attr(test, ts(type = "number"))]
    pub memberships: u64,
    /// Sessions with more people than a list keeps: the most active ones
    /// were taken.
    #[cfg_attr(test, ts(type = "number"))]
    pub capped: u64,
    #[cfg_attr(test, ts(type = "number"))]
    pub frames: u64,
    /// Distinct authors and DM parties the exclusions left out (the owner,
    /// his agents, the house's names, a bot's or a UI's: theseus-0p1r).
    #[serde(default)]
    #[cfg_attr(test, ts(type = "number"))]
    pub excluded: u64,
    pub ms: f64,
    /// `propose`'s report, in place of the counts above.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub propose: Option<PeopleProposeReport>,
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
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct HealthImported {
    /// Imported sessions still held: the tags' sessions less their erased.
    #[cfg_attr(test, ts(type = "number"))]
    pub sessions: u64,
    /// Imported sessions an `import.erase` tombstoned. Their keys remain.
    #[cfg_attr(test, ts(type = "number"))]
    pub erased: u64,
    /// The OpenClaw sync's last run (theseus-pjmj7), when one has run or the
    /// config names a store: absent otherwise.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub sync: Option<crate::import_sync::HealthSync>,
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

// Python's canonical JSON, as the episode format's hash is taken over it: keys
// sorted, no spaces, either string form. Pure, so the daemon's check of an
// episode (theseus-core's `import::episode`) and the CLI's making of one
// (`theseus import claude-code`, theseus-o73y) share one writer.

/// `v` as Python's `json.dumps(v, sort_keys=True, separators=(",", ":"),
/// ensure_ascii=ascii)` writes it.
pub fn canonical(v: &Value, ascii: bool, out: &mut String) {
    match v {
        Value::Null => out.push_str("null"),
        Value::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
        Value::Number(n) => match (n.as_i64(), n.as_u64(), n.as_f64()) {
            (Some(i), _, _) => out.push_str(&i.to_string()),
            (_, Some(u), _) => out.push_str(&u.to_string()),
            (_, _, Some(f)) => out.push_str(&py_float(f)),
            _ => out.push_str(&n.to_string()),
        },
        Value::String(s) => string(s, ascii, out),
        Value::Array(a) => {
            out.push('[');
            for (i, x) in a.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                canonical(x, ascii, out);
            }
            out.push(']');
        }
        Value::Object(o) => {
            let mut keys: Vec<&String> = o.keys().collect();
            keys.sort();
            out.push('{');
            for (i, k) in keys.into_iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                string(k, ascii, out);
                out.push(':');
                canonical(&o[k], ascii, out);
            }
            out.push('}');
        }
    }
}

fn string(s: &str, ascii: bool, out: &mut String) {
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{8}' => out.push_str("\\b"),
            '\u{c}' => out.push_str("\\f"),
            c if (c as u32) < 0x20 || (ascii && (c as u32) == 0x7f) => {
                out.push_str(&format!("\\u{:04x}", c as u32));
            }
            c if ascii && !c.is_ascii() => {
                let mut units = [0u16; 2];
                for u in c.encode_utf16(&mut units) {
                    out.push_str(&format!("\\u{u:04x}"));
                }
            }
            c => out.push(c),
        }
    }
    out.push('"');
}

/// A float as Python's `repr` writes it: the shortest digits that read
/// back, positional from 1e-4 to below 1e16, else `1e-05`, `1.5e+16`.
pub fn py_float(f: f64) -> String {
    if f == 0.0 {
        return if f.is_sign_negative() { "-0.0" } else { "0.0" }.into();
    }
    // Rust's `{:e}` writes the shortest digits that read back.
    let e = format!("{:e}", f.abs());
    let (mant, exp) = e.split_once('e').unwrap_or((&e, "0"));
    let digits: String = mant.chars().filter(char::is_ascii_digit).collect();
    let exp: i64 = exp.parse().unwrap_or(0);
    let decpt = exp + 1;
    let sign = if f < 0.0 { "-" } else { "" };
    let n = digits.len() as i64;
    if -4 < decpt && decpt <= 16 {
        let body = if decpt <= 0 {
            format!("0.{}{digits}", "0".repeat((-decpt) as usize))
        } else if decpt >= n {
            format!("{digits}{}.0", "0".repeat((decpt - n) as usize))
        } else {
            let (a, b) = digits.split_at(decpt as usize);
            format!("{a}.{b}")
        };
        format!("{sign}{body}")
    } else {
        let (first, rest) = digits.split_at(1);
        let frac = if rest.is_empty() {
            String::new()
        } else {
            format!(".{rest}")
        };
        let es = if exp < 0 { "-" } else { "+" };
        format!("{sign}{first}{frac}e{es}{:02}", exp.abs())
    }
}
