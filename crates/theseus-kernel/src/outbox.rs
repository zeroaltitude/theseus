//! The outbox (theseus-q4v; spec §3.16): what must reach a channel, as the
//! kernel's actions.
//!
//! A post is written when it becomes true, whether or not anything can
//! deliver it yet: a turn's reply, a confirm card and how it closed, a notice
//! the operator must see. It is an action of its own record kind (`OUTBOX`),
//! planned and authorized in one record (the core wrote it, so no gate stands
//! between), with what to post in its proposal and where in its resource. A
//! binding delivers it:
//! - `outbox_dispatch` is committed before the first call (the transactional
//!   outbox);
//! - `outbox_settle` folds the channel's answer (its message ids) into the
//!   action as its completion, idempotently.
//!
//! A crash between the two leaves the action dispatched, and the binding sends
//! it again under the same downstream key (Discord's nonce), so the channel
//! keeps one copy.
//!
//! No execution waits on a post. An outbox action is never outstanding, queues
//! no result, and wakes nothing, and neither a cancel nor the end of its
//! execution touches it. The reconciler, and every reader of `ACTION` records,
//! never see it.

use anyhow::Result;
use serde_json::{json, Value};
use theseus_protocol::LedgerKind;
use theseus_store::{kinds, NewRecord};

use crate::gate::{digest_proposal, Proposal};
use crate::kernel::{Kernel, KernelError};
use crate::types::*;

/// The tool name every outbox action carries.
pub const OUTBOX_TOOL: &str = "outbox";

/// A post to write.
#[derive(Debug, Clone)]
pub struct Post {
    /// The session it belongs to, or "" for a notice about the daemon itself.
    pub session_id: String,
    /// That session's execution, or "".
    pub execution_id: String,
    /// Where it goes: `discord:dm:<user>`, `discord:channel:<id>`, or
    /// `discord:operator` (wherever the operator's approvals go).
    pub target: String,
    /// What it says: `{"kind": "reply" | "card" | "settle" | "notice" | …}`.
    pub body: Value,
    pub retry_class: RetryClass,
}

/// What a settle did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Settled {
    /// The action is succeeded or failed now.
    Now(Action),
    /// It was settled before; nothing changed.
    Already(Action),
}

impl Kernel {
    /// A post's action, planned and authorized in one record, and its ledger
    /// row, for the caller's frame: a turn's reply rides in the frame that
    /// ends the turn, and a card in the frame that plans its question.
    pub fn outbox_stage(&self, post: Post) -> Result<(Action, Vec<NewRecord>)> {
        let now = self.now_ms();
        let kind = post.body["kind"].as_str().unwrap_or("").to_string();
        let proposal = Proposal {
            tool: OUTBOX_TOOL.into(),
            args: post.body,
            resource: Some(post.target.clone()),
            policy_context: Value::Null,
        };
        let a = Action {
            correlation_id: new_id("out"),
            schema: SCHEMA,
            execution_id: post.execution_id,
            session_id: post.session_id,
            tool: OUTBOX_TOOL.into(),
            args_digest: digest_proposal(&proposal),
            proposal: Some(proposal),
            resource: Some(post.target),
            retry_class: post.retry_class,
            state: ActionState::Authorized,
            // A post waits for its channel as long as that takes.
            deadline_at_ms: 0,
            planned_at_ms: now,
            authorized_at_ms: Some(now),
            dispatched_at_ms: None,
            settled_at_ms: None,
            external_op_id: None,
            result_ref: None,
            confirm: None,
            cancel: None,
            verdict: None,
            reservation_id: None,
            reserved_micros: 0,
            resolution: None,
            completions_seen: 0,
            detail: None,
        };
        let row = self.ledger(
            LedgerKind::ActionPlanned,
            scope(&a),
            json!({"correlation_id": a.correlation_id, "execution_id": a.execution_id, "tool": a.tool,
                   "outbox": a.resource, "kind": kind, "retry_class": a.retry_class}),
        )?;
        Ok((a.clone(), vec![outbox_record(&a)?, row]))
    }

    /// `outbox_stage`, committed in a frame of its own.
    pub fn outbox_plan(&self, post: Post) -> Result<Action> {
        let (a, frame) = self.outbox_stage(post)?;
        self.commit(&frame)?;
        Ok(a)
    }

    pub fn outbox_action(&self, correlation_id: &str) -> Result<Option<Action>> {
        match self.store().latest_by_key(kinds::OUTBOX, correlation_id)? {
            Some(r) => Ok(Some(r.decode()?)),
            None => Ok(None),
        }
    }

    /// Every outbox action's latest record, settled or not.
    pub fn outbox_actions(&self) -> Result<Vec<Action>> {
        self.store()
            .latest_of_kind(kinds::OUTBOX)?
            .iter()
            .map(|r| r.decode())
            .collect()
    }

    /// `dispatched`, committed before the first call. A post dispatched
    /// before (a retry, after an error or a restart) stays as it was, with
    /// the time of its first dispatch, and costs no frame.
    pub fn outbox_dispatch(&self, correlation_id: &str) -> Result<Action> {
        let _w = self.locks().lock(&lock_key(correlation_id));
        let mut a = self
            .outbox_action(correlation_id)?
            .ok_or_else(|| KernelError::UnknownAction(correlation_id.into()))?;
        match a.state {
            ActionState::Authorized => {}
            ActionState::Dispatched => return Ok(a),
            other => {
                return Err(KernelError::ActionState {
                    correlation_id: a.correlation_id,
                    state: other.as_str(),
                    expected: "authorized",
                }
                .into())
            }
        }
        a.state = ActionState::Dispatched;
        a.dispatched_at_ms = Some(self.now_ms());
        let row = self.ledger(
            LedgerKind::ActionDispatched,
            scope(&a),
            json!({"correlation_id": a.correlation_id, "tool": a.tool, "outbox": a.resource}),
        )?;
        self.commit(&[outbox_record(&a)?, row])?;
        Ok(a)
    }

    /// Fold a completion into its post: succeeded (what the channel
    /// answered, in `detail`) or failed (a refusal no retry can change).
    /// Idempotent: a post settled before is left as it was. A post settled
    /// without a dispatch never reached the channel (another post took its
    /// place, or there was nothing left to say).
    pub fn outbox_settle(&self, c: &Completion) -> Result<Settled> {
        let _w = self.locks().lock(&lock_key(&c.correlation_id));
        let mut a = self
            .outbox_action(&c.correlation_id)?
            .ok_or_else(|| KernelError::UnknownAction(c.correlation_id.clone()))?;
        if a.state.is_settled() {
            return Ok(Settled::Already(a));
        }
        a.state = match c.outcome {
            Outcome::Succeeded => ActionState::Succeeded,
            Outcome::Failed => ActionState::Failed,
            Outcome::Unknown => anyhow::bail!(
                "outbox post {} settles as succeeded or failed, never unknown",
                c.correlation_id
            ),
        };
        a.settled_at_ms = Some(self.now_ms());
        a.completions_seen += 1;
        if c.external_op_id.is_some() {
            a.external_op_id = c.external_op_id.clone();
        }
        a.detail = c.detail.clone();
        let kind = match a.state {
            ActionState::Succeeded => LedgerKind::ActionSucceeded,
            _ => LedgerKind::ActionFailed,
        };
        let row = self.ledger(
            kind,
            scope(&a),
            json!({"correlation_id": a.correlation_id, "tool": a.tool, "outbox": a.resource,
                   "outcome": c.outcome, "producer": c.producer, "external_op_id": a.external_op_id,
                   "duration_ms": c.finished_at_ms.saturating_sub(c.started_at_ms), "detail": c.detail}),
        )?;
        self.commit(&[outbox_record(&a)?, row])?;
        Ok(Settled::Now(a))
    }
}

/// The lock a post's transitions take: its own, never an execution's.
fn lock_key(correlation_id: &str) -> String {
    format!("outbox:{correlation_id}")
}

fn scope(a: &Action) -> Option<&str> {
    (!a.session_id.is_empty()).then_some(a.session_id.as_str())
}

fn outbox_record(a: &Action) -> Result<NewRecord> {
    let r = NewRecord::json(kinds::OUTBOX, Some(&a.correlation_id), a)?;
    Ok(match scope(a) {
        Some(s) => r.scoped(s),
        None => r,
    })
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use theseus_store::{WalConfig, WalStore};

    use super::*;
    use crate::clock::VirtualClock;
    use crate::kernel::KernelConfig;

    fn kernel(dir: &std::path::Path) -> Kernel {
        let store = Arc::new(WalStore::open(dir, WalConfig::default()).unwrap());
        Kernel::new(store, VirtualClock::new(1_000), KernelConfig::default())
    }

    fn post(target: &str, kind: &str) -> Post {
        Post {
            session_id: "ses_1".into(),
            execution_id: "exe_1".into(),
            target: target.into(),
            body: json!({"kind": kind, "text": "hello"}),
            retry_class: RetryClass::IdempotentWithKey {
                key: "discord.nonce".into(),
            },
        }
    }

    fn done(corr: &str, outcome: Outcome, id: &str) -> Completion {
        Completion {
            correlation_id: corr.into(),
            outcome,
            result_ref: None,
            external_op_id: Some(id.into()),
            started_at_ms: 1_000,
            finished_at_ms: 1_010,
            producer: "discord".into(),
            signature: None,
            cost_micros: None,
            detail: Some(json!({"messages": [{"key": "note", "id": id}]})),
        }
    }

    #[test]
    fn a_post_is_planned_dispatched_and_settled_in_three_frames_and_reads_back_after_a_restart() {
        let dir = tempfile::tempdir().unwrap();
        let k = kernel(dir.path());
        let before = k.store().stats().unwrap().frames_appended;
        let a = k.outbox_plan(post("discord:dm:1", "notice")).unwrap();
        assert_eq!(a.state, ActionState::Authorized);
        assert!(a.correlation_id.starts_with("out_"));
        let d = k.outbox_dispatch(&a.correlation_id).unwrap();
        assert_eq!(d.state, ActionState::Dispatched);
        // A retry of the dispatch keeps the first and writes nothing.
        let again = k.outbox_dispatch(&a.correlation_id).unwrap();
        assert_eq!(again.dispatched_at_ms, d.dispatched_at_ms);
        let s = k
            .outbox_settle(&done(&a.correlation_id, Outcome::Succeeded, "m1"))
            .unwrap();
        assert!(matches!(s, Settled::Now(ref a) if a.state == ActionState::Succeeded));
        assert_eq!(k.store().stats().unwrap().frames_appended - before, 3);
        // A second completion is a no-op.
        let dup = k
            .outbox_settle(&done(&a.correlation_id, Outcome::Succeeded, "m2"))
            .unwrap();
        assert!(
            matches!(dup, Settled::Already(ref a) if a.external_op_id.as_deref() == Some("m1"))
        );
        assert_eq!(k.store().stats().unwrap().frames_appended - before, 3);
        drop(k);
        let k = kernel(dir.path());
        let back = k.outbox_action(&a.correlation_id).unwrap().unwrap();
        assert_eq!(back.state, ActionState::Succeeded);
        assert_eq!(back.detail.unwrap()["messages"][0]["id"], "m1");
        // Kept apart from the kernel's own actions.
        assert!(k.actions().unwrap().is_empty());
        assert_eq!(k.outbox_actions().unwrap().len(), 1);
    }

    #[test]
    fn a_crash_between_dispatch_and_settle_leaves_the_post_dispatched_for_a_retry() {
        let dir = tempfile::tempdir().unwrap();
        let k = kernel(dir.path());
        let a = k.outbox_plan(post("discord:channel:9", "reply")).unwrap();
        let first = k.outbox_dispatch(&a.correlation_id).unwrap();
        drop(k); // the process dies here
        let k = kernel(dir.path());
        let open: Vec<Action> = k
            .outbox_actions()
            .unwrap()
            .into_iter()
            .filter(|a| !a.state.is_settled())
            .collect();
        assert_eq!(open.len(), 1);
        assert_eq!(open[0].state, ActionState::Dispatched);
        let retry = k.outbox_dispatch(&a.correlation_id).unwrap();
        assert_eq!(retry.dispatched_at_ms, first.dispatched_at_ms);
        k.outbox_settle(&done(&a.correlation_id, Outcome::Succeeded, "m1"))
            .unwrap();
        assert!(k
            .outbox_action(&a.correlation_id)
            .unwrap()
            .unwrap()
            .state
            .is_settled());
    }

    #[test]
    fn a_staged_post_is_written_only_by_the_callers_frame_and_a_failure_settles_it() {
        let dir = tempfile::tempdir().unwrap();
        let k = kernel(dir.path());
        let (a, frame) = k.outbox_stage(post("discord:dm:1", "reply")).unwrap();
        assert!(k.outbox_action(&a.correlation_id).unwrap().is_none());
        k.store().append(&frame).unwrap();
        assert!(k.outbox_action(&a.correlation_id).unwrap().is_some());
        // Unknown is no outcome for a post, and a missing post is an error.
        assert!(k
            .outbox_settle(&done(&a.correlation_id, Outcome::Unknown, "x"))
            .is_err());
        assert!(k.outbox_dispatch("out_nope").is_err());
        let failed = k
            .outbox_settle(&done(&a.correlation_id, Outcome::Failed, "x"))
            .unwrap();
        assert!(matches!(failed, Settled::Now(ref a) if a.state == ActionState::Failed));
        // A settled post cannot be dispatched again.
        assert!(k.outbox_dispatch(&a.correlation_id).is_err());
    }
}
