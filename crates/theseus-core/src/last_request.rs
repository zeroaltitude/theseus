//! What a turn's last request leaves behind in memory, for the turn's own later steps (issue 44: they lived
//! beside the keep-warm read until that read was deleted):
//!
//! - **The room a task wait records**: when a response asks for a task wait, its request's tokens and window,
//!   which bound the fan-in of the reports that wait collects (`task_wait`). A turn's end drops it.
//! - **The model and effort each session's last request carried**, and whether the sticky rule held that
//!   effort (theseus-rwpbg.4, theseus-u5a37), which routing reads (`turn::effort_step`). A restart forgets it,
//!   and the first request after applies Jev's level as asked.
//!
//! Nothing here is stored, and nothing here sends anything.

use std::collections::HashMap;
use std::sync::Mutex;

use crate::provider::ToolUse;

/// The rooms of task waits, and each session's last request's model and effort.
#[derive(Default)]
pub struct LastRequest {
    /// The tokens and the window of each session's last request that called a task wait.
    rooms: Mutex<HashMap<String, (u64, u64)>>,
    /// The model and effort each session's last request carried (`sent`).
    sent: Mutex<HashMap<String, Sent>>,
}

/// A session's last request: its model, its effort, and whether the sticky rule held the effort.
type Sent = (String, Option<crate::config::Effort>, bool);

impl LastRequest {
    /// Record the size of `session`'s request, and the window it fits.
    pub(crate) fn set_room(&self, session: &str, tokens: u64, limit: u64) {
        self.rooms
            .lock()
            .unwrap()
            .insert(session.to_string(), (tokens, limit));
    }

    /// The tokens and the window of `session`'s last request that called a task wait, when it did.
    pub(crate) fn room(&self, session: &str) -> Option<(u64, u64)> {
        self.rooms.lock().unwrap().get(session).copied()
    }

    /// A response is about to run its calls: when one is a task wait, record the room its request leaves
    /// before the clearing line, which bounds the fan-in of that wait (whichever model answered it).
    pub(crate) fn record_wait_room(
        &self,
        session: &str,
        uses: &[ToolUse],
        compiled: &crate::compiler::Compiled,
    ) {
        if uses
            .iter()
            .any(|u| crate::task_wait::is_task_wait_wire(&u.name))
        {
            self.set_room(
                session,
                compiled.estimate.tokens,
                compiled.budget.limit_tokens,
            );
        }
    }

    /// A turn of `session_id` ended: the room its task wait recorded goes.
    pub fn turn_ended(&self, session_id: &str) {
        self.rooms.lock().unwrap().remove(session_id);
    }

    /// A real request of `session_id` went out on `model` at `effort`: the sticky rule holds to it.
    pub fn sent(
        &self,
        session_id: &str,
        model: &str,
        effort: Option<crate::config::Effort>,
        held: bool,
    ) {
        self.sent
            .lock()
            .unwrap()
            .insert(session_id.to_string(), (model.to_string(), effort, held));
    }

    /// The effort `session_id`'s last request carried, if it went to `model`.
    pub fn last_effort(&self, session_id: &str, model: &str) -> Option<crate::config::Effort> {
        let sent = self.sent.lock().unwrap();
        sent.get(session_id)
            .filter(|(m, ..)| m == model)
            .and_then(|(_, e, _)| *e)
    }

    /// Whether the sticky rule held the effort `session_id`'s last request carried, if it went to `model`.
    pub fn last_hold(&self, session_id: &str, model: &str) -> bool {
        let sent = self.sent.lock().unwrap();
        sent.get(session_id)
            .is_some_and(|(m, _, held)| m == model && *held)
    }
}

#[cfg(test)]
mod tests {
    /// A turn's end drops the room it recorded for a task wait, so a session's entry does not outlive its
    /// turn (the stack review's N10).
    #[test]
    fn a_turns_end_forgets_the_room_a_wait_recorded() {
        let k = super::LastRequest::default();
        k.set_room("ses_room", 40_000, 200_000);
        assert_eq!(k.room("ses_room"), Some((40_000, 200_000)));
        k.turn_ended("ses_room");
        assert_eq!(k.room("ses_room"), None);
    }
}
