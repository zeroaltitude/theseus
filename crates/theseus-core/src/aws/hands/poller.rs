//! The completion queue's poller (AWS design §3.3, "Completions come home
//! over SQS"; spec §3.16).
//!
//! - **When.** It starts after serving (`Core::poll_hands_after_serving`)
//!   and polls only while a group is open: an `aws.hands.run` call, or one
//!   of its hands, dispatched and not settled, read from the kernel's open
//!   actions. With none it waits for a launch's wake and sends nothing.
//!   While hands run it long-polls (20 s), about three requests a minute.
//! - **Every message is checked.** A hand's envelope is settled only when
//!   its signature is the key derived for its correlation id; one that fails
//!   is quarantined (a `quarantine:` completion record and a
//!   `completion.quarantined` row with the reason, which health counts),
//!   never settled. A Lambda failure-destination record counts only when its
//!   request carries the hand's own key. An ECS task that stopped with its
//!   hand's container failing makes the hand unknown, which its envelope, if
//!   it comes, resolves.
//! - **Settling uses the kernel's path** (`accept_completion`): idempotent
//!   by correlation id, so a duplicate is a logged no-op and a late one after
//!   a cancel is recorded as such; then the group takes its step.
//! - A message is deleted once what it brought is in the store. Anything
//!   else on the queue (a budget alert, another source) is left for its own
//!   reader and, unread, for the dead-letter queue.

use std::collections::BTreeSet;
use std::sync::{Arc, Weak};
use std::time::Duration;

use anyhow::{anyhow, Result};
use serde_json::{json, Value};
use theseus_kernel::{terms, Accepted, ActionState, Completion, Outcome};
use theseus_store::{kinds, NewRecord};

use super::envelope::{derive_key, Envelope, HandSpec};
use super::group::{self, GroupRecord};
use super::launch::{self, HAND, RUN};
use crate::aws::session::Kind;
use crate::aws::{Account, Aws, Request, Signer};
use crate::fact;
use crate::Core;

/// A long poll's wait.
pub const WAIT_SECS: u64 = 20;

/// After a failed receive, the poller waits this long.
const BACKOFF: Duration = Duration::from_secs(5);

/// How long a message stays hidden once received: past it, a message not
/// deleted (one this poller does not read) shows again.
const VISIBILITY_SECS: u64 = 300;

/// The kernel's key prefix for a quarantined completion.
const QUARANTINE: &str = "quarantine:";

/// A completion that failed its check: quarantined, never settled.
pub struct Quarantined<'a> {
    pub completion: &'a Completion,
    pub why: &'a str,
}

impl fact::Fact for Quarantined<'_> {
    const KIND: Option<theseus_protocol::LedgerKind> =
        Some(theseus_protocol::LedgerKind::CompletionQuarantined);

    fn row(&self) -> Value {
        json!({"correlation_id": self.completion.correlation_id,
            "producer": self.completion.producer, "outcome": self.completion.outcome,
            "why": self.why})
    }
}

impl Core {
    /// Start the poller, after serving: idle until a group is open.
    pub fn poll_hands_after_serving(self: &Arc<Self>) {
        if let Some(aws) = self.tools.aws.clone() {
            tokio::spawn(run(Arc::downgrade(self), aws));
        }
    }
}

/// The open groups: each whose `aws.hands.run` call, or one of whose hands,
/// is dispatched and not settled (a hand still running after its group is
/// done comes home too), by its record.
pub fn open_groups(core: &Core) -> Result<Vec<GroupRecord>> {
    let open = core
        .kernel
        .actions_by(&[terms::one(&terms::action_state(ActionState::Dispatched))])?;
    let groups: BTreeSet<&str> = open
        .iter()
        .filter_map(|a| match a.tool.as_str() {
            RUN => Some(a.correlation_id.as_str()),
            HAND => a.resource.as_deref()?.strip_prefix(group::PREFIX),
            _ => None,
        })
        .collect();
    let mut out = Vec::new();
    for g in groups {
        if let Some(rec) = GroupRecord::load(&core.store, g)? {
            out.push(rec);
        }
    }
    Ok(out)
}

/// The poller's loop. It holds the core while it reads or writes the store
/// and takes a batch, never across a poll's wait.
pub async fn run(core: Weak<Core>, aws: Arc<Aws>) {
    let mut first = true;
    loop {
        let Some(c) = core.upgrade() else { return };
        let open = open_groups(&c).unwrap_or_else(|e| {
            tracing::warn!(error = %format!("{e:#}"), "hands: reading the open groups failed");
            Vec::new()
        });
        if std::mem::take(&mut first) {
            recover(&c, &aws, &open).await;
        }
        drop(c);
        if open.is_empty() {
            aws.hands.wake.notified().await;
            continue;
        }
        let queues: BTreeSet<(String, String, String)> = open
            .iter()
            .map(|g| (g.account.clone(), g.region.clone(), g.env.queue_url.clone()))
            .collect();
        for (account, region, url) in queues {
            if !poll_once(&core, &aws, &account, &region, &url).await {
                return;
            }
        }
    }
}

/// After a restart, a group whose hands all settled before its own step
/// was written takes it now.
async fn recover(core: &Core, aws: &Arc<Aws>, open: &[GroupRecord]) {
    for g in open {
        step(core, aws, &g.group).await;
    }
}

/// One receive from one queue, and each message taken. False once the core
/// is gone.
async fn poll_once(
    core: &Weak<Core>,
    aws: &Arc<Aws>,
    account: &str,
    region: &str,
    url: &str,
) -> bool {
    let Ok(acct) = aws.account(Some(account)).cloned() else {
        return true;
    };
    let msgs = match receive(&acct, region, url).await {
        Ok(m) => m,
        Err(e) => {
            tracing::warn!(account, error = %e, "hands: the completion queue");
            tokio::time::sleep(BACKOFF).await;
            return true;
        }
    };
    let Some(c) = core.upgrade() else {
        return false;
    };
    for (receipt, body) in sorted(msgs) {
        take(&c, aws, &acct, (region, url), &receipt, &body).await;
    }
    true
}

/// Take one message, and delete it once what it brought is in the store.
async fn take(
    core: &Core,
    aws: &Arc<Aws>,
    acct: &Arc<Account>,
    (region, url): (&str, &str),
    receipt: &str,
    body: &Value,
) {
    match handle(core, aws, acct, body).await {
        Ok(true) => {
            if let Err(e) = delete(acct, region, url, receipt).await {
                tracing::warn!(error = %e, "hands: a message was not deleted; it comes again, and settles once");
            }
        }
        Ok(false) => {}
        Err(e) => {
            tracing::warn!(error = %format!("{e:#}"), "hands: a message was not taken; it comes again");
        }
    }
}

/// What a message is.
#[derive(Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Sort {
    Envelope,
    LambdaFailure,
    EcsTask,
    Other,
}

fn sort_of(body: &Value) -> Sort {
    if body.get("signature").is_some() && body.get("correlation_id").is_some() {
        Sort::Envelope
    } else if body["requestPayload"].get("correlation_id").is_some()
        && body.get("requestContext").is_some()
    {
        Sort::LambdaFailure
    } else if body["source"] == "aws.ecs" && body["detail-type"] == "ECS Task State Change" {
        Sort::EcsTask
    } else {
        Sort::Other
    }
}

/// A batch in the order it is taken: envelopes first, so a task's stop
/// that came with its hand's envelope never makes the hand unknown.
fn sorted(msgs: Vec<(String, Value)>) -> Vec<(String, Value)> {
    let mut m = msgs;
    m.sort_by_key(|(_, b)| sort_of(b));
    m
}

/// One receive: at most ten messages, waiting up to `WAIT_SECS`. Each is
/// its receipt and its body as JSON (null when it is not JSON).
async fn receive(
    acct: &Arc<Account>,
    region: &str,
    url: &str,
) -> Result<Vec<(String, Value)>, String> {
    let input = json!({"QueueUrl": url, "MaxNumberOfMessages": 10,
        "WaitTimeSeconds": WAIT_SECS, "VisibilityTimeout": VISIBILITY_SECS});
    let out = acct
        .request(
            None,
            &Request {
                service: "sqs",
                operation: "ReceiveMessage",
                input: &input,
                region,
                pages: 1,
                class: "write",
                signer: Signer::As(Kind::Work),
            },
        )
        .await
        .map_err(|e| e.to_string())?;
    Ok(out.body["Messages"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|m| {
            let body = m["Body"]
                .as_str()
                .and_then(|b| serde_json::from_str(b).ok())
                .unwrap_or(Value::Null);
            (
                m["ReceiptHandle"].as_str().unwrap_or_default().to_string(),
                body,
            )
        })
        .collect())
}

async fn delete(acct: &Arc<Account>, region: &str, url: &str, receipt: &str) -> Result<(), String> {
    let input = json!({"QueueUrl": url, "ReceiptHandle": receipt});
    acct.request(
        None,
        &Request {
            service: "sqs",
            operation: "DeleteMessage",
            input: &input,
            region,
            pages: 1,
            class: "write",
            signer: Signer::As(Kind::Work),
        },
    )
    .await
    .map(|_| ())
    .map_err(|e| e.to_string())
}

/// The key a hand's correlation id derives.
async fn key_for(
    acct: &Arc<Account>,
    correlation_id: &str,
) -> Result<zeroize::Zeroizing<[u8; 32]>> {
    let root = acct
        .root_key()
        .await
        .map_err(|e| anyhow!("no key to check a completion with: {e}"))?;
    Ok(derive_key(root.expose_secret().as_bytes(), correlation_id))
}

/// Take one message. Whether it is done with (and deleted).
pub async fn handle(
    core: &Core,
    aws: &Arc<Aws>,
    acct: &Arc<Account>,
    body: &Value,
) -> Result<bool> {
    match sort_of(body) {
        Sort::Envelope => envelope(core, aws, acct, body).await,
        Sort::LambdaFailure => lambda_failure(core, aws, acct, body).await,
        Sort::EcsTask => ecs_task(core, aws, body).await,
        Sort::Other => Ok(false),
    }
}

/// Quarantine a completion that failed its check, with why: its record
/// and its row in one frame. It never settles anything.
pub fn quarantine(core: &Core, c: &Completion, why: &str) -> Result<()> {
    tracing::warn!(correlation_id = %c.correlation_id, producer = %c.producer, why, "hands: a completion quarantined");
    let records = [
        NewRecord::json(
            kinds::COMPLETION,
            Some(&format!("{QUARANTINE}{}", c.correlation_id)),
            c,
        )?,
        fact::row(&Quarantined { completion: c, why }, None, None)?,
    ];
    core.store.append(&records)?;
    Ok(())
}

/// A hand's envelope: checked, then settled, then its group's step.
async fn envelope(core: &Core, aws: &Arc<Aws>, acct: &Arc<Account>, body: &Value) -> Result<bool> {
    let e: Envelope = match serde_json::from_value(body.clone()) {
        Ok(e) => e,
        Err(err) => {
            let c = unreadable(body, "hand:?");
            quarantine(core, &c, &format!("an envelope that does not read: {err}"))?;
            return Ok(true);
        }
    };
    let key = key_for(acct, &e.correlation_id).await?;
    let action = core.kernel.action(&e.correlation_id)?;
    let rec = GroupRecord::load(&core.store, &e.group)?;
    let c = completion_of(&e, rec.as_ref());
    if !e.verify(key.as_ref()) {
        quarantine(core, &c, "its signature is not its hand's key's")?;
        return Ok(true);
    }
    let resource = format!("{}{}", group::PREFIX, e.group);
    match &action {
        // No record: the kernel quarantines it.
        None => {
            core.kernel.accept_completion(&c)?;
            return Ok(true);
        }
        Some(a) if a.tool != HAND || a.resource.as_deref() != Some(resource.as_str()) => {
            quarantine(
                core,
                &c,
                "it names an action that is not a hand of its group",
            )?;
            return Ok(true);
        }
        Some(_) => {}
    }
    let accepted = core.kernel.accept_completion(&c)?;
    tracing::info!(correlation_id = %e.correlation_id, result = ?accepted, "hands: a completion");
    if matches!(
        accepted,
        Accepted::Settled { .. } | Accepted::ResolvedUnknown { .. }
    ) {
        step(core, aws, &e.group).await;
    }
    Ok(true)
}

/// The completion an envelope brings: its outcome, its result's place, its
/// signature, and its cost at its backend's rate for its duration.
fn completion_of(e: &Envelope, rec: Option<&GroupRecord>) -> Completion {
    let secs = e.finished_at_ms.saturating_sub(e.started_at_ms) as f64 / 1000.0;
    let cost = rec.map(|r| launch::cost_usd(r.backend, &r.request, secs, r.env.lambda_memory_mb));
    Completion {
        correlation_id: e.correlation_id.clone(),
        outcome: match e.outcome.as_str() {
            "succeeded" => Outcome::Succeeded,
            "failed" => Outcome::Failed,
            _ => Outcome::Unknown,
        },
        result_ref: e.result_ref.clone(),
        external_op_id: e.external_op_id.clone(),
        started_at_ms: e.started_at_ms,
        finished_at_ms: e.finished_at_ms,
        producer: e.producer.clone(),
        signature: Some(e.signature.clone()),
        cost_micros: cost.map(|u| (u * 1e6).round() as u64),
        detail: Some(
            json!({"group": e.group, "index": e.index, "exit_code": e.exit_code,
            "timed_out": e.timed_out, "tail": e.tail, "note": e.note}),
        ),
    }
}

/// A message that cannot be read as what it claims, as a completion to
/// quarantine.
fn unreadable(body: &Value, producer: &str) -> Completion {
    let now = theseus_protocol::now_unix_ms();
    Completion {
        correlation_id: body["correlation_id"]
            .as_str()
            .unwrap_or("unknown")
            .to_string(),
        outcome: Outcome::Unknown,
        result_ref: None,
        external_op_id: None,
        started_at_ms: now,
        finished_at_ms: now,
        producer: producer.into(),
        signature: None,
        cost_micros: None,
        detail: None,
    }
}

/// Lambda's failure destination: the async invocation failed (the hand
/// could not send its envelope, or never ran). It counts only when its
/// request carries the hand's own key.
async fn lambda_failure(
    core: &Core,
    aws: &Arc<Aws>,
    acct: &Arc<Account>,
    body: &Value,
) -> Result<bool> {
    let Ok(spec) = serde_json::from_value::<HandSpec>(body["requestPayload"].clone()) else {
        return Ok(false);
    };
    let key = key_for(acct, &spec.correlation_id).await?;
    let ctx = &body["requestContext"];
    let now = theseus_protocol::now_unix_ms();
    let why = format!(
        "the Lambda hand failed ({}): {}",
        ctx["condition"].as_str().unwrap_or("an error"),
        body["responsePayload"]["errorMessage"]
            .as_str()
            .unwrap_or("no message")
    );
    let c = Completion {
        correlation_id: spec.correlation_id.clone(),
        outcome: Outcome::Failed,
        result_ref: None,
        external_op_id: ctx["requestId"].as_str().map(String::from),
        started_at_ms: now,
        finished_at_ms: now,
        producer: "lambda:failure-destination".into(),
        signature: None,
        cost_micros: None,
        detail: Some(json!({"group": spec.group, "index": spec.index, "error": why})),
    };
    if spec.key != hex::encode(*key) {
        quarantine(
            core,
            &c,
            "a failure record whose request does not carry its hand's key",
        )?;
        return Ok(true);
    }
    let accepted = core.kernel.accept_completion(&c)?;
    if matches!(
        accepted,
        Accepted::Settled { .. } | Accepted::ResolvedUnknown { .. }
    ) {
        step(core, aws, &spec.group).await;
    }
    Ok(true)
}

/// ECS's task state change: a hand's task that stopped with its hand's
/// container failing (it never sent its envelope, or could not start) makes
/// the hand unknown. One whose hand exited 0 sent its envelope: nothing to
/// do.
async fn ecs_task(core: &Core, aws: &Arc<Aws>, body: &Value) -> Result<bool> {
    let d = &body["detail"];
    let Some(group) = d["startedBy"].as_str() else {
        return Ok(false);
    };
    let Some(rec) = GroupRecord::load(&core.store, group)? else {
        return Ok(false);
    };
    if d["lastStatus"] != "STOPPED" {
        return Ok(true);
    }
    let task = d["taskArn"].as_str().unwrap_or_default();
    let Some(hand) = rec
        .hands
        .iter()
        .find(|h| h.external_op_id.as_deref() == Some(task))
    else {
        return Ok(true);
    };
    let exit = d["containers"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|c| c["name"] == "hand")
        .and_then(|c| c["exitCode"].as_i64());
    if exit == Some(0) {
        return Ok(true);
    }
    let still = core
        .kernel
        .action(&hand.correlation_id)?
        .is_some_and(|a| a.state == ActionState::Dispatched);
    if still {
        let why = format!(
            "its task stopped before its hand reported ({})",
            d["stoppedReason"].as_str().unwrap_or("no reason given")
        );
        core.kernel.mark_unknown(&hand.correlation_id, &why)?;
        step(core, aws, group).await;
    }
    Ok(true)
}

/// The group's step after one of its hands settled.
async fn step(core: &Core, aws: &Arc<Aws>, group: &str) {
    let ctx = group::Ctx {
        kernel: &core.kernel,
        store: &core.store,
        aws,
    };
    if let Err(e) = group::step(&ctx, group).await {
        tracing::warn!(group, error = %format!("{e:#}"), "hands: a step");
    }
}
