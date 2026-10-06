//! The inbound point's dispatch (M5 step 25a; `crate::judge::inbound`): a
//! turn whose input is a person's message hands it to the judge once its
//! node is written. The judge marks the turn's trace and spawns the rest;
//! the turn waits on nothing.

use super::{Turn, TurnRunner};
use crate::judge::inbound::{author_of, place_kind, Inbound};
use crate::node::{AttachmentContent, Body, Node};

impl TurnRunner {
    /// `node` is this turn's input, written; `author` the client that sent
    /// it (`sock#3`, `web#1`, or the binding's person). With the judge off,
    /// nothing is read.
    pub(super) fn inbound_point(&self, t: &mut Turn<'_>, node: &Node, author: &str) {
        if !self.judge.config().enabled {
            return;
        }
        let Body::UserMessage { text, attachments } = &node.body else {
            return;
        };
        let class = t.tc.class;
        let place = self.outbox.try_target(t.tc.session_id).ok().flatten();
        // route.v1 (25e): live, or shadow for a turn whose profile the
        // owner chose; its verdict's channel waits for the first compile. A
        // routed session's turn read it at its start (theseus-9yyr).
        let route = t
            .route
            .read
            .take()
            .unwrap_or_else(|| self.route_mode(t.target, t.tc.session_id));
        // A pin after a routed turn counts on route.v1's ladder (26a).
        if t.target.chosen.is_some() {
            self.judge.pinned(t.tc.session_id, &t.target.profile);
        }
        let images = attachments
            .iter()
            .any(|a| matches!(a.content, AttachmentContent::Image { .. }));
        let wait = self.judge.at_inbound(
            &mut t.trace,
            Inbound {
                session_id: t.tc.session_id.into(),
                execution_id: t.tc.execution_id.into(),
                turn_id: t.tc.turn_id.into(),
                node_id: node.id.clone(),
                text: text.clone(),
                place_kind: place_kind(place.as_deref(), author, class),
                author: author_of(class).into(),
                kernel: self.kernel.clone(),
                route,
                chosen: t.target.chosen.clone(),
                task: t.tc.task.is_some(),
            },
        );
        Self::route_asked(t, wait, route, images);
    }
}
