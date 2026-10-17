//! Projects on the wire (theseus-n5ui2, the horizon row HZ10): long-term work
//! on the task graph. A project is a TASK record of kind `project` with
//! milestones (kind `milestone`, dated) and plan items under it; a session
//! attaches to one (`project.attach`) and its requests carry the project's
//! slice. These are the owner's verbs' params and results, and the progress a
//! list shows. The records themselves are `tasks::TaskRecord`'s.

use serde::{Deserialize, Serialize};

use crate::tasks::TaskRecord;

/// The weekly review's default time, in the routine's schedule words
/// (the owner's decision D-H12: Monday morning).
pub const DEFAULT_REVIEW: &str = "mon 08:00";

/// What a project has done, from its subtree: plan items (steps) and
/// milestones done of total, the next milestone due, how many are overdue.
#[cfg_attr(test, derive(ts_rs::TS))]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectProgress {
    pub steps_done: u32,
    pub steps_total: u32,
    pub milestones_done: u32,
    pub milestones_total: u32,
    /// The soonest due date among the open milestones, unix ms.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub next_due_ms: Option<u64>,
    /// Open milestones whose date has passed.
    pub overdue: u32,
}

impl ProjectProgress {
    /// `▰▰▰▱▱▱▱ 3/7`: the bar over `width` cells, milestones first (the
    /// project's own steps), else its steps; empty when it has neither.
    pub fn bar(&self, width: usize) -> String {
        let (done, total) = if self.milestones_total > 0 {
            (self.milestones_done, self.milestones_total)
        } else {
            (self.steps_done, self.steps_total)
        };
        let width = width.max(1);
        let filled = if total == 0 {
            0
        } else {
            (done as usize * width) / total as usize
        };
        format!(
            "{}{} {done}/{total}",
            "▰".repeat(filled.min(width)),
            "▱".repeat(width - filled.min(width))
        )
    }
}

/// What an attached conversation's request showed of its project: the slice's
/// digest, lines and size (`context.compiled`'s `project`, which `theseus why`
/// reads). Absent from a conversation with no project.
#[cfg_attr(test, derive(ts_rs::TS))]
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectSliceSummary {
    pub project: String,
    /// The slice's text's digest.
    pub digest: String,
    pub lines: u32,
    /// Milestones and steps left out past the slice's bound.
    pub left_out: u32,
    /// Its size, estimated.
    pub tokens: u64,
}

/// One project as a list shows it.
#[cfg_attr(test, derive(ts_rs::TS))]
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectInfo {
    /// `tsk_…`.
    pub id: String,
    pub title: String,
    pub objective: String,
    #[serde(default)]
    pub acceptance: Vec<String>,
    /// The project task's state as it reads now (`accepted`, `done`, …).
    pub state: String,
    pub progress: ProjectProgress,
    /// The next open milestone's title and date.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub next_title: Option<String>,
    /// The weekly review's routine (`ses_…`), and its next run.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub review: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub review_next_ms: Option<u64>,
}

/// A milestone to create: its title and its date (`when` words:
/// `2026-11-03`, `Fri 09:00`, `in 3d`).
#[cfg_attr(test, derive(ts_rs::TS))]
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct MilestoneSpec {
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub due: Option<String>,
}

/// `project.create`: the owner's. The goal is the owner's layer: the
/// objective and acceptance given here are the owner's, and an agent's
/// edit of them waits for a yes.
#[cfg_attr(test, derive(ts_rs::TS))]
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ProjectCreateParams {
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub objective: Option<String>,
    #[serde(default)]
    pub acceptance: Vec<String>,
    #[serde(default)]
    pub milestones: Vec<MilestoneSpec>,
    /// The weekly review's schedule, in a routine's words (`mon 08:00`, the
    /// default), or `none` for no review.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub review: Option<String>,
    /// A review's run budget, dollars.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub review_usd: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub author: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub discord: Option<crate::DiscordOrigin>,
}

#[cfg_attr(test, derive(ts_rs::TS))]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectCreateResult {
    pub project: ProjectInfo,
    /// The project and its milestones, as written.
    pub records: Vec<TaskRecord>,
}

#[cfg_attr(test, derive(ts_rs::TS))]
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ProjectListParams {
    /// Closed projects too (default: open ones).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub all: bool,
}

#[cfg_attr(test, derive(ts_rs::TS))]
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ProjectListResult {
    pub projects: Vec<ProjectInfo>,
}

/// `project.show`: one project by its id or the end of it.
#[cfg_attr(test, derive(ts_rs::TS))]
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ProjectShowParams {
    pub id: String,
}

#[cfg_attr(test, derive(ts_rs::TS))]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectShowResult {
    pub project: ProjectInfo,
    /// The project and everything under it, depth first.
    pub records: Vec<TaskRecord>,
}

/// `project.attach`: the session carries the project's slice in every
/// request from its next loop on. Without `project`, it detaches.
#[cfg_attr(test, derive(ts_rs::TS))]
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ProjectAttachParams {
    pub session: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub project: Option<String>,
    /// Who asked, and from where, for the Discord binding (a reaction on a task's message makes
    /// a project, theseus-bh3t.2): the core judges them as it does `project.create`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub author: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub discord: Option<crate::DiscordOrigin>,
}

#[cfg_attr(test, derive(ts_rs::TS))]
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ProjectAttachResult {
    pub session: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub project: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The bar the list draws: filled cells by share, then done of total.
    #[test]
    fn the_bar_fills_by_milestones_then_steps() {
        let mut p = ProjectProgress {
            milestones_done: 3,
            milestones_total: 7,
            ..Default::default()
        };
        assert_eq!(p.bar(7), "▰▰▰▱▱▱▱ 3/7");
        p.milestones_total = 0;
        p.steps_done = 1;
        p.steps_total = 2;
        assert_eq!(p.bar(4), "▰▰▱▱ 1/2");
        assert_eq!(ProjectProgress::default().bar(3), "▱▱▱ 0/0");
    }
}
