//! Projects on the wire (theseus-n5ui2, the horizon row HZ10): long-term work
//! on the task graph. A project is a TASK record of kind `project` with
//! milestones (kind `milestone`, dated) and plan items under it; a session
//! attaches to one (`project.attach`) and its requests carry the project's
//! slice. These are the owner's verbs' params and results, and the progress a
//! list shows. The records themselves are `tasks::TaskRecord`'s. The last
//! types are the owner's grant of a repository's own instruction files
//! (theseus-wadu): `project.trust`, `project.untrust`, `project.trusted`.

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

/// `project.item.add`: a step on the project's own list. An item is a task
/// record of kind `item` under the project, the same thing the slice's "next
/// steps" and the progress counts read, so nothing is a second list.
#[cfg_attr(test, derive(ts_rs::TS))]
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ProjectItemAddParams {
    /// The project, by id, an end of it, or its title.
    pub project: String,
    /// One line, at most 200 characters.
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub author: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub discord: Option<crate::DiscordOrigin>,
}

/// `project.item.close`: an item done, by its id or an end of it.
#[cfg_attr(test, derive(ts_rs::TS))]
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ProjectItemCloseParams {
    pub item: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub author: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub discord: Option<crate::DiscordOrigin>,
}

/// `project.item.list`: the project's open items, or all with `all`.
#[cfg_attr(test, derive(ts_rs::TS))]
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ProjectItemListParams {
    pub project: String,
    #[serde(default)]
    pub all: bool,
}

/// How the owner ends a project: as finished, or as given up. They are the end states the model's own close
/// writes (`done` and `abandoned`), so no state is new.
#[cfg_attr(test, derive(ts_rs::TS))]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProjectEnd {
    #[default]
    Done,
    Dropped,
}

impl ProjectEnd {
    /// The record state it writes.
    pub fn state(self) -> crate::tasks::TaskState {
        match self {
            ProjectEnd::Done => crate::tasks::TaskState::Done,
            ProjectEnd::Dropped => crate::tasks::TaskState::Abandoned,
        }
    }

    pub fn word(self) -> &'static str {
        match self {
            ProjectEnd::Done => "done",
            ProjectEnd::Dropped => "dropped",
        }
    }
}

/// `project.close`: the owner's end of a project, by its id, an end of it, or its title. It ends the
/// project and its open items and milestones, and stops its weekly review and its reminders. It is
/// refused while a task carries one of them.
#[cfg_attr(test, derive(ts_rs::TS))]
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ProjectCloseParams {
    pub project: String,
    #[serde(default)]
    pub how: ProjectEnd,
    /// One line on why, kept with the project's record.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub author: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub discord: Option<crate::DiscordOrigin>,
}

/// What a close did: the project as it now reads, and what ended and what stopped with it.
#[cfg_attr(test, derive(ts_rs::TS))]
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectCloseResult {
    pub project: ProjectInfo,
    pub how: ProjectEnd,
    pub items_ended: u32,
    pub milestones_ended: u32,
    /// The weekly review's routine that was ended (`ses_…`), when the project had a live one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub review_stopped: Option<String>,
    /// Milestone reminders that were cancelled.
    pub reminders_cancelled: u32,
}

fn many(n: u32, one: &str, more: &str) -> String {
    format!("{n} {}", if n == 1 { one } else { more })
}

/// What a close would end, counted from the project's records (`project.show`'s): its open items, its
/// open milestones, and how many of those milestones have a date, so a reminder.
pub fn close_counts(records: &[TaskRecord]) -> (u32, u32, u32) {
    use crate::tasks::TaskKind;
    let open = |k: TaskKind| {
        records
            .iter()
            .filter(move |t| t.kind == k && !t.state.is_closed())
    };
    (
        open(TaskKind::Item).count() as u32,
        open(TaskKind::Milestone).count() as u32,
        open(TaskKind::Milestone)
            .filter(|t| t.due_at_ms.is_some())
            .count() as u32,
    )
}

/// What a close is about to do, in one place so the terminal and the cockpit's sheet say the same words:
/// the project `title`, `how`, its open items, open milestones and dated ones, and whether a review runs.
pub fn close_card(
    title: &str,
    how: ProjectEnd,
    (items, milestones, dated): (u32, u32, u32),
    review: bool,
) -> String {
    let end = match how {
        ProjectEnd::Done => "Mark done",
        ProjectEnd::Dropped => "Drop",
    };
    let mut stops = Vec::new();
    if review {
        stops.push("its weekly review".to_string());
    }
    if dated > 0 {
        stops.push(format!(
            "the reminders of {}",
            many(dated, "dated milestone", "dated milestones")
        ));
    }
    let stops = if stops.is_empty() {
        String::new()
    } else {
        format!(" and stop {}", stops.join(" and "))
    };
    format!(
        "{end} the project \"{title}\": end {} and {}{stops}",
        many(items, "open item", "open items"),
        many(milestones, "open milestone", "open milestones"),
    )
}

/// What a close says it did.
pub fn project_closed(r: &ProjectCloseResult) -> String {
    let mut out = format!(
        "Project {} {}: {} and {} ended",
        r.project.id.rsplit('_').next().unwrap_or(&r.project.id),
        match r.how {
            ProjectEnd::Done => "marked done",
            ProjectEnd::Dropped => "dropped",
        },
        many(r.items_ended, "item", "items"),
        many(r.milestones_ended, "milestone", "milestones"),
    );
    match (&r.review_stopped, r.reminders_cancelled) {
        (Some(_), 0) => out.push_str("; its weekly review was stopped"),
        (Some(_), n) => out.push_str(&format!(
            "; its weekly review and {} were stopped",
            many(n, "reminder", "reminders")
        )),
        (None, 0) => {}
        (None, n) => out.push_str(&format!("; {} stopped", many(n, "reminder", "reminders"))),
    }
    out.push('.');
    out
}

/// What the three answer: the project's title and id, the items (the list's,
/// or after an add or a close, the whole list as it now stands), and the one
/// that changed.
#[cfg_attr(test, derive(ts_rs::TS))]
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ProjectItemsResult {
    pub project: String,
    pub title: String,
    pub items: Vec<TaskRecord>,
    /// The item an add made or a close ended.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub changed: Option<TaskRecord>,
}

/// What an item's add, close and list say, in one place so the CLI, a slash
/// command and a chat tool say the same words (issue 13).
pub fn item_added(project: &str, item: &str) -> String {
    format!("Added to *{project}*: {item}")
}

/// What a close says.
pub fn item_closed(item: &str) -> String {
    format!("Closed: {item}")
}

/// What a list says, a line an item with its short id; "No open items" when
/// there are none.
pub fn item_list(project: &str, items: &[TaskRecord]) -> String {
    if items.is_empty() {
        return format!("No open items in *{project}*");
    }
    let mut out = format!("Items in *{project}*:");
    for t in items {
        let mark = match t.state {
            crate::tasks::TaskState::Done => "✓",
            crate::tasks::TaskState::Abandoned => "✗",
            _ => "•",
        };
        let n = t.id.chars().count();
        let short: String = t.id.chars().skip(n.saturating_sub(6)).collect();
        out.push_str(&format!("\n{mark} {} ({short})", t.title));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// What a close says it will do, and what it did, in counts and kinds.
    #[test]
    fn a_close_says_what_ends_and_what_stops() {
        use crate::tasks::{TaskKind, TaskState};
        let rec = |kind, state, due: Option<u64>| TaskRecord {
            kind,
            state,
            due_at_ms: due,
            ..Default::default()
        };
        let records = [
            rec(TaskKind::Project, TaskState::Accepted, None),
            rec(TaskKind::Item, TaskState::Accepted, None),
            rec(TaskKind::Item, TaskState::Done, None),
            rec(TaskKind::Milestone, TaskState::Accepted, Some(5)),
            rec(TaskKind::Milestone, TaskState::Accepted, None),
        ];
        assert_eq!(close_counts(&records), (1, 2, 1));
        assert_eq!(
            close_card("Harbour", ProjectEnd::Dropped, (1, 2, 1), true),
            "Drop the project \"Harbour\": end 1 open item and 2 open milestones and stop its \
             weekly review and the reminders of 1 dated milestone"
        );
        assert_eq!(
            close_card("Harbour", ProjectEnd::Done, (0, 0, 0), false),
            "Mark done the project \"Harbour\": end 0 open items and 0 open milestones"
        );
        let r = ProjectCloseResult {
            project: ProjectInfo {
                id: "tsk_00harbour".into(),
                ..Default::default()
            },
            how: ProjectEnd::Dropped,
            items_ended: 2,
            milestones_ended: 1,
            review_stopped: Some("ses_00review".into()),
            reminders_cancelled: 2,
        };
        assert_eq!(
            project_closed(&r),
            "Project 00harbour dropped: 2 items and 1 milestone ended; its weekly review and 2 \
             reminders were stopped."
        );
        let quiet = ProjectCloseResult {
            review_stopped: None,
            reminders_cancelled: 0,
            how: ProjectEnd::Done,
            ..r
        };
        assert_eq!(
            project_closed(&quiet),
            "Project 00harbour marked done: 2 items and 1 milestone ended."
        );
        assert_eq!(ProjectEnd::Done.state(), TaskState::Done);
        assert_eq!(ProjectEnd::Dropped.state(), TaskState::Abandoned);
    }

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

/// `project.trust` and `project.untrust`: the repository holding `path`, an
/// absolute path inside it. `author` and `discord` say who asks, as
/// `standing.revoke`'s do.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct ProjectTrustParams {
    pub path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub author: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub discord: Option<crate::DiscordOrigin>,
}

/// `project.trusted`: no params.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct ProjectTrustedParams {}

/// One trusted repository: its id, canonical root, who granted it and when.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct ProjectGrant {
    pub id: String,
    pub root: String,
    pub by: String,
    pub via: String,
    pub at_ms: u64,
}

/// `project.trusted`'s answer.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct ProjectTrustedResult {
    pub trusted: Vec<ProjectGrant>,
}
