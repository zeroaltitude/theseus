//! A call's completion frame. Split from
//! `toolrun.rs` at rewind's join (theseus-23rk), which gave the frame the
//! records that ride in it.

use anyhow::Result;
use theseus_kernel::{Accepted, Completion};

use super::{ToolRuntime, TurnCtx};
use crate::fact;
use crate::node::Node;

impl ToolRuntime {
    /// A result node is written: its `tool.ended` row, and the turn told that
    /// the node is in its transcript. Six modules end a call this way; it
    /// lives here, beside the completion frame it follows, rather than in
    /// `toolrun.rs`, which is at its shape ceiling.
    pub(super) fn announce_end(tc: &TurnCtx<'_>, node: &Node) {
        tc.record(&fact::tool::ToolEnded {
            session_id: tc.session_id,
            turn_id: tc.turn_id,
            node,
        });
        tc.node_written(node);
    }
}

impl ToolRuntime {
    /// An in-process result's completion frame, with the records that ride
    /// in it (an edit's pre-images, theseus-23rk), and what the kernel did
    /// with it. A result marked external (DD5) is written as any other: the
    /// marker rides on the node, and holds nothing.
    pub(super) fn complete(
        &self,
        tc: &TurnCtx<'_>,
        c: &Completion,
        node: &Node,
        extra: Vec<theseus_store::NewRecord>,
    ) -> Result<Accepted> {
        let sid = tc.session_id;
        let mut frame = vec![node.record()?];
        frame.extend(extra);
        // A Google result makes its session private, in the frame that
        // writes it (the Google design, §4.1).
        frame.extend(crate::google::private::record(node, sid));
        let accepted = tc.kernel.accept_completion_with(c, frame)?;
        self.queue_content(sid, std::slice::from_ref(node));
        Ok(accepted)
    }

    /// Queue result nodes just written for content judgment (theseus-9ymz0):
    /// every file read, fetch, search and program output reaches a session
    /// through a site that calls this, so each is judged once, by itself.
    /// Nothing waits on it: the judge asks off the turn's path and the verdict
    /// lands between turns. Results written together are queued together, so
    /// one loop's make one decision point.
    pub(super) fn queue_content(&self, session_id: &str, nodes: &[Node]) {
        let Some(judge) = self.judge.get() else {
            return;
        };
        let items: Vec<_> = nodes
            .iter()
            .filter_map(crate::judge::content::ContentItem::of_result)
            .collect();
        if !items.is_empty() {
            judge.ask_content(session_id, items);
        }
    }
}
