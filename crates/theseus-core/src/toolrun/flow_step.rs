//! A flow's step as the model's own call (theseus-pw1q.21.4): the glue
//! `flow::run` calls. `flow_step` is `admit` and `start` for one `ToolUse`,
//! the path every call of a response takes (`run_batch`), except that a call
//! the gate would make wait is not asked: it comes back to the flow, which
//! hands it to the model. `run_flow` is the flow tool's own call.

use anyhow::Result;
use theseus_tools::Tool;

use super::{Admitted, Call, CallOutcome, ToolRuntime, TurnCtx};
use crate::fact::flow::FlowRan;
use crate::policy::Decision;
use crate::provider::ToolUse;

/// What one step came to.
pub enum Stepped {
    /// The gate would wait for approval: nothing was planned or asked.
    Asks { why: String },
    /// The call was answered at the gate (unknown, invalid, refused) or ran.
    Ran(CallOutcome),
}

impl ToolRuntime {
    /// One step: gated, planned, run and answered exactly as a call of a
    /// model response is, with `assistant_node` the response of the flow's
    /// own call.
    pub(crate) async fn flow_step(
        &self,
        tc: &TurnCtx<'_>,
        assistant_node: &str,
        call: &ToolUse,
    ) -> Result<Stepped> {
        match self.admit(
            tc,
            assistant_node,
            &Call {
                call,
                invalid: None,
            },
        )? {
            Admitted::Answered(o) => Ok(Stepped::Ran(o)),
            Admitted::Asks(_, g) => Ok(Stepped::Asks {
                why: reason(&g.decision),
            }),
            Admitted::Runs(tool, g) => {
                let (outcome, _judged) = self.start(tc, assistant_node, call, tool, g).await?;
                Ok(Stepped::Ran(outcome))
            }
        }
    }

    /// The flow tool's call: its steps, then its result in the call's own
    /// completion frame, and `flow.ran` in the turn's next one.
    pub(crate) async fn run_flow(
        &self,
        tc: &TurnCtx<'_>,
        correlation_id: &str,
        tool: &dyn Tool,
        call: &ToolUse,
    ) -> Result<CallOutcome> {
        Self::harness_started(tc, correlation_id, tool, call);
        let started = theseus_protocol::now_unix_ms();
        let t0 = std::time::Instant::now();
        let name = tool
            .name()
            .strip_prefix(crate::flow::tool::PREFIX)
            .unwrap_or_default();
        let done = match self.flows.get(name) {
            None => Err(format!("no flow named {name} is loaded")),
            Some(entry) => {
                let assistant = crate::flow::run::assistant_of(tc, &call.id);
                let run = crate::flow::run::drive(self, tc, &entry, call, &assistant).await;
                tc.record(&FlowRan {
                    name,
                    digest: &entry.digest,
                    exit: &run.exit,
                    steps: run.steps,
                    secs: t0.elapsed().as_secs_f64(),
                });
                run.result
            }
        };
        self.harness_done(
            tc,
            correlation_id,
            tool,
            call,
            (started, t0),
            (done, crate::task_graph::tools::Done::default()),
        )
    }

    /// Whether the flow may run here: its repositories hold the session's
    /// directory, and its kinds of place include this one. The catalog
    /// offers a flow only here; this is the backstop for a call that arrives
    /// some other way.
    pub(crate) fn flow_scope(
        &self,
        tc: &TurnCtx<'_>,
        spec: &crate::flow::spec::Spec,
    ) -> std::result::Result<(), String> {
        let cwd = self.ctx_in(tc).cwd.clone();
        let kind = crate::tools_defer::PlaceKind::of(
            tc.task.is_some(),
            tc.outbox.target(tc.session_id).as_deref(),
            false,
        );
        crate::flow::tool::in_scope(spec, &cwd, kind)
            .then_some(())
            .ok_or_else(|| {
                format!(
                    "flow {} is not offered in {} (its repositories: {})",
                    spec.name,
                    cwd.display(),
                    spec.scope.repos.join(", ")
                )
            })
    }
}

/// The setting that makes a step wait, as the card would say it.
fn reason(d: &Decision) -> String {
    d.reason.clone()
}
