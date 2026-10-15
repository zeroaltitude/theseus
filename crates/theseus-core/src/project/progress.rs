//! A project's progress, from its subtree (theseus-n5ui2): steps done of
//! total, milestones done of total, the next due, how many are overdue.
//! Pure: the records and the clock are passed in.

use theseus_protocol::projects::ProjectProgress;
use theseus_protocol::tasks::{TaskKind, TaskRecord, TaskState};

use crate::task_graph::subtree;

/// The milestones under `project`, soonest due first (undated last), then
/// oldest first.
pub fn milestones<'a>(tasks: &'a [TaskRecord], project: &str) -> Vec<&'a TaskRecord> {
    let mut v: Vec<&TaskRecord> = subtree(tasks, project)
        .into_iter()
        .filter(|t| t.kind == TaskKind::Milestone)
        .collect();
    v.sort_by_key(|t| (t.due_at_ms.unwrap_or(u64::MAX), t.created_at_ms));
    v
}

/// The numbers for `project` at `now_ms`. A step is a task under the project
/// that is neither a milestone nor a project; an abandoned one is not counted
/// (it is not work left, and it did not get done).
pub fn progress(tasks: &[TaskRecord], project: &str, now_ms: u64) -> ProjectProgress {
    let mut p = ProjectProgress::default();
    for t in subtree(tasks, project) {
        if t.id == project || t.state == TaskState::Abandoned {
            continue;
        }
        let done = t.state == TaskState::Done;
        match t.kind {
            TaskKind::Milestone => {
                p.milestones_total += 1;
                p.milestones_done += u32::from(done);
                if !t.state.is_closed() {
                    if let Some(due) = t.due_at_ms {
                        p.next_due_ms = Some(p.next_due_ms.map_or(due, |n| n.min(due)));
                        p.overdue += u32::from(due < now_ms);
                    }
                }
            }
            TaskKind::Item => {
                p.steps_total += 1;
                p.steps_done += u32::from(done);
            }
            TaskKind::Project => {}
        }
    }
    p
}
