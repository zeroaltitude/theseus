//! The tasks involved in a project (issue 37), read from the records and never stored: "nothing is a second
//! list". A task is involved when
//!
//! - it was **started for the project**: its record's `origin.project` names it, the one mark that
//!   `task.start` and a model's `task.create` make when the starting conversation is attached to a project
//!   (`attached`), since an attachment can be changed and nothing else says what it was then;
//! - its own record hangs **under** the project's (`parent` chains);
//! - it **holds a step**: a claim on one of the project's records, for as long as the lease holds;
//! - it is a run of the project's **weekly review**: a task of its routine's execution, which the routine's
//!   own record names even after the routine is deleted.
//!
//! Where several hold, the first listed wins. Pure but for the kernel reads in `Core::project_involved`.

use theseus_protocol::projects::ProjectTask;
use theseus_protocol::tasks::TaskRecord;

use crate::task_graph::subtree;

pub const STARTED: &str = "started for it";
pub const UNDER: &str = "under it";
pub const HOLDS: &str = "holds a step";
pub const REVIEW: &str = "weekly review";

/// The project a conversation serves now, for the mark a task it starts carries.
pub fn attached(store: &crate::store::Store, session: &str) -> Option<String> {
    store
        .get_session::<crate::session::SessionRecord>(session)
        .ok()
        .flatten()
        .and_then(|r| r.project)
}

/// The sessions of the tasks the records tie to `project`, each with how, as of `now_ms` (a lapsed claim holds
/// nothing). The review's runs are not in the records and come from the routine (`Core::project_involved`).
pub fn tied(
    tasks: &[TaskRecord],
    project: &TaskRecord,
    now_ms: u64,
) -> Vec<(String, &'static str)> {
    let under = subtree(tasks, &project.id);
    let mut out: Vec<(String, &'static str)> = Vec::new();
    let mut add = |session: &str, how: &'static str| {
        if !out.iter().any(|(s, _)| s == session) {
            out.push((session.to_string(), how));
        }
    };
    for t in tasks {
        if let (Some(s), Some(p)) = (&t.session, &t.origin.project) {
            if *p == project.id {
                add(s, STARTED);
            }
        }
    }
    for t in under.iter().filter(|t| t.id != project.id) {
        if let Some(s) = &t.session {
            add(s, UNDER);
        }
    }
    for c in under.iter().filter_map(|t| t.claim.as_ref()) {
        if c.until_ms > now_ms {
            add(&c.session, HOLDS);
        }
    }
    out
}

/// The order a list is read in: tasks still going first, then the newest first.
pub fn order(tasks: &mut [ProjectTask]) {
    tasks.sort_by_key(|t| (t.ended, std::cmp::Reverse(t.started_at_ms)));
}

/// The first `n` of an ordered list, and how many more there were.
pub fn bounded(mut tasks: Vec<ProjectTask>, n: usize) -> (Vec<ProjectTask>, u32) {
    let more = tasks.len().saturating_sub(n) as u32;
    tasks.truncate(n);
    (tasks, more)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn task(id: &str, ended: bool, at: u64) -> ProjectTask {
        ProjectTask {
            task_id: id.into(),
            ended,
            started_at_ms: at,
            ..Default::default()
        }
    }

    /// Those still going come first, then the newest; the bound keeps the first and counts the rest.
    #[test]
    fn the_list_runs_first_then_newest_and_is_bounded() {
        let mut v = vec![
            task("old_done", true, 1),
            task("new_done", true, 9),
            task("old_run", false, 2),
            task("new_run", false, 8),
        ];
        order(&mut v);
        let ids: Vec<&str> = v.iter().map(|t| t.task_id.as_str()).collect();
        assert_eq!(ids, ["new_run", "old_run", "new_done", "old_done"]);
        let (kept, more) = bounded(v.clone(), 3);
        assert_eq!((kept.len(), more), (3, 1));
        assert_eq!(kept[0].task_id, "new_run");
        let (all, none) = bounded(v, 50);
        assert_eq!((all.len(), none), (4, 0));
    }
}
