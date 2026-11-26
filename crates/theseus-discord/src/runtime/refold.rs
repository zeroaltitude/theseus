//! A place whose daemon restarted reads its renderer's held turns back from
//! the session's nodes, when an event first names a turn the renderer never saw
//! start (issue 8). The fold is `Renderer::rebuild` (`render/refold.rs`); this is
//! when it is asked for, and what the lane is told after it.

use theseus_protocol::Event as CoreEvent;

use super::{LaneMsg, Place};

impl Place {
    /// Before an event is read: if it names a turn, or a call, this place's
    /// renderer has no line for, and the held turns were never read back, read
    /// them back now and tell the lane which turns it keeps whole.
    pub(super) fn rebuild_held(&mut self, e: &CoreEvent) {
        if matches!(e, CoreEvent::TurnStarted(_)) || !self.renderer.needs_rebuild(e) {
            return;
        }
        self.read_back();
    }

    /// Read the held turns back from the session's nodes, once (a press on
    /// a fold of a turn this place has not seen since it started asks too,
    /// `runtime/fold.rs`), and tell the lane which turns it keeps whole.
    pub(super) fn read_back(&mut self) {
        let nodes = match self.shared.core.store.session_nodes(&self.session_id) {
            Ok(nodes) => nodes,
            Err(err) => {
                tracing::warn!(place = %self.label, error = %format!("{err:#}"), "the held turns were not read back from the session");
                Vec::new()
            }
        };
        self.renderer.rebuild(&nodes);
        let held = self.renderer.held();
        self.view.hold(&held);
        let _ = self.lane.send(LaneMsg::Held(held));
    }
}
