//! Projects (theseus-n5ui2, the horizon row HZ10): long-term work on the task
//! graph (39a). A project is a TASK record of kind `project`; its milestones
//! (kind `milestone`, dated) and its steps (plan items) are the records under
//! it. Nothing is a second list: the records are the truth and every number
//! here is read from them.
//!
//! - **Progress** (`progress.rs`): steps and milestones done of total, the
//!   next due, how many are overdue, from the subtree.
//! - **The slice** (`slice.rs`): an attached conversation's request ends with
//!   the project's goal, its open milestones with dates, and its next steps,
//!   within `slice::MAX_TOKENS` of the task view's bound, recompiled every
//!   loop from the records in memory, so it survives every compaction.
//! - **Reminders** (`remind.rs`): a milestone's date sets a notice reminder
//!   (`⚑ due today: …`), moved when the date moves, cancelled when the
//!   milestone closes, and an overdue notice a grace after it.
//! - **The review** (`review.rs`): the routine made with the project, whose
//!   brief reads the project and proposes next week's steps.
//! - **Create** (`create.rs`): the project and its milestones in one frame.
//!
//! The goal is the owner's layer (39a, layer 1) when the owner sets it: a
//! project made by `project.create` is the owner's, and an agent's edit of
//! its objective or acceptance waits for a yes like any owner's task. The
//! milestones' dates are the agent's plan.

pub mod create;
pub mod info;
pub mod progress;
pub mod remind;
pub mod review;
pub mod slice;

use theseus_protocol::tasks::{TaskKind, TaskRecord};

/// What a project's creation needs beside the turn: the routines (its weekly
/// review is one) and the defaults.
pub type Ext<'a> = (&'a crate::routine::Book, &'a crate::config::ProjectsConfig);

/// Why `task.create` of a project with a review always asks: its authority.
pub const ASKS_REVIEW: &str =
    "a project's weekly review is a routine, which spends money on a schedule, so it waits for your yes";

/// The card for a project and its review.
pub fn card(title: &str, milestones: usize, review: Option<&str>) -> String {
    let when = review.map_or("on its default schedule", |r| r.trim());
    format!(
        "Create project \"{title}\" with {milestones} milestone{} and a weekly review routine ({when}); \
         each review run spends from its own month and asks before it uses a tool",
        if milestones == 1 { "" } else { "s" }
    )
}

/// Whether `t` is a project.
pub fn is_project(t: &TaskRecord) -> bool {
    t.kind == TaskKind::Project
}

/// The project a task belongs to: itself when it is one, else the nearest
/// project above it.
pub fn project_of<'a>(tasks: &'a [TaskRecord], t: &'a TaskRecord) -> Option<&'a TaskRecord> {
    let mut at = t;
    for _ in 0..32 {
        if is_project(at) {
            return Some(at);
        }
        at = at
            .parent
            .as_deref()
            .and_then(|p| tasks.iter().find(|x| x.id == p))?;
    }
    None
}

/// The open projects, oldest first.
pub fn open_projects(tasks: &[TaskRecord]) -> Vec<&TaskRecord> {
    tasks
        .iter()
        .filter(|t| is_project(t) && !t.state.is_closed())
        .collect()
}

#[cfg(test)]
mod tests;
