//! The continuation (`ToolRuntime::resume`): every `tool_use` of the
//! last assistant message that has no result yet is placed
//! (`Pending`), then answered: run once confirmed, declined, superseded,
//! or reported unknown after a restart. Split from `toolrun.rs`
//! (theseus-5gw9).

use std::collections::HashMap;

use anyhow::{anyhow, Result};
use serde_json::Value;
use theseus_kernel::{Action, ActionState};
use theseus_tools::Backend;

use super::{
    confirm_proposal, not_run_answer, unanswered, Call, CallOutcome, ResultNode, ResumeOutcome,
    ToolRuntime, TurnCtx,
};
use crate::fact;
use crate::node::{Body, Node, ResultStatus};
use crate::policy::Posture;
use crate::provider::ToolUse;
use crate::sandbox;

impl ToolRuntime {
    /// Continuation: answer every `tool_use` of the last assistant message that
    /// has no result yet — run what was confirmed, close what was declined or
    /// superseded by new input, report what the restart left unknown, and wait
    /// on what is still pending. Each call is first placed (`Pending`), then
    /// acted on.
    pub async fn resume(&self, tc: &TurnCtx<'_>, has_input: bool) -> Result<ResumeOutcome> {
        let mut out = ResumeOutcome::default();
        let nodes = tc.store.transcript(tc.session_id)?;
        let Some((assistant, pending)) = unanswered(&nodes) else {
            return Ok(out);
        };
        let calls: HashMap<&str, &Node> = nodes
            .iter()
            .filter_map(|(_, n)| match &n.body {
                Body::ToolCall { tool_use_id, .. } => Some((tool_use_id.as_str(), &**n)),
                _ => None,
            })
            .collect();
        // Calls no turn has gated run as a response's calls do (theseus-a60).
        let mut fresh = Vec::new();
        for u in pending {
            let node = calls.get(u.id.as_str()).copied();
            let place = Self::where_is(tc, node)?;
            if matches!(place, Pending::NeverPlanned) && !has_input {
                fresh.push(u);
                continue;
            }
            if self
                .run_fresh(tc, &assistant.id, &mut fresh, &mut out)
                .await?
            {
                return Ok(out);
            }
            let job = match place {
                Pending::NeverPlanned => {
                    self.not_run(tc, &u, "the operator sent a new message before this ran")?;
                    None
                }
                Pending::StoppedAtGate => {
                    // Stopped at the gate (invalid input), but the result write was lost: answer again.
                    self.not_run(
                        tc,
                        &u,
                        "the harness restarted before its result was recorded",
                    )?;
                    None
                }
                Pending::NeverAsked(a) => {
                    self.answer_never_asked(tc, &u, &a)?;
                    None
                }
                Pending::Waiting(corr) if has_input => {
                    self.supersede(tc, &u, &corr)?;
                    None
                }
                Pending::Waiting(corr) => {
                    out.awaiting = Some(corr);
                    return Ok(out);
                }
                Pending::Confirmed(a) => self.run_confirmed(tc, &u, &a, node).await?,
                Pending::Authorized(a) => self.run_authorized(tc, &u, &a).await?,
                Pending::Dispatched(a) => self.check_dispatched(tc, &u, &a)?,
                Pending::Settled(a) => {
                    self.answer_settled(tc, &u, &a)?;
                    None
                }
                Pending::Cancelled(a) => {
                    self.answer_cancelled(tc, &u, &a)?;
                    None
                }
            };
            out.background.extend(job);
            out.wrote += 1;
        }
        self.run_fresh(tc, &assistant.id, &mut fresh, &mut out)
            .await?;
        Ok(out)
    }

    /// Calls no turn has gated, through `run_calls`. True when one of them
    /// now waits for the operator, where the continuation stops.
    async fn run_fresh(
        &self,
        tc: &TurnCtx<'_>,
        assistant_node: &str,
        fresh: &mut Vec<ToolUse>,
        out: &mut ResumeOutcome,
    ) -> Result<bool> {
        if fresh.is_empty() {
            return Ok(false);
        }
        let calls: Vec<Call<'_>> = fresh
            .iter()
            .map(|call| Call {
                call,
                invalid: None,
            })
            .collect();
        let batch = self.run_calls(tc, assistant_node, &calls).await?;
        drop(calls);
        fresh.clear();
        for r in batch.ran {
            match r.outcome {
                CallOutcome::AwaitingConfirm { .. } => continue,
                CallOutcome::Background { correlation_id } => out.background.push(correlation_id),
                CallOutcome::Done { .. } => {}
            }
            out.wrote += 1;
        }
        out.awaiting = batch.awaiting;
        Ok(out.awaiting.is_some())
    }

    /// Where a call stands, from its tool-call node and its action.
    fn where_is(tc: &TurnCtx<'_>, node: Option<&Node>) -> Result<Pending> {
        let Some(node) = node else {
            return Ok(Pending::NeverPlanned);
        };
        let Body::ToolCall {
            correlation_id: Some(corr),
            ..
        } = &node.body
        else {
            return Ok(Pending::StoppedAtGate);
        };
        let a = tc
            .kernel
            .action(corr)?
            .ok_or_else(|| anyhow!("action {corr} vanished"))?;
        Ok(match a.state {
            _ if a.awaits_confirm() && Self::never_asked(&a, node) => Pending::NeverAsked(a),
            _ if a.awaits_confirm() => Pending::Waiting(a.correlation_id),
            ActionState::Planned => Pending::Confirmed(a),
            ActionState::Authorized => Pending::Authorized(a),
            ActionState::Dispatched => Pending::Dispatched(a),
            ActionState::Succeeded | ActionState::Failed | ActionState::OutcomeUnknown => {
                Pending::Settled(a)
            }
            ActionState::Cancelled => Pending::Cancelled(a),
        })
    }

    /// Whether a planned call that `awaits_confirm` was never asked
    /// (theseus-ni5). `awaits_confirm` reads "planned, no confirm bound" as a
    /// question, which is also what a call looks like when a restart came
    /// between its plan and its authorization, in a build that wrote the two
    /// in separate frames. The kernel cannot tell the two apart; the call's
    /// node can: a question asked since theseus-0g4 keeps its proposal on
    /// the action, and one from before it kept it on the node, whose gate
    /// said `needs_confirm`. A call with no proposal whose gate said `allow`
    /// was never asked.
    fn never_asked(a: &Action, node: &Node) -> bool {
        a.proposal.is_none()
            && matches!(&node.body, Body::ToolCall { gate: Some(g), .. } if g.result.gate == "allow")
    }

    /// A call planned and never asked, found by a continuation: nothing asked
    /// the operator and nothing ran it, so it is declined by the harness and
    /// answered not run, and the model may ask again (theseus-ni5). Before,
    /// the turn parked on a question no card ever posted.
    fn answer_never_asked(&self, tc: &TurnCtx<'_>, u: &ToolUse, a: &Action) -> Result<()> {
        let (_, name) = self.tool_of(u);
        let corr = &a.correlation_id;
        tc.kernel
            .decline_action(corr, "harness", "planned before a restart and never asked")?;
        tc.record(&fact::tool::CallNeverAsked { tool: &name });
        self.answer(
            tc,
            ResultNode {
                correlation_id: Some(corr),
                ..ResultNode::new(
                    &u.id,
                    &name,
                    ResultStatus::Cancelled,
                    "Not run: this call was planned before a restart and the operator was never \
                     asked about it. Ask again if it is still wanted.",
                )
            },
        )?;
        Ok(())
    }

    /// New input came instead of an answer: the waiting call is declined.
    fn supersede(&self, tc: &TurnCtx<'_>, u: &ToolUse, corr: &str) -> Result<()> {
        let (_, name) = self.tool_of(u);
        tc.kernel.decline_action(
            corr,
            &self.policy.confirmer,
            "superseded: the operator sent a new message instead of confirming",
        )?;
        tc.record(&fact::tool::CallSuperseded {
            session_id: tc.session_id,
            correlation_id: corr,
            tool: &name,
        });
        if let Err(e) = tc
            .outbox
            .closed(corr, crate::outbox::Closed::new("superseded", None))
        {
            tracing::warn!(error = %format!("{e:#}"), "the card's settle was not written");
        }
        self.answer(
            tc,
            ResultNode {
                correlation_id: Some(corr),
                ..ResultNode::new(
                    &u.id,
                    &name,
                    ResultStatus::Declined,
                    "Not run: the operator sent a new message instead of confirming this call.",
                )
            },
        )?;
        Ok(())
    }

    /// A confirmed call: authorized against the proposal its confirm bound,
    /// then run. A confirm that no longer holds declines it instead. Returns
    /// the job it left running in the background, if any.
    async fn run_confirmed(
        &self,
        tc: &TurnCtx<'_>,
        u: &ToolUse,
        a: &Action,
        node: Option<&Node>,
    ) -> Result<Option<String>> {
        let (tool, name) = self.tool_of(u);
        let Some(tool) = tool else {
            self.not_run(tc, u, "the tool is no longer registered")?;
            return Ok(None);
        };
        let corr = &a.correlation_id;
        // Authorized and dispatched in one frame (theseus-l6y). A confirm
        // that no longer holds writes nothing, and is declined below; a
        // cancel or a stop that landed first is the error.
        // It runs in the class, and with the egress list, its proposal
        // names, as the confirm bound them.
        let mut class = sandbox::Bound::default();
        let authorized = match confirm_proposal(tc.store, a, node) {
            Ok(p) => {
                class = sandbox::Bound::of(&p);
                tc.kernel
                    .authorize_and_dispatch(corr, &p, Some(&self.policy.confirmer), None)?
            }
            Err(e) => Err(e),
        };
        match authorized {
            Ok(_) => {
                // `action.confirm` announced the answer; this only acts on it.
                tc.record(&fact::tool::ApprovedRunning { tool: &name });
                let ran = self.execute(tc, corr, tool, u, Posture::Approve, class);
                match ran.await? {
                    CallOutcome::Background { correlation_id } => Ok(Some(correlation_id)),
                    CallOutcome::AwaitingConfirm { .. } => {
                        unreachable!("an authorized action does not ask again")
                    }
                    CallOutcome::Done { .. } => Ok(None),
                }
            }
            Err(e) => {
                // The confirm expired or no longer matches: say so, never run it.
                tc.record(&fact::tool::ApprovalVoid {
                    tool: &name,
                    error: &e,
                });
                tc.kernel
                    .decline_action(corr, "harness", &format!("confirmation invalid: {e}"))?;
                self.answer(
                    tc,
                    ResultNode {
                        correlation_id: Some(corr),
                        ..ResultNode::new(
                            &u.id,
                            &name,
                            ResultStatus::Declined,
                            format!("Not run: the confirmation is no longer valid ({e})."),
                        )
                    },
                )?;
                Ok(None)
            }
        }
    }

    /// Authorized before a restart and never dispatched: run it now.
    async fn run_authorized(
        &self,
        tc: &TurnCtx<'_>,
        u: &ToolUse,
        a: &Action,
    ) -> Result<Option<String>> {
        let (tool, name) = self.tool_of(u);
        let Some(tool) = tool else {
            self.not_run(tc, u, "the tool is no longer registered")?;
            return Ok(None);
        };
        tc.record(&fact::tool::AuthorizedResumed { tool: &name });
        // One with no proposal to read predates L1 (theseus-0g4): L0.
        let class = confirm_proposal(tc.store, a, None)
            .map_or_else(|_| Default::default(), |p| sandbox::Bound::of(&p));
        tc.kernel.dispatch(&a.correlation_id, None)?;
        Ok(
            match self
                .execute(tc, &a.correlation_id, tool, u, Posture::Approve, class)
                .await?
            {
                CallOutcome::Background { correlation_id } => Some(correlation_id),
                _ => None,
            },
        )
    }

    /// Dispatched before a restart: the job's settled result, a placeholder if
    /// it still runs (returned as a background job), or `unknown`.
    fn check_dispatched(
        &self,
        tc: &TurnCtx<'_>,
        u: &ToolUse,
        a: &Action,
    ) -> Result<Option<String>> {
        let (tool, name) = self.tool_of(u);
        let corr = &a.correlation_id;
        // A harness tool run again finds what it did (DD7's task ids).
        if let Some(t) = tool.as_ref().filter(|t| t.backend() == Backend::Harness) {
            self.run_harness(tc, corr, t.as_ref(), u, approved_at(a))?;
            return Ok(None);
        }
        let is_job = tool.as_ref().is_some_and(|t| t.backend() == Backend::Job);
        let settled = match &self.spool {
            Some(sp) if is_job => Self::job_settled(tc.kernel, sp, corr)?,
            _ => None,
        };
        if let Some(done) = settled {
            self.answer_job(
                tc,
                self.job_result(tc.store, &done, &u.id, &name, Some(&u.input)),
                &done,
            )?;
            return Ok(None);
        }
        let alive = is_job
            && self
                .spool
                .as_ref()
                .and_then(|sp| sp.read_pid(corr))
                .is_some_and(|pid| theseus_kernel::job::wrapper_alive(pid, corr));
        if alive {
            self.answer(tc, ResultNode { correlation_id: Some(corr), ..ResultNode::new(&u.id, &name, ResultStatus::Background, format!("Still running as background job {corr} (the harness restarted meanwhile). Its result will arrive in a later message.")) })?;
            return Ok(Some(corr.clone()));
        }
        let _ = tc.kernel.mark_unknown(corr, "interrupted_by_restart");
        self.answer(tc, ResultNode { correlation_id: Some(corr), ..ResultNode::new(&u.id, &name, ResultStatus::Unknown, "The harness restarted while this call was running, and whether it completed cannot be established. Check the current state before retrying.") })?;
        Ok(None)
    }

    /// Settled, but its result node was lost in a restart: a job's output is
    /// in the spool; an in-process call's is gone.
    fn answer_settled(&self, tc: &TurnCtx<'_>, u: &ToolUse, a: &Action) -> Result<()> {
        let (tool, name) = self.tool_of(u);
        // A harness call the reconciler marked unknown (the daemon was down
        // past its deadline) runs again, and finds what it did (DD7).
        if let Some(t) = tool
            .as_ref()
            .filter(|t| t.backend() == Backend::Harness)
            .filter(|_| a.state == ActionState::OutcomeUnknown)
        {
            self.run_harness(tc, &a.correlation_id, t.as_ref(), u, approved_at(a))?;
            return Ok(());
        }
        if tool.as_ref().is_some_and(|t| t.backend() == Backend::Job) {
            self.answer_job(
                tc,
                self.job_result(tc.store, a, &u.id, &name, Some(&u.input)),
                a,
            )?;
            return Ok(());
        }
        let status = if a.state == ActionState::Succeeded {
            ResultStatus::Ok
        } else {
            ResultStatus::Unknown
        };
        self.answer(tc, ResultNode { correlation_id: Some(&a.correlation_id), ..ResultNode::new(&u.id, &name, status, "The call settled but its output was lost in a restart. Check the current state before relying on it.") })?;
        Ok(())
    }

    /// Declined or cancelled before it ran.
    pub(super) fn answer_cancelled(&self, tc: &TurnCtx<'_>, u: &ToolUse, a: &Action) -> Result<()> {
        let (_, name) = self.tool_of(u);
        let (status, text) = not_run_answer(a);
        let mut meta = Value::Null;
        crate::cancel::stopped_meta(&mut meta, status, a);
        self.answer(
            tc,
            ResultNode {
                correlation_id: Some(&a.correlation_id),
                meta,
                ..ResultNode::new(&u.id, &name, status, text)
            },
        )?;
        Ok(())
    }
}

/// Where one `tool_use` of the last assistant message stands when a turn
/// resumes it.
enum Pending {
    /// No tool-call node: the gate never saw it.
    NeverPlanned,
    /// A tool-call node and no action: it stopped at the gate, and its result
    /// was lost.
    StoppedAtGate,
    /// Planned and never asked: a restart came between its plan and its
    /// authorization, and its gate said `allow` (theseus-ni5).
    NeverAsked(Action),
    /// Waiting for the operator (`Action::awaits_confirm`).
    Waiting(String),
    /// Confirmed, and not yet authorized.
    Confirmed(Action),
    /// Authorized before a restart, and never dispatched.
    Authorized(Action),
    /// Dispatched: still running, settled in the spool, or lost.
    Dispatched(Action),
    /// Settled, and its result node lost in a restart.
    Settled(Action),
    /// Declined or cancelled before it ran.
    Cancelled(Action),
}

/// The posture a harness call run again runs at: `approve` when the
/// operator approved it (its confirm is bound), which a layer-1 task change
/// needs (39a).
fn approved_at(a: &Action) -> crate::policy::Posture {
    match a.confirm {
        Some(_) => crate::policy::Posture::Approve,
        None => crate::policy::Posture::Open,
    }
}
