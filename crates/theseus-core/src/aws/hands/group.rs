//! A group of hands (AWS design §3.3): its record, its hands' actions, and
//! each step it takes as they settle.
//!
//! - **The record.** A group is the `aws.hands.run` call's own action (its
//!   correlation id is the group's id), a META record keyed
//!   `aws.hands.group.<group>` ([`GroupRecord`]: the request, the backend,
//!   where its hands run, and each hand's index, correlation id, and
//!   external id), and one kernel action per hand (tool `aws.hand`). The
//!   call writes the record and every hand's action (planned and
//!   authorized, the first wave dispatched) in one frame, before anything is
//!   launched.
//! - **A wave.** A hand is dispatched (its own frame, the outbox) before it
//!   is launched, and the launches' external ids and `aws.called` rows land
//!   in the next frame with an `aws.hands.launched` row. A dispatch is the
//!   claim: two launchers of one hand cannot both dispatch it.
//! - **A step** ([`step`]), after a hand settles: the group is done when
//!   `until` is met (or can no longer be); then the hands still running are
//!   stopped by their backend's means (`cancel::stop_hands`, part 2), the
//!   hands never launched are cancelled (verified: nothing ran), and the
//!   call settles with the aggregate, which its continuation reads
//!   (`result_text`). Otherwise the next hands launch while fewer than
//!   `concurrency` run and `max_usd` allows them. A `first_success` group
//!   launches nothing after its first success.

use std::sync::Arc;

use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use theseus_kernel::{Action, ActionState, Completion, Kernel, Outcome};
use theseus_store::{kinds, NewRecord, Store as _};
use theseus_tools::AwsBinding;

use super::launch::{self, Backend, GroupIds, HandsEnv, HandsRequest, Until};
use crate::aws::{Account, Aws};
use crate::fact;
use crate::store::Store;

/// The group record's META key's prefix.
pub const PREFIX: &str = "aws.hands.group.";

/// A group's record, as its frames write it.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct GroupRecord {
    pub v: u32,
    /// The `aws.hands.run` call's correlation id.
    pub group: String,
    pub execution_id: String,
    pub session_id: String,
    pub account: String,
    pub region: String,
    pub deployment: String,
    pub backend: Backend,
    pub request: HandsRequest,
    pub env: HandsEnv,
    /// Its Fargate task definition, registered at its launch.
    #[serde(default)]
    pub task_definition: Option<String>,
    pub hands: Vec<HandEntry>,
    /// `theseus:ttl`.
    pub ttl_at: String,
    /// A hand's worst case.
    pub hand_max_usd: f64,
    pub created_at_ms: u64,
    /// The endpoint every request went to instead of AWS's: a test's fake.
    #[serde(default)]
    pub endpoint: Option<String>,
    /// How it ended, once it has.
    #[serde(default)]
    pub settled: Option<String>,
}

/// One hand of a group.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct HandEntry {
    pub index: u32,
    pub correlation_id: String,
    /// Lambda's request id, or the task's ARN, once launched.
    #[serde(default)]
    pub external_op_id: Option<String>,
}

impl GroupRecord {
    pub fn key(group: &str) -> String {
        format!("{PREFIX}{group}")
    }

    pub fn record(&self) -> Result<NewRecord> {
        NewRecord::json(kinds::META, Some(&Self::key(&self.group)), self)
    }

    pub fn load(store: &Store, group: &str) -> Result<Option<GroupRecord>> {
        store.get_meta(&Self::key(group))
    }

    pub fn ids(&self) -> GroupIds<'_> {
        GroupIds {
            group: &self.group,
            execution_id: &self.execution_id,
            session_id: &self.session_id,
            deployment: &self.deployment,
            ttl_at: self.ttl_at.clone(),
        }
    }
}

/// What a group's hands are now.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct Tally {
    pub succeeded: u32,
    /// Failed or unknown after a launch.
    pub failed: u32,
    /// Stopped after a launch: its group was done, or its call cancelled.
    pub cancelled: u32,
    pub running: u32,
    /// Of `running`, those whose stop was asked and not yet seen.
    pub stopping: u32,
    /// Not launched yet.
    pub waiting: Vec<u32>,
    /// Never launched, and cancelled when the group ended.
    pub not_launched: u32,
    /// The settled hands' cost, from their completions.
    pub spent_usd: f64,
}

/// Each hand's action, in index order.
pub fn hands(kernel: &Kernel, rec: &GroupRecord) -> Result<Vec<Action>> {
    rec.hands
        .iter()
        .map(|h| {
            kernel
                .action(&h.correlation_id)?
                .ok_or_else(|| anyhow!("hand {} has no action", h.correlation_id))
        })
        .collect()
}

/// A hand's completion, as stored.
pub fn completion(store: &Store, correlation_id: &str) -> Option<Completion> {
    store
        .inner()
        .latest_by_key(kinds::COMPLETION, correlation_id)
        .ok()
        .flatten()
        .and_then(|r| r.decode().ok())
}

pub fn tally(store: &Store, rec: &GroupRecord, actions: &[Action]) -> Tally {
    let mut t = Tally::default();
    for (h, a) in rec.hands.iter().zip(actions) {
        match a.state {
            ActionState::Succeeded => t.succeeded += 1,
            ActionState::Planned | ActionState::Authorized => t.waiting.push(h.index),
            ActionState::Dispatched => {
                t.running += 1;
                t.stopping += u32::from(a.cancel.is_some());
            }
            ActionState::Cancelled if a.dispatched_at_ms.is_none() => t.not_launched += 1,
            ActionState::Cancelled => t.cancelled += 1,
            _ => t.failed += 1,
        }
        if a.state.is_settled() {
            if let Some(c) = completion(store, &a.correlation_id).and_then(|c| c.cost_micros) {
                t.spent_usd += c as f64 / 1e6;
            }
        }
    }
    t
}

/// What a group does next.
#[derive(Debug, PartialEq)]
pub enum Next {
    /// It is done: whether `until` was met, and why not if it was not.
    Done { met: bool, why: Option<String> },
    /// Launch these hands.
    Launch(Vec<u32>),
    /// Wait for the running ones.
    Wait,
}

/// The rule: `until` first, then the room `concurrency` and `max_usd`
/// leave.
pub fn next(rec: &GroupRecord, t: &Tally) -> Next {
    let open = t.running + t.waiting.len() as u32;
    let (met, done) = match rec.request.until {
        // `all` runs every hand to its end, and is met when none failed.
        Until::All => (t.failed == 0, open == 0),
        Until::FirstSuccess => (t.succeeded >= 1, t.succeeded >= 1 || open == 0),
        Until::Quorum(q) => (t.succeeded >= q, t.succeeded >= q || t.succeeded + open < q),
    };
    if done {
        return Next::Done {
            met,
            why: (!met).then(|| match rec.request.until {
                Until::All => format!("{} of its hands failed", t.failed),
                u => format!("until {} cannot be met", u.words()),
            }),
        };
    }
    let room = rec.request.concurrency.saturating_sub(t.running) as usize;
    let mut take = room.min(t.waiting.len());
    if let Some(cap) = rec.request.max_usd {
        let committed = t.spent_usd + f64::from(t.running) * rec.hand_max_usd;
        let affordable = ((cap - committed) / rec.hand_max_usd).floor().max(0.0) as usize;
        if affordable < take {
            take = affordable;
            if take == 0 && t.running == 0 {
                return Next::Done {
                    met: false,
                    why: Some(format!(
                        "max_usd ${cap:.2} leaves no room for another hand at ${:.4} each",
                        rec.hand_max_usd
                    )),
                };
            }
        }
    }
    match take {
        0 => Next::Wait,
        n => Next::Launch(t.waiting[..n].to_vec()),
    }
}

/// Where a step's records go: the kernel, the store, and the AWS layer.
pub struct Ctx<'a> {
    pub kernel: &'a Kernel,
    pub store: &'a Store,
    pub aws: &'a Arc<Aws>,
}

/// The `aws.hands.launched` row of a wave.
pub struct Launched<'a> {
    pub rec: &'a GroupRecord,
    pub wave: &'a [(String, Result<String, String>)],
}

impl fact::Fact for Launched<'_> {
    const KIND: Option<theseus_protocol::LedgerKind> =
        Some(theseus_protocol::LedgerKind::AwsHandsLaunched);

    fn row(&self) -> Value {
        let hands: Vec<Value> = self
            .wave
            .iter()
            .map(|(c, r)| match r {
                Ok(ext) => json!({"correlation_id": c, "external_op_id": ext}),
                Err(e) => json!({"correlation_id": c, "error": e}),
            })
            .collect();
        json!({"group": self.rec.group, "execution_id": self.rec.execution_id,
            "account": self.rec.account, "region": self.rec.region,
            "backend": self.rec.backend.as_str(), "hands": hands,
            "task_definition": self.rec.task_definition})
    }
}

/// The `aws.hands.settled` row of a group.
pub struct Settled<'a> {
    pub rec: &'a GroupRecord,
    pub tally: &'a Tally,
    pub met: bool,
    pub why: Option<&'a str>,
}

impl fact::Fact for Settled<'_> {
    const KIND: Option<theseus_protocol::LedgerKind> =
        Some(theseus_protocol::LedgerKind::AwsHandsSettled);

    fn row(&self) -> Value {
        json!({"group": self.rec.group, "execution_id": self.rec.execution_id,
            "backend": self.rec.backend.as_str(), "until": self.rec.request.until.words(),
            "met": self.met, "why": self.why, "succeeded": self.tally.succeeded,
            "failed": self.tally.failed, "cancelled": self.tally.cancelled,
            "not_launched": self.tally.not_launched,
            "running": self.tally.running, "cost_usd": self.tally.spent_usd})
    }
}

/// The per-dispatch key of each hand, from the account's secret.
async fn keys(account: &Arc<Account>, ids: &[String]) -> Result<Vec<zeroize::Zeroizing<[u8; 32]>>> {
    let root = account
        .root_key()
        .await
        .map_err(|e| anyhow!("no key to derive the hands' keys: {e}"))?;
    Ok(ids
        .iter()
        .map(|c| super::envelope::derive_key(root.expose_secret().as_bytes(), c))
        .collect())
}

/// Launch the dispatched hands `wave` (their indexes): each launched at
/// once, their external ids, rows, and the record in one frame, and each
/// that failed to start settled failed. What the group's record is now.
pub async fn launch(ctx: &Ctx<'_>, mut rec: GroupRecord, wave: &[u32]) -> Result<GroupRecord> {
    let account = ctx
        .aws
        .account(Some(&rec.account))
        .map_err(|e| anyhow!(e))?
        .clone();
    let binding = AwsBinding::new(&rec.execution_id, &rec.group);
    let corrs: Vec<String> = wave
        .iter()
        .map(|i| rec.hands[*i as usize].correlation_id.clone())
        .collect();
    let keys = keys(&account, &corrs).await;
    if rec.backend == Backend::Fargate && rec.task_definition.is_none() {
        let ids = rec.ids();
        let td = launch::register_task_definition(
            &account,
            Some(&binding),
            &rec.region,
            &rec.env,
            &rec.request,
            &ids,
        )
        .await;
        drop(ids);
        match td {
            Ok(arn) => rec.task_definition = Some(arn),
            Err(e) => {
                let results: Vec<(String, Result<String, String>)> = corrs
                    .iter()
                    .map(|c| (c.clone(), Err(format!("no task definition: {e}"))))
                    .collect();
                return finish_wave(ctx, rec, &binding, results);
            }
        }
    }
    let results: Vec<(String, Result<String, String>)> = match keys {
        Err(e) => corrs
            .iter()
            .map(|c| (c.clone(), Err(e.to_string())))
            .collect(),
        Ok(keys) => {
            let ids = rec.ids();
            let launches = wave.iter().zip(&keys).map(|(i, k)| {
                let h = &rec.hands[*i as usize];
                let spec = launch::spec(
                    &rec.env,
                    &rec.request,
                    rec.backend,
                    &rec.group,
                    h.index,
                    &h.correlation_id,
                    k.as_ref(),
                    &rec.region,
                    rec.endpoint.as_deref(),
                );
                let (account, binding, rec, ids) = (&account, &binding, &rec, &ids);
                async move {
                    let r = launch::launch(
                        account,
                        Some(binding),
                        &rec.region,
                        &rec.env,
                        rec.backend,
                        rec.task_definition.as_deref(),
                        &spec,
                        ids,
                    )
                    .await;
                    (spec.correlation_id.clone(), r)
                }
            });
            futures_util::future::join_all(launches).await
        }
    };
    finish_wave(ctx, rec, &binding, results)
}

/// A wave's frame: the record with its external ids, the `aws.called` and
/// `aws.hands.launched` rows; then each hand that did not start, settled
/// failed.
fn finish_wave(
    ctx: &Ctx<'_>,
    mut rec: GroupRecord,
    binding: &AwsBinding,
    results: Vec<(String, Result<String, String>)>,
) -> Result<GroupRecord> {
    for (c, r) in &results {
        if let (Ok(ext), Some(h)) = (r, rec.hands.iter_mut().find(|h| &h.correlation_id == c)) {
            h.external_op_id = Some(ext.clone());
        }
    }
    let session = Some(rec.session_id.as_str());
    let mut records = vec![rec.record()?];
    for r in binding.requests() {
        records.push(fact::row(
            &fact::tool::AwsCalled { row: &r.row },
            session,
            None,
        )?);
    }
    records.push(fact::row(
        &Launched {
            rec: &rec,
            wave: &results,
        },
        session,
        None,
    )?);
    ctx.kernel
        .frame(&[&rec.execution_id], |k| k.stage(&records))?;
    let now = theseus_protocol::now_unix_ms();
    for (c, r) in &results {
        if let Err(e) = r {
            ctx.kernel.accept_completion(&Completion {
                correlation_id: c.clone(),
                outcome: Outcome::Failed,
                result_ref: None,
                external_op_id: None,
                started_at_ms: now,
                finished_at_ms: now,
                producer: "hands:launch".into(),
                signature: None,
                // It never started: its reservation settles at nothing.
                cost_micros: Some(0),
                detail: Some(json!({"error": format!("did not start: {e}")})),
            })?;
        }
    }
    Ok(rec)
}

/// One step of a group, after a hand settled (or its first wave launched).
/// Whether the group is done.
pub async fn step(ctx: &Ctx<'_>, group: &str) -> Result<bool> {
    // A launch's own failures settle hands, so a step may need another.
    for _ in 0..=super::launch::MAX_HANDS {
        let Some(rec) = GroupRecord::load(ctx.store, group)? else {
            return Err(anyhow!("no hands group {group}"));
        };
        let Some(call) = ctx.kernel.action(group)? else {
            return Err(anyhow!("the hands group {group} has no call"));
        };
        if rec.settled.is_some() || call.state.is_settled() {
            return Ok(true);
        }
        let actions = hands(ctx.kernel, &rec)?;
        let t = tally(ctx.store, &rec, &actions);
        match next(&rec, &t) {
            Next::Wait => return Ok(false),
            Next::Done { met, why } => {
                // The hands still running are stopped first (part 2): their
                // group no longer needs them.
                let running: Vec<String> = actions
                    .iter()
                    .filter(|a| a.state == ActionState::Dispatched && a.cancel.is_none())
                    .map(|a| a.correlation_id.clone())
                    .collect();
                if !running.is_empty() {
                    let stopped = super::cancel::stop_hands(ctx, &rec, &running).await;
                    let rows: Vec<_> = stopped
                        .iter()
                        .filter_map(|(a, _)| crate::cancel::verdict_row(a))
                        .collect();
                    if !rows.is_empty() {
                        ctx.store.append(&rows)?;
                    }
                }
                settle(ctx, rec, met, why.as_deref())?;
                return Ok(true);
            }
            Next::Launch(wave) => {
                let corrs: Vec<&str> = wave
                    .iter()
                    .map(|i| rec.hands[*i as usize].correlation_id.as_str())
                    .collect();
                let claimed = dispatch(ctx.kernel, &rec.execution_id, &corrs)?;
                let wave: Vec<u32> = wave
                    .into_iter()
                    .filter(|i| claimed.contains(&rec.hands[*i as usize].correlation_id))
                    .collect();
                if wave.is_empty() {
                    return Ok(false);
                }
                launch(ctx, rec, &wave).await?;
            }
        }
    }
    Ok(false)
}

/// Dispatch these hands in one frame: the ones this caller claimed (a hand
/// another launcher dispatched first is not one).
pub fn dispatch(kernel: &Kernel, execution_id: &str, corrs: &[&str]) -> Result<Vec<String>> {
    kernel.frame(&[execution_id], |k| {
        let mut claimed = Vec::new();
        for c in corrs {
            let a = k.action(c)?.ok_or_else(|| anyhow!("hand {c} vanished"))?;
            if matches!(a.state, ActionState::Planned | ActionState::Authorized) {
                k.dispatch(c, None)?;
                claimed.push((*c).to_string());
            }
        }
        Ok(claimed)
    })
}

/// The group is done: its hands never launched are cancelled (nothing
/// ran), and its call settles with the aggregate, its record and its
/// `aws.hands.settled` row in the same frame.
fn settle(ctx: &Ctx<'_>, mut rec: GroupRecord, met: bool, why: Option<&str>) -> Result<()> {
    for h in &rec.hands {
        if let Some(a) = ctx.kernel.action(&h.correlation_id)? {
            if matches!(a.state, ActionState::Planned | ActionState::Authorized) {
                ctx.kernel.cancel_verified(&h.correlation_id, None)?;
            }
        }
    }
    let actions = hands(ctx.kernel, &rec)?;
    let t = tally(ctx.store, &rec, &actions);
    let per_hand: Vec<Value> = rec
        .hands
        .iter()
        .zip(&actions)
        .map(|(h, a)| {
            let c = completion(ctx.store, &a.correlation_id);
            let d = c.as_ref().and_then(|c| c.detail.clone()).unwrap_or(Value::Null);
            json!({
                "index": h.index,
                "correlation_id": h.correlation_id,
                "state": a.state,
                "exit_code": d.get("exit_code"),
                "timed_out": d.get("timed_out"),
                "duration_ms": c.as_ref().filter(|_| a.state.is_settled() && a.dispatched_at_ms.is_some())
                    .map(|c| c.finished_at_ms.saturating_sub(c.started_at_ms)),
                "cost_usd": c.as_ref().and_then(|c| c.cost_micros).map(|m| m as f64 / 1e6),
                "result_ref": a.result_ref,
                "tail": d.get("tail"),
                "error": d.get("error"),
                "note": d.get("note"),
                "cancel": crate::cancel::words(a),
            })
        })
        .collect();
    rec.settled = Some(if met { "met" } else { "not_met" }.into());
    let session = Some(rec.session_id.as_str());
    let extra = vec![
        rec.record()?,
        fact::row(
            &Settled {
                rec: &rec,
                tally: &t,
                met,
                why,
            },
            session,
            None,
        )?,
    ];
    let c = Completion {
        correlation_id: rec.group.clone(),
        outcome: if met {
            Outcome::Succeeded
        } else {
            Outcome::Failed
        },
        result_ref: None,
        external_op_id: None,
        started_at_ms: rec.created_at_ms,
        finished_at_ms: theseus_protocol::now_unix_ms(),
        producer: "hands:group".into(),
        signature: None,
        cost_micros: Some((t.spent_usd * 1e6).round() as u64),
        detail: Some(json!({
            "group": rec.group,
            "backend": rec.backend.as_str(),
            "until": rec.request.until.words(),
            "met": met,
            "why": why,
            "succeeded": t.succeeded,
            "failed": t.failed,
            "cancelled": t.cancelled,
            "running": t.running,
            "not_launched": t.not_launched,
            "cost_usd": t.spent_usd,
            "hands": per_hand,
        })),
    };
    ctx.kernel.accept_completion_with(&c, extra)?;
    Ok(())
}

/// What the call's continuation reads: the group's aggregate, one line a
/// hand.
pub fn result_text(detail: &Value) -> String {
    let n = detail["hands"].as_array().map_or(0, Vec::len);
    let mut out = format!(
        "Hands group {} on {}: {} of {n} succeeded, {} failed, {} not launched{}{}; until {} {}{}. \
         Cost about ${:.4}.\n",
        detail["group"].as_str().unwrap_or_default(),
        detail["backend"].as_str().unwrap_or_default(),
        detail["succeeded"],
        detail["failed"],
        detail["not_launched"],
        match detail["cancelled"].as_u64() {
            Some(c) if c > 0 => format!(", {c} cancelled"),
            _ => String::new(),
        },
        match detail["running"].as_u64() {
            Some(r) if r > 0 => format!(", {r} still being stopped (StopTask sent)"),
            _ => String::new(),
        },
        detail["until"].as_str().unwrap_or_default(),
        if detail["met"].as_bool() == Some(true) { "was met" } else { "was not met" },
        detail["why"]
            .as_str()
            .map_or(String::new(), |w| format!(": {w}")),
        detail["cost_usd"].as_f64().unwrap_or(0.0),
    );
    for h in detail["hands"].as_array().into_iter().flatten() {
        let mut line = format!(
            "#{} {}",
            h["index"],
            h["state"].as_str().unwrap_or_default()
        );
        if let Some(c) = h["exit_code"].as_i64() {
            line.push_str(&format!(" exit {c}"));
        }
        if h["timed_out"].as_bool() == Some(true) {
            line.push_str(" (timed out)");
        }
        if let Some(ms) = h["duration_ms"].as_u64() {
            line.push_str(&format!(" {:.1} s", ms as f64 / 1000.0));
        }
        if let Some(r) = h["result_ref"].as_str() {
            line.push_str(&format!(" {r}"));
        }
        for k in ["error", "note", "cancel"] {
            if let Some(e) = h[k].as_str() {
                line.push_str(&format!(" ({e})"));
            }
        }
        if let Some(t) = h["tail"].as_str() {
            let last = t.trim_end().lines().last().unwrap_or_default();
            if !last.is_empty() {
                let cut: String = last.chars().take(200).collect();
                line.push_str(&format!(": {cut}"));
            }
        }
        out.push_str(&line);
        out.push('\n');
    }
    out
}
