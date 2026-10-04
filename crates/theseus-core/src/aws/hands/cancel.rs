//! Cancellation per backend (AWS design §3.3's table; step 40 part 2,
//! theseus-mgw.11): how a running hand is stopped, and how the stop is
//! known.
//!
//! - **Lambda** offers no stop: a hand's cancel is `unsupported`, and its
//!   function's timeout (its TTL) is the bound. Its reservation is held,
//!   and its envelope, when it comes, is recorded as late and books its
//!   real cost (`Kernel::accept_completion`'s `LateAfterCancel`).
//! - **Fargate:** `StopTask`, and the cancel is acknowledged; it is verified
//!   (`VerifiedBy::Ecs`) once the task shows STOPPED, read by `DescribeTasks`
//!   at once and for a few seconds after, then by the poller's next pass
//!   ([`verify`]) or ECS's own state change on the queue. A verified stop
//!   books the time the task ran, at its size's rate.
//! - **A group's call** ([`stop_group`]): a cancel or `/stop` of the
//!   `aws.hands.run` call stops every hand of its group (never launched:
//!   verified, nothing ran), and its own verdict sums theirs.
//! - **`until` met** (`group::step`): the hands still running are stopped
//!   the same way before the group settles.

use std::sync::Arc;
use std::time::Duration;

use anyhow::{anyhow, Result};
use serde_json::{json, Value};
use theseus_kernel::{Action, ActionState, CancelState, Verdict, VerifiedBy};
use theseus_tools::AwsBinding;

use super::group::{self, Ctx, GroupRecord};
use super::launch::{self, Backend};
use crate::aws::session::Kind;
use crate::aws::{Account, Request, Signer};
use crate::fact;

/// Why a Lambda hand cannot be stopped.
pub const LAMBDA_UNSUPPORTED: &str =
    "a Lambda invocation cannot be stopped; its function's timeout (the hand's TTL) ends it";

/// How long a cancel reads `DescribeTasks` before it leaves the rest to the
/// poller: ECS stops a task within its stop timeout (30 s by default).
const VERIFY_FOR: Duration = Duration::from_secs(4);
const VERIFY_EVERY: Duration = Duration::from_secs(1);

/// The reason a Theseus stop gives ECS (`StopTask`'s `reason`).
pub const STOP_REASON: &str = "theseus: cancelled";

/// One ECS call of a group's, signed as its work session, its request
/// bound to the group's execution so its `aws.called` row names it.
async fn ecs(
    account: &Arc<Account>,
    binding: &AwsBinding,
    region: &str,
    operation: &'static str,
    input: &Value,
) -> Result<Value, String> {
    account
        .request(
            Some(binding),
            &Request {
                service: "ecs",
                operation,
                input,
                region,
                pages: 1,
                class: if operation == "DescribeTasks" {
                    "read"
                } else {
                    "write"
                },
                signer: Signer::As(Kind::Work),
            },
        )
        .await
        .map(|o| o.body)
        .map_err(|e| e.to_string())
}

/// The `aws.called` rows of what `binding` sent, in one frame.
pub(super) fn called(ctx: &Ctx<'_>, rec: &GroupRecord, binding: &AwsBinding) {
    let session = Some(rec.session_id.as_str());
    let rows: Vec<_> = binding
        .requests()
        .iter()
        .filter_map(|r| fact::row(&fact::tool::AwsCalled { row: &r.row }, session, None).ok())
        .collect();
    if !rows.is_empty() {
        if let Err(e) = ctx.store.append(&rows) {
            tracing::warn!(error = %format!("{e:#}"), "hands: the stop's aws.called rows were not written");
        }
    }
}

/// A task as `DescribeTasks` reads it: whether it is STOPPED, why, and how
/// long it ran.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct TaskState {
    pub stopped: bool,
    pub reason: Option<String>,
    pub exit_code: Option<i64>,
    /// Seconds it ran, when ECS says when it started and stopped.
    pub ran_secs: Option<f64>,
    /// ECS no longer knows it (`MISSING`): gone past its retention.
    pub missing: bool,
}

/// `DescribeTasks` of `arns` (at most 100), by ARN.
pub async fn describe(
    account: &Arc<Account>,
    binding: &AwsBinding,
    rec: &GroupRecord,
    arns: &[String],
) -> Result<Vec<(String, TaskState)>, String> {
    let input = json!({"cluster": rec.env.cluster_arn, "tasks": arns});
    let body = ecs(account, binding, &rec.region, "DescribeTasks", &input).await?;
    let mut out = Vec::new();
    for t in body["tasks"].as_array().into_iter().flatten() {
        let Some(arn) = t["taskArn"].as_str() else {
            continue;
        };
        let secs = |k: &str| t[k].as_f64();
        out.push((
            arn.to_string(),
            TaskState {
                stopped: t["lastStatus"] == "STOPPED",
                reason: t["stoppedReason"].as_str().map(String::from),
                exit_code: t["containers"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .find(|c| c["name"] == "hand")
                    .and_then(|c| c["exitCode"].as_i64()),
                ran_secs: match (secs("startedAt"), secs("stoppedAt")) {
                    (Some(a), Some(b)) if b >= a => Some(b - a),
                    _ => None,
                },
                missing: false,
            },
        ));
    }
    for f in body["failures"].as_array().into_iter().flatten() {
        if let (Some(arn), Some("MISSING")) = (f["arn"].as_str(), f["reason"].as_str()) {
            out.push((
                arn.to_string(),
                TaskState {
                    missing: true,
                    ..Default::default()
                },
            ));
        }
    }
    Ok(out)
}

/// What a hand's stopped task cost: the time ECS says it ran, else from its
/// dispatch until now, at its size's rate.
fn cost_micros(rec: &GroupRecord, a: &Action, ran_secs: Option<f64>) -> u64 {
    let secs = ran_secs.unwrap_or_else(|| {
        let from = a.dispatched_at_ms.unwrap_or(a.planned_at_ms);
        theseus_protocol::now_unix_ms().saturating_sub(from) as f64 / 1000.0
    });
    (launch::cost_usd(Backend::Fargate, &rec.request, secs, 0) * 1e6).round() as u64
}

/// Verify a stopped hand: its cancel settles `termination_verified`, by ECS,
/// at the cost of the time its task ran.
fn verified(
    ctx: &Ctx<'_>,
    rec: &GroupRecord,
    a: &Action,
    st: &TaskState,
    ms: u64,
) -> Result<Action> {
    let v = Verdict {
        ms,
        ..Verdict::verified_as(VerifiedBy::Ecs, None)
    };
    ctx.kernel
        .cancel_verified_costing(&a.correlation_id, &v, cost_micros(rec, a, st.ran_secs))
}

/// A stop the poller verified, after its cancel's caller had gone: its
/// fact's row (`action.cancel_verified`).
fn record(ctx: &Ctx<'_>, a: &Action) {
    if let Some(r) = crate::cancel::verdict_row(a) {
        if let Err(e) = ctx.store.append(&[r]) {
            tracing::warn!(error = %format!("{e:#}"), "hands: a stop's row was not written");
        }
    }
}

/// The task ARN of a hand, by its correlation id.
fn task_of<'r>(rec: &'r GroupRecord, correlation_id: &str) -> Option<&'r str> {
    rec.hands
        .iter()
        .find(|h| h.correlation_id == correlation_id)
        .and_then(|h| h.external_op_id.as_deref())
}

/// Stop these running hands of `rec` (correlation ids), each by its
/// backend's means. What each cancel's step wrote that settled it, with its
/// backend's name: a Fargate hand whose task has not shown STOPPED yet is
/// acknowledged, not settled, and is not among them.
pub async fn stop_hands(
    ctx: &Ctx<'_>,
    rec: &GroupRecord,
    corrs: &[String],
) -> Vec<(Action, &'static str)> {
    let mut out = Vec::new();
    let open: Vec<Action> = corrs
        .iter()
        .filter_map(|c| ctx.kernel.action(c).ok().flatten())
        .filter(|a| a.state == ActionState::Dispatched)
        .collect();
    if open.is_empty() {
        return out;
    }
    if rec.backend == Backend::Lambda {
        for a in &open {
            match ctx
                .kernel
                .cancel_unsupported(&a.correlation_id, LAMBDA_UNSUPPORTED)
            {
                Ok(a) => out.push((a, "lambda")),
                Err(e) => {
                    tracing::warn!(hand = %a.correlation_id, error = %format!("{e:#}"), "hands: a cancel");
                }
            }
        }
        return out;
    }
    let Ok(account) = ctx.aws.account(Some(&rec.account)).cloned() else {
        return out;
    };
    let binding = AwsBinding::new(&rec.execution_id, &rec.group);
    let t0 = std::time::Instant::now();
    let mut asked = Vec::new();
    for a in &open {
        let Some(task) = task_of(rec, &a.correlation_id) else {
            // Dispatched and never launched (its launch's frame was lost):
            // nothing runs that a stop could reach.
            if let Ok(a) = ctx.kernel.cancel_verified(&a.correlation_id, None) {
                out.push((a, "fargate"));
            }
            continue;
        };
        let input = json!({"cluster": rec.env.cluster_arn, "task": task, "reason": STOP_REASON});
        match ecs(&account, &binding, &rec.region, "StopTask", &input).await {
            Ok(_) => {
                let _ = ctx.kernel.cancel_acknowledged(&a.correlation_id);
                asked.push(a.clone());
            }
            Err(e) => {
                let v = Verdict {
                    ms: t0.elapsed().as_millis() as u64,
                    ..Verdict::uncertain(VerifiedBy::Ecs, format!("StopTask failed: {e}"))
                };
                if let Ok(a) = ctx.kernel.cancel_uncertain(&a.correlation_id, &v) {
                    out.push((a, "fargate"));
                }
            }
        }
    }
    // Read the tasks until each shows STOPPED, for a few seconds; the rest
    // are the poller's (`verify`).
    while !asked.is_empty() {
        let arns: Vec<String> = asked
            .iter()
            .filter_map(|a| task_of(rec, &a.correlation_id).map(String::from))
            .collect();
        if let Ok(states) = describe(&account, &binding, rec, &arns).await {
            asked.retain(|a| {
                let arn = task_of(rec, &a.correlation_id).unwrap_or_default();
                let Some((_, st)) = states.iter().find(|(t, _)| t == arn) else {
                    return true;
                };
                if !(st.stopped || st.missing) {
                    return true;
                }
                if let Ok(a) = verified(ctx, rec, a, st, t0.elapsed().as_millis() as u64) {
                    out.push((a, "fargate"));
                }
                false
            });
        }
        if asked.is_empty() || t0.elapsed() >= VERIFY_FOR {
            break;
        }
        tokio::time::sleep(VERIFY_EVERY).await;
    }
    called(ctx, rec, &binding);
    out
}

/// The poller's pass over a group's stopping hands: each Fargate hand whose
/// cancel was acknowledged, read by `DescribeTasks`, and verified once its
/// task shows STOPPED. How many it verified.
pub async fn verify(ctx: &Ctx<'_>, rec: &GroupRecord) -> Result<usize> {
    if rec.backend != Backend::Fargate {
        return Ok(0);
    }
    let stopping: Vec<Action> = group::hands(ctx.kernel, rec)?
        .into_iter()
        .filter(|a| {
            a.state == ActionState::Dispatched && a.cancel == Some(CancelState::Acknowledged)
        })
        .collect();
    if stopping.is_empty() {
        return Ok(0);
    }
    let account = ctx
        .aws
        .account(Some(&rec.account))
        .map_err(|e| anyhow!(e))?
        .clone();
    let binding = AwsBinding::new(&rec.execution_id, &rec.group);
    let arns: Vec<String> = stopping
        .iter()
        .filter_map(|a| task_of(rec, &a.correlation_id).map(String::from))
        .collect();
    let states = describe(&account, &binding, rec, &arns)
        .await
        .map_err(|e| anyhow!(e))?;
    called(ctx, rec, &binding);
    let mut n = 0;
    for a in &stopping {
        let arn = task_of(rec, &a.correlation_id).unwrap_or_default();
        if let Some((_, st)) = states.iter().find(|(t, _)| t == arn) {
            if st.stopped || st.missing {
                let ms = theseus_protocol::now_unix_ms()
                    .saturating_sub(a.dispatched_at_ms.unwrap_or(a.planned_at_ms));
                record(ctx, &verified(ctx, rec, a, st, ms)?);
                n += 1;
            }
        }
    }
    Ok(n)
}

/// ECS says a hand's task stopped, and its cancel was asked: verified.
/// Whether it was one this stop asked for.
pub fn stopped_event(ctx: &Ctx<'_>, rec: &GroupRecord, a: &Action, st: &TaskState) -> Result<bool> {
    if a.state != ActionState::Dispatched || a.cancel.is_none() {
        return Ok(false);
    }
    let ms = theseus_protocol::now_unix_ms()
        .saturating_sub(a.dispatched_at_ms.unwrap_or(a.planned_at_ms));
    record(ctx, &verified(ctx, rec, a, st, ms)?);
    Ok(true)
}

/// A cancel or `/stop` of a group's call: every hand of its group stops
/// (one never launched is cancelled, verified: nothing ran), and the call
/// settles cancelled with a verdict that sums theirs: verified when every
/// hand that ran is STOPPED, `unsupported` while a Lambda hand runs on to
/// its timeout, uncertain while a task has not shown STOPPED yet (its hand's
/// own verdict follows). The group's record and its `aws.hands.settled` row
/// say so. What each step that settled wrote, the call's last.
pub async fn stop_group(ctx: &Ctx<'_>, group: &str) -> Result<Vec<(Action, &'static str)>> {
    let mut rec =
        GroupRecord::load(ctx.store, group)?.ok_or_else(|| anyhow!("no hands group {group}"))?;
    let t0 = std::time::Instant::now();
    let actions = group::hands(ctx.kernel, &rec)?;
    for a in &actions {
        if matches!(a.state, ActionState::Planned | ActionState::Authorized) {
            ctx.kernel.cancel_verified(&a.correlation_id, None)?;
        }
    }
    let running: Vec<String> = actions
        .iter()
        .filter(|a| a.state == ActionState::Dispatched)
        .map(|a| a.correlation_id.clone())
        .collect();
    let mut out = stop_hands(ctx, &rec, &running).await;
    let actions = group::hands(ctx.kernel, &rec)?;
    let ran: Vec<&Action> = actions
        .iter()
        .filter(|a| running.contains(&a.correlation_id))
        .collect();
    let pending = ran
        .iter()
        .filter(|a| a.state == ActionState::Dispatched)
        .count();
    let unsupported = ran
        .iter()
        .filter(|a| a.cancel == Some(CancelState::Unsupported))
        .count();
    let uncertain = ran
        .iter()
        .filter(|a| a.cancel == Some(CancelState::OutcomeUncertain))
        .count();
    let ms = t0.elapsed().as_millis() as u64;
    let call = if pending + uncertain > 0 {
        let why = format!(
            "{} of its hands' tasks not yet STOPPED (StopTask sent; each hand's own verdict follows)",
            pending + uncertain
        );
        ctx.kernel.cancel_uncertain(
            group,
            &Verdict {
                ms,
                ..Verdict::uncertain(VerifiedBy::Ecs, why)
            },
        )?
    } else if unsupported > 0 {
        ctx.kernel.cancel_unsupported(
            group,
            &format!(
                "{unsupported} Lambda hand{} cannot be stopped; each ends by its timeout (its TTL)",
                if unsupported == 1 { "" } else { "s" }
            ),
        )?
    } else if ran.is_empty() {
        ctx.kernel.cancel_verified(group, None)?
    } else {
        ctx.kernel.cancel_verified(
            group,
            Some(&Verdict {
                ms,
                ..Verdict::verified_as(VerifiedBy::Ecs, None)
            }),
        )?
    };
    if rec.settled.is_none() {
        rec.settled = Some("cancelled".into());
        let t = group::tally(ctx.store, &rec, &actions);
        let why = call
            .resolution
            .clone()
            .unwrap_or_else(|| "cancelled".into());
        let records = vec![
            rec.record()?,
            fact::row(
                &group::Settled {
                    rec: &rec,
                    tally: &t,
                    met: false,
                    why: Some(&why),
                },
                Some(rec.session_id.as_str()),
                None,
            )?,
        ];
        ctx.kernel
            .frame(&[&rec.execution_id], |k| k.stage(&records))?;
    }
    out.push((call, rec.backend.as_str()));
    Ok(out)
}
