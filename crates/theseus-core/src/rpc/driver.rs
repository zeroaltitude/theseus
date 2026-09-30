//! What the harness loop drives: the heartbeat (drain the spool, reconcile),
//! continuation turns, and the cancel control path.

use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::Result;
use serde_json::Value;

use super::Core;
use crate::bus::EventSink;
use crate::narrative::narrate;
use crate::session::SessionRecord;
use crate::turn::TurnRequest;
use theseus_kernel::job::WrapperEvidence;
use theseus_kernel::Execution;

impl Core {
    /// Heartbeat: drain the spool, reconcile against the wrapper evidence.
    /// Called by the harness loop on its timer and when a wrapper pokes the
    /// notify socket.
    pub fn heartbeat(&self, why: &str) {
        let t0 = Instant::now();
        let drained = self.drain_spool();
        let ev = WrapperEvidence {
            spool: self.spool.clone(),
        };
        match self.kernel.reconcile(&ev) {
            Ok(rep) => {
                let changed = !rep.woke_due.is_empty()
                    || !rep.settled_from_evidence.is_empty()
                    || !rep.marked_unknown.is_empty()
                    || !rep.resolved_unknown.is_empty()
                    || drained > 0;
                if changed {
                    tracing::info!(
                        why,
                        drained,
                        woke_due = rep.woke_due.len(),
                        settled_from_evidence = rep.settled_from_evidence.len(),
                        marked_unknown = rep.marked_unknown.len(),
                        resolved_unknown = rep.resolved_unknown.len(),
                        open_actions = rep.open_actions,
                        open_executions = rep.open_executions,
                        us = t0.elapsed().as_micros() as u64,
                        "heartbeat"
                    );
                    let (due, evidence, unknown) = (
                        rep.woke_due.len() as u64,
                        rep.settled_from_evidence.len() as u64,
                        rep.marked_unknown.len() as u64,
                    );
                    if self.narrator.on() && due + evidence + unknown > 0 {
                        narrate!(
                            self.narrator,
                            Job,
                            None,
                            None,
                            "Heartbeat ({why}): {} woke because a wait came due, {} \
                             settled from a job wrapper's evidence, {} marked unknown.",
                            crate::narrative::count(due, "execution", "executions"),
                            crate::narrative::count(evidence, "action", "actions"),
                            unknown
                        );
                    }
                    self.admission.notify_waiters();
                } else {
                    tracing::debug!(
                        why,
                        open_actions = rep.open_actions,
                        open_executions = rep.open_executions,
                        us = t0.elapsed().as_micros() as u64,
                        "heartbeat: nothing to do"
                    );
                }
            }
            Err(e) => tracing::warn!(error = %e, "reconcile failed"),
        }
        // A card whose question closed without an event that said so (its
        // execution ended, say) gets its settle (theseus-q4v).
        match self.outbox.reconcile_cards() {
            Ok(0) => {}
            Ok(n) => tracing::info!(why, settles = n, "heartbeat: cards whose question closed"),
            Err(e) => tracing::warn!(error = %format!("{e:#}"), "reconciling cards failed"),
        }
    }

    /// The narrative's line for a job's completion that came from the spool.
    fn narrate_spooled(&self, c: &theseus_kernel::Completion) {
        let Ok(Some(a)) = self.kernel.action(&c.correlation_id) else {
            return;
        };
        let outcome = match c.outcome {
            theseus_kernel::Outcome::Succeeded => "succeeded",
            theseus_kernel::Outcome::Failed => "failed",
            theseus_kernel::Outcome::Unknown => "an unknown outcome",
        };
        let exit = c
            .detail
            .as_ref()
            .and_then(|d| d.get("exit_code"))
            .and_then(Value::as_i64)
            .map(|x| format!(", exit code {x}"))
            .unwrap_or_default();
        narrate!(
            self.narrator,
            Job,
            Some(&a.session_id),
            None,
            "Job {} ({}) finished: {outcome}{exit}; its completion came \
             from the spool.",
            crate::narrative::short(&c.correlation_id),
            a.tool
        );
    }

    /// Accept every spooled completion, removing each file after its frame.
    pub fn drain_spool(&self) -> u32 {
        let mut n = 0;
        match self.spool.drain() {
            Ok(d) => {
                if d.malformed > 0 {
                    tracing::warn!(
                        malformed = d.malformed,
                        "spool: unparseable completions moved to malformed/"
                    );
                }
                for (path, c) in d.completions {
                    match self.kernel.accept_completion(&c) {
                        Ok(acc) => {
                            tracing::info!(correlation_id = %c.correlation_id, producer = %c.producer, result = ?acc, "completion accepted from spool");
                            if self.narrator.on() {
                                self.narrate_spooled(&c);
                            }
                            if let Err(e) = self.spool.remove(&path) {
                                tracing::warn!(error = %e, "spool remove failed");
                            }
                            n += 1;
                        }
                        Err(e) => {
                            tracing::warn!(error = %e, correlation_id = %c.correlation_id, "completion refused")
                        }
                    }
                }
            }
            Err(e) => tracing::warn!(error = %e, "spool drain failed"),
        }
        n
    }

    /// `/cancel <execution>`: deterministic control path. Terminates wrapper
    /// processes the spool knows about and walks each action's cancel lifecycle.
    pub fn cancel_execution(&self, id: &str, by: &str) -> Result<(Execution, Vec<String>)> {
        let to_kill = self.kernel.cancel_execution(id, by)?;
        for corr in &to_kill {
            match self.spool.read_pid(corr) {
                Some(pid) => {
                    let _ = self.kernel.cancel_acknowledged(corr);
                    if theseus_kernel::job::terminate(pid, corr, Duration::from_secs(2)) {
                        let _ = self.kernel.cancel_verified(corr);
                    } else {
                        let _ = self.kernel.cancel_uncertain(corr);
                    }
                }
                None => {
                    // In-process or already gone: nothing to reach.
                    let _ = self.kernel.cancel_unsupported(corr);
                }
            }
        }
        self.admission.notify_waiters();
        let e = self
            .kernel
            .execution(id)?
            .ok_or_else(|| anyhow::anyhow!("execution {id} vanished"))?;
        narrate!(
            self.narrator,
            Session,
            Some(&e.session_id),
            None,
            "Execution {} cancelled by {by}: {} stopped; it is {} now.",
            crate::narrative::short(id),
            crate::narrative::count(to_kill.len() as u64, "action", "actions"),
            e.state.as_str()
        );
        Ok((e, to_kill))
    }

    /// A job wrapper the reaper took, which a signal ended (theseus-6uo). One
    /// that had not reported, and that no cancel stopped, was killed by its
    /// own job or by something beside it, which is a security event. Its
    /// action is marked unknown at once, instead of at its deadline, and the
    /// ledger (`job.wrapper_lost`) and the narrative say so. Returns whether
    /// it was lost.
    pub fn wrapper_signalled(&self, pid: u32, job: &str, signal: i32) -> bool {
        let Ok(Some(a)) = self.kernel.action(job) else {
            return false;
        };
        let reported = matches!(self.spool.read_completion(job), Ok(Some(_)));
        if a.state != theseus_kernel::ActionState::Dispatched || a.cancel.is_some() || reported {
            return false;
        }
        // It rechecks under the execution's lock: a completion that landed
        // meanwhile wins, and this was no loss.
        if self.kernel.mark_unknown(job, "wrapper_lost").is_err() {
            return false;
        }
        tracing::warn!(pid, job, signal, tool = %a.tool, "a job's wrapper was killed before it reported; its outcome is unknown");
        let row = crate::ledger::LedgerRow::new(
            "job.wrapper_lost",
            Some(&a.session_id),
            None,
            serde_json::json!({"correlation_id": job, "pid": pid, "signal": signal,
                "tool": a.tool, "execution_id": a.execution_id}),
        );
        if let Err(e) = self.store.append_ledger(&row) {
            tracing::warn!(error = %e, "ledger append failed");
        }
        narrate!(
            self.narrator,
            Job,
            Some(&a.session_id),
            None,
            "Job {} ({}) lost its wrapper (pid {pid}, killed by signal {signal}) before it \
             reported: the job, or something beside it, killed it. Its outcome is unknown.",
            crate::narrative::short(job),
            a.tool
        );
        self.admission.notify_waiters();
        true
    }

    /// The harness driver: take a continuation turn for an execution that is
    /// runnable without human input. Returns quickly if it is not ready.
    pub async fn continue_execution(
        self: &Arc<Self>,
        execution_id: &str,
    ) -> Result<Option<theseus_protocol::TurnSubmitResult>> {
        let Some(e) = self.kernel.execution(execution_id)? else {
            return Ok(None);
        };
        let Some(session) = self.store.get_session::<SessionRecord>(&e.session_id)? else {
            return Ok(None);
        };
        narrate!(
            self.narrator,
            Session,
            Some(&session.session_id),
            None,
            "The driver resumes execution {}: {}.",
            crate::narrative::short(&e.id),
            match e.queued_results.len() {
                0 if e.resume_pending => "it was woken".to_string(),
                0 => "it is queued".to_string(),
                n => format!(
                    "{} arrived",
                    crate::narrative::count(n as u64, "result", "results")
                ),
            }
        );
        let (live, _) = self.live_profile();
        let target = self.runner.target_for_session(&session, &live)?;
        let sink = EventSink::new(self.bus.clone(), &session.session_id, None);
        let res = self
            .runner
            .run(TurnRequest {
                session,
                input: None,
                target,
                sink,
                author: "harness".into(),
                recompile: None,
                attachments: vec![],
                arrived: None,
                config_wait_us: 0,
                reply_to: None,
            })
            .await?;
        self.telemetry().record_turn(&res);
        Ok(Some(res))
    }
}
