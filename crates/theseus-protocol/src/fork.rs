//! `session.fork` (theseus-2qzx, the killer-features plan's R19): a new
//! conversation from any moment of another, by reference.
//!
//! The fork's first compilation is the source's prefix through `at`: it
//! names the source's nodes by a run (`Inherits`: the session and where the
//! run ends), never by copying them, so a fork costs one frame of a few
//! hundred bytes at ten nodes or at a million. The source is never written
//! to: its reverse entry (an `includes` edge into the session, one for the
//! run) is the fork's own record.
//!
//! Not here, with where each hooks in: the files' pre-images and
//! `--worktree` are R20 (theseus-23rk); the TUI's `f`, Discord's "Fork
//! from here" and the Ship's mark are R21. Each calls `session.fork` and
//! shows `SessionForkResult.not_restored`.

use serde::{Deserialize, Serialize};

use crate::ReachExposure;

/// `session.fork`'s params.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct SessionForkParams {
    /// The session to fork.
    pub session: String,
    /// Where: a node (`msg·a1b2c3`, its id, or its id's last six or more
    /// characters, as `node.reach` names one) or a WAL position.
    pub at: String,
    /// The fork's title; the source's plus " (fork)" when none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub title: Option<String>,
    /// The profile its turns run on; the source's last when none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub profile: Option<String>,
    /// The fork's own spend limit in dollars (pinned, as a task's carve is);
    /// the daemon's session limit when none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub budget_usd: Option<f64>,
}

/// What `session.fork` made.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct SessionForkResult {
    /// The new session.
    pub session_id: String,
    pub title: String,
    pub source_session: String,
    /// The node it was forked at, and its WAL position (the fork point).
    pub at_node: String,
    pub at_position: u64,
    /// The fork's first compilation, and the source's that it derives from
    /// when one held the node.
    pub compilation_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub derived_from: Option<String>,
    /// The frame's bytes and the milliseconds the core took.
    pub bytes: u64,
    pub ms: u64,
    /// What a fork does not bring back, in words (R20's files are not
    /// restored, and what programs did after the fork point ran in the
    /// original).
    pub not_restored: String,
}

/// Where a fork's conversation comes from: a session record's, written in
/// the frame that opens it, and the key of the transcript's prefix.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct ForkOrigin {
    /// The session whose nodes the fork inherits (a fork's own source: a
    /// fork of a fork names the fork, whose own prefix comes first).
    pub session: String,
    /// The last node inherited, and its WAL position: the run ends there.
    pub node: String,
    pub position: u64,
    /// The source's compilation the fork derives from.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub compilation: Option<String>,
    pub at_ms: u64,
}

/// A compilation's inherited run: every node of `session`'s transcript at or
/// before `through_position`, in order, which a fork's first compilation
/// includes by reference. A compilation made later lists its nodes by id as
/// every other does (`Compilation.includes`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct Inherits {
    pub session: String,
    pub through_node: String,
    pub through_position: u64,
}

/// A fork found by `node.reach`: a session whose first compilation took the
/// node by reference, generation by generation (a fork of a fork is one
/// further).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct ReachFork {
    pub session_id: String,
    /// The fork's first compilation: the entry's `includes` run.
    pub compilation_id: String,
    pub generation: u32,
    /// The session it was forked from, one generation up.
    pub from: String,
    /// Where the run ends.
    pub at_position: u64,
    #[serde(flatten)]
    pub exposure: ReachExposure,
}
