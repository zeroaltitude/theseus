//! The inbound point's dispatch (M5 step 25a; `crate::judge::inbound`): a
//! turn whose input is a person's message hands it to the judge once its
//! node is written. The judge marks the turn's trace and spawns the rest;
//! the turn waits on nothing.

use super::{Turn, TurnRunner};
use crate::judge::inbound::{author_of, place_kind, Inbound};
use crate::node::{Body, Node};

impl TurnRunner {
    /// `node` is this turn's input, written; `author` the client that sent
    /// it (`sock#3`, `web#1`, or the binding's person). With the judge off,
    /// nothing is read.
    pub(super) fn inbound_point(&self, t: &mut Turn<'_>, node: &Node, author: &str) {
        if !self.judge.config().enabled {
            return;
        }
        let Body::UserMessage { text, .. } = &node.body else {
            return;
        };
        let class = t.tc.class;
        let place = self.outbox.try_target(t.tc.session_id).ok().flatten();
        self.judge.at_inbound(
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
            },
        );
    }
}
