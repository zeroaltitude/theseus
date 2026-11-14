//! A chat act's run (chat-acts-are-tools, S43-1), the harness's side of the
//! three tools the model calls in answer to what the owner said: awaited, as
//! `extend.propose`'s trial is, and dispatched to the board that holds them.

use std::time::Instant;

use anyhow::Result;
use theseus_tools::Tool;

use super::{CallOutcome, ToolRuntime, TurnCtx};
use crate::provider::ToolUse;

impl ToolRuntime {
    /// A chat act (chat-acts-are-tools, S43-1): the owner said something, the
    /// model chose to call one of the three tools about it, and this runs it.
    /// Awaited here, as `extend.propose` is, because the allow tool's second
    /// reading is one Jev call; its result is written as any harness call's.
    ///
    /// **Nothing here reads the text of his message.** The tools were offered
    /// by `chat_acts::Board::offers`, which asks what is WAITING and what is
    /// HELD; the message reaches only `allow_said.v1`, which is a judge's
    /// question and not a matcher, and only after the model has already
    /// chosen.
    pub(super) async fn run_chat_act(
        &self,
        tc: &TurnCtx<'_>,
        correlation_id: &str,
        tool: &dyn Tool,
        call: &ToolUse,
    ) -> Result<CallOutcome> {
        Self::harness_started(tc, correlation_id, tool, call);
        let started = theseus_protocol::now_unix_ms();
        let t0 = Instant::now();
        let acts = &self.chat_acts;
        let done = match tool.name() {
            crate::chat_acts::ANSWER => acts.answer(tc, &call.input),
            crate::chat_acts::ALLOW => acts.allow(tc, &call.input).await,
            crate::chat_acts::SYNC => acts.sync(tc, &call.input),
            other => Err(format!("{other} is not a chat act")),
        };
        let none = crate::task_graph::tools::Done::default();
        self.harness_done(tc, correlation_id, tool, call, (started, t0), (done, none))
    }
}
