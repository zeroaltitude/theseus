//! Ending an execution's work by a cancel, and the reconciler's pass over every action that may have run.

// Named here and not only through the glob: the reader rule's scan (`tests_registry`) places a `LedgerKind::` use by
// its file's own imports.
use theseus_protocol::LedgerKind;

use super::*;

impl Kernel {
    /// `/cancel`: a deterministic control path (§3.15). Acts directly, never
    /// through admission or Jev. Returns the correlation ids whose backends
    /// must now be terminated.
    pub fn cancel_execution(&self, execution_id: &str, by: &str) -> Result<Vec<CorrelationId>> {
        Ok(self
            .cancel(execution_id, by)?
            .map(|c| c.to_kill)
            .unwrap_or_default())
    }

    /// `cancel_execution`, with records `extra` builds from what the cancel
    /// ended, in the same frame, a transaction: a task's report that it was
    /// cancelled (DD7), and the result of each tool call it planned and never
    /// sent (theseus-w98). `extra` runs only when this call cancels it, so a
    /// second cancel writes nothing, and says nothing twice.
    pub fn cancel_execution_with(
        &self,
        execution_id: &str,
        by: &str,
        extra: impl FnOnce(&Ending<'_>) -> Result<Vec<NewRecord>>,
    ) -> Result<Cancel> {
        self.frame(&[execution_id], |k| {
            let Some(c) = k.cancel(execution_id, by)? else {
                return Ok(Cancel::default());
            };
            k.stage(&extra(&Ending {
                execution: &c.execution,
                not_run: &c.not_run,
                turn_running: c.turn_running,
            })?)?;
            Ok(Cancel {
                to_kill: c.to_kill,
                not_run: c.not_run,
            })
        })
    }

    /// The cancel (`cancel_execution`). A task reaches its parent here as it
    /// does at `end_turn`. `None`: the execution had ended already, and
    /// nothing is written.
    ///
    /// Everything the execution planned and never sent ends in this frame:
    /// a tool call waiting for the operator, a call planned or authorized and
    /// not yet dispatched, and the budget question. Each settles `Cancelled`,
    /// its resolution "the execution was cancelled by <by>", its reservation
    /// released, with an `action.cancelled` row; nothing can run it now, and
    /// nothing counts it as waiting (theseus-w98). Only a dispatched call has
    /// a backend to stop: it is marked `cancel = requested` and returned in
    /// `to_kill`.
    fn cancel(&self, execution_id: &str, by: &str) -> Result<Option<Cancelled>> {
        let _w = self.lock_family(execution_id)?;
        let mut e = self
            .execution(execution_id)?
            .ok_or_else(|| KernelError::UnknownExecution(execution_id.into()))?;
        if e.state.is_terminal() {
            return Ok(None);
        }
        let now = self.now_ms();
        // A turn held and not yet admitted (theseus-2uby) holds it too.
        let turn_running = e.state == ExecState::Running || self.holds_turn(&e.id);
        e.state = ExecState::Cancelled;
        e.cancel = Some(CancelState::Requested);
        e.ended_reason = Some(format!("cancelled by {by}"));
        e.wake = None;
        e.stopped = None;
        e.report_wakes.clear();
        e.updated_at_ms = now;
        let mut frame = Vec::new();
        let why = format!("the execution was cancelled by {by}");
        let (not_run, day) = self.end_unsent(&mut e, by, &why, &mut frame)?;
        // Its wakes end with it (DD8); a `/stop` ends nothing and keeps them.
        self.drop_wakes(&mut e, by, "the execution was cancelled", &mut frame)?;
        frame.insert(0, exec_record(&e)?);
        let mut to_kill = Vec::new();
        for c in &e.outstanding {
            if let Some(mut a) = self.action(c)? {
                if a.state == ActionState::Dispatched {
                    a.cancel = Some(CancelState::Requested);
                    frame.push(action_record(&a)?);
                    to_kill.push(a.correlation_id.clone());
                }
            }
        }
        frame.push(self.ledger(
            LedgerKind::ExecutionCancelled,
            Some(&e.session_id),
            json!({"execution_id": e.id, "by": by, "outstanding": to_kill,
                   "not_run": not_run.iter().map(|a| a.correlation_id.as_str()).collect::<Vec<_>>()}),
        )?);
        self.task_ended(&e, &mut frame)?;
        self.commit(&frame)?;
        self.day_after(day);
        Ok(Some(Cancelled {
            execution: e,
            not_run,
            turn_running,
            to_kill,
        }))
    }

    /// Settle everything `e` planned and never sent, as `e` ends: a cancel,
    /// or a turn that ends it (theseus-w98). Each action of `e` planned or
    /// authorized and not yet dispatched (a tool call waiting for the
    /// operator, a call a crash left between its plan and its dispatch, the
    /// budget question) settles `Cancelled` with `why` as its resolution, its
    /// reservation released, in `frame`, with an `action.cancelled` row. The
    /// caller writes `e` after. Returns them as they are now.
    pub(super) fn end_unsent(
        &self,
        e: &mut Execution,
        by: &str,
        why: &str,
        frame: &mut Vec<NewRecord>,
    ) -> Result<(Vec<Action>, Vec<Effect>)> {
        e.budget.question = None;
        e.budget.question_needs_micros = 0;
        let now = self.now_ms();
        let mut ended = Vec::new();
        let mut day = Vec::new();
        for mut a in self
            .unsettled_actions(&e.id)?
            .into_iter()
            .filter(|a| matches!(a.state, ActionState::Planned | ActionState::Authorized))
        {
            a.state = ActionState::Cancelled;
            a.settled_at_ms = Some(now);
            a.resolution = Some(why.to_string());
            if let Some(r) = &a.reservation_id {
                settle_reservation_in(&mut e.budget, r, Some(0));
                day.extend(Effect::of(&a, Some(0)));
            }
            frame.push(action_record(&a)?);
            frame.push(self.ledger(
                LedgerKind::ActionCancelled,
                Some(&a.session_id),
                json!({"correlation_id": a.correlation_id, "tool": a.tool, "by": by, "why": why}),
            )?);
            ended.push(a);
        }
        Ok((ended, day))
    }

    /// Settle a call a cancel had stopped, from the verdict its wrapper wrote: acknowledged, then
    /// verified or uncertain, in one frame, as the cancel's own last step does.
    fn settle_from_stop(&self, correlation_id: &str, v: &Verdict) -> Result<Action> {
        let execution_id = self
            .action(correlation_id)?
            .ok_or_else(|| KernelError::UnknownAction(correlation_id.into()))?
            .execution_id;
        self.frame(&[&execution_id], |k| {
            k.cancel_acknowledged(correlation_id)?;
            if v.verified() {
                k.cancel_verified(correlation_id, Some(v))
            } else {
                k.cancel_uncertain(correlation_id, v)
            }
        })
    }

    /// The heartbeat reconciler (§3.3, §3.16): due wakes fire; dispatched
    /// actions past their deadline are checked against evidence and settled
    /// or marked unknown; unknowns are re-probed. Cost scales with the work
    /// it may act on (theseus-lv2): the executions a due time may wake, and
    /// the actions sent; the open ones are counted, not read.
    pub fn reconcile(&self, evidence: &dyn Evidence) -> Result<ReconcileReport> {
        self.reconcile_with(evidence, true)
    }

    /// `reconcile`; `due`: whether due wakes fire in this pass (startup's
    /// pass leaves them to the driver, DD8, and reads no execution).
    pub(super) fn reconcile_with(
        &self,
        evidence: &dyn Evidence,
        due: bool,
    ) -> Result<ReconcileReport> {
        let t0 = std::time::Instant::now();
        let now = self.now_ms();
        let mut rep = ReconcileReport {
            open_executions: self.count_open_executions()?,
            open_actions: self.count_open_actions()?,
            ..Default::default()
        };
        // A due time it waits on, or a wake of its own (DD8). The scan may be
        // a frame stale: `fire_due` decides again from a read under the lock
        // (a cancel may have landed since), and queues it for the driver.
        if due {
            rep.marked_unknown = self.mark_earlier_calls_unknown()?;
            let execs = self.executions_by(&[terms::one("due")])?;
            for e in execs.iter().filter(|e| crate::wakes::due_now(e, now)) {
                if let Some(e) = self.fire_due(&e.id)? {
                    rep.woke_due.push(e.id);
                }
            }
        }
        let actions = self.actions_by(&[
            terms::one(&terms::action_state(ActionState::Dispatched)),
            terms::one(&terms::action_state(ActionState::OutcomeUnknown)),
        ])?;
        for a in actions {
            match a.state {
                ActionState::Dispatched => {
                    // A cancel had stopped it before the earlier process died (theseus-1og8,
                    // theseus-fvxh): the verdict its wrapper left settles it, as the cancel
                    // would have, and the execution's transcript is answered once serving.
                    if !due && a.cancel.is_some() {
                        self.earlier_noted().cancelled.push(a.execution_id.clone());
                        if let Some(v) = evidence.stopped(&a) {
                            if self.settle_from_stop(&a.correlation_id, &v).is_ok() {
                                evidence.stop_taken(&a);
                                rep.settled_from_stop.push(a.correlation_id);
                                continue;
                            }
                        }
                    }
                    if a.deadline_at_ms > now && a.cancel.is_none() {
                        if !due {
                            self.note_earlier(&a, evidence);
                        }
                        continue; // not overdue; event-first, poll only stuck work
                    }
                    match evidence.probe(&a) {
                        Probe::Completed(c) => {
                            self.accept_completion(&c)?;
                            rep.settled_from_evidence.push(a.correlation_id);
                        }
                        Probe::StillRunning => {
                            rep.still_running_past_deadline.push(a.correlation_id);
                        }
                        Probe::Gone => {
                            // Decided from the scan; `mark_unknown` checks again under
                            // the lock, and an action settled since is left as it is.
                            match self.mark_unknown(&a.correlation_id, "overdue_no_evidence") {
                                Ok(_) => rep.marked_unknown.push(a.correlation_id),
                                Err(e)
                                    if matches!(
                                        e.downcast_ref::<KernelError>(),
                                        Some(KernelError::ActionState { .. })
                                    ) => {}
                                Err(e) => return Err(e),
                            }
                        }
                    }
                }
                ActionState::OutcomeUnknown => {
                    if let Probe::Completed(c) = evidence.probe(&a) {
                        self.accept_completion(&c)?;
                        rep.resolved_unknown.push(a.correlation_id);
                    }
                }
                _ => {}
            }
        }
        rep.elapsed_us = t0.elapsed().as_micros() as u64;
        if !(rep.woke_due.is_empty()
            && rep.settled_from_stop.is_empty()
            && rep.settled_from_evidence.is_empty()
            && rep.marked_unknown.is_empty()
            && rep.resolved_unknown.is_empty())
        {
            self.commit(&[self.ledger(
                LedgerKind::Reconcile,
                None,
                serde_json::to_value(&rep)?,
            )?])?;
        }
        Ok(rep)
    }
}
