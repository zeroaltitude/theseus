//! Setting one item aside (issue 6, the loop's failure handling, condition 6).
//!
//! The owner's ruling: dropping an item must not be the only stop short of the kill switch,
//! because a drop also forbids its whole cause for good. A **set-aside** stops one item and
//! forbids nothing: the pipeline starts nothing for it (it waits for the owner like an item at
//! the keel), the night still files evidence against it, and the owner's `theseus self
//! bring-back` (which is `self.retry`) puts it back in the order. Nothing is dropped from the
//! record and no cause is remembered as dropped.
//!
//! It is a task state the record already has (`waiting_human`) and one evidence entry, so no
//! stored record gains a field.

use anyhow::Result;
use theseus_protocol::tasks::{TaskEvidence, TaskRecord, TaskState};

use super::word::BadItem;
use super::Work;

/// The evidence a set-aside leaves.
pub(crate) const SET_ASIDE: &str = "set-aside:owner";
/// The evidence a retry leaves.
const RETRIED: &str = "retried:owner";

/// Whether the owner set this item aside and has not brought it back.
pub(crate) fn is_set_aside(t: &TaskRecord) -> bool {
    t.state == TaskState::WaitingHuman
        && t.evidence
            .iter()
            .rev()
            .find(|e| e.identity == SET_ASIDE || e.identity == RETRIED)
            .is_some_and(|e| e.identity == SET_ASIDE)
}

impl Work {
    /// Set `id` aside. False when it already is. An item that waits at the keel for the
    /// owner's own yes, or that is closed, cannot be: a set-aside is reversible, and the
    /// owner's `bring-back` would otherwise start what the keel holds.
    pub(super) fn set_aside_item(&mut self, id: &str, by: &str, why: Option<&str>) -> Result<bool> {
        let keel = self.st.items.get(id).and_then(|m| m.keel.clone());
        let Some(t) = self.tasks.get_mut(id) else {
            return Err(BadItem(format!("{id} is not on the backlog")).into());
        };
        if is_set_aside(t) {
            return Ok(false);
        }
        if let Some(class) = keel {
            return Err(BadItem(format!(
                "{id} waits for the owner's own yes ({class}), so it is already not started"
            ))
            .into());
        }
        if !matches!(
            t.state,
            TaskState::Proposed | TaskState::InProgress | TaskState::WaitingHuman
        ) {
            return Err(BadItem(format!(
                "{id} is {}: only an open item can be set aside",
                t.state.as_str()
            ))
            .into());
        }
        let note = why
            .map(str::trim)
            .filter(|w| !w.is_empty())
            .unwrap_or("set aside by the owner");
        t.state = TaskState::WaitingHuman;
        t.version += 1;
        t.updated_at_ms = self.now;
        t.evidence.push(TaskEvidence {
            node: None,
            identity: SET_ASIDE.into(),
            note: Some(note.into()),
            by: by.into(),
            at_ms: self.now,
        });
        if let Some(m) = self.st.items.get_mut(id) {
            m.rank = 0;
        }
        self.touch(id);
        Ok(true)
    }
}
