//! `fact.retract` (theseus-2ced, the killer-features plan's R13): the
//! owner marks one node wrong, and the answer is every place it spread:
//! each exposure `node.reach` finds, sorted by what happens to it. The
//! types are the wire's; the walk and the acts are theseus-core's
//! `retract.rs`.

use serde::{Deserialize, Serialize};

/// `fact.retract`: mark a node wrong, and find where it went. Acting: the
/// owner, from a private place (the CLI refuses it inside a job).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct FactRetractParams {
    /// A node's whole id, the last six or more characters of one, or a
    /// recalled item's handle (`[m:a1b2c3]`, read as that ending until
    /// handles are the recall's own).
    pub node: String,
    /// Why it is wrong, in a sentence: the correction notes carry it.
    pub why: String,
    /// The session the request is run for, when a place's session runs
    /// it: its place rules what the answer shows (a shared place's never
    /// shows a private exposure's text). None: the CLI's, a private place.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub session_id: Option<String>,
}

/// What a retraction found and did.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct FactRetractResult {
    pub node_id: String,
    /// The session that holds the node.
    pub session_id: String,
    /// The node's first words, unless the place the request is run for may
    /// not read them.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub gist: Option<String>,
    pub why: String,
    /// Who retracted it, and when.
    pub by: String,
    pub at_ms: u64,
    /// The node was retracted before: nothing was written now, and this is
    /// the table the first retraction made.
    pub already: bool,
    /// Every exposure, in the order its kind lists (`exposure_kind`).
    pub exposures: Vec<RetractExposure>,
    /// The walk stopped at its caps (16 generations, 256 copies): there
    /// are more.
    pub partial: bool,
    /// The card to the owner (tasks to re-run) was posted. Never a card
    /// per task: one per retraction, and none when no task used the node.
    pub card: bool,
}

/// The kinds of exposure, in the order the table lists them.
pub mod exposure_kind {
    /// A session that goes on: it gets a correction note at its next turn.
    pub const SESSION: &str = "session";
    /// A synthesis that cites it: labeled stale, and re-checked at the next
    /// consolidation.
    pub const SYNTHESIS: &str = "synthesis";
    /// A task that used it: named on the card, to re-run.
    pub const TASK: &str = "task";
    /// A message posted to a place: it cannot be unpublished.
    pub const PUBLISHED: &str = "published";
    /// Seen, not used: nothing to do.
    pub const SEEN: &str = "seen";
    /// Every kind, in table order.
    pub const ALL: [&str; 5] = [SESSION, SYNTHESIS, TASK, PUBLISHED, SEEN];
}

/// One place the node went.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct RetractExposure {
    /// One of `exposure_kind::ALL`.
    pub kind: String,
    /// The session that held it (a synthesis's is the memory's own).
    pub session_id: String,
    /// The copy there, or the node itself in its own session.
    pub node_id: String,
    /// 0 for the node's own session, else how many copies away.
    pub generation: u32,
    /// Where, in words: the session's id and its title, or the place's name;
    /// a private exposure shown to a shared place says only that it is
    /// private.
    pub place: String,
    /// When it was last in a context there, if it was.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub at_ms: Option<u64>,
    /// Whether its turn used it (`memory.used`): none where nothing records
    /// use (the node's own session, a published copy).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub used: Option<bool>,
    /// What happens to it, in a sentence.
    pub what: String,
    /// The exposure is private, and its words were left out of this answer.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub hidden: bool,
}
