//! A view's `progress` has two sources: the plan steps below it, which the
//! rollup pass counts, and the model's own line, which an execution's view
//! carries from its newest `loop.ended` row's `say` (theseus-xlias). This
//! module joins them without either overwriting the other.

use theseus_protocol::work::{Progress, WorkView};

/// A progress of the model's line alone: no plan steps counted (0 of 0).
pub(super) fn said_only(said: Option<&str>, at_ms: u64) -> Option<Progress> {
    said.map(|s| Progress {
        steps_done: 0,
        steps_total: 0,
        current: None,
        said: Some(s.to_string()),
        at_ms,
    })
}

/// The held view's progress for the view just built: the held steps' count,
/// with the built view's line (`new.progress` holds only that, from the
/// execution's `said`).
pub(super) fn carried(old: Option<&Progress>, new: &WorkView, now: u64) -> Option<Progress> {
    let said = new.progress.as_ref().and_then(|p| p.said.clone());
    match old {
        Some(o) if o.steps_total > 0 => Some(Progress { said, ..o.clone() }),
        Some(o) if o.said == said => Some(o.clone()),
        _ => said_only(said.as_deref(), now),
    }
}
