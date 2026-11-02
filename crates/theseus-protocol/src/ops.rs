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

/// One session or task held to a profile (`session.profile`, META `session.profile.<id>`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct PinnedSession {
    /// The session's id; a task's own session is its `task_id`.
    pub session_id: String,
    /// `session`, or `task` when the session is a task's own; `task-start`, `task-fixed` or `task-unknown` in `task_pins`.
    pub kind: String,
    /// The profile it is held to, as stored.
    pub profile: String,
    /// False when the profile is no longer configured: the hold stays on record but no turn follows it.
    pub configured: bool,
    /// Where a task in `task_pins` is in its life, which decides what letting go of its model changes:
    /// `pending` (no turn yet: the brief may still choose), `asleep` (past its first turn, no turn running:
    /// only checkpoints, from its next turn) or `running` (a turn is running now and finishes on the model
    /// it started on). Absent on a hold, and from a daemon that did not say.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub state: Option<String>,
}

/// `profile.pins`' answer (theseus-h66v4.2): how many sessions and tasks carry a held profile, and which.
/// The totals ride beside the pinned counts, so "0 pinned" can be told from "0 sessions".
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct ProfilePinsResult {
    pub live: String,
    /// `runtime` when the live profile is itself a pin (`profile.use`); `config` otherwise.
    pub live_source: String,
    /// Session records that are not tasks' own, and task sessions.
    pub sessions: u64,
    pub tasks: u64,
    pub pinned_sessions: u64,
    pub pinned_tasks: u64,
    pub pins: Vec<PinnedSession>,
    /// Live tasks no brief or checkpoint ever routes: each has a model fixed when it was made and no
    /// `task.route.<session>` mark (`turn/brief_step.rs`), whatever turn it is on. Beside the holds, never inside
    /// them, and told apart by `kind`: `task-start` (`task.start --profile`: the owner named the model; `unpin
    /// --all` clears it by writing the mark), `task-fixed` (Theseus made it on a model of its own choosing: an
    /// agent's child task, a check, a draft's reviewer, a routine's run, a retry; not cleared, nothing to drop)
    /// and `task-unknown` (the ledger could not say who made it; not cleared). Each one's `profile` is the one
    /// it was made on.
    #[serde(default)]
    pub task_pins: Vec<PinnedSession>,
}

/// `profile.unpin`'s params.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct ProfileUnpinParams {
    /// Also let go every session's and task's held profile, each in its own `session.profile_set` row.
    #[serde(default)]
    pub all: bool,
}

/// A hold `profile.unpin --all` could not let go.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct UnpinFailure {
    pub session_id: String,
    pub error: String,
}

/// `profile.unpin`'s answer (theseus-h66v4.2). The live half is `ProfileReleaseResult`'s, field for field
/// (`released` is carried, never `previous != live`); `before` and `after` are counted, not inferred, and
/// `cleared` lists the holds let go, each of which wrote its own `session.profile_set` row.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct ProfileUnpinResult {
    pub released: bool,
    pub previous: String,
    pub live: String,
    pub by: String,
    pub all: bool,
    pub before: ProfilePinsResult,
    pub cleared: Vec<PinnedSession>,
    pub failed: Vec<UnpinFailure>,
    pub after: ProfilePinsResult,
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
