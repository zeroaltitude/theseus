//! A place's spend limit (step 38a, theseus-ext.3): the bindings file may
//! give a place a lower limit than `[kernel] spend_limit_usd`, and the
//! place's session then has the lower of the two.
//!
//! The binding tells the kernel at each of its starts, after the session is
//! found or opened, so the limit follows either figure when it changes, as
//! theseus-3pj's rule does for the config's alone: the config's changes come
//! with a restart, and the bindings file is read once a start. While a place
//! caps it the execution's limit is pinned (`Budget::pinned`), so the start's
//! own follow (step 2) leaves it to the binding; without a cap it is the
//! config's again, and follows it as any execution does.

use anyhow::Result;

use crate::kernel::{exec_record, Kernel, KernelError, LimitFollowed};
use crate::types::*;

impl Kernel {
    /// Give `execution_id` the lower of the config's limit and `cap`, pinned
    /// while `cap` is set; with none, the config's, followed. One frame with
    /// the execution and its `budget.limit_changed` row (`why: place`); a
    /// raise withdraws the budget question it waits on, as `follow_limit`
    /// does. `None` when nothing changes, or the execution has ended:
    /// nothing is written.
    pub fn place_limit(
        &self,
        execution_id: &str,
        cap: Option<Micros>,
    ) -> Result<Option<LimitFollowed>> {
        self.require_accepting()?;
        let _w = self.lock_family(execution_id)?;
        let mut e = self
            .execution(execution_id)?
            .ok_or_else(|| KernelError::UnknownExecution(execution_id.into()))?;
        let config = self.config().spend_limit_micros;
        let to = cap.map_or(config, |c| c.min(config));
        let pinned = cap.is_some();
        if e.state.is_terminal() || (e.budget.limit_micros == to && e.budget.pinned == pinned) {
            return Ok(None);
        }
        let now = self.now_ms();
        e.budget.pinned = pinned;
        e.updated_at_ms = now;
        if e.budget.limit_micros == to {
            // Only its pin changed: no limit row, since the limit did not.
            self.commit(&[exec_record(&e)?])?;
            return Ok(None);
        }
        let (followed, rows) = self.limit_to(&mut e, to, now, "place")?;
        let mut frame = vec![exec_record(&e)?];
        frame.extend(rows);
        self.commit(&frame)?;
        Ok(Some(followed))
    }
}
