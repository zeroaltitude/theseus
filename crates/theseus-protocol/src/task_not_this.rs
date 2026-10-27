//! `task.not_this`: the owner's one-tap "not this" on a task, most of all one
//! that started itself. It cancels the task, writes the one rolling standing
//! preference that says so (a new one ends the last), and labels the offer
//! that produced the task, when the ledger holds one.

use serde::{Deserialize, Serialize};

use crate::standing::StandingRecord;
use crate::TaskInfo;

/// The `source` of the standing preference `task.not_this` writes: the one
/// live record with it is the rolling one.
pub const SOURCE: &str = "task.not_this";

/// `task.not_this`'s params.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct TaskNotThisParams {
    /// Its id, its execution's id, or the end of either, as `task.cancel` takes it.
    pub task: String,
    /// Why, in the owner's words; it rides the preference.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub reason: Option<String>,
    /// Who asked, as the ledger names them. Default: the surface.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub author: Option<String>,
    /// Set by the Discord binding: where the owner acted, so the core judges them.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub discord: Option<crate::DiscordOrigin>,
}

/// `task.not_this`'s answer.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct TaskNotThisResult {
    /// The task, as `task.cancel` answers it: already ended is left as it was.
    pub task: TaskInfo,
    /// The one rolling preference, as written now.
    pub preference: StandingRecord,
    /// The preference this one ended, if the last "not this" was still live.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub replaced: Option<String>,
    /// The judgment of the offer that produced the task, labelled wrong;
    /// none when the ledger holds no offer for it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub labeled: Option<String>,
}
