//! The short line's facts, kept as the board's views change (theseus-8kn12).
//!
//! `theseus status --short` runs on every prompt draw. It needs three counts
//! and the first execution that needs you; reading them off a snapshot cost a
//! clone and a sort of every view the board holds. Here each change of a view
//! moves its execution between three sets, so the answer costs the failures
//! (which stay on the board for good) and nothing else.
//!
//! The groups are the CLI's (`status.rs`'s `group`): a needs-you view that
//! failed, a needs-you view that did not, a working view. Any other view is in
//! none of them.

use std::collections::{BTreeMap, BTreeSet};

use theseus_protocol::{ExecutionView, Level, ShortFailed, StatusShortResult};

/// Where a view stands on the short line.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Slot {
    Needs,
    Failed,
    Working,
}

fn slot(v: &ExecutionView) -> Option<Slot> {
    match (v.attention.level, v.state.as_str()) {
        (Level::NeedsYou, "failed") => Some(Slot::Failed),
        (Level::NeedsYou, _) => Some(Slot::Needs),
        (Level::Working, _) => Some(Slot::Working),
        _ => None,
    }
}

/// The three groups: those that need you ordered by how long, the failed with
/// the position of their views, and how many work.
#[derive(Default, Debug, PartialEq, Eq)]
pub(super) struct Short {
    needs: BTreeSet<(u64, String)>,
    failed: BTreeMap<String, (u64, u64)>,
    failed_order: BTreeSet<(u64, String)>,
    working: u32,
}

impl Short {
    /// The groups of these views, counted from scratch: the seed's, and the
    /// oracle the kept ones must equal.
    pub(super) fn of<'a>(views: impl IntoIterator<Item = &'a ExecutionView>) -> Self {
        let mut s = Self::default();
        for v in views {
            s.change(None, Some(v));
        }
        s
    }

    /// An execution's view went from `old` to `new`.
    pub(super) fn change(&mut self, old: Option<&ExecutionView>, new: Option<&ExecutionView>) {
        if let Some(v) = old {
            self.leave(v);
        }
        if let Some(v) = new {
            self.join(v);
        }
    }

    fn leave(&mut self, v: &ExecutionView) {
        match slot(v) {
            Some(Slot::Needs) => {
                self.needs
                    .remove(&(v.attention.since_ms, v.execution_id.clone()));
            }
            Some(Slot::Failed) => {
                if let Some((since, _)) = self.failed.remove(&v.execution_id) {
                    self.failed_order.remove(&(since, v.execution_id.clone()));
                }
            }
            Some(Slot::Working) => self.working = self.working.saturating_sub(1),
            None => {}
        }
    }

    fn join(&mut self, v: &ExecutionView) {
        match slot(v) {
            Some(Slot::Needs) => {
                self.needs
                    .insert((v.attention.since_ms, v.execution_id.clone()));
            }
            Some(Slot::Failed) => {
                self.failed
                    .insert(v.execution_id.clone(), (v.attention.since_ms, v.position));
                self.failed_order
                    .insert((v.attention.since_ms, v.execution_id.clone()));
            }
            Some(Slot::Working) => self.working += 1,
            None => {}
        }
    }

    /// The answer at board position `position`.
    pub(super) fn result(&self, position: u64) -> StatusShortResult {
        StatusShortResult {
            position,
            needs_you: self.needs.len() as u32,
            working: self.working,
            failed: self
                .failed
                .iter()
                .map(|(id, (_, position))| ShortFailed {
                    execution_id: id.clone(),
                    position: *position,
                })
                .collect(),
            first: self
                .needs
                .first()
                .or_else(|| self.failed_order.first())
                .map(|(_, id)| id.clone()),
            next_due_ms: None,
            routines_failed: 0,
        }
    }
}
