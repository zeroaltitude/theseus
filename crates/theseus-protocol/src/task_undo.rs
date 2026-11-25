//! `task.undo` (issue 57): the owner takes back a change to their own task's
//! objective or acceptance, or its abandoning, that ran without asking them.
//! The task gets back the record it had before that change, as its next
//! version; a task that has changed since is refused, so a later edit is never
//! lost.

use serde::{Deserialize, Serialize};

use crate::tasks::TaskRecord;

/// `task.undo`'s params.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct TaskUndoParams {
    /// The task: its id, or the end of it.
    pub id: String,
    /// The version the change made, as the note that told of it names it.
    pub version: u64,
    /// Who asked, as the ledger names them. Default: the surface.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub author: Option<String>,
    /// Set by the Discord binding: where the owner acted, so the core judges them.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub discord: Option<crate::DiscordOrigin>,
}

/// `task.undo`'s answer: the task as it is now, its content the one it had
/// before the change.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct TaskUndoResult {
    pub task: TaskRecord,
    /// The version whose content it has again.
    pub restored: u64,
}
