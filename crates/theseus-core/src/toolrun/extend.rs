//! `extend.propose`'s run (M7 43a; the work is `crate::extend`'s), the
//! harness's side of a proposed extension's trial: awaited, and settled by
//! the cancel that aborted it when one did.

use std::time::Instant;

use anyhow::Result;
use theseus_tools::Tool;

use super::{CallOutcome, ResultNode, ToolRuntime, TurnCtx};
use crate::provider::ToolUse;

impl ToolRuntime {
    /// `extend.propose` (M7 43a): a harness call whose trial waits on its
    /// server, so it is awaited here, as an async tool is, and never on a
    /// core; its result is written as any harness call's.
    pub(super) async fn run_extend(
        &self,
        tc: &TurnCtx<'_>,
        correlation_id: &str,
        tool: &dyn Tool,
        call: &ToolUse,
    ) -> Result<CallOutcome> {
        Self::harness_started(tc, correlation_id, tool, call);
        let started = theseus_protocol::now_unix_ms();
        let t0 = Instant::now();
        let done = crate::extend::propose(self, tc, &call.input, correlation_id).await;
        // A trial a cancel or a stop aborted (18a): the cancel settles the
        // call, with its verdict, and its result says so, riding as a late
        // one does.
        if matches!(&done, Err(e) if e == crate::extend::STOPPED) {
            if let Some(a) = crate::cancel::after_abort(tc.kernel, correlation_id).await {
                let (status, text, meta) = crate::cancel::aborted_result(&a);
                let node = self.result_node(
                    tc,
                    ResultNode {
                        correlation_id: Some(correlation_id),
                        duration_ms: Some(t0.elapsed().as_millis() as u64),
                        meta,
                        ..ResultNode::new(&call.id, tool.name(), status, text)
                    },
                );
                tc.store.append(&[node.record()?])?;
                Self::announce_end(tc, &node);
                return Ok(CallOutcome::Done { status });
            }
        }
        let none = crate::task_graph::tools::Done::default();
        self.harness_done(tc, correlation_id, tool, call, (started, t0), (done, none))
    }
}
