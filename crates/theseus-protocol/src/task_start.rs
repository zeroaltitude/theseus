//! `task.start` (theseus-7oq0w): the owner starts a task from any surface with no
//! model turn. The CLI's `theseus task start`, the terminal UI's `N`, and the
//! rows that follow (Discord's "New task", "Make this a task", the horizon's
//! deferred start) all call this one method.

use serde::{Deserialize, Serialize};

use crate::DiscordOrigin;

/// Where a started task's report goes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum ReportTo {
    /// The parent conversation: its place hears the post, and its next turn
    /// reads the report.
    #[default]
    Home,
    /// Nothing is posted: the report waits on the board (`theseus tasks`).
    Board,
}

/// How long a task is expected to take (theseus-bh3t.1): `long` is likely over
/// ten minutes, which a surface may give a place of its own. Short, the
/// default, is never stored.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum Expect {
    #[default]
    Short,
    Long,
}

/// `task.start`: a task from the owner's word.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct TaskStartParams {
    /// The work the task does.
    pub brief: String,
    /// Its title; the brief's first line when not given.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub title: Option<String>,
    /// The conversation it goes under, and carves its budget from; none is
    /// the owner's Home.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub home: Option<String>,
    /// Its budget; a quarter of what the parent has left when not given.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub budget_usd: Option<f64>,
    /// The profile its turns run on; the parent's model when not given.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub profile: Option<String>,
    /// Start then (unix ms), asleep until it; or after this many ms. Not both.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub start_at_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub start_after_ms: Option<u64>,
    /// The same, as a person types it (`fri 9:00`, `2h`, `tomorrow 9:00`),
    /// read in the daemon's zone as `wake.create`'s `when` is: for a client
    /// that does not know the zone. Not with the two above.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub start_when: Option<String>,
    /// Text the task should read verbatim (a message, a pasted log).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub quotes: Vec<String>,
    /// The quotes are not the starter's own words: an email, another person's
    /// message (theseus-nfhaf). Accepted and ignored since the external-text
    /// hold went (theseus-gyjyo.8): it chose whether the task's session held
    /// its quotes as web text, and a quote is an item like any other now.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    #[cfg_attr(test, ts(as = "Option<bool>", optional))]
    pub outside_quotes: bool,
    /// Retired with the open view (theseus-9ymz0): a start that sets it is
    /// refused. What keeps a session off a model outside the owner's trust is
    /// the content labels on it.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    #[cfg_attr(test, ts(as = "Option<bool>", optional))]
    pub open: bool,
    /// Where its report goes; `home` when not given.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub report_to: Option<ReportTo>,
    /// Its expected length; `short` when not given.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub expect: Option<Expect>,
    /// Who asked, as a label. It names and proves nothing; the connection's
    /// surface decides.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub author: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub discord: Option<DiscordOrigin>,
}

/// What `task.start` opened.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct TaskStartResult {
    /// The task's session id (`theseus tasks` and `theseus history` take it,
    /// or its last six characters).
    pub task_id: String,
    pub session_id: String,
    /// The last six characters, how people name a task.
    pub short: String,
    /// The conversation it goes under.
    pub home_session: String,
    /// What was carved from that conversation for it, in dollars.
    pub limit_usd: f64,
    /// When it starts, for a deferred task: it sleeps until then.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub starts_at_ms: Option<u64>,
}
