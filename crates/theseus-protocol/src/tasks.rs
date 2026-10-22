//! The task record (M7 step 39a, theseus-ext.6; spec §3.5, M7 §2.4): the
//! task graph as a persisted structure. A task is a record of the store's
//! `TASK` kind, with a session (DD7's execution) or none (an item of a
//! plan). Its edits name the `version` they read (compare-and-swap); its
//! objective and acceptance are the operator's (layer 1: a change is a
//! proposal), its plan the agent's (layer 2), and its evidence only grows
//! (layer 3). These are the record as it is stored and as the protocol shows
//! it: `task.list`'s `records` and `task.changed`.

use serde::{Deserialize, Serialize};

/// A task's state (§3.5's machine). A task with a session shows its
/// execution's running state when read (`in_progress`, `waiting_human`,
/// `suspended`); its record is written at its own changes only.
#[cfg_attr(test, derive(ts_rs::TS))]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskState {
    Proposed,
    #[default]
    Accepted,
    InProgress,
    Blocked,
    WaitingHuman,
    Suspended,
    Done,
    Failed,
    Abandoned,
}

impl TaskState {
    pub const ALL: [TaskState; 9] = [
        TaskState::Proposed,
        TaskState::Accepted,
        TaskState::InProgress,
        TaskState::Blocked,
        TaskState::WaitingHuman,
        TaskState::Suspended,
        TaskState::Done,
        TaskState::Failed,
        TaskState::Abandoned,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            TaskState::Proposed => "proposed",
            TaskState::Accepted => "accepted",
            TaskState::InProgress => "in_progress",
            TaskState::Blocked => "blocked",
            TaskState::WaitingHuman => "waiting_human",
            TaskState::Suspended => "suspended",
            TaskState::Done => "done",
            TaskState::Failed => "failed",
            TaskState::Abandoned => "abandoned",
        }
    }

    /// Done, failed, or abandoned: nothing changes it again but evidence.
    pub fn is_closed(self) -> bool {
        matches!(
            self,
            TaskState::Done | TaskState::Failed | TaskState::Abandoned
        )
    }
}

/// What a task is in the long run (theseus-n5ui2, store format 45): an
/// `item` (every task before it), a `project` (a goal with milestones and a
/// weekly review), or a `milestone` (a dated step of a project).
#[cfg_attr(test, derive(ts_rs::TS))]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskKind {
    #[default]
    Item,
    Project,
    Milestone,
}

impl TaskKind {
    pub fn as_str(self) -> &'static str {
        match self {
            TaskKind::Item => "item",
            TaskKind::Project => "project",
            TaskKind::Milestone => "milestone",
        }
    }

    pub fn parse(s: &str) -> Option<TaskKind> {
        match s.trim() {
            "item" => Some(TaskKind::Item),
            "project" => Some(TaskKind::Project),
            "milestone" => Some(TaskKind::Milestone),
            _ => None,
        }
    }

    /// For `skip_serializing_if`: an item is what a record without a kind is.
    pub fn is_item(&self) -> bool {
        *self == TaskKind::Item
    }
}

/// Who asked for a task: the session its creating call ran in, and that
/// session's principal (the requester a layer-1 change waits for).
#[cfg_attr(test, derive(ts_rs::TS))]
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TaskOrigin {
    pub session: String,
    pub principal: String,
    /// The model wrote its objective and acceptance (theseus-ext.10): a plan
    /// item (`task.create` without a brief) or a split's child, so a change
    /// to its layer 1 applies at once. Absent, it is the owner's and a change
    /// waits: a task session `task.create` opened with a brief and its
    /// arrangement, and every record written before this field.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub by_model: bool,
}

/// One entry of a task's evidence (layer 3): a node and its identity (a
/// commit, a job id, a snapshot id). Nothing removes one.
#[cfg_attr(test, derive(ts_rs::TS))]
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TaskEvidence {
    /// The node that holds it: a report, or the reply that closed the task.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub node: Option<String>,
    /// `commit:<sha>`, `job:<id>`, `report:<task>`, or the model's own words.
    pub identity: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub note: Option<String>,
    /// Who added it: a session, or `report`.
    pub by: String,
    pub at_ms: u64,
}

/// A layer-1 change waiting for the requester, else the owner (§2.4): it
/// asks through the gate as a call that always waits, and its question is
/// `card`. Accept applies it in one frame; decline leaves the task.
#[cfg_attr(test, derive(ts_rs::TS))]
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TaskProposal {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub objective: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub acceptance: Option<Vec<String>>,
    /// It abandons the task ("abandoning is not completing").
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub abandon: bool,
    /// The session that proposed it.
    pub by: String,
    /// The question that asks it (an action's correlation id).
    pub card: String,
    /// The version it was proposed against: accept applies it only there.
    pub base_version: u64,
    pub at_ms: u64,
}

/// A claim's lease on a task (39b, M7 §2.4): the execution that holds it,
/// its session (how a refusal names it), and when it lapses. The holder's own
/// edits renew it; a close ends it; past `until_ms` it reads free, and the
/// due pass clears it (`task.lease_expired`).
#[cfg_attr(test, derive(ts_rs::TS))]
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TaskClaim {
    /// `exe_…`.
    pub by: String,
    pub session: String,
    pub until_ms: u64,
}

impl TaskClaim {
    /// Whether it still holds at `now_ms`: free at `until_ms`, not before.
    pub fn holds_at(&self, now_ms: u64) -> bool {
        now_ms < self.until_ms
    }

    /// How people name its session: the last six characters of its id.
    pub fn session_short(&self) -> &str {
        let n = self.session.len();
        self.session
            .get(n.saturating_sub(6)..)
            .unwrap_or(&self.session)
    }
}

/// A task (§2.4).
#[cfg_attr(test, derive(ts_rs::TS))]
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TaskRecord {
    /// `tsk_…`.
    pub id: String,
    /// Moves by one at every change to it; an edit names the one it read.
    pub version: u64,
    pub title: String,
    /// Layer 1: what it is for, the operator's.
    #[serde(default)]
    pub objective: String,
    /// Layer 1: how to know it is done, one line each.
    #[serde(default)]
    pub acceptance: Vec<String>,
    pub state: TaskState,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub parent: Option<String>,
    /// Tasks it waits on.
    #[serde(default)]
    pub deps: Vec<String>,
    /// `agent`, or a principal.
    pub owner: String,
    /// The task's own session, when it delegates (DD7).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub session: Option<String>,
    pub origin: TaskOrigin,
    /// Layer 3: append-only.
    #[serde(default)]
    pub evidence: Vec<TaskEvidence>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub proposal: Option<TaskProposal>,
    /// The lease one execution holds on it (39b; format 19).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub claim: Option<TaskClaim>,
    /// `item` (absent), `project`, or `milestone` (store format 35).
    #[serde(default, skip_serializing_if = "TaskKind::is_item")]
    pub kind: TaskKind,
    /// When it is due (a milestone's date; a project's end), unix ms.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub due_at_ms: Option<u64>,
    /// A project's weekly review: the id of its routine (`ses_…`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub review: Option<String>,
    pub created_at_ms: u64,
    pub updated_at_ms: u64,
}

impl TaskRecord {
    /// Whether its objective, acceptance, and abandoning are the owner's
    /// (layer 1, theseus-ext.10): a change waits for the operator's yes.
    pub fn is_owners(&self) -> bool {
        !self.origin.by_model
    }

    /// How people name it: the last six characters of its id.
    pub fn short(&self) -> &str {
        let n = self.id.len();
        self.id.get(n.saturating_sub(6)..).unwrap_or(&self.id)
    }

    /// Its claim, while it holds at `now_ms`.
    pub fn claim_at(&self, now_ms: u64) -> Option<&TaskClaim> {
        self.claim.as_ref().filter(|c| c.holds_at(now_ms))
    }
}

/// The layer-1 change a question asks (39b): the task, its title, the field,
/// and the field before and after; or abandoning it (`field` `abandon`, its
/// state before). The card on Discord, `theseus answer`, and the cockpit
/// word it the one way (`question`).
#[cfg_attr(test, derive(ts_rs::TS))]
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TaskChange {
    pub task: String,
    pub title: String,
    /// `objective`, `acceptance`, `objective and acceptance`, or `abandon`.
    pub field: String,
    pub before: String,
    pub after: String,
}

impl TaskChange {
    /// `Change the acceptance of tsk_… (title)? Before: … After: …`, or
    /// `Abandon tsk_… (title)? Before: accepted After: abandoned`.
    pub fn question(&self) -> String {
        let (before, after) = (or_none(&self.before), or_none(&self.after));
        match self.field.as_str() {
            "abandon" => format!(
                "Abandon {} ({})? Before: {before} After: {after}",
                self.task, self.title
            ),
            f => format!(
                "Change the {f} of {} ({})? Before: {before} After: {after}",
                self.task, self.title
            ),
        }
    }
}

fn or_none(s: &str) -> &str {
    if s.trim().is_empty() {
        "(none)"
    } else {
        s
    }
}

/// `task.list`: every task, the newest first, or only one session's, or
/// only those that report to one place. The filters below are all absent by
/// default, so a call without them answers what it always did.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct TaskListParams {
    /// Only the tasks this session started.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub session_id: Option<String>,
    /// Only the tasks that report to this place (`discord:dm:<user>`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub target: Option<String>,
    /// Only the tasks that have ended (`true`) or have not (`false`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub ended: Option<bool>,
    /// Only the tasks that ended, or for an open one were updated, at or
    /// after this time (unix ms).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub since_ms: Option<u64>,
    /// Words, in any case, that must all appear in a task's title, its
    /// objective (the brief) or its report's first line.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub query: Option<String>,
    /// One task, by its id or the end of it (at least four characters),
    /// with its children and its whole report.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub id: Option<String>,
    /// The newest this many.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub limit: Option<u32>,
}

impl TaskListParams {
    /// Whether any filter beyond the session and the place is set.
    pub fn narrows(&self) -> bool {
        self.ended.is_some()
            || self.since_ms.is_some()
            || self.query.is_some()
            || self.id.is_some()
            || self.limit.is_some()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct TaskListResult {
    pub tasks: Vec<crate::TaskInfo>,
    /// The task records (39a), each task once: its graph, sessions' and plan items'.
    #[serde(default)]
    pub records: Vec<TaskRecord>,
    /// What a narrowed read adds about the tasks it names, by task id: the
    /// report's first line (for `ended`, `query` and `id`), and for `id`
    /// the whole report and whether the task is held. Empty otherwise.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub details: Vec<TaskDetail>,
}

/// A task's report and hold, beside its `TaskInfo` (`task.list` with a filter).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct TaskDetail {
    /// Its session's id, as `TaskInfo.task_id`.
    pub task_id: String,
    /// The first line of its report; for a task that failed or was stopped,
    /// why it ended.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub report_line: Option<String>,
    /// The whole report (only when the read named the task by `id`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub report: Option<String>,
    /// Held by `task.pause` (only when the read named the task by `id`).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub paused: bool,
}

/// `task.changed`: a task's record after a change, with the verb that made
/// it (`created`, `updated`, `split`, `closed`, `claimed`, `lease_expired`,
/// `change_proposed`, `change_accepted`, `change_declined`, `change_expired`).
#[cfg_attr(test, derive(ts_rs::TS))]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskChanged {
    /// The session whose call or answer changed it.
    pub session_id: String,
    pub verb: String,
    pub task: TaskRecord,
    /// The turn whose call made the change, when a turn did: a chat follows
    /// a thread's session and shows only the changes of its own turns.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub turn_id: Option<String>,
}

/// What a turn's request showed of the task graph (`context.compiled`):
/// absent when its scope has no task.
#[cfg_attr(test, derive(ts_rs::TS))]
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TaskViewSummary {
    /// The view's text's digest.
    pub digest: String,
    pub open: u32,
    pub closed: u32,
    /// Lines shown, and the tasks left out past the bound.
    pub lines: u32,
    pub left_out: u32,
    /// Its size, estimated.
    pub tokens: u64,
    /// In a check's view, the lines shown as id, title, and state alone: the
    /// checked task, every task under it, and any record of an excluded
    /// session (theseus-w8ys).
    #[serde(default, skip_serializing_if = "is_zero")]
    pub restricted: u32,
}

fn is_zero(n: &u32) -> bool {
    *n == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The card's words (39b): a field's change, before and after, and an
    /// abandon; an empty side reads `(none)`.
    #[test]
    fn a_changes_question_says_the_field_before_and_after() {
        let mut c = TaskChange {
            task: "tsk_0000reef".into(),
            title: "Chart the reef".into(),
            field: "acceptance".into(),
            before: "every marker has a depth".into(),
            after: "every marker has a depth; the chart is signed".into(),
        };
        assert_eq!(
            c.question(),
            "Change the acceptance of tsk_0000reef (Chart the reef)? Before: every marker has \
             a depth After: every marker has a depth; the chart is signed"
        );
        c.field = "abandon".into();
        c.before = "accepted".into();
        c.after = "abandoned".into();
        assert_eq!(
            c.question(),
            "Abandon tsk_0000reef (Chart the reef)? Before: accepted After: abandoned"
        );
        c.before = String::new();
        assert!(c.question().contains("Before: (none)"));
    }
}
