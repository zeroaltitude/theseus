//! The budget question (theseus-0sg): a reservation that did not fit asks
//! the operator whether the execution's spend may go back to $0, and the
//! answer resets it. Moved out of kernel.rs whole (theseus-kp20), unchanged.

use anyhow::Result;
use serde_json::{json, Value};
use theseus_protocol::LedgerKind;

use crate::gate::{digest_proposal, Proposal};
use crate::kernel::*;
use crate::types::*;

impl Kernel {
    /// A reservation did not fit (`OverBudget`): ask the operator whether
    /// the execution's spend may go back to $0 (theseus-0sg). Requires the
    /// held turn. The question is a planned `budget.reset` action with no
    /// reservation that keeps its proposal, like every action that waits for
    /// the operator, answered through the confirm path: `reset_budget` on an
    /// approval, `decline_action` otherwise. The turn then ends waiting on
    /// `Wake::Budget`. An earlier question still open is superseded in the
    /// same frame, so one is open at a time.
    pub fn ask_budget(&self, guard: &TurnGuard, needed_micros: Micros) -> Result<Action> {
        self.ask_budget_for(guard, needed_micros, Value::Null)
    }

    /// `ask_budget`, with what the core knows of the call that did not fit
    /// (`call`: its profile, model, and output cap) kept in the question's
    /// proposal as `args.call`, so every surface that renders the question
    /// later can name them (theseus-kks). `Null` keeps the proposal as
    /// `ask_budget` writes it. The `budget.asked` row says `exceeds_limit`
    /// when the call alone needs more than the whole limit: no reset can
    /// make it fit.
    pub fn ask_budget_for(
        &self,
        guard: &TurnGuard,
        needed_micros: Micros,
        call: Value,
    ) -> Result<Action> {
        let _w = self.lock(&[&guard.execution_id]);
        let mut e = self
            .execution(&guard.execution_id)?
            .ok_or_else(|| KernelError::UnknownExecution(guard.execution_id.clone()))?;
        require_turn(&e)?;
        let now = self.now_ms();
        let mut frame = Vec::new();
        if let Some(old) = e.budget.question.take() {
            if let Some(mut q) = self.action(&old)? {
                if q.state == ActionState::Planned {
                    q.state = ActionState::Cancelled;
                    q.settled_at_ms = Some(now);
                    q.resolution = Some("superseded by a newer budget question".into());
                    frame.push(action_record(&q)?);
                    frame.push(self.ledger(
                        LedgerKind::ActionDeclined,
                        Some(&q.session_id),
                        json!({"correlation_id": q.correlation_id, "tool": q.tool, "by": "harness", "reason": "superseded by a newer budget question"}),
                    )?);
                }
            }
        }
        let b = &e.budget;
        let mut args = json!({"spent_micros": b.spent_micros, "limit_micros": b.limit_micros, "needed_micros": needed_micros, "resets": b.resets});
        if !call.is_null() {
            args["call"] = call;
        }
        let proposal = Proposal {
            tool: BUDGET_TOOL.into(),
            args,
            resource: None,
            policy_context: json!({}),
        };
        let q = Action {
            correlation_id: new_id("act"),
            schema: SCHEMA,
            execution_id: e.id.clone(),
            session_id: e.session_id.clone(),
            tool: BUDGET_TOOL.into(),
            args_digest: digest_proposal(&proposal),
            proposal: Some(proposal),
            resource: None,
            retry_class: RetryClass::NonRepeatable,
            state: ActionState::Planned,
            deadline_at_ms: now + self.config().confirm_ttl_ms,
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
            parent: None,
        };
        let asked = json!({
            "execution_id": e.id,
            "correlation_id": q.correlation_id,
            "spent_usd": micros_to_usd(b.spent_micros),
            "limit_usd": micros_to_usd(b.limit_micros),
            "needed_usd": micros_to_usd(needed_micros),
            "available_usd": micros_to_usd(b.available()),
            "resets": b.resets,
            "exceeds_limit": needed_micros > b.limit_micros,
            // What a reset leaves held (theseus-6g6): when the call cannot
            // fit once the spend is $0, the question says so, and no reset
            // answers it.
            "reserved_usd": micros_to_usd(b.reserved_micros),
            "held_unknown_usd": micros_to_usd(b.held_unknown_micros),
            "fits_after_reset": needed_micros <= b.available_after_reset(),
        });
        e.budget.question = Some(q.correlation_id.clone());
        e.budget.question_needs_micros = needed_micros;
        e.updated_at_ms = now;
        frame.push(exec_record(&e)?);
        frame.push(action_record(&q)?);
        frame.push(self.ledger(
            LedgerKind::ActionPlanned,
            Some(&q.session_id),
            json!({"execution_id": q.execution_id, "correlation_id": q.correlation_id, "tool": q.tool, "args_digest": q.args_digest, "retry_class": q.retry_class, "deadline_at_ms": q.deadline_at_ms, "reserved_usd": 0.0}),
        )?);
        frame.push(self.ledger(LedgerKind::BudgetAsked, Some(&e.session_id), asked)?);
        self.commit(&frame)?;
        Ok(q)
    }

    /// The operator approved a budget question: the execution's spend goes
    /// back to $0 and the waiting call proceeds (theseus-0sg). One frame: the
    /// question settles `Succeeded`; `spent_micros` becomes zero while the
    /// reservations and held amounts stay (they are calls in flight, or not
    /// yet accounted for); `resets` counts one more; the question joins the
    /// results the next turn consumes; and a waiting execution is queued for
    /// the driver. `budget.reset` records who approved
    /// it, the spend before, and the limit. This is the only transition that
    /// lowers spend. Returns the execution and the spend before.
    pub fn reset_budget(&self, correlation_id: &str, by: &str) -> Result<(Execution, Micros)> {
        let (_w, mut q) = self.locked_known_action(correlation_id)?;
        if q.tool != BUDGET_TOOL || q.state != ActionState::Planned {
            return Err(KernelError::ActionState {
                correlation_id: q.correlation_id.clone(),
                state: q.state.as_str(),
                expected: "a planned budget question",
            }
            .into());
        }
        let mut e = self
            .execution(&q.execution_id)?
            .ok_or_else(|| KernelError::UnknownExecution(q.execution_id.clone()))?;
        if e.state.is_terminal() {
            return Err(KernelError::NotRunnable {
                id: e.id.clone(),
                state: e.state.as_str(),
            }
            .into());
        }
        let now = self.now_ms();
        let before = e.budget.spent_micros;
        q.state = ActionState::Succeeded;
        q.settled_at_ms = Some(now);
        q.resolution = Some(format!(
            "approved by {by}: spend reset from {} to $0",
            usd(before)
        ));
        e.budget.spent_micros = 0;
        e.budget.resets += 1;
        e.budget.question = None;
        e.budget.question_needs_micros = 0;
        if !e.queued_results.contains(&q.correlation_id) {
            e.queued_results.push(q.correlation_id.clone());
        }
        // Approved means go on: a waiting execution is queued whatever it
        // waits on (after a crash between asking and parking, it may wait on
        // input with the question still open).
        let woke = e.state == ExecState::Waiting;
        if woke {
            e.state = ExecState::Queued;
            e.wake = None;
            e.resume_pending = true;
        }
        e.updated_at_ms = now;
        let mut frame = vec![
            action_record(&q)?,
            exec_record(&e)?,
            self.ledger(
                LedgerKind::BudgetReset,
                Some(&e.session_id),
                json!({
                    "execution_id": e.id,
                    "correlation_id": q.correlation_id,
                    "by": by,
                    "spent_before_usd": micros_to_usd(before),
                    "limit_usd": micros_to_usd(e.budget.limit_micros),
                    "reserved_usd": micros_to_usd(e.budget.reserved_micros),
                    "held_unknown_usd": micros_to_usd(e.budget.held_unknown_micros),
                    "resets": e.budget.resets,
                }),
            )?,
        ];
        if woke {
            frame.push(self.ledger(
                LedgerKind::ExecutionQueued,
                Some(&e.session_id),
                json!({"execution_id": e.id, "why": "budget_reset"}),
            )?);
        }
        self.commit(&frame)?;
        Ok((e, before))
    }
}
