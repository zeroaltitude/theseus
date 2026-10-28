//! `self.propose_change`'s run (theseus-pw1q.8; the work is
//! `crate::rsi::pr`'s): a harness call whose gate runs for minutes, so it is
//! awaited here, as `extend.propose`'s trial is, and its result is written as
//! any harness call's. Like a harness call, a cancel does not cut it short:
//! the kill switch is asked between its phases, and its gate is bounded by
//! `rsi::tree::GATE_SILENCE`.

use std::time::Instant;

use anyhow::Result;
use theseus_tools::Tool;

use super::{CallOutcome, ToolRuntime, TurnCtx};
use crate::provider::ToolUse;

impl ToolRuntime {
    pub(super) async fn run_self_pr(
        &self,
        tc: &TurnCtx<'_>,
        correlation_id: &str,
        tool: &dyn Tool,
        call: &ToolUse,
    ) -> Result<CallOutcome> {
        Self::harness_started(tc, correlation_id, tool, call);
        let started = theseus_protocol::now_unix_ms();
        let t0 = Instant::now();
        let board = self.self_pr.clone();
        let done = crate::rsi::pr::propose(&board, tc, &call.input).await;
        let none = crate::task_graph::tools::Done::default();
        self.harness_done(tc, correlation_id, tool, call, (started, t0), (done, none))
    }
}
