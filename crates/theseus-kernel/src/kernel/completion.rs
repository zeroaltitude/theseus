//! A completion's arrival: accepting it into its action's settlement (the books, the ledger rows), and marking an
//! action whose outcome cannot be known as unknown.

// Named here and not only through the glob: the reader rule's scan (`tests_registry`) places a `LedgerKind::` use by
// its file's own imports.
use theseus_protocol::LedgerKind;

use super::*;

impl Kernel {
    /// `accept_completion`, with `extra` records (the node that carries the
    /// result) in the same frame as the settlement, a transaction. `extra` is
    /// written only when this call settles or resolves the action; a
    /// duplicate, a late arrival after cancel, and a quarantined stray write
    /// nothing extra. A stray stays one inside the transaction: an action's
    /// id is minted at its plan, before anything can complete it.
    pub fn accept_completion_with(
        &self,
        c: &Completion,
        extra: Vec<NewRecord>,
    ) -> Result<Accepted> {
        self.completion_with(c, extra, false)
    }

    /// `accept_completion_with` for a spooled completion, which the drain and
    /// the turn waiting on its job both read (Tier 7.1): whichever takes it
    /// first settles the action, and the other finds it settled and writes
    /// nothing (`Taken`), where a delivery from elsewhere writes
    /// `completion.duplicate`.
    pub fn take_completion_with(&self, c: &Completion, extra: Vec<NewRecord>) -> Result<Accepted> {
        self.completion_with(c, extra, true)
    }

    fn completion_with(
        &self,
        c: &Completion,
        extra: Vec<NewRecord>,
        take: bool,
    ) -> Result<Accepted> {
        let execution = self.action(&c.correlation_id)?.map(|a| a.execution_id);
        let ids: Vec<&str> = execution.iter().map(String::as_str).collect();
        self.frame(&ids, |k| {
            // A cancelled action whose late completion was taken is taken too.
            let settled = |a: &Action| {
                matches!(a.state, ActionState::Succeeded | ActionState::Failed)
                    || (a.state == ActionState::Cancelled && a.completions_seen >= 1)
            };
            if take {
                if let Some(a) = k.action(&c.correlation_id)?.filter(settled) {
                    return Ok(Accepted::Taken {
                        correlation_id: a.correlation_id,
                    });
                }
            }
            let accepted = k.accept_completion(c)?;
            if matches!(
                accepted,
                Accepted::Settled { .. } | Accepted::ResolvedUnknown { .. }
            ) {
                k.stage(&extra)?;
            }
            Ok(accepted)
        })
    }

    /// Accept a completion from any transport (§3.16). Idempotent; atomic
    /// with the owning execution's continuation. A task's action locks the
    /// parent too (`locked_action`).
    pub fn accept_completion(&self, c: &Completion) -> Result<Accepted> {
        self.accept_completion_as(c, false)
    }

    /// `accept_completion`; `book` books an unknown outcome's reservation as
    /// spent instead of holding it (an earlier process's call, `earlier.rs`).
    #[expect(clippy::too_many_lines, reason = "shape budget: split it")]
    pub(crate) fn accept_completion_as(&self, c: &Completion, book: bool) -> Result<Accepted> {
        let Some((_w, mut a)) = self.locked_action(&c.correlation_id)? else {
            // No action, so no execution to lock.
            let key = format!("{QUARANTINE_PREFIX}{}", c.correlation_id);
            self.commit(&[
                NewRecord::json(kinds::COMPLETION, Some(&key), c)?,
                self.ledger(
                    LedgerKind::CompletionQuarantined,
                    None,
                    json!({"correlation_id": c.correlation_id, "producer": c.producer, "outcome": c.outcome}),
                )?,
            ])?;
            return Ok(Accepted::Quarantined {
                correlation_id: c.correlation_id.clone(),
            });
        };
        let now = self.now_ms();
        a.completions_seen += 1;
        let completion_rec =
            NewRecord::json(kinds::COMPLETION, Some(&c.correlation_id), c)?.scoped(&a.session_id);
        match a.state {
            ActionState::Succeeded | ActionState::Failed => {
                self.commit(&[
                    action_record(&a)?,
                    self.ledger(
                        LedgerKind::CompletionDuplicate,
                        Some(&a.session_id),
                        json!({"correlation_id": a.correlation_id, "seen": a.completions_seen, "producer": c.producer}),
                    )?,
                ])?;
                return Ok(Accepted::DuplicateNoop {
                    correlation_id: a.correlation_id,
                });
            }
            ActionState::Cancelled => {
                a.resolution = Some(format!(
                    "late completion after cancel: {:?} from {}",
                    c.outcome, c.producer
                ));
                let mut frame = vec![completion_rec, action_record(&a)?];
                let mut day = None;
                // A reservation the cancel held as unknown (its backend could
                // not be stopped) is reconciled by the cost this first late
                // completion brings: booked as spent, and the hold released
                // (DD7: else a cancelled task keeps it carved from its parent).
                let held = matches!(
                    a.cancel,
                    Some(CancelState::Unsupported | CancelState::OutcomeUncertain)
                );
                if held
                    && a.reservation_id.is_some()
                    && a.completions_seen == 1
                    && c.outcome != Outcome::Unknown
                {
                    if let Some(mut e) = self.execution(&a.execution_id)? {
                        let spent_before = e.budget.spent_micros;
                        crate::earlier::resolve_in(&mut e.budget, &a, c.cost_micros);
                        day = Effect::resolved(&a, c.cost_micros);
                        e.updated_at_ms = now;
                        frame.push(exec_record(&e)?);
                        self.carry_to_parent(&e, spent_before, &mut frame)?;
                    }
                }
                let mut data = json!({"correlation_id": a.correlation_id, "outcome": c.outcome, "producer": c.producer, "cost_usd": c.cost_micros.map(micros_to_usd)});
                Effect::stamp(&mut data, &day);
                frame.push(self.ledger(
                    LedgerKind::CompletionLateAfterCancel,
                    Some(&a.session_id),
                    data,
                )?);
                self.commit(&frame)?;
                self.day_after(day);
                return Ok(Accepted::LateAfterCancel {
                    correlation_id: a.correlation_id,
                });
            }
            // `Planned` and `Authorized` are a result for something never
            // dispatched: only the in-process transport can do this legitimately
            // (dispatch and completion in one call). Treat as dispatched-then-settled.
            ActionState::Planned
            | ActionState::Authorized
            | ActionState::Dispatched
            | ActionState::OutcomeUnknown => {}
        }
        let was_unknown = a.state == ActionState::OutcomeUnknown;
        let book = book && !was_unknown && c.outcome == Outcome::Unknown;
        let mut e = self
            .execution(&a.execution_id)?
            .ok_or_else(|| KernelError::UnknownExecution(a.execution_id.clone()))?;
        let spent_before = e.budget.spent_micros;

        // Settle the action.
        a.state = match c.outcome {
            Outcome::Succeeded => ActionState::Succeeded,
            Outcome::Failed => ActionState::Failed,
            Outcome::Unknown => ActionState::OutcomeUnknown,
        };
        a.settled_at_ms = Some(now);
        a.result_ref = c.result_ref.clone().or(a.result_ref.take());
        if c.external_op_id.is_some() {
            a.external_op_id = c.external_op_id.clone();
        }
        if was_unknown {
            a.resolution = Some(format!("resolved by {} as {:?}", c.producer, c.outcome));
        }
        if book {
            crate::earlier::mark_booked(&mut a);
        }

        // Continue the execution, in the same frame. A terminal execution
        // still drops the action from `outstanding` and settles its budget;
        // it just does not wake or queue a result (nothing will consume it).
        let mut frame = vec![completion_rec, action_record(&a)?];
        e.outstanding.retain(|x| x != &a.correlation_id);
        if a.cancel.is_some() && a.resolution.is_none() {
            a.resolution = Some(format!(
                "completed as {:?} after cancel was requested",
                c.outcome
            ));
            frame[1] = action_record(&a)?;
        }
        let mut day = None;
        if let Some(r) = &a.reservation_id {
            if was_unknown {
                if c.outcome != Outcome::Unknown {
                    crate::earlier::resolve_in(&mut e.budget, &a, c.cost_micros);
                    day = Effect::resolved(&a, c.cost_micros);
                }
            } else {
                // The day counts an unknown call at its reservation.
                day = Effect::of(&a, c.cost_micros.filter(|_| c.outcome != Outcome::Unknown));
                match c.outcome {
                    Outcome::Unknown if book => book_reservation_in(&mut e.budget, r),
                    Outcome::Unknown => hold_reservation_in(&mut e.budget, r),
                    _ => settle_reservation_in(&mut e.budget, r, c.cost_micros),
                }
            }
        }
        let mut queued = false;
        if !e.state.is_terminal() {
            // A turn's own result is read by that turn (theseus-l6y): only
            // what settles outside it waits in the queue for the next turn.
            match self.own.as_deref().filter(|o| o.execution_id == e.id) {
                Some(own) => {
                    own.settled
                        .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                }
                None => {
                    if !e.queued_results.contains(&a.correlation_id) {
                        e.queued_results.push(a.correlation_id.clone());
                    }
                }
            }
            // A waiting execution wakes if this completion is what it waited on.
            let wakes = match &e.wake {
                Some(Wake::Actions { correlation_ids }) => {
                    correlation_ids.contains(&a.correlation_id) || correlation_ids.is_empty()
                }
                _ => false,
            };
            if e.state == ExecState::Waiting && wakes {
                e.state = ExecState::Queued;
                e.wake = None;
                queued = true;
            }
        }
        e.updated_at_ms = now;
        let exec_state = e.state;
        frame.push(exec_record(&e)?);
        // A task's cost is its parent's spend too, in this frame (DD7).
        self.carry_to_parent(&e, spent_before, &mut frame)?;
        let kind = match (was_unknown, c.outcome) {
            (true, _) => LedgerKind::ActionResolved,
            (false, Outcome::Unknown) => LedgerKind::ActionOutcomeUnknown,
            (false, Outcome::Succeeded) => LedgerKind::ActionSucceeded,
            (false, Outcome::Failed) => LedgerKind::ActionFailed,
        };
        let mut data = json!({"correlation_id": a.correlation_id, "execution_id": e.id, "outcome": c.outcome, "producer": c.producer, "duration_ms": c.finished_at_ms.saturating_sub(c.started_at_ms), "execution_state": exec_state, "cost_usd": c.cost_micros.map(micros_to_usd)});
        if book {
            crate::earlier::booked_row(&mut data, &a);
        }
        Effect::stamp(&mut data, &day);
        // How a job failed, for the nightly read (failure.rs).
        crate::failure::stamp(&mut data, c);
        frame.push(self.ledger(kind, Some(&a.session_id), data)?);
        // Every queue writes its row, in its frame (theseus-2xep).
        if queued {
            let row = json!({"execution_id": e.id, "why": "result"});
            frame.push(self.ledger(LedgerKind::ExecutionQueued, Some(&e.session_id), row)?);
        }
        self.commit(&frame)?;
        self.day_after(day);
        if was_unknown {
            Ok(Accepted::ResolvedUnknown {
                correlation_id: a.correlation_id,
                outcome: c.outcome,
            })
        } else {
            Ok(Accepted::Settled {
                correlation_id: a.correlation_id,
                execution_id: e.id,
                execution_state: exec_state,
            })
        }
    }

    /// The reconciler (or a wrapper's deadline) could not establish an outcome.
    /// `OutcomeUnknown` is knowledge: the execution gets it as a result and the
    /// reservation is held, never released (§3.16).
    pub fn mark_unknown(&self, correlation_id: &str, reason: &str) -> Result<Action> {
        self.mark_unknown_as(correlation_id, reason, false)
    }

    /// `mark_unknown`; `book` books the reservation as spent (`earlier.rs`).
    pub(crate) fn mark_unknown_as(
        &self,
        correlation_id: &str,
        reason: &str,
        book: bool,
    ) -> Result<Action> {
        // The check and the settlement are one transaction, under one lock:
        // nothing can settle the action between them.
        let execution_id = self
            .action(correlation_id)?
            .ok_or_else(|| KernelError::UnknownAction(correlation_id.into()))?
            .execution_id;
        self.frame(&[&execution_id], |k| {
            let a = k
                .action(correlation_id)?
                .ok_or_else(|| KernelError::UnknownAction(correlation_id.into()))?;
            if a.state != ActionState::Dispatched {
                return Err(KernelError::ActionState {
                    correlation_id: a.correlation_id,
                    state: a.state.as_str(),
                    expected: "dispatched",
                }
                .into());
            }
            let unknown = Completion {
                correlation_id: correlation_id.into(),
                outcome: Outcome::Unknown,
                result_ref: None,
                external_op_id: None,
                started_at_ms: k.now_ms(),
                finished_at_ms: k.now_ms(),
                producer: format!("reconciler:{reason}"),
                signature: None,
                cost_micros: None,
                detail: None,
            };
            k.accept_completion_as(&unknown, book)?;
            Ok(k.action(correlation_id)?
                .ok_or_else(|| KernelError::UnknownAction(correlation_id.into()))?)
        })
    }
}
