//! A call's completion frame and the hold's narrative line. Split from
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
    /// with it. A result marked external (DD5) that its session is the first
    /// to read since it was last trusted brings the session's hold in the
    /// same frame, under the session record's lock (theseus-9bp): no crash
    /// leaves the text in the context without the hold.
    pub(super) fn complete(
        &self,
        tc: &TurnCtx<'_>,
        c: &Completion,
        node: &Node,
        extra: Vec<theseus_store::NewRecord>,
    ) -> Result<Accepted> {
        let sid = tc.session_id;
        let (accepted, newly) =
            crate::external::with_hold(tc.store, sid, Some(tc.turn_id), node, |mut frame| {
                frame.extend(extra);
                tc.kernel.accept_completion_with(c, frame)
            })?;
        self.held(tc, newly);
        self.label_google(sid, node);
        self.queue_content(sid, std::slice::from_ref(node));
        Ok(accepted)
    }

    /// A Google result raises its label where it is written (theseus-gyjyo.5):
    /// after the frame that wrote it, since landing takes the session's lock.
    /// Nothing is raised with `session_flags` off (`land_deterministic`).
    fn label_google(&self, session_id: &str, node: &Node) {
        let Some(judge) = self.judge.get() else {
            return;
        };
        let at = theseus_protocol::now_unix_ms();
        if let Some(label) = crate::google::private::label(node, at) {
            judge.land_deterministic(session_id, vec![label]);
        }
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

    /// The narrative's line for a hold a result began.
    pub(super) fn held(&self, tc: &TurnCtx<'_>, newly: Option<theseus_protocol::ExternalText>) {
        if let Some(h) = newly {
            let mode = self.external.mode;
            tc.record(&fact::tool::HoldTaken { hold: &h, mode });
        }
    }
}
