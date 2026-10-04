//! The task record (M7 step 39a, theseus-ext.6; spec §3.5, M7 §2.4): the
//! task graph as a persisted structure. A task is a record of the store's
//! `TASK` kind, with a session (DD7's execution) or none (an item of a
//! plan). Its edits name the `version` they read (compare-and-swap); its
//! objective and acceptance are the operator's (layer 1: a change is a
//! proposal), its plan the agent's (layer 2), and its evidence only grows
//! (layer 3). These are the record as it is stored and as the protocol shows
//! it: `task.list`'s `records`, `task.get`, and `task.changed`.

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

/// A task (§2.4). `claim` is 39b's.
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
}

/// `task.get`: one task's record, by its id or the end of it.
#[cfg_attr(test, derive(ts_rs::TS))]
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TaskGetParams {
    pub id: String,
}

#[cfg_attr(test, derive(ts_rs::TS))]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskGetResult {
    pub task: TaskRecord,
    /// Its children, in the order they were made.
    #[serde(default)]
    pub children: Vec<TaskRecord>,
}

/// `task.changed`: a task's record after a change, with the verb that made
/// it (`created`, `updated`, `split`, `closed`, `change_proposed`,
/// `change_accepted`, `change_declined`, `change_expired`).
#[cfg_attr(test, derive(ts_rs::TS))]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskChanged {
    /// The session whose call or answer changed it.
    pub session_id: String,
    pub verb: String,
    pub task: TaskRecord,
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
}
