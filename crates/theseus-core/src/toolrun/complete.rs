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
        super::flow_result::written(node);
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
        let mut frame = vec![super::parts::record(node)?];
        frame.extend(extra);
        let told = Self::tell_google(tc, node, &mut frame)?;
        let accepted = tc.kernel.accept_completion_with(c, frame)?;
        if let Some(post) = told {
            tc.outbox.posted(&post);
        }
        // A Google result raises its label where it is written
        // (theseus-gyjyo.5), after the frame, since landing takes the
        // session's lock.
        self.label_google(sid, node);
        self.queue_content(sid, std::slice::from_ref(node));
        Ok(accepted)
    }

    /// A told Google act the owner did not approve is told to him once, in
    /// its result's frame (issue 57, `google::told`): never to the session's
    /// own place, which may be shared. One that can be put back carries its
    /// Undo (`owner_note::told`), named by this result node; one with none
    /// names the act, for the owner to look up. A call that failed partway is
    /// told for the part that was done.
    fn tell_google(
        tc: &TurnCtx<'_>,
        node: &Node,
        frame: &mut Vec<theseus_store::NewRecord>,
    ) -> Result<Option<theseus_kernel::Action>> {
        use crate::google::told;
        let crate::node::Body::ToolResult { meta, .. } = &node.body else {
            return Ok(None);
        };
        if !crate::google::private::marked(meta) {
            return Ok(None);
        }
        let Some(p) = told::parts(&meta[told::KEY], tc.session_id) else {
            return Ok(None);
        };
        let (post, more) = match told::undo_before(node) {
            Some(_) => {
                let undo = crate::owner_note::told::Undo::GoogleTakeBack(node.id.clone());
                crate::owner_note::told::stage(tc.outbox, None, &p.title, &p.by, &p.body, &undo)?
            }
            None => {
                let body = format!("{}\nAct: {}", p.body, node.id);
                let text =
                    crate::owner_note::note(crate::owner_note::Kind::Fyi, &p.title, &p.by, &body);
                tc.outbox.stage_to_operator(
                    None,
                    serde_json::json!({"kind": "notice", "text": text, "announce": true}),
                )?
            }
        };
        frame.extend(more);
        Ok(Some(post))
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
}
