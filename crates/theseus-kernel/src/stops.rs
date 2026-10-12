//! Stopping a conversation's work while keeping the conversation (W1,
//! theseus-lji; spec §3.15): `/stop` halts what the session is doing now, and
//! its next message continues the same session.
//!
//! - **What stops.** Every job and tool call the execution has running is
//!   told to stop, as a cancel tells it (`cancel = requested`; the caller
//!   terminates their backends). Every call planned but not yet sent, and
//!   every approval and budget question still open, is declined, so nothing
//!   the stopped work asked for runs later. A turn running now plans nothing
//!   more (`KernelError::Stopped`), and its end parks the execution on input
//!   (`end_turn_with`), as startup does after a crash.
//! - **What goes on.** The execution stays open, and with it the session, its
//!   history, its budget and spend, its pending wakes, and its tasks, which
//!   only their own cancel stops. The model call a running turn waits on is
//!   not an action to terminate here: the core, which holds its stream, cuts
//!   it (theseus-yey) and settles it at an estimate of what it used; a call
//!   that has already answered is kept, and the turn acts on nothing it says.
//!   A job's result, once it is stopped, reaches the next turn as a late
//!   result that says it was cancelled.
//! - **Once.** A stop of an execution that has ended writes nothing; a second
//!   stop finds nothing more to stop, and writes its row only.
//! - **Not a task.** A stopped task would wait for input no one sends, so a
//!   task is stopped by its cancel instead (`KernelError::StopTask`).

use anyhow::Result;
use serde_json::json;
use theseus_protocol::LedgerKind;

use crate::kernel::{action_record, exec_record, settle_reservation_in, Kernel, KernelError};
use crate::types::*;

/// What `stop_execution` did.
#[derive(Debug, Clone)]
pub struct Stop {
    /// The execution as the stop left it.
    pub execution: Execution,
    /// The running jobs and calls now told to stop: the caller terminates
    /// their backends and walks each one's cancel, as for a cancel.
    pub to_kill: Vec<CorrelationId>,
    /// The planned calls, approvals, and budget question that will not run.
    pub declined: Vec<Action>,
    /// A turn held the execution: it ends at its next step.
    pub turn_running: bool,
}

impl Kernel {
    /// `/stop` (W1): halt `execution_id`'s work for `by`, in one frame with an
    /// `execution.stopped` row, and keep the execution open. `None` when it
    /// has ended: nothing is written. A task is refused (`StopTask`).
    pub fn stop_execution(&self, execution_id: &str, by: &str) -> Result<Option<Stop>> {
        let _w = self.lock_family(execution_id)?;
        let mut e = self
            .execution(execution_id)?
            .ok_or_else(|| KernelError::UnknownExecution(execution_id.into()))?;
        if e.parent.is_some() || e.kind == SessionKind::Task {
            return Err(KernelError::StopTask { id: e.id }.into());
        }
        if e.state.is_terminal() {
            return Ok(None);
        }
        let now = self.now_ms();
        let mut frame = Vec::new();
        // Running jobs and calls are told to stop. The model call is not one
        // of them: its stream is the core's to cut (theseus-yey), which then
        // settles it itself.
        let mut to_kill = Vec::new();
        for c in &e.outstanding {
            if let Some(mut a) = self.action(c)? {
                if a.state == ActionState::Dispatched
                    && a.cancel.is_none()
                    && a.tool != PROVIDER_TOOL
                {
                    a.cancel = Some(CancelState::Requested);
                    // What the next turn reads as its result says who.
                    a.resolution = Some(format!("stopped by {by}"));
                    frame.push(action_record(&a)?);
                    to_kill.push(a.correlation_id.clone());
                }
            }
        }
        // Whatever it planned and has not sent, or still asks the operator,
        // never runs: declined, with its reservation released.
        let mut declined = Vec::new();
        for mut a in self
            .unsettled_actions(&e.id)?
            .into_iter()
            .filter(|a| matches!(a.state, ActionState::Planned | ActionState::Authorized))
        {
            a.state = ActionState::Cancelled;
            a.settled_at_ms = Some(now);
            a.resolution = Some(format!("stopped by {by}"));
            if let Some(r) = &a.reservation_id {
                settle_reservation_in(&mut e.budget, r, Some(0));
                self.day_after(crate::day_ceiling::Effect::of(&a, Some(0)));
            }
            frame.push(action_record(&a)?);
            frame.push(self.ledger(
                LedgerKind::ActionDeclined,
                Some(&a.session_id),
                json!({"correlation_id": a.correlation_id, "tool": a.tool, "by": by, "reason": "stopped"}),
            )?);
            declined.push(a);
        }
        e.budget.question = None;
        e.budget.question_needs_micros = 0;
        let before = e.state;
        // A turn held and not yet admitted (theseus-2uby) is marked as a
        // running one is: its admission keeps the mark, so its first step is
        // refused, where parking the execution here would leave it for the
        // admission's wake to undo.
        let turn_running = e.state == ExecState::Running || self.holds_turn(&e.id);
        if turn_running {
            // The turn holds the execution: its next step is refused, and its
            // end parks it on input.
            if e.stopped.is_none() {
                e.stopped = Some(Stopped {
                    by: by.to_string(),
                    at_ms: now,
                    turn: e.turns,
                });
            }
        } else {
            e.state = ExecState::Waiting;
            e.wake = Some(Wake::Input);
            e.resume_pending = false;
        }
        e.updated_at_ms = now;
        frame.insert(0, exec_record(&e)?);
        frame.push(self.ledger(
            LedgerKind::ExecutionStopped,
            Some(&e.session_id),
            json!({"execution_id": e.id, "by": by, "state_before": before, "turn_running": turn_running,
                   "turn": e.turns, "outstanding": to_kill,
                   "declined": declined.iter().map(|a| a.correlation_id.as_str()).collect::<Vec<_>>(),
                   "queued_results": e.queued_results.len(), "wakes_pending": e.wakes.len()}),
        )?);
        self.commit(&frame)?;
        Ok(Some(Stop {
            execution: e,
            to_kill,
            declined,
            turn_running,
        }))
    }
}

impl Kernel {
    /// Tell one running job or call to stop, for `by`, and leave its
    /// execution as it is (theseus-ht82): the daemon stopping a job below the
    /// disk's floor, where `/stop` and a cancel stop a whole execution's. The
    /// action, dispatched and not yet told, is marked `cancel = requested`,
    /// its resolution `stopped by <by>`, which the call's result reads as, in
    /// one frame with an `action.cancel` row. The caller terminates its
    /// backend and walks its cancel (`cancel_acknowledged`, then
    /// `cancel_verified` or `cancel_uncertain`). None when it is not a running
    /// call, or was told to stop already: nothing is written.
    pub fn stop_call(&self, correlation_id: &str, by: &str) -> Result<Option<Action>> {
        let Some((_w, mut a)) = self.locked_action(correlation_id)? else {
            return Ok(None);
        };
        if a.state != ActionState::Dispatched || a.cancel.is_some() || a.tool == PROVIDER_TOOL {
            return Ok(None);
        }
        a.cancel = Some(CancelState::Requested);
        a.resolution = Some(format!("stopped by {by}"));
        self.commit(&[
            action_record(&a)?,
            self.ledger(
                LedgerKind::ActionCancel,
                Some(&a.session_id),
                json!({"correlation_id": a.correlation_id, "cancel": CancelState::Requested, "why": by, "settled": false}),
            )?,
        ])?;
        Ok(Some(a))
    }
}
