//! The owner's two words on money the kernel holds or has stopped (issue 6; theseus-x3m9, theseus-cv3v).
//!
//! **Reopen** (`execution.reopen`): an execution that ended at its dollar limit (`budget_exhausted`)
//! was a dead end: only the old unit budget's executions were ever reopened, once, at startup
//! (`reopen.rs`). The owner gives it a new limit, above what it has spent, and it waits on input
//! again; that limit is its own (pinned), so the config's does not take it back, and no default
//! changes. One row, `budget.reopened`, names who, and the old and new limits.
//!
//! **Settle** (`action.settle`): a call whose outcome is unknown holds its reservation as unknown spend
//! (`held_unknown`), which shrinks its execution's room for good, since a reset leaves held what it
//! cannot free, and no verb released it. The owner says what it cost (from the provider's bill) or
//! forgives it (counts it as nothing); the held amount is released into that cost, and one row,
//! `action.resolved`, says who, how much, and that it was the owner's word. Its outcome stays unknown:
//! only the money is settled, and the call reads as counted at that figure, as an earlier process's
//! booked call does, so a result that arrives later adds only what it costs above it.

use anyhow::{bail, Result};
use serde_json::json;
use theseus_protocol::LedgerKind;

use crate::kernel::{action_record, exec_record, Kernel, KernelError};
use crate::types::*;

impl Kernel {
    /// Reopen an execution that ended at its dollar limit with the new limit `to`, which must be
    /// above what it has spent. It waits on input; its limit is pinned to the owner's.
    pub fn reopen_exhausted(&self, execution_id: &str, to: Micros, by: &str) -> Result<Execution> {
        self.require_accepting()?;
        let _w = self.lock_family(execution_id)?;
        let mut e = self
            .execution(execution_id)?
            .ok_or_else(|| KernelError::UnknownExecution(execution_id.into()))?;
        if e.state != ExecState::BudgetExhausted {
            bail!(
                "{execution_id} is {}: only an execution that ended at its dollar limit can be reopened",
                e.state.as_str()
            );
        }
        if to <= e.budget.spent_micros {
            bail!(
                "the new limit, {}, must be above what {execution_id} has spent, {}",
                usd(to),
                usd(e.budget.spent_micros)
            );
        }
        let was = e.budget.limit_micros;
        let ended = e.ended_reason.take();
        e.budget.limit_micros = to;
        e.budget.pinned = true;
        e.state = ExecState::Waiting;
        e.wake = Some(Wake::Input);
        e.resume_pending = false;
        e.updated_at_ms = self.now_ms();
        let row = self.ledger(
            LedgerKind::BudgetReopened,
            Some(&e.session_id),
            json!({"execution_id": e.id, "state": e.state, "wake": e.wake, "by": by,
                   "spent_usd": micros_to_usd(e.budget.spent_micros),
                   "was_limit_usd": micros_to_usd(was), "limit_usd": micros_to_usd(to),
                   "ended_reason": ended}),
        )?;
        self.commit(&[exec_record(&e)?, row])?;
        Ok(e)
    }

    /// Settle a call whose outcome is unknown and whose reservation is held as unknown spend: it counts
    /// as `cost` (nothing, to forgive it), and the held amount is released. Returns the action and the
    /// amount that was held.
    pub fn settle_held_spend(
        &self,
        correlation_id: &str,
        cost: Micros,
        by: &str,
    ) -> Result<(Action, Micros)> {
        self.require_accepting()?;
        let (_w, mut a) = self.locked_known_action(correlation_id)?;
        if a.state != ActionState::OutcomeUnknown {
            return Err(KernelError::ActionState {
                correlation_id: a.correlation_id,
                state: a.state.as_str(),
                expected: "outcome_unknown",
            }
            .into());
        }
        if crate::earlier::booked(&a) || a.reserved_micros == 0 {
            bail!(
                "{correlation_id}'s spend is not held as unknown: it is counted already, or was settled, so there is nothing to settle"
            );
        }
        // A reservation of nothing holds nothing to release, as above.

        let held = a.reserved_micros;
        let day = crate::day_ceiling::Effect::resolved(&a, Some(cost));
        let mut e = self
            .execution(&a.execution_id)?
            .ok_or_else(|| KernelError::UnknownExecution(a.execution_id.clone()))?;
        let spent_before = e.budget.spent_micros;
        crate::earlier::resolve_in(&mut e.budget, &a, Some(cost));
        e.updated_at_ms = self.now_ms();
        // The call now reads as counted at `cost`, as an earlier process's booked call does: a result
        // that arrives later adds only what it costs above that, and a second settle finds it counted.
        a.reserved_micros = cost;
        crate::earlier::mark_booked(&mut a);
        let mut detail = match a.detail.take() {
            Some(serde_json::Value::Object(m)) => m,
            _ => serde_json::Map::new(),
        };
        detail.insert(
            "settled_by_owner".into(),
            json!({"by": by, "cost_usd": micros_to_usd(cost), "held_usd": micros_to_usd(held)}),
        );
        if a.tool == PROVIDER_TOOL {
            // The day's cautious basis never decreases when the owner forgives a bill.
            detail.insert("day_counted_micros".into(), held.max(cost).into());
        }
        a.detail = Some(serde_json::Value::Object(detail));
        let mut frame = vec![exec_record(&e)?];
        self.carry_to_parent(&e, spent_before, &mut frame)?;
        frame.push(action_record(&a)?);
        let mut data = json!({"correlation_id": a.correlation_id, "execution_id": e.id, "outcome": "unknown",
                   "owner": true, "by": by, "cost_usd": micros_to_usd(cost),
                   "held_usd": micros_to_usd(held), "forgiven": cost == 0});
        crate::day_ceiling::Effect::stamp(&mut data, &day);
        frame.push(self.ledger(LedgerKind::ActionResolved, Some(&a.session_id), data)?);
        self.commit(&frame)?;
        self.day_after(day);
        Ok((a, held))
    }
}
