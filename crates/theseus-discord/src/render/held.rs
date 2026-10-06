//! The turns a renderer holds, and what goes with a turn it drops
//! (theseus-6809, theseus-whb0). A child of `render`, so it sees `Renderer`'s
//! fields.
//!
//! A renderer holds the last `RECENT_TURNS` turns, so a late event (a
//! background job's result, a parked call approved in a later turn) still
//! finds its tool line. The place's actor tells its lane the turns
//! held (`held`) whenever they change, and the lane keeps their messages'
//! ids out of its recency bound until then.
//!
//! A turn's keys all begin `<turn_id>:`: its stream's `<turn>:L<n>:p<i>` and
//! `<turn>:L<n>:tools`, its reply's `<turn>:footer`, and the notice card of
//! each call whose line it holds, `<turn>:notice:<tool_use_id>`.

use super::{Renderer, RECENT_TURNS};

impl Renderer {
    /// The turns it holds, oldest first: what the place's lane keeps whole.
    pub fn held(&self) -> Vec<String> {
        self.turns.iter().map(|t| t.turn_id.clone()).collect()
    }

    /// Drop the turns past `RECENT_TURNS`, oldest first.
    pub(super) fn drop_past_recent(&mut self) {
        while self.turns.len() > RECENT_TURNS {
            self.turns.pop_front();
        }
    }

    /// A call's notice card's key: under the turn that holds its line, so it
    /// is kept and forgotten with that turn. A call no held turn shows keeps
    /// the lane's recency bound, and its card leaves at the next drop.
    pub(super) fn notice_key(&self, tool_use_id: &str) -> String {
        let holds = |t: &&super::TurnView| {
            t.loops
                .values()
                .any(|lv| lv.tools.iter().any(|l| l.tool_use_id == tool_use_id))
        };
        match self.turns.iter().rev().find(holds) {
            Some(t) => format!("{}:notice:{tool_use_id}", t.turn_id),
            None => format!("notice:{tool_use_id}"),
        }
    }
}
