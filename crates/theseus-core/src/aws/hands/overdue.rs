//! Overdue and reaped hands (AWS design §3.3, "The reconciler", and the
//! TTL's third layer; step 40 part 2, theseus-mgw.11).
//!
//! - **Overdue.** The heartbeat's reconciler leaves a hand to this one
//!   ([`Evidence`]): a hand's action is past its deadline (its TTL and a
//!   grace) with no envelope, and before it is called unknown the backend
//!   is asked. A Fargate hand's task is read with `DescribeTasks`: still
//!   RUNNING, it is left (the reaper stops it); STOPPED by the TTL reaper,
//!   it settles failed with the reaper's reason; STOPPED otherwise, or gone
//!   from ECS, it is unknown, saying what ECS said. A Lambda hand has
//!   nothing to ask: its timeout has passed, so it is unknown. Its envelope,
//!   if it ever comes, resolves it.
//! - **Reaped.** A hand the reaper stopped (its task's stopped reason is the
//!   reaper's) settles failed with that reason, from ECS's state change on
//!   the queue or from this pass, never unknown.

use anyhow::Result;
use serde_json::json;
use theseus_kernel::{Action, ActionState, Completion, Outcome, Probe};
use theseus_tools::AwsBinding;

use super::cancel::{describe, TaskState};
use super::group::{self, Ctx, GroupRecord};
use super::launch::{self, Backend, HAND};

/// The reaper's stopped reason begins so (infra/aws/theseus-hands.yaml).
pub const REAPER: &str = "theseus ttl reaper";

/// Whether a task's stopped reason is the TTL reaper's.
pub fn by_reaper(reason: Option<&str>) -> bool {
    reason.is_some_and(|r| r.starts_with(REAPER))
}

/// The heartbeat's evidence, with hands left to their own reconciler: it
/// asks the backend first, which the heartbeat cannot (it never waits on
/// the network).
pub struct Evidence<'a>(pub &'a dyn theseus_kernel::Evidence);

impl theseus_kernel::Evidence for Evidence<'_> {
    fn probe(&self, action: &Action) -> Probe {
        if action.tool == HAND {
            return Probe::StillRunning;
        }
        self.0.probe(action)
    }
}

/// A hand the reaper stopped: failed, with the reaper's reason, at the
/// cost of the time its task ran (its worst case when ECS does not say).
/// Whether it settled now.
pub fn reaped(ctx: &Ctx<'_>, rec: &GroupRecord, a: &Action, st: &TaskState) -> Result<bool> {
    if a.state != ActionState::Dispatched || a.cancel.is_some() {
        return Ok(false);
    }
    let index = rec
        .hands
        .iter()
        .position(|h| h.correlation_id == a.correlation_id);
    let cost = st.ran_secs.map_or(a.reserved_micros, |s| {
        (launch::cost_usd(rec.backend, &rec.request, s, rec.env.lambda_memory_mb) * 1e6).round()
            as u64
    });
    let now = theseus_protocol::now_unix_ms();
    let why = format!(
        "stopped by the TTL reaper: {}",
        st.reason.as_deref().unwrap_or(REAPER)
    );
    ctx.kernel.accept_completion(&Completion {
        correlation_id: a.correlation_id.clone(),
        outcome: Outcome::Failed,
        result_ref: None,
        external_op_id: None,
        started_at_ms: a.dispatched_at_ms.unwrap_or(now),
        finished_at_ms: now,
        producer: "hands:reaper".into(),
        signature: None,
        cost_micros: Some(cost),
        detail: Some(
            json!({"group": rec.group, "index": index, "exit_code": st.exit_code,
            "error": why}),
        ),
    })?;
    Ok(true)
}

/// The pass over a group's overdue hands as of `now`. Whether one settled,
/// so the group takes its step.
pub async fn pass(ctx: &Ctx<'_>, rec: &GroupRecord, now: u64) -> Result<bool> {
    let overdue: Vec<Action> = group::hands(ctx.kernel, rec)?
        .into_iter()
        .filter(|a| {
            a.state == ActionState::Dispatched && a.cancel.is_none() && a.deadline_at_ms <= now
        })
        .collect();
    if overdue.is_empty() {
        return Ok(false);
    }
    let mut settled = false;
    if rec.backend == Backend::Lambda {
        for a in &overdue {
            unknown(
                ctx,
                a,
                "overdue: no envelope and no failure record came by its TTL and grace",
            )?;
            settled = true;
        }
        return Ok(settled);
    }
    let account = ctx
        .aws
        .account(Some(&rec.account))
        .map_err(|e| anyhow::anyhow!(e))?
        .clone();
    let binding = AwsBinding::new(&rec.execution_id, &rec.group);
    let arns: Vec<String> = overdue
        .iter()
        .filter_map(|a| task_of(rec, &a.correlation_id))
        .collect();
    let states = describe(&account, &binding, rec, &arns)
        .await
        .map_err(|e| anyhow::anyhow!("DescribeTasks of overdue hands: {e}"))?;
    super::cancel::called(ctx, rec, &binding);
    for a in &overdue {
        let Some(arn) = task_of(rec, &a.correlation_id) else {
            unknown(ctx, a, "overdue, and it has no task to ask ECS about")?;
            settled = true;
            continue;
        };
        let Some((_, st)) = states.iter().find(|(t, _)| *t == arn) else {
            continue;
        };
        if st.missing {
            unknown(ctx, a, "overdue, and ECS no longer knows its task")?;
            settled = true;
        } else if st.stopped && by_reaper(st.reason.as_deref()) {
            settled |= reaped(ctx, rec, a, st)?;
        } else if st.stopped {
            let why = format!(
                "overdue: its task STOPPED ({}) and its envelope never came",
                st.reason.as_deref().unwrap_or("no reason given")
            );
            unknown(ctx, a, &why)?;
            settled = true;
        } else {
            tracing::info!(hand = %a.correlation_id, "hands: overdue, and ECS says its task still runs; the reaper stops it");
        }
    }
    Ok(settled)
}

/// Unknown, with why; one settled since is left as it is.
fn unknown(ctx: &Ctx<'_>, a: &Action, why: &str) -> Result<()> {
    match ctx.kernel.mark_unknown(&a.correlation_id, why) {
        Ok(_) => Ok(()),
        Err(e)
            if matches!(
                e.downcast_ref::<theseus_kernel::KernelError>(),
                Some(theseus_kernel::KernelError::ActionState { .. })
            ) =>
        {
            Ok(())
        }
        Err(e) => Err(e),
    }
}

fn task_of(rec: &GroupRecord, correlation_id: &str) -> Option<String> {
    rec.hands
        .iter()
        .find(|h| h.correlation_id == correlation_id)
        .and_then(|h| h.external_op_id.clone())
}
