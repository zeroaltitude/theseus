//! A task's arrangement (M5 step 27, theseus-vug.2): the messages its
//! parent quoted when it started the task, as a task's surfaces show them
//! (`task.list`, `theseus tasks`, the cockpit's task view). The pieces' text
//! stays in the task's session, in its arrangement node; these name them.

use serde::{Deserialize, Serialize};

/// A task's arrangement, as `TaskInfo` carries it.
#[cfg_attr(test, derive(ts_rs::TS))]
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TaskArrangement {
    /// The node in the task's session that holds the pieces.
    pub node_id: String,
    /// Every piece, in the order the call gave them.
    pub pieces: Vec<ArrangementPiece>,
    /// The call said `fidelity_ack: true`: a one-line brief from a long
    /// discussion, with a single piece, started anyway.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub fidelity_ack: bool,
}

/// One piece of an arrangement: a message of the parent's session, by
/// reference.
#[cfg_attr(test, derive(ts_rs::TS))]
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArrangementPiece {
    /// Its index in the call's `pieces`, from 0.
    pub index: u32,
    /// `objective`, `acceptance`, `design`, or `context`.
    pub role: String,
    /// The node it quotes, and that node's session (the parent's).
    pub node_id: String,
    pub session_id: String,
    /// Who wrote the node: `operator`, `agent`, `tool`, or `harness`, and
    /// the client or principal that did, when known.
    pub origin: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub author: Option<String>,
    pub at_ms: u64,
    /// The node's first line, for a list.
    pub first_line: String,
    /// The call marked it trusted testimony.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub trusted: bool,
    /// A later piece supersedes it: the child sees it by reference only.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub superseded_by: Option<u32>,
}
