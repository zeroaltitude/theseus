//! A failed task stays flagged until you dismiss it, and a retry opens a new,
//! linked task (theseus-7zgsm; the plan's §1.8, 1). Two verbs for every
//! surface, and the three answers a failure draws: Retry, Dismiss, Details.
//! Types and pure functions only, like `work.rs`.
//!
//! - `execution.dismiss { execution_id }`: a failed, blocked or
//!   budget-exhausted execution (a task, or a conversation's failed turn)
//!   stops needing anyone, on every surface. The daemon records it on the
//!   execution's own record in one frame, so it survives a restart and the
//!   board's seed. The work view keeps its report and says `dismissed`.
//! - `task.retry { execution_id, note? }`: a failed task's brief and quoted
//!   pieces open as a new task under the same parent, carved from what the
//!   parent has left and never more than the failed one had, with
//!   `retry_of` naming the failed one; the failed one is dismissed in the
//!   same frame.

use serde::{Deserialize, Serialize};

use crate::work::{AnswerOption, Style, WorkKind, WorkState, WorkView};

/// `execution.dismiss`'s params.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct ExecutionDismissParams {
    /// The whole id (`exe_…`) of a failed, blocked or budget-exhausted
    /// execution.
    pub execution_id: String,
    /// Set by the Discord binding: the channel and user the press came from,
    /// which the place rule judges (theseus-a5uds).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub discord: Option<crate::DiscordOrigin>,
}

/// `execution.dismiss`'s answer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct ExecutionDismissResult {
    pub execution_id: String,
    /// When it was dismissed; the first dismissal's time when it was
    /// dismissed before (nothing is written again).
    pub dismissed_at_ms: u64,
}

/// `task.retry`'s params.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct TaskRetryParams {
    /// The whole id (`exe_…`) of the failed task's execution.
    pub execution_id: String,
    /// Words for the new task's report and the ledger: why it is tried
    /// again, or what is different.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub note: Option<String>,
    /// Set by the Discord binding, as for `execution.dismiss`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub discord: Option<crate::DiscordOrigin>,
}

/// `task.retry`'s answer: the new task.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct TaskRetryResult {
    /// The new task's session id (`ses_…`), as `task.list` names it.
    pub task_id: String,
    pub execution_id: String,
    /// The failed task's execution: dismissed in the same frame.
    pub retry_of: String,
    pub limit_usd: f64,
    /// What the failed task had, for a surface to say "carved $2 of $5".
    pub failed_limit_usd: f64,
}

/// Whether a view is a failure that still needs the owner: a failed task or
/// conversation turn, or one blocked or out of budget with no question open,
/// and not dismissed.
pub fn is_failure(v: &WorkView) -> bool {
    !v.dismissed
        && v.question.is_none()
        && matches!(v.kind, WorkKind::Task | WorkKind::Conversation)
        && match v.state {
            WorkState::Failed => true,
            WorkState::NeedsYou => v
                .report
                .as_ref()
                .is_some_and(|r| matches!(r.outcome.as_str(), "blocked" | "budget_exhausted")),
            _ => false,
        }
}

/// Whether an execution's view is a failure that still asks for the owner:
/// failed, blocked or out of budget, with no question open, not dismissed. The
/// execution board's form of `is_failure`.
pub fn needs_dismissal(x: &crate::ExecutionView) -> bool {
    !x.dismissed
        && x.pending.is_empty()
        && matches!(x.state.as_str(), "failed" | "blocked" | "budget_exhausted")
}

/// The answers a failure draws, worded once for every surface: Retry (a task
/// only), Dismiss, Details. Empty for a view that is no failure, or is
/// dismissed already. `Question`s keep `answer_options`; these are verbs
/// (`task.retry`, `execution.dismiss`), not answers to `work.answer`.
pub fn failure_options(v: &WorkView) -> Vec<AnswerOption> {
    if !is_failure(v) {
        return Vec::new();
    }
    let option = |id: &str, label: &str, style, takes_note, effect: &str| AnswerOption {
        id: id.into(),
        label: label.into(),
        style,
        takes_note,
        effect: effect.into(),
    };
    let mut out = Vec::new();
    if v.kind == WorkKind::Task {
        out.push(option(
            "retry",
            "Retry",
            Style::Primary,
            true,
            "a new task with the same brief opens, and this one is dismissed",
        ));
    }
    out.push(option(
        "dismiss",
        "Dismiss",
        Style::Plain,
        false,
        "it stops needing you, on every surface",
    ));
    out.push(option(
        "details",
        "Details",
        Style::Plain,
        false,
        "shows why it failed",
    ));
    out
}
