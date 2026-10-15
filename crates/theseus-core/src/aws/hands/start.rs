//! A group's start (AWS design §3.3), shared by `aws.hands.run`'s call
//! (`toolrun/hands.rs`) and an extension's broker (`extend/broker.rs`,
//! theseus-pw1q.14), so both launch through one path: where the hands run,
//! each hand's worst case reserved against the execution's budget, runaway
//! mode's lines, then the group's record and every hand's action in one
//! frame, the first wave dispatched ([`plan`]); then that wave's launch and
//! the group's first step ([`launch_first`]).

use std::sync::Arc;

use anyhow::Result;
use serde_json::{json, Value};
use theseus_kernel::{Kernel, Proposal, RetryClass, TurnGuard};

use super::group::{self, GroupRecord, HandEntry};
use super::launch::{self, Backend, HandsRequest};
use super::HAND;
use crate::aws::Aws;
use crate::fact::{self, Rec};
use crate::outbox::Outbox;
use crate::store::Store;

/// How long past its TTL a hand's action waits for its completion before
/// the reconciler may call it unknown.
const REPORT_GRACE_MS: u64 = 5 * 60 * 1000;

/// Where a group's frames go, and the turn that plans its hands.
pub struct Starter<'a> {
    pub kernel: &'a Kernel,
    pub store: &'a Store,
    pub outbox: &'a Outbox,
    pub rec: Rec<'a>,
    pub guard: &'a TurnGuard,
    pub execution_id: &'a str,
    pub session_id: &'a str,
    /// The extension whose broker starts it (theseus-pw1q.14), if one does.
    pub extension: Option<&'a str>,
}

/// A group planned: its record and the first wave, dispatched.
pub struct Planned {
    pub rec: GroupRecord,
    pub wave: Vec<u32>,
    /// Each hand's worst case, in micro-dollars, as it reserved.
    pub reserve: u64,
}

/// Plan the group `group` (its call's correlation id, dispatched) of `r` on
/// `backend`: its record and every hand's action in one frame. `over` says
/// why a group whose worst case (its argument, in micro-dollars) does not
/// fit the budget is not run, or `None` when it fits. A refusal is `Err`'s
/// words, the call's to settle; nothing launched.
pub async fn plan(
    s: &Starter<'_>,
    aws: &Arc<Aws>,
    group: &str,
    tool_use_id: &str,
    r: HandsRequest,
    backend: Backend,
    over: &(dyn Fn(u64) -> Result<Option<String>> + Sync),
) -> Result<Result<Planned, String>> {
    let account = match aws.account(r.account.as_deref()) {
        Ok(a) => a.clone(),
        Err(e) => return Ok(Err(e)),
    };
    let region = match account.region(r.region.as_deref()) {
        Ok(r) => r,
        Err(e) => return Ok(Err(e)),
    };
    let binding = aws.bind(s.execution_id, group, tool_use_id);
    let env = aws.hands.env(&account, &region, Some(&binding)).await;
    for req in binding.requests() {
        s.rec.record(&fact::tool::AwsCalled { row: &req.row });
    }
    let env = match env {
        Ok(e) => e,
        Err(e) => return Ok(Err(format!("cannot find where hands run: {e}"))),
    };
    if let Some(why) = launch::unready(&env, backend, &r) {
        return Ok(Err(why));
    }
    let now = theseus_protocol::now_unix_ms();
    let ttl_ms = r.ttl_secs * 1000;
    let hand_max_usd = launch::cost_usd(backend, &r, r.ttl_secs as f64, env.lambda_memory_mb);
    let n = r.inputs.len();
    // Each hand reserves its worst case (its TTL at its size's rate)
    // against the execution's budget in the group's frame, and settles at
    // its real cost (part 2). A group that does not fit is not run.
    let reserve = (hand_max_usd * 1e6).ceil() as u64;
    if let Some(why) = over(reserve.saturating_mul(n as u64))? {
        return Ok(Err(why));
    }
    // Runaway mode (theseus-ext.12): the meters' figure, current now,
    // with this group's worst case added, against runaway_factor times
    // the account's lines.
    let sink = super::runaway::Sink {
        kernel: s.kernel,
        store: s.store,
        outbox: s.outbox,
        rec: s.rec,
    };
    if let Some(why) = sink.admit(&account, reserve.saturating_mul(n as u64), now)? {
        return Ok(Err(why));
    }
    let rec = GroupRecord {
        v: 1,
        group: group.into(),
        execution_id: s.execution_id.into(),
        session_id: s.session_id.into(),
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
        extension: s.extension.map(str::to_string),
    };
    // The first wave: as `concurrency` and `max_usd` allow.
    let t = group::Tally {
        waiting: (0..n as u32).collect(),
        ..Default::default()
    };
    // The room the account's quota leaves (part 2): a bigger group
    // launches in waves, and never fails for a quota.
    let cap = super::quota::cap(&aws.hands.quotas, &account, &region, backend, &rec.request).await;
    let wave = match group::next(&rec, &t, cap) {
        group::Next::Launch(w) => w,
        group::Next::Done { why, .. } => {
            return Ok(Err(format!(
                "no hand can launch: {}",
                why.unwrap_or_default()
            )))
        }
        group::Next::Wait => Vec::new(),
    };
    let rec = match frame(s, rec, &wave, reserve, ttl_ms)? {
        Some(rec) => rec,
        // Another call's reservation landed between the check and the frame.
        None => {
            let why = over(reserve.saturating_mul(n as u64))?
                .unwrap_or_else(|| "Not run: over the budget.".into());
            return Ok(Err(why));
        }
    };
    aws.hands.launched();
    Ok(Ok(Planned { rec, wave, reserve }))
}

/// One frame: every hand's action, the first wave dispatched, and the
/// group's record; none when another reservation took the room first.
fn frame(
    s: &Starter<'_>,
    rec: GroupRecord,
    wave: &[u32],
    reserve: u64,
    ttl_ms: u64,
) -> Result<Option<GroupRecord>> {
    let (group, n) = (rec.group.clone(), rec.request.inputs.len());
    let rec = s.kernel.frame(&[s.execution_id], |k| {
        let mut rec = rec;
        for i in 0..n as u32 {
            let proposal = Proposal {
                tool: HAND.into(),
                args: json!({"group": group, "index": i}),
                resource: Some(format!("{}{group}", group::PREFIX)),
                policy_context: Value::Null,
            };
            let a = k.plan_action(
                s.guard,
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
    match rec {
        Err(e)
            if matches!(
                e.downcast_ref::<theseus_kernel::KernelError>(),
                Some(theseus_kernel::KernelError::OverBudget { .. })
            ) =>
        {
            Ok(None)
        }
        r => r.map(Some),
    }
}

/// The planned group's first wave launched, then its first step.
pub async fn launch_first(ctx: &group::Ctx<'_>, p: Planned) {
    let group = p.rec.group.clone();
    if let Err(e) = group::launch(ctx, p.rec, &p.wave).await {
        tracing::warn!(group, error = %format!("{e:#}"), "hands: the first wave");
    }
    if let Err(e) = group::step(ctx, &group).await {
        tracing::warn!(group, error = %format!("{e:#}"), "hands: a step");
    }
}

/// `theseus:ttl`'s form, from milliseconds since the epoch: a UTC time to
/// the second, `YYYY-MM-DDTHH:MM:SSZ` (civil from days, Howard Hinnant's
/// algorithm).
pub(crate) fn utc(ms: u64) -> String {
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
