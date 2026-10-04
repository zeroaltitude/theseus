//! `aws.hands.run`'s call (AWS design §3.3): the group's frame, its first
//! wave, and its answer, `background` until the group settles; then its
//! late result, the aggregate (`hands_result`). The group's logic is
//! `aws::hands::group`'s; this is the turn's side of it.

use anyhow::Result;
use serde_json::{json, Value};
use theseus_kernel::{Action, ActionState, Proposal, RetryClass};
use theseus_tools::Tool;

use super::{CallOutcome, ResultNode, ToolRuntime, TurnCtx};
use crate::aws::hands::group::{self, GroupRecord, HandEntry};
use crate::aws::hands::{launch, HAND};
use crate::fact;
use crate::node::ResultStatus;
use crate::provider::ToolUse;

/// How long past its TTL a hand's action waits for its completion before
/// the reconciler may call it unknown.
const REPORT_GRACE_MS: u64 = 5 * 60 * 1000;

impl ToolRuntime {
    /// Start a group: its record and every hand's action in one frame (the
    /// first wave dispatched), then that wave's launch. It answers
    /// `background`, unless the group is already done (every launch failed,
    /// say), when it answers with the aggregate.
    #[expect(clippy::too_many_lines, reason = "shape budget: split it")]
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
        let account = match aws.account(r.account.as_deref()) {
            Ok(a) => a.clone(),
            Err(e) => return fail(&e),
        };
        let region = match account.region(r.region.as_deref()) {
            Ok(r) => r,
            Err(e) => return fail(&e),
        };
        let binding = aws.bind(tc.execution_id, correlation_id, &call.id);
        let env = aws.hands.env(&account, &region, Some(&binding)).await;
        for req in binding.requests() {
            tc.record(&fact::tool::AwsCalled { row: &req.row });
        }
        let env = match env {
            Ok(e) => e,
            Err(e) => return fail(&format!("cannot find where hands run: {e}")),
        };
        if let Some(why) = launch::unready(&env, backend, &r) {
            return fail(&why);
        }
        let now = theseus_protocol::now_unix_ms();
        let ttl_ms = r.ttl_secs * 1000;
        let hand_max_usd = launch::cost_usd(backend, &r, r.ttl_secs as f64, env.lambda_memory_mb);
        let n = r.inputs.len();
        // Each hand reserves its worst case (its TTL at its size's rate)
        // against the session's budget in the group's frame, and settles at
        // its real cost (part 2). A group that does not fit is not run, and
        // its turn asks the budget question, as a model call does.
        let reserve = (hand_max_usd * 1e6).ceil() as u64;
        let over = |need: u64| -> Result<Option<String>> {
            let Some(e) = tc.kernel.execution(tc.execution_id)? else {
                return Ok(None);
            };
            let b = &e.budget;
            if need <= b.available() {
                return Ok(None);
            }
            aws.hands.over_budget(
                tc.execution_id,
                crate::aws::hands::OverBudget {
                    needed: need,
                    available: b.available(),
                    spent: b.spent_micros,
                    limit: b.limit_micros,
                },
            );
            Ok(Some(format!(
                "Not run: over the session's budget. The group's worst case is {} ({n} hand{} at \
                 {} each: its TTL at its size's rate), and {} of the session's {} limit is \
                 available. The operator is asked whether its spend may go back to $0; call again \
                 once they answer, or with fewer hands or a shorter ttl_secs.",
                crate::narrative::dollars(need),
                if n == 1 { "" } else { "s" },
                crate::narrative::dollars(reserve),
                crate::narrative::dollars(b.available()),
                crate::narrative::dollars(b.limit_micros),
            )))
        };
        if let Some(why) = over(reserve.saturating_mul(n as u64))? {
            return fail(&why);
        }
        let rec = GroupRecord {
            v: 1,
            group: correlation_id.into(),
            execution_id: tc.execution_id.into(),
            session_id: tc.session_id.into(),
            account: account.id.clone(),
            region: region.clone(),
            deployment: account.cfg.deployment().into(),
            backend,
            request: r,
            env,
            task_definition: None,
            hands: Vec::with_capacity(n),
            ttl_at: utc(now + ttl_ms),
            hand_max_usd,
            created_at_ms: now,
            endpoint: account.cfg.endpoint.clone(),
            settled: None,
        };
        // The first wave: as `concurrency` and `max_usd` allow.
        let t = group::Tally {
            waiting: (0..n as u32).collect(),
            ..Default::default()
        };
        // The room the account's quota leaves (part 2): a bigger group
        // launches in waves, and never fails for a quota.
        let cap = crate::aws::hands::quota::cap(
            &aws.hands.quotas,
            &account,
            &region,
            backend,
            &rec.request,
        )
        .await;
        let wave = match group::next(&rec, &t, cap) {
            group::Next::Launch(w) => w,
            group::Next::Done { why, .. } => {
                return fail(&format!("no hand can launch: {}", why.unwrap_or_default()))
            }
            group::Next::Wait => Vec::new(),
        };
        // One frame: every hand's action, the first wave dispatched, and the
        // group's record.
        let rec = tc.kernel.frame(&[tc.execution_id], |k| {
            let mut rec = rec;
            for i in 0..n as u32 {
                let proposal = Proposal {
                    tool: HAND.into(),
                    args: json!({"group": correlation_id, "index": i}),
                    resource: Some(format!("{}{correlation_id}", group::PREFIX)),
                    policy_context: Value::Null,
                };
                let a = k.plan_action(
                    tc.guard,
                    &proposal,
                    RetryClass::NonRepeatable,
                    Some(ttl_ms + REPORT_GRACE_MS),
                    reserve,
                )?;
                k.authorize(&a.correlation_id, &proposal, None)?;
                if wave.contains(&i) {
                    k.dispatch(&a.correlation_id, None)?;
                }
                rec.hands.push(HandEntry {
                    index: i,
                    correlation_id: a.correlation_id,
                    external_op_id: None,
                });
            }
            k.stage(&[rec.record()?])?;
            Ok(rec)
        });
        // Another call's reservation landed between the check and the frame.
        let rec = match rec {
            Err(e)
                if matches!(
                    e.downcast_ref::<theseus_kernel::KernelError>(),
                    Some(theseus_kernel::KernelError::OverBudget { .. })
                ) =>
            {
                let why = over(reserve.saturating_mul(n as u64))?
                    .unwrap_or_else(|| "Not run: over the session's budget.".into());
                return fail(&why);
            }
            r => r?,
        };
        aws.hands.launched();
        let ctx = group::Ctx {
            kernel: tc.kernel,
            store: tc.store,
            aws: &aws,
        };
        let launched = group::launch(&ctx, rec, &wave).await;
        if let Err(e) = &launched {
            tracing::warn!(group = correlation_id, error = %format!("{e:#}"), "hands: the first wave");
        }
        if let Err(e) = group::step(&ctx, correlation_id).await {
            tracing::warn!(group = correlation_id, error = %format!("{e:#}"), "hands: a step");
        }
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
                        wave.len()
                    ),
                )
            },
        )?;
        Ok(CallOutcome::Background {
            correlation_id: correlation_id.into(),
        })
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

/// `theseus:ttl`'s form, from milliseconds since the epoch: a UTC time to
/// the second, `YYYY-MM-DDTHH:MM:SSZ` (civil from days, Howard Hinnant's
/// algorithm).
fn utc(ms: u64) -> String {
    let secs = ms / 1000;
    let days = (secs / 86_400) as i64;
    let rem = secs % 86_400;
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!(
        "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z",
        rem / 3600,
        rem % 3600 / 60,
        rem % 60
    )
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_ttl_is_a_utc_time() {
        assert_eq!(super::utc(0), "1970-01-01T00:00:00Z");
        assert_eq!(super::utc(1_791_072_000_000), "2026-10-04T00:00:00Z");
        assert_eq!(
            super::utc((951_782_400 + 3661) * 1000),
            "2000-02-29T01:01:01Z"
        );
    }
}
