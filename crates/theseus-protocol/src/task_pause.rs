//! `task.pause` and `task.resume`: the owner holds a running task at its next
//! loop and lets it go on. A paused task parks without reporting; resume wakes
//! it. Both take the same params (the Discord binding sets `discord` for a
//! reaction, as for `task.retry`).

use serde::{Deserialize, Serialize};

/// Where a held task waits: a due time that never comes (the year 2100). The
/// kernel queues a task left waiting on input with nothing to wait for, so a
/// task the owner holds waits on this instead; `task.resume` wakes it. A
/// surface that words a due time says "paused" for it.
pub const PAUSED_AT_MS: u64 = 4_102_444_800_000;

/// `task.pause`'s and `task.resume`'s params.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct TaskPauseParams {
    /// Its id, its execution's id, or the end of either, as `task.cancel` takes it.
    pub task: String,
    /// Who asked, as the ledger names them. Default: the surface.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub author: Option<String>,
    /// Set by the Discord binding: where the owner acted, so the core judges them.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub discord: Option<crate::DiscordOrigin>,
}

/// `task.pause`'s and `task.resume`'s answer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct TaskPauseResult {
    /// The task's session id.
    pub task_id: String,
    /// Whether the task is held now.
    pub paused: bool,
    /// False when it was in that state already (nothing is written).
    pub changed: bool,
}
