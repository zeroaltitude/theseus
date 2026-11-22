//! `work.watch` (theseus-hkx30): the daemon's work board on the wire. A
//! snapshot of every root that needs you or works, each with its open
//! subtree, then `work.changed` (one per committed frame, every view the
//! frame changed with its ancestors' rollups) and `work.notice` (the
//! notification policy's `Notice`, at most one per changed view per frame)
//! until `work.unwatch`. Types only: the board is the core's.
//!
//! A client applies a view only if its `position` is greater than the last it
//! applied for the same `id` (the position rule, as `executions.watch`'s),
//! and drops each id in `gone`. After `events.lost` it asks `work.watch`
//! again and applies the snapshot by the same rule.

use serde::{Deserialize, Serialize};

use crate::work::WorkView;

/// `work.watch`'s params. All optional: no params watches every root.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct WorkWatchParams {
    /// How many of the roots that need no one and do nothing the snapshot
    /// carries, the most recently active first: 200 by default. Every root
    /// that needs you or works is always there, with its open subtree.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub limit: Option<u32>,
    /// Only these roots (a conversation's session id): what a binding for a
    /// shared place asks. A connection that reads no private text must name
    /// them, and sees the roots of a shared place alone. `system` names the
    /// daemon's own work (`work_system`): the root `Theseus` and its jobs, for a
    /// connection that reads private text; a shared place's never has it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub roots: Option<Vec<String>>,
    /// A screen a person is looking at (the TUI, a cockpit page, `theseus
    /// chat`): the daemon counts these, and an owner-ask reads the count.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub interactive: Option<bool>,
}

/// `work.watch`'s answer: the board as it stood when the watch began. The
/// subscription comes first, so no change falls between the two.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct WorkWatchResult {
    /// The last frame the board had applied.
    pub position: u64,
    /// Every root that needs you or works with its open subtree (needs you
    /// first, the longest waiting first), then the `limit` most recently
    /// active other roots.
    pub views: Vec<WorkView>,
    /// How many roots the board holds, the cold ones included.
    pub total: u64,
}

/// `work.unwatch`'s answer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct WorkUnwatchResult {
    pub watching: bool,
}

/// `work.changed`: one committed frame's changes. Every view the frame
/// changed, and its ancestors' with their rollups; `gone` names the ids that
/// left the board (an ended child past its root's last event).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct WorkChanged {
    pub position: u64,
    pub at_ms: u64,
    pub views: Vec<WorkView>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub gone: Vec<String>,
}
