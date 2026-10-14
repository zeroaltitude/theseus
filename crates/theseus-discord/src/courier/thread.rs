//! A thread's reply in the owner's DM (theseus-sjzm): the tag it carries, and
//! the record that lets a reply to it address the thread. A child of
//! `courier`.

use serde_json::Value;

use super::Lane;

impl Lane {
    /// The title a reply carries as its first line, and the persona that signs it, when the turn ran in a
    /// thread and this lane is the owner's DM; none in a place that runs the
    /// thread (the place says it). The turn is noted as the thread's, so a
    /// reply to any of its messages addresses the thread
    /// (`Shared::thread_of_reply`).
    pub(super) fn thread_tag(
        &self,
        body: &Value,
        turn_id: &str,
    ) -> Option<(String, Option<String>)> {
        if self.kind != "dm" {
            return None;
        }
        let sid = body["result"]["session_id"].as_str()?;
        let board = self.shared.core.threads();
        let t = board.of_session(sid)?;
        self.shared.replies.thread_turn(turn_id, &t.slug);
        Some((t.title.clone(), t.persona.clone()))
    }
}
