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

use crate::threads::ThreadState;
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
    /// them, and sees the roots of a shared place alone.
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
    /// The threads that are not done, each a group of the board
    /// (theseus-5i8i). Empty for a connection that reads no private text: a
    /// thread is private always.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub threads: Vec<ThreadGroup>,
}

/// A thread as the board groups it: the record's words, and what its session
/// holds. The views that belong to it say so (`WorkView.thread`); its open
/// tasks, questions and wakes are those views, so a group carries no copy of
/// them. A client keeps the groups by `slug`, and an update's `None` stat
/// leaves the stat it holds (`ThreadGroup::merge`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct ThreadGroup {
    pub slug: String,
    pub title: String,
    pub state: ThreadState,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub persona: Option<String>,
    /// The owner has sealed its boundary: its title shows a lock.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub sealed: bool,
    /// A seal recommended and not yet answered: the reason. It needs the owner.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub seal_pending: Option<String>,
    /// The session the thread runs on now; none before its first message.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub session_id: Option<String>,
    /// The record's last activity; a client takes the later of it and its
    /// session's conversation view.
    #[serde(default)]
    pub last_active_ms: u64,
    /// The session's last compile, in tokens. Read when a watch begins and
    /// when the thread is written; an update leaves it as it was.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub context_tokens: Option<u64>,
    /// What the thread's sessions spent since local midnight; read as
    /// `context_tokens` is.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub today_usd: Option<f64>,
}

impl ThreadGroup {
    /// This group with a newer version of it: the newer one's words, the
    /// stats it did not read kept, and the later activity.
    pub fn merge(&mut self, newer: ThreadGroup) {
        let (tokens, usd, active) = (self.context_tokens, self.today_usd, self.last_active_ms);
        *self = newer;
        self.context_tokens = self.context_tokens.or(tokens);
        self.today_usd = self.today_usd.or(usd);
        self.last_active_ms = self.last_active_ms.max(active);
    }
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
    /// The thread groups this frame or a thread's write changed (theseus-5i8i).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub threads: Vec<ThreadGroup>,
}
