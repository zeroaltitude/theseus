//! The input node, with the surface it was written from (theseus-slvl.3):
//! the code the requester's connection gave the sink (`surface::code_of`),
//! recorded on the node beside its author. The compiler renders it as the
//! tag at the message's head; the node's text stays as typed.

use super::Turn;
use crate::node::{Attachment, Node};

impl Turn<'_> {
    /// The person's message of this turn, as the turn writes it.
    pub(super) fn user_node(&self, author: &str, text: &str, files: Vec<Attachment>) -> Node {
        let mut node = Node::user_with(
            self.tc.session_id,
            Some(self.tc.turn_id),
            author,
            text,
            files,
        );
        node.surface.clone_from(&self.tc.sink.surface);
        node
    }
}
