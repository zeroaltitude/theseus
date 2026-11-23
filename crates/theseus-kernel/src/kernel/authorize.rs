//! An action's way from a plan to its dispatch: a confirm bound to its action, the authorization, and the dispatch
//! (or the refusal that stands in for it), each one frame.

// Named here and not only through the glob: the reader rule's scan (`tests_registry`) places a `LedgerKind::` use by
// its file's own imports.
use theseus_protocol::LedgerKind;

use super::*;

impl Kernel {
    /// Bind a confirmation to the action's *current* digest (§3.9).
    pub fn bind_confirm(
        &self,
        correlation_id: &str,
        by: &str,
        proposal: &Proposal,
    ) -> Result<Action> {
        let (_w, mut a) = self.locked_known_action(correlation_id)?;
        if a.state != ActionState::Planned {
            return Err(KernelError::ActionState {
                correlation_id: a.correlation_id.clone(),
                state: a.state.as_str(),
                expected: "planned",
            }
            .into());
        }
        let d = digest_proposal(proposal);
        if d != a.args_digest {
            return Err(KernelError::ConfirmInvalidated {
                reason: "confirmation presented for different arguments than were planned".into(),
            }
            .into());
        }
        let now = self.now_ms();
        a.confirm = Some(ConfirmBinding {
            confirm_id: new_id("cfm"),
            by: by.into(),
            bound_digest: d,
            expires_at_ms: now + self.cfg.confirm_ttl_ms,
        });
        self.commit(&[
            action_record(&a)?,
            self.ledger(
                LedgerKind::ActionConfirmed,
                Some(&a.session_id),
                json!({"correlation_id": a.correlation_id, "by": by, "confirm": a.confirm}),
            )?,
        ])?;
        Ok(a)
    }

    /// `authorized`: the gate has passed (§3.17 ordering, see `gate.rs`). The
    /// final revalidation happens here: the proposal's digest must equal the
    /// planned digest, and if a confirmation is required it must be bound to
    /// that same digest and unexpired.
    pub fn authorize(
        &self,
        correlation_id: &str,
        proposal: &Proposal,
        confirm_required_from: Option<&str>,
    ) -> Result<Action> {
        let digest = digest_proposal(proposal);
        self.authorize_or_invalid(correlation_id, &digest, confirm_required_from)?
    }

    /// `authorize`, with its two refusals apart, and nothing written by
    /// either: `Err` when the action's execution ended it (a cancel or a
    /// stop settled it) or it cannot be read; `Ok(Err(why))` when the
    /// proposal or its confirm no longer holds (`authorize_frame`'s checks).
    /// `digest` is the proposal's (`digest_proposal`).
    pub(super) fn authorize_or_invalid(
        &self,
        correlation_id: &str,
        digest: &str,
        confirm_required_from: Option<&str>,
    ) -> Result<Result<Action>> {
        let (_w, mut a) = self.locked_known_action(correlation_id)?;
        if let Some(end) = self.ended_with_execution(&a)? {
            return Err(end);
        }
        let frame = match self.authorize_frame(&mut a, digest, confirm_required_from) {
            Ok(frame) => frame,
            Err(why) => return Ok(Err(why)),
        };
        self.commit(&frame)?;
        Ok(Ok(a))
    }

    /// The refusal for a transition of `a`, which the end of its execution
    /// already settled: a cancel settles everything the execution planned
    /// and never sent (theseus-w98), and a stop declines it (W1). The turn
    /// that planned it hears what `dispatch` tells it when the cancel or the
    /// stop comes first: the execution is cancelled, or stopped. `None` for
    /// any other action.
    fn ended_with_execution(&self, a: &Action) -> Result<Option<anyhow::Error>> {
        if a.state != ActionState::Cancelled {
            return Ok(None);
        }
        let Some(e) = self.execution(&a.execution_id)? else {
            return Ok(None);
        };
        Ok(if e.state == ExecState::Cancelled {
            Some(
                KernelError::NotRunnable {
                    id: e.id,
                    state: "cancelled",
                }
                .into(),
            )
        } else {
            e.stopped
                .map(|s| KernelError::Stopped { id: e.id, by: s.by }.into())
        })
    }

    /// `authorize`'s checks on `a`, and its frame: the action authorized and
    /// its `action.authorized` row.
    fn authorize_frame(
        &self,
        a: &mut Action,
        d: &str,
        confirm_required_from: Option<&str>,
    ) -> Result<Vec<NewRecord>> {
        if a.state != ActionState::Planned {
            return Err(KernelError::ActionState {
                correlation_id: a.correlation_id.clone(),
                state: a.state.as_str(),
                expected: "planned",
            }
            .into());
        }
        if d != a.args_digest {
            return Err(KernelError::ConfirmInvalidated {
                reason: format!("planned digest {} != final digest {}", a.args_digest, d),
            }
            .into());
        }
        if let Some(by) = confirm_required_from {
            match &a.confirm {
                None => return Err(KernelError::ConfirmRequired { by: by.into() }.into()),
                Some(c) if c.bound_digest != d => {
                    return Err(KernelError::ConfirmInvalidated {
                        reason: "bound digest differs from final digest".into(),
                    }
                    .into())
                }
                Some(c) if c.expires_at_ms < self.now_ms() => {
                    return Err(KernelError::ConfirmInvalidated {
                        reason: "confirmation expired".into(),
                    }
                    .into())
                }
                Some(c) if c.by != by => {
                    return Err(KernelError::ConfirmInvalidated {
                        reason: format!("confirmed by {} but {by} is required", c.by),
                    }
                    .into())
                }
                Some(_) => {}
            }
        }
        let now = self.now_ms();
        a.state = ActionState::Authorized;
        a.authorized_at_ms = Some(now);
        Ok(vec![
            action_record(a)?,
            self.ledger(
                LedgerKind::ActionAuthorized,
                Some(&a.session_id),
                json!({"correlation_id": a.correlation_id, "confirmed": a.confirm.is_some()}),
            )?,
        ])
    }

    /// `dispatched`: committed before the call is made (transactional
    /// outbox). The execution records the action as outstanding.
    pub fn dispatch(&self, correlation_id: &str, external_op_id: Option<&str>) -> Result<Action> {
        self.dispatch_or_refuse(correlation_id, external_op_id, None)?
    }

    /// An action whose lifetime starts when it leaves, not while it waits to run.
    pub fn dispatch_for(
        &self,
        correlation_id: &str,
        external_op_id: Option<&str>,
        lifetime_ms: u64,
    ) -> Result<Action> {
        self.dispatch_or_refuse(correlation_id, external_op_id, Some(lifetime_ms))?
    }

    /// `authorize` and `dispatch` in one frame, a transaction, for a call the
    /// operator confirmed (theseus-l6y). Each keeps its record and row, in
    /// that order, so the WAL reads as the two transitions did in two
    /// frames. `Ok(Err(why))`: the confirm no longer holds (`authorize`'s
    /// refusal), and nothing was written. A cancel or a stop that landed
    /// first is the error, as `dispatch` returns it. It settled the action
    /// itself, so nothing is written (theseus-w98), except for an action an
    /// older build's cancel left planned: that frame writes the authorization
    /// and the action's cancel.
    pub fn authorize_and_dispatch(
        &self,
        correlation_id: &str,
        proposal: &Proposal,
        confirm_required_from: Option<&str>,
        external_op_id: Option<&str>,
    ) -> Result<Result<Action>> {
        /// How the transaction ended. Each commits what it staged: nothing
        /// when the confirm no longer holds, and the action's cancel when a
        /// cancel or a stop landed first.
        enum Answered {
            Dispatched(Box<Action>),
            Invalid(anyhow::Error),
            Refused(anyhow::Error),
        }
        let execution_id = self
            .action(correlation_id)?
            .ok_or_else(|| KernelError::UnknownAction(correlation_id.into()))?
            .execution_id;
        let digest = digest_proposal(proposal);
        let answered = self.frame(&[&execution_id], |k| {
            Ok(
                match k.authorize_or_invalid(correlation_id, &digest, confirm_required_from)? {
                    Err(why) => Answered::Invalid(why),
                    Ok(_) => match k.dispatch_or_refuse(correlation_id, external_op_id, None)? {
                        Ok(a) => Answered::Dispatched(Box::new(a)),
                        Err(refused) => Answered::Refused(refused),
                    },
                },
            )
        })?;
        match answered {
            Answered::Dispatched(a) => Ok(Ok(*a)),
            Answered::Invalid(why) => Ok(Err(why)),
            Answered::Refused(refused) => Err(refused),
        }
    }

    /// `dispatch`, with its refusal apart. `Ok(Err(why))`: a cancel or a stop
    /// (W1) that landed between plan and dispatch kept the action from
    /// leaving; it is cancelled instead, and that is written. `Err`: nothing
    /// is written. The frame: the action, the execution with the action
    /// outstanding, and the `action.dispatched` row.
    fn dispatch_or_refuse(
        &self,
        correlation_id: &str,
        external_op_id: Option<&str>,
        lifetime_ms: Option<u64>,
    ) -> Result<Result<Action>> {
        let (_w, mut a) = self.locked_known_action(correlation_id)?;
        if let Some(end) = self.ended_with_execution(&a)? {
            return Err(end);
        }
        if a.state != ActionState::Authorized {
            return Err(KernelError::ActionState {
                correlation_id: a.correlation_id.clone(),
                state: a.state.as_str(),
                expected: "authorized",
            }
            .into());
        }
        let mut e = self
            .execution(&a.execution_id)?
            .ok_or_else(|| KernelError::UnknownExecution(a.execution_id.clone()))?;
        if e.state == ExecState::Cancelled || e.stopped.is_some() {
            let stopped = e.stopped.as_ref().map(|s| s.by.clone());
            a.state = ActionState::Cancelled;
            a.cancel = Some(CancelState::TerminationVerified);
            a.settled_at_ms = Some(self.now_ms());
            let why = if stopped.is_some() {
                "execution stopped before dispatch"
            } else {
                "execution cancelled before dispatch"
            };
            self.commit(&[
                action_record(&a)?,
                self.ledger(
                    LedgerKind::ActionCancelled,
                    Some(&a.session_id),
                    json!({"correlation_id": a.correlation_id, "why": why}),
                )?,
            ])?;
            return Ok(Err(match stopped {
                Some(by) => KernelError::Stopped { id: e.id, by },
                None => KernelError::NotRunnable {
                    id: e.id,
                    state: "cancelled",
                },
            }
            .into()));
        }
        let now = self.now_ms();
        a.state = ActionState::Dispatched;
        a.dispatched_at_ms = Some(now);
        if let Some(lifetime) = lifetime_ms {
            a.deadline_at_ms = now.saturating_add(lifetime);
        }
        a.external_op_id = external_op_id.map(str::to_string);
        if !e.outstanding.contains(&a.correlation_id) {
            e.outstanding.push(a.correlation_id.clone());
        }
        e.updated_at_ms = now;
        self.commit(&[
            action_record(&a)?,
            exec_record(&e)?,
            self.ledger(
                LedgerKind::ActionDispatched,
                Some(&a.session_id),
                json!({"correlation_id": a.correlation_id, "execution_id": e.id, "tool": a.tool, "external_op_id": a.external_op_id, "deadline_at_ms": a.deadline_at_ms}),
            )?,
        ])?;
        Ok(Ok(a))
    }
}
