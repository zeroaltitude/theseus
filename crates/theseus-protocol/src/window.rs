//! `ledger.window` (theseus-m2tiu): the rows `theseus postmortem` reads, one
//! page of a time window. Read only. The daemon answers it with the rows of
//! `POSTMORTEM_KINDS` alone, their strings scrubbed as history's are, and for
//! a surface that reads no private text only the named session's and its
//! tasks', so the report is the CLI's own work over rows the daemon vouched
//! for.

use crate::LedgerEntry;

/// Every kind the postmortem reads: turns, calls and their settles, model
/// calls, spend notices, tasks, questions and their answers, wakes, and the
/// startup's rows. A kind no report words is not here, so a window of a busy
/// store reads a fraction of its ledger.
pub const POSTMORTEM_KINDS: &[&str] = &[
    "turn.started",
    "turn.ended",
    "turn.failed",
    "turn.refused",
    "provider.call",
    "provider.cut",
    "provider.error",
    "provider.fallback",
    "provider.refusal",
    "spend.ceiling",
    "budget.reached",
    "loop.cap_reached",
    "task.created",
    "task.ended",
    "task.closed",
    "tool.confirm_requested",
    "action.planned",
    "action.dispatched",
    "action.succeeded",
    "action.failed",
    "action.outcome_unknown",
    "action.resolved",
    "action.cancelled",
    "action.confirm_answered",
    "action.confirmed",
    "action.declined",
    "action.expired",
    "approval.refused",
    "policy.tightened",
    "tool.job_started",
    "execution.queued",
    "execution.opened",
    "execution.interrupted",
    "wake.set",
    "wake.fired",
    "wake.cancelled",
    "wake.ended",
    "server.started",
    "server.serving",
    "server.stopping",
    "server.crashed",
    "startup.step",
    "reconcile",
];

/// `ledger.window`'s params.
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct LedgerWindowParams {
    /// Rows written at this time or after (unix ms), and with `until_ms` at
    /// it or before.
    pub since_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub until_ms: Option<u64>,
    /// Only this session's rows, and its tasks' and theirs, as far down as
    /// tasks go; the daemon's own rows (its starts and stops) stay. A task's
    /// id (`tsk_…`) names its session.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub session_id: Option<String>,
    /// Only rows after this WAL position: the previous page's `next`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub after: Option<u64>,
    /// At most this many rows a page (default 500, at most 1,000).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub limit: Option<usize>,
}

/// `ledger.window`'s answer: one page, oldest first.
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct LedgerWindowResult {
    pub rows: Vec<LedgerEntry>,
    /// The `after` for the next page while more rows follow; absent at the
    /// window's end.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub next: Option<u64>,
    /// The sessions the window was narrowed to (the one asked for and its
    /// tasks), when it was.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub scope: Vec<String>,
    /// The ledger's index is still being built after a start: nothing could
    /// be read yet.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub building: bool,
    /// The surface reads no private text, so the window held only the
    /// session named and its tasks.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub shared: bool,
    /// The daemon's clock, unix ms, when it answered.
    pub now_ms: u64,
}
