//! A call's completion frame and the hold's narrative line. Split from
//! `toolrun.rs` at rewind's join (theseus-23rk), which gave the frame the
//! records that ride in it.

use anyhow::Result;
use theseus_kernel::{Accepted, Completion};

use super::{ToolRuntime, TurnCtx};
use crate::fact;
use crate::node::Node;

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
                // A Google result makes its session private, in the frame that
                // writes it (the Google design, §4.1).
                frame.extend(crate::google::private::record(node, sid));
                tc.kernel.accept_completion_with(c, frame)
            })?;
        self.held(tc, newly);
        Ok(accepted)
    }

    /// The narrative's line for a hold a result began.
    pub(super) fn held(&self, tc: &TurnCtx<'_>, newly: Option<theseus_protocol::ExternalText>) {
        if let Some(h) = newly {
            let mode = self.external_text;
            tc.record(&fact::tool::HoldTaken { hold: &h, mode });
        }
    }
}
