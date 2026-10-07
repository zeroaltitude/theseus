//! The turns a lane keeps whole (theseus-6809). A child of `courier`.
//!
//! The place's renderer holds its last few turns, and re-renders a tool line
//! in whichever of them has the call: a background job's late result, or a
//! parked call approved in a later turn. Recency alone would forget that
//! line's message id once later turns named enough keys, and past Discord's
//! nonce window the late state would post a second line. So the place's actor
//! tells its lane the turns its renderer holds (`LaneMsg::Held`) whenever they
//! change, and the lane keeps every key of a held turn (each begins
//! `<turn_id>:`) out of the recency bound. When a turn leaves the list, its
//! keys are forgotten from every map; the lane needs nothing else of the
//! renderer.
//!
//! A turn that leaves while a waiting live op still names one of its keys is
//! forgotten once that op is written (or dropped, when Discord is away), so
//! its last state edits its message instead of posting a new one.

use super::Lane;

impl Lane {
    /// The turns the place's renderer holds now, oldest first.
    pub(super) fn hold(&mut self, turns: Vec<String>) {
        let gone: Vec<String> = self
            .held_turns
            .iter()
            .filter(|t| !turns.contains(t))
            .cloned()
            .collect();
        self.released.retain(|t| !turns.contains(t));
        self.held_turns = turns;
        self.released.extend(gone);
        let waits = self
            .live
            .iter()
            .filter_map(|o| o.key())
            .any(|k| of_any(&self.released, k));
        if !waits {
            self.release();
        }
    }

    /// Is `key` a held turn's (or a released one's not yet forgotten)?
    /// At most a few turns' prefixes: one map insert's cost.
    pub(super) fn held_key(&self, key: &str) -> bool {
        of_any(&self.held_turns, key) || of_any(&self.released, key)
    }

    /// Forget the released turns' keys from every map.
    pub(super) fn release(&mut self) {
        if self.released.is_empty() {
            return;
        }
        let gone = std::mem::take(&mut self.released);
        self.msgs.retain(|k, _| !of_any(&gone, k));
        self.sent.retain(|k, _| !of_any(&gone, k));
        self.sealed.retain(|k| !of_any(&gone, k));
        self.touched.retain(|k, _| !of_any(&gone, k));
    }
}

/// `key` begins `<turn>:` for one of `turns`.
fn of_any(turns: &[String], key: &str) -> bool {
    turns.iter().any(|t| {
        key.strip_prefix(t.as_str())
            .is_some_and(|rest| rest.starts_with(':'))
    })
}
