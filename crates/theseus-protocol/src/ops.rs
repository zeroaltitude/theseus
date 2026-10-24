//! The Operations page's verbs (theseus-jid7t.2): a sticky model for a session
//! (`session.profile`), a task's report home (`task.rehome`), a deferred
//! task's start (`task.reschedule`), and the data mark the page greys by.
//! Each is the owner's act; none sends a Discord message.

use serde::{Deserialize, Serialize};

/// What a session's data may reach, as the floor reads it: `open` (any model a
/// turn admitted open may use), `private` (US origin only), `sensitive` (US
/// origin only, and never a shared place). Ordered weakest first.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum DataMark {
    Open,
    Private,
    Sensitive,
}

/// `session.profile`'s params.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct SessionProfileParams {
    pub session_id: String,
    /// The profile its turns run on from the next one; none (or null) reverts
    /// to the default.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub profile: Option<String>,
    /// Who asked, as the ledger names them. Default: the surface.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub author: Option<String>,
    /// Set by the Discord binding: where the owner acted, so the core judges them.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub discord: Option<crate::DiscordOrigin>,
}

/// `session.profile`'s answer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct SessionProfileResult {
    pub session_id: String,
    /// The sticky profile now (none: the default). Unchanged when refused.
    pub profile: Option<String>,
    /// What was set before: one more call with it is the undo.
    pub previous: Option<String>,
    /// Why the floor refuses that profile for this session; nothing was written.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub refused: Option<String>,
    /// What the answer cannot promise: the floor decides each turn.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub note: Option<String>,
}

/// `task.rehome`'s params.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct TaskRehomeParams {
    /// Its id, its execution's id, or the end of either, as `task.cancel` takes it.
    pub task: String,
    /// A conversation's session id, or `thread:<slug>`.
    pub home: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub author: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub discord: Option<crate::DiscordOrigin>,
}

/// `task.rehome`'s answer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct TaskRehomeResult {
    /// The task's session id.
    pub task: String,
    /// Where its report went before and goes now (none: the board alone).
    pub from: Option<String>,
    pub to: Option<String>,
    /// What stays: the budget is still carved from the original parent, and that parent's next
    /// turn still reads the report.
    pub note: String,
}

/// `task.reschedule`'s params: exactly one of the three times, as `task.start`
/// takes them.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct TaskRescheduleParams {
    pub task: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub start_at_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub start_after_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub start_when: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub author: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub discord: Option<crate::DiscordOrigin>,
}

/// `task.reschedule`'s answer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct TaskRescheduleResult {
    pub task: String,
    pub starts_at_ms: u64,
    /// The start before: one more call with it as `start_at_ms` is the undo.
    pub previous: u64,
}
