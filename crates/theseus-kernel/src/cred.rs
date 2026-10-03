//! Credential requests under L1 (M4 18d; design `m4-boundaries` §2.4,
//! decision 15): a job's request for a secret, as an action of the job's
//! execution whose `parent` is the job's call (ACTION schema 4).
//!
//! Like a held post's question (19c), a request runs outside every turn: no
//! turn is held, nothing is reserved, and it is never dispatched, so it is
//! never outstanding, queues no result, and wakes nothing. It is planned, and
//! then it settles:
//! - granted at once (open, notify): planned and settled `succeeded` in the
//!   one frame that decides it;
//! - approve: planned with its proposal, so it waits for the operator
//!   (`awaits_confirm`) until the job's own deadline; `grant_cred` settles it
//!   on an approval, and `decline_action` on a decline, a lapse, or the job's
//!   end.

use anyhow::Result;
use serde_json::{json, Value};
use theseus_protocol::{LedgerKind, CRED_TOOL};
use theseus_store::NewRecord;

use crate::gate::{digest_proposal, Proposal};
use crate::kernel::{Kernel, KernelError};
use crate::types::*;

impl Kernel {
    /// A request of `job`'s for the secret `secret` (`args` says what the
    /// operator is asked), staged for the caller's frame with its rows: its
    /// `action.planned`, and, when `granted`, its settle (`succeeded`,
    /// `granted by <by>`) with its `action.succeeded`. Its deadline is the
    /// job's.
    pub fn cred_request_stage(
        &self,
        job: &Action,
        secret: &str,
        args: Value,
        granted: Option<&str>,
    ) -> Result<(Action, Vec<NewRecord>)> {
        let now = self.now_ms();
        let proposal = Proposal {
            tool: CRED_TOOL.into(),
            args,
            resource: Some(secret.into()),
            policy_context: json!({}),
        };
        let mut a = Action {
            correlation_id: new_id("act"),
            schema: SCHEMA,
            execution_id: job.execution_id.clone(),
            session_id: job.session_id.clone(),
            tool: CRED_TOOL.into(),
            args_digest: digest_proposal(&proposal),
            proposal: Some(proposal),
            resource: Some(secret.into()),
            retry_class: RetryClass::NonRepeatable,
            state: ActionState::Planned,
            deadline_at_ms: job.deadline_at_ms,
            planned_at_ms: now,
            authorized_at_ms: None,
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
            parent: Some(job.correlation_id.clone()),
        };
        let mut records = vec![
            crate::kernel::action_record(&a)?,
            self.ledger(
                LedgerKind::ActionPlanned,
                Some(&a.session_id),
                json!({"execution_id": a.execution_id, "correlation_id": a.correlation_id,
                       "tool": a.tool, "args_digest": a.args_digest, "retry_class": a.retry_class,
                       "deadline_at_ms": a.deadline_at_ms, "parent": a.parent}),
            )?,
        ];
        if let Some(by) = granted {
            records.extend(self.settle_granted(&mut a, by)?);
        }
        Ok((a, records))
    }

    /// A waiting request approved: it settles `succeeded`, approved by `by`,
    /// and the job's socket hands the value over.
    pub fn grant_cred(&self, correlation_id: &str, by: &str) -> Result<Action> {
        let (_w, mut a) = self.locked_known_action(correlation_id)?;
        if a.tool != CRED_TOOL || !a.awaits_confirm() {
            return Err(KernelError::ActionState {
                correlation_id: a.correlation_id.clone(),
                state: a.state.as_str(),
                expected: "a credential request, waiting",
            }
            .into());
        }
        let records = self.settle_granted(&mut a, &format!("approved by {by}"))?;
        self.commit(&records)?;
        Ok(a)
    }

    /// `a` settled succeeded, as `how` says ("granted at notify", "approved
    /// by …"): its record and its `action.succeeded` row.
    fn settle_granted(&self, a: &mut Action, how: &str) -> Result<Vec<NewRecord>> {
        a.state = ActionState::Succeeded;
        a.settled_at_ms = Some(self.now_ms());
        a.resolution = Some(how.to_string());
        Ok(vec![
            crate::kernel::action_record(a)?,
            self.ledger(
                LedgerKind::ActionSucceeded,
                Some(&a.session_id),
                json!({"correlation_id": a.correlation_id, "tool": a.tool, "by": how}),
            )?,
        ])
    }

    /// The requests of the job `job` that still wait for the operator: its
    /// execution's unsettled actions, by their term, never every action.
    pub fn waiting_cred_requests(&self, execution_id: &str, job: &str) -> Result<Vec<Action>> {
        Ok(self
            .unsettled_actions(execution_id)?
            .into_iter()
            .filter(|a| a.tool == CRED_TOOL && a.parent.as_deref() == Some(job))
            .collect())
    }
}
