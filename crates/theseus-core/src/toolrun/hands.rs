//! `aws.hands.run`'s call (AWS design §3.3): the group's frame, its first
//! wave, and its answer, `background` until the group settles; then its
//! late result, the aggregate (`hands_result`). The group's logic is
//! `aws::hands::group`'s; this is the turn's side of it.

use anyhow::Result;
use serde_json::{json, Value};
use theseus_kernel::{Action, ActionState};
use theseus_tools::Tool;

use super::{CallOutcome, ResultNode, ToolRuntime, TurnCtx};
use crate::aws::hands::group;
use crate::aws::hands::{launch, start, OverBudget};
use crate::fact;
use crate::node::ResultStatus;
use crate::provider::ToolUse;

impl ToolRuntime {
    /// The budget question a hands group of `execution_id`'s left its turn
    /// to ask (step 40 part 2: the turn asks it as a model call's).
    pub(crate) fn hands_over_budget(&self, execution_id: &str) -> Option<OverBudget> {
        self.aws
            .as_ref()
            .and_then(|a| a.hands.take_over_budget(execution_id))
    }

    /// Start a group: its record and every hand's action in one frame (the
    /// first wave dispatched), then that wave's launch. It answers
    /// `background`, unless the group is already done (every launch failed,
    /// say), when it answers with the aggregate. The group's start is
    /// `aws::hands::start`'s, which an extension's broker shares.
    pub(super) async fn run_hands(
        &self,
        tc: &TurnCtx<'_>,
        correlation_id: &str,
        tool: &dyn Tool,
        call: &ToolUse,
    ) -> Result<CallOutcome> {
        tc.record(&fact::tool::ToolStarted {
            session_id: tc.session_id,
            turn_id: tc.turn_id,
            tool_use_id: &call.id,
            tool: tool.name(),
            correlation_id,
            backend: tool.backend().as_str(),
        });
        let fail = |why: &str| self.settle_job_failure(tc, correlation_id, tool.name(), call, why);
        let Some(aws) = self.aws.clone() else {
            return fail("no AWS account is bound ([aws.accounts])");
        };
        let r = match launch::parse(&call.input) {
            Ok(r) => r,
            Err(e) => return fail(&e),
        };
        let backend = match launch::choose(&r) {
            Ok(b) => b,
            Err(e) => return fail(&e),
        };
        let n = r.inputs.len();
        let over = |need: u64| Self::hands_over(tc, &aws, n, need);
        let starter = start::Starter {
            kernel: tc.kernel,
            store: tc.store,
            outbox: tc.outbox,
            rec: tc.rec(),
            guard: tc.guard,
            execution_id: tc.execution_id,
            session_id: tc.session_id,
            extension: None,
        };
        let planned =
            match start::plan(&starter, &aws, correlation_id, &call.id, r, backend, &over).await? {
                Ok(p) => p,
                Err(why) => return fail(&why),
            };
        let wave = planned.wave.len();
        let ctx = group::Ctx {
            kernel: tc.kernel,
            store: tc.store,
            aws: &aws,
        };
        start::launch_first(&ctx, planned).await;
        let a = tc.kernel.action(correlation_id)?;
        if let Some(a) = a.filter(|a| a.state.is_settled()) {
            let r = Self::hands_result(tc.store, &a, &call.id, tool.name());
            let status = self.answer(tc, r)?;
            return Ok(CallOutcome::Done { status });
        }
        self.answer(
            tc,
            ResultNode {
                correlation_id: Some(correlation_id),
                meta: json!({"hands": n, "backend": backend.as_str()}),
                ..ResultNode::new(
                    &call.id,
                    tool.name(),
                    ResultStatus::Background,
                    format!(
                        "Started a group of {n} hand{} on {} as background group {correlation_id} \
                         ({} launched now). Its result, each hand's outcome, arrives in a later \
                         message; you can keep working or tell the operator you are waiting.",
                        if n == 1 { "" } else { "s" },
                        backend.as_str(),
                        wave
                    ),
                )
            },
        )?;
        Ok(CallOutcome::Background {
            correlation_id: correlation_id.into(),
        })
    }

    /// A group over the session's budget is not run, and its turn asks the
    /// budget question, as a model call does (part 2): its words, or `None`
    /// when the group's worst case `need` fits.
    fn hands_over(
        tc: &TurnCtx<'_>,
        aws: &crate::aws::Aws,
        n: usize,
        need: u64,
    ) -> Result<Option<String>> {
        let Some(e) = tc.kernel.execution(tc.execution_id)? else {
            return Ok(None);
        };
        let b = &e.budget;
        if need <= b.available() {
            return Ok(None);
        }
        aws.hands.over_budget(
            tc.execution_id,
            OverBudget {
                needed: need,
                available: b.available(),
                spent: b.spent_micros,
                limit: b.limit_micros,
            },
        );
        Ok(Some(format!(
            "Not run: over the session's budget. The group's worst case is {} ({n} hand{} at {} \
             each: its TTL at its size's rate), and {} of the session's {} limit is available. \
             The operator is asked whether its spend may go back to $0; call again once they \
             answer, or with fewer hands or a shorter ttl_secs.",
            crate::narrative::dollars(need),
            if n == 1 { "" } else { "s" },
            crate::narrative::dollars(need / n.max(1) as u64),
            crate::narrative::dollars(b.available()),
            crate::narrative::dollars(b.limit_micros),
        )))
    }

    /// A settled group's result: its aggregate, from its completion.
    pub(super) fn hands_result<'a>(
        store: &crate::store::Store,
        a: &'a Action,
        tool_use_id: &'a str,
        tool: &'a str,
    ) -> ResultNode<'a> {
        let detail = group::completion(store, &a.correlation_id)
            .and_then(|c| c.detail)
            .unwrap_or(Value::Null);
        let status = match a.state {
            ActionState::Succeeded => ResultStatus::Ok,
            ActionState::Failed => ResultStatus::Error,
            ActionState::Cancelled => ResultStatus::Cancelled,
            _ => ResultStatus::Unknown,
        };
        let text = if detail.get("hands").is_some() {
            group::result_text(&detail)
        } else {
            match detail["error"].as_str() {
                Some(e) => e.to_string(),
                None => format!("The hands group ended {:?}.", a.state),
            }
        };
        ResultNode {
            correlation_id: Some(&a.correlation_id),
            meta: json!({"detail": detail}),
            ..ResultNode::new(tool_use_id, tool, status, text)
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_ttl_is_a_utc_time() {
        assert_eq!(crate::aws::hands::start::utc(0), "1970-01-01T00:00:00Z");
        assert_eq!(
            crate::aws::hands::start::utc(1_791_072_000_000),
            "2026-10-04T00:00:00Z"
        );
        assert_eq!(
            crate::aws::hands::start::utc((951_782_400 + 3661) * 1000),
            "2000-02-29T01:01:01Z"
        );
    }
}
