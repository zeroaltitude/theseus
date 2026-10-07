//! Reopening an execution the old unit budget ended (theseus-3ebd, the owner's
//! option (a), 2026-10-01): startup's step 2 does it, once, beside the unit
//! budget's migration to dollars (theseus-0sg).

use anyhow::Result;
use serde_json::json;
use theseus_protocol::LedgerKind;

use crate::kernel::Kernel;
use crate::types::*;

/// An execution the old unit budget ended whose dollar spend is under its
/// limit: startup reopens it to wait on input, as if the operator had reset
/// it. Budgets in dollars end nothing (a call that does not fit asks the
/// operator), so only a budget migrated from units (`units_before`) can be
/// here, and one at or over its dollar limit stays ended.
pub(crate) fn reopens(e: &Execution) -> bool {
    e.state == ExecState::BudgetExhausted
        && e.budget.units_before.is_some()
        && e.budget.spent_micros < e.budget.limit_micros
}

impl Kernel {
    /// Reopen `e`, read under its lock in step 2, when it `reopens`: it waits
    /// on input, its ended reason cleared, and a `budget.reopened` row, which
    /// keeps that reason, joins the step's frame (`rows`). It runs after the
    /// migration, so its spend is in dollars, and before the limit is
    /// followed, which an open execution does.
    pub(crate) fn reopen(
        &self,
        e: &mut Execution,
        now: u64,
        rows: &mut Vec<theseus_store::NewRecord>,
        reopened: &mut Vec<ExecutionId>,
    ) -> Result<()> {
        if !reopens(e) {
            return Ok(());
        }
        let ended = e.ended_reason.take();
        e.state = ExecState::Waiting;
        e.wake = Some(Wake::Input);
        e.resume_pending = false;
        e.updated_at_ms = now;
        rows.push(self.ledger(
            LedgerKind::BudgetReopened,
            Some(&e.session_id),
            json!({"execution_id": e.id, "state": e.state, "wake": e.wake,
                   "spent_usd": micros_to_usd(e.budget.spent_micros),
                   "limit_usd": micros_to_usd(e.budget.limit_micros),
                   "ended_reason": ended, "units_before": e.budget.units_before}),
        )?);
        reopened.push(e.id.clone());
        Ok(())
    }
}
