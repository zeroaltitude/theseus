//! A cancel's lifecycle on one action (§3.16): acknowledged, then settled
//! as verified, unsupported, or uncertain. Since M4 18a the settle records how
//! the cancel was verified, and what its stop counted (`verified_by`,
//! `killed`, `survivors`).

use anyhow::Result;
use serde_json::json;
use theseus_protocol::LedgerKind;

use crate::kernel::{
    action_record, exec_record, hold_reservation_in, settle_reservation_in, Kernel,
};
use crate::types::*;

impl Kernel {
    /// The backend acknowledged the cancel (signal delivered, stop requested).
    pub fn cancel_acknowledged(&self, correlation_id: &str) -> Result<Action> {
        self.cancel_step(correlation_id, CancelState::Acknowledged, false, None)
    }
    /// Termination verified (process gone, task stopped): the action settles
    /// `Cancelled`, with its verdict: how it was verified, and what its stop
    /// counted (18a). `None`: no backend ran (a job stopped before its launch).
    pub fn cancel_verified(&self, correlation_id: &str, how: Option<&Verdict>) -> Result<Action> {
        self.cancel_step(
            correlation_id,
            CancelState::TerminationVerified,
            true,
            how.cloned(),
        )
    }
    /// `cancel_verified` for a call that ran, and cost, until its stop: its
    /// reservation settles at `cost_micros`, not at nothing (a hand's ECS
    /// task, billed until it stopped; step 40 part 2).
    pub fn cancel_verified_costing(
        &self,
        correlation_id: &str,
        how: &Verdict,
        cost_micros: Micros,
    ) -> Result<Action> {
        self.cancel_step_costing(
            correlation_id,
            CancelState::TerminationVerified,
            true,
            Some(how.clone()),
            Some(cost_micros),
        )
    }
    /// The backend offers no external termination; the action settles
    /// `Cancelled` but the side effect may still complete (`LateAfterCancel`).
    /// `why`: what kept it from being reached (18a).
    pub fn cancel_unsupported(&self, correlation_id: &str, why: &str) -> Result<Action> {
        let v = Verdict::uncertain(VerifiedBy::None, why);
        self.cancel_step(correlation_id, CancelState::Unsupported, true, Some(v))
    }
    /// Told to stop, and its end not verified (18a: the verdict says why).
    pub fn cancel_uncertain(&self, correlation_id: &str, how: &Verdict) -> Result<Action> {
        self.cancel_step(
            correlation_id,
            CancelState::OutcomeUncertain,
            true,
            Some(how.clone()),
        )
    }

    /// One step of a cancel's lifecycle; `verdict`, its last step's.
    fn cancel_step(
        &self,
        correlation_id: &str,
        st: CancelState,
        settle: bool,
        verdict: Option<Verdict>,
    ) -> Result<Action> {
        self.cancel_step_costing(correlation_id, st, settle, verdict, None)
    }

    /// `cancel_step`; `cost`: what a verified stop's call cost (nothing when
    /// `None`).
    fn cancel_step_costing(
        &self,
        correlation_id: &str,
        st: CancelState,
        settle: bool,
        verdict: Option<Verdict>,
        cost: Option<Micros>,
    ) -> Result<Action> {
        let (_w, mut a) = self.locked_known_action(correlation_id)?;
        if a.state.is_settled() {
            return Ok(a);
        }
        let now = self.now_ms();
        a.cancel = Some(st);
        if verdict.is_some() {
            a.verdict = verdict;
        }
        let mut frame = Vec::new();
        if settle {
            a.state = ActionState::Cancelled;
            a.settled_at_ms = Some(now);
            if let Some(mut e) = self.execution(&a.execution_id)? {
                let spent_before = e.budget.spent_micros;
                e.outstanding.retain(|x| x != &a.correlation_id);
                if let Some(r) = &a.reservation_id {
                    if st == CancelState::TerminationVerified {
                        settle_reservation_in(&mut e.budget, r, Some(cost.unwrap_or(0)));
                    } else {
                        hold_reservation_in(&mut e.budget, r);
                    }
                }
                // A stop (W1) leaves the execution open: its next turn reads
                // the call as cancelled, as it reads any late result. Nothing
                // queues the execution for it.
                if !e.state.is_terminal() && !e.queued_results.contains(&a.correlation_id) {
                    e.queued_results.push(a.correlation_id.clone());
                }
                e.updated_at_ms = now;
                frame.push(exec_record(&e)?);
                self.carry_to_parent(&e, spent_before, &mut frame)?;
            }
        }
        frame.push(action_record(&a)?);
        frame.push(self.ledger(
            LedgerKind::ActionCancel,
            Some(&a.session_id),
            json!({"correlation_id": a.correlation_id, "cancel": st, "settled": settle}),
        )?);
        self.commit(&frame)?;
        Ok(a)
    }
}
