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
//!
//! A turn that leaves while an open post of the lane names it (`turn_id` in
//! the post's body) is kept the same way, until the post settles
//! (theseus-llo6). A reply staged before an outage, or before a `/new`, is
//! delivered after the renderer has dropped its turn: without the stream's
//! ids it would create every part again, replying to the anchor, and past
//! Discord's nonce window the text would show twice.

use theseus_core::outbox::body_of;

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
        self.release_unless_waited();
    }

    /// `release`, unless a waiting live op still names a released turn's key.
    pub(super) fn release_unless_waited(&mut self) {
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

    /// Forget the released turns' keys from every map, but the keys of a turn
    /// an open post of this lane names: they stay released until it settles.
    pub(super) fn release(&mut self) {
        if self.released.is_empty() {
            return;
        }
        let posted = self.posted_turns();
        let (kept, gone): (Vec<String>, Vec<String>) = std::mem::take(&mut self.released)
            .into_iter()
            .partition(|t| posted.contains(t));
        self.released = kept;
        self.msgs.retain(|k, _| !of_any(&gone, k));
        self.sent.retain(|k, _| !of_any(&gone, k));
        self.sealed.retain(|k| !of_any(&gone, k));
        self.touched.retain(|k, _| !of_any(&gone, k));
    }

    /// The turns the lane's open posts name, when a turn has been released.
    /// One read of the lane's open posts, only while a turn waits to be
    /// forgotten.
    fn posted_turns(&self) -> Vec<String> {
        self.shared
            .core
            .outbox
            .open_for(&self.target)
            .iter()
            .filter_map(|a| body_of(a)["turn_id"].as_str().map(str::to_string))
            .collect()
    }
}

/// `key` begins `<turn>:` for one of `turns`.
fn of_any(turns: &[String], key: &str) -> bool {
    turns.iter().any(|t| {
        key.strip_prefix(t.as_str())
            .is_some_and(|rest| rest.starts_with(':'))
    })
}
