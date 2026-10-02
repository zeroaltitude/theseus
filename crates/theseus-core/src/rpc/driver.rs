//! What the harness loop drives: the heartbeat (drain the spool, reconcile),
//! continuation turns, and the cancel control path.

use std::sync::Arc;
use std::time::Instant;

use anyhow::Result;

use super::Core;
use crate::bus::EventSink;
use crate::fact;
use crate::session::SessionRecord;
use crate::turn::TurnRequest;
use theseus_kernel::job::WrapperEvidence;
use theseus_kernel::Execution;

impl Core {
    /// Heartbeat: drain the spool, reconcile against the wrapper evidence.
    /// Called by the harness loop on its timer and when a wrapper pokes the
    /// notify socket.
    #[expect(clippy::cognitive_complexity, reason = "shape budget: split it")]
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
                    if due + evidence + unknown > 0 {
                        self.rec(None).record(&fact::driver::HeartbeatActed {
                            why,
                            due,
                            evidence,
                            unknown,
                        });
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

    /// The narrative's line for a job's completion that came from the spool:
    /// its action is read only when the narrative is on.
    fn narrate_spooled(&self, c: &theseus_kernel::Completion) {
        let Ok(Some(a)) = self.kernel.action(&c.correlation_id) else {
            return;
        };
        self.session_rec(&a.session_id)
            .record(&fact::driver::SpooledCompletion {
                completion: c,
                action: &a,
            });
    }

    /// Accept every spooled completion, removing each file after its frame.
    #[expect(clippy::cognitive_complexity, reason = "shape budget: split it")]
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
    /// A task says it was cancelled where it reports, once, in the cancel's
    /// frame (DD7).
    ///
    /// What the execution planned and never sent is cancelled in that frame
    /// too (theseus-w98): each tool call gets its result there, "Not run: the
    /// execution was cancelled by …", as the session's next turn would have
    /// written it, unless a turn holds the execution, whose transcript is its
    /// own. Then each question's card settles where it was posted, and the
    /// session's watchers hear that it closed.
    pub async fn cancel_execution(&self, id: &str, by: &str) -> Result<(Execution, Vec<String>)> {
        let mut report = None;
        let cancel = self.kernel.cancel_execution_with(id, by, |end| {
            let (mut records, post) =
                crate::task::cancelled_report(&self.outbox, &self.store, end.execution)?;
            report = post;
            if !end.turn_running {
                records.extend(crate::toolrun::not_run_results(
                    &self.store,
                    &end.execution.session_id,
                    end.not_run,
                )?);
            }
            Ok(records)
        })?;
        if let Some(post) = report {
            self.outbox.posted(&post);
        }
        for a in &cancel.not_run {
            if let Some(how) = crate::outbox::Closed::of(a) {
                if let Err(e) = self.outbox.closed(&a.correlation_id, how) {
                    tracing::warn!(error = %format!("{e:#}"), "a cancelled question's settle was not written");
                }
            }
            // A question the operator was asked: a tool call waiting for an
            // answer, or the budget question. Each keeps its proposal.
            if a.proposal.is_some() && a.confirm.is_none() {
                self.session_rec(&a.session_id)
                    .record(&fact::driver::QuestionCancelled { question: a, by });
            }
        }
        let to_kill = cancel.to_kill;
        self.terminate_all(&to_kill).await;
        self.admission.notify_waiters();
        let e = self
            .kernel
            .execution(id)?
            .ok_or_else(|| anyhow::anyhow!("execution {id} vanished"))?;
        self.session_rec(&e.session_id)
            .record(&fact::driver::ExecutionCancelled {
                execution: &e,
                by,
                stopped: to_kill.len(),
            });
        Ok((e, to_kill))
    }

    /// Terminate the backends of actions a cancel or a stop told to stop,
    /// and walk each one's cancel: a job's wrapper through the spool, and an
    /// in-process call, which nothing can reach, as unsupported.
    ///
    /// The jobs are stopped together (theseus-bzq): every one gets SIGTERM
    /// first, they share one grace, and the stragglers get SIGKILL together.
    /// The waits are the runtime's timer, so no worker is held meanwhile, and
    /// N jobs that ignore SIGTERM cost one grace, not N.
    async fn terminate_all(&self, to_kill: &[String]) {
        let mut jobs = Vec::new();
        for corr in to_kill {
            match self.spool.read_pid(corr) {
                Some(pid) => {
                    let _ = self.kernel.cancel_acknowledged(corr);
                    jobs.push((pid, corr.clone()));
                }
                None => {
                    // In-process or already gone: nothing to reach.
                    let _ = self.kernel.cancel_unsupported(corr);
                }
            }
        }
        if jobs.is_empty() {
            return;
        }
        let mut stopping =
            theseus_kernel::job::Stopping::start(jobs, theseus_kernel::job::STOP_GRACE);
        while let Some(wait) = stopping.poll() {
            tokio::time::sleep(wait).await;
        }
        for (corr, gone) in stopping.outcome() {
            let _ = if gone {
                self.kernel.cancel_verified(corr)
            } else {
                self.kernel.cancel_uncertain(corr)
            };
        }
    }

    /// `/stop` (W1, theseus-lji): halt a conversation's work and keep the
    /// conversation. The kernel tells its running jobs and calls to stop and
    /// declines what waits on the operator (`Kernel::stop_execution`); here
    /// each job's backend is terminated, and each declined question's card
    /// settles where it was posted. A running turn ends at its next step.
    /// The session's tasks and pending wakes go on.
    pub async fn stop_execution(
        &self,
        id: &str,
        by: &str,
    ) -> Result<theseus_protocol::ExecutionStopResult> {
        let stop = self.kernel.stop_execution(id, by)?;
        let Some(stop) = stop else {
            let e = self
                .kernel
                .execution(id)?
                .ok_or_else(|| anyhow::anyhow!("execution {id} vanished"))?;
            return Ok(theseus_protocol::ExecutionStopResult {
                execution: Self::execution_info(&e),
                stopped: false,
                stopped_actions: vec![],
                declined: vec![],
                turn_running: false,
                tasks_running: 0,
                wakes_pending: 0,
            });
        };
        for a in &stop.declined {
            if let Err(e) = self.outbox.closed(
                &a.correlation_id,
                crate::outbox::Closed::new("stopped", Some(by)),
            ) {
                tracing::warn!(error = %format!("{e:#}"), "a stopped question's settle was not written");
            }
            self.session_rec(&stop.execution.session_id)
                .record(&fact::driver::QuestionStopped { question: a, by });
        }
        self.terminate_all(&stop.to_kill).await;
        self.admission.notify_waiters();
        let e = self
            .kernel
            .execution(id)?
            .ok_or_else(|| anyhow::anyhow!("execution {id} vanished"))?;
        let tasks_running = self
            .kernel
            .tasks(Some(id))?
            .iter()
            .filter(|t| !t.state.is_terminal())
            .count() as u32;
        self.session_rec(&e.session_id)
            .record(&fact::driver::ExecutionStopped {
                execution: &e,
                by,
                to_kill: stop.to_kill.len(),
                declined: stop.declined.len(),
                turn_running: stop.turn_running,
                tasks_running,
            });
        Ok(theseus_protocol::ExecutionStopResult {
            execution: Self::execution_info(&e),
            stopped: true,
            stopped_actions: stop.to_kill,
            declined: stop
                .declined
                .iter()
                .map(|a| a.correlation_id.clone())
                .collect(),
            turn_running: stop.turn_running,
            tasks_running,
            wakes_pending: e.wakes.len() as u32,
        })
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
        self.session_rec(&a.session_id)
            .record(&fact::driver::WrapperLost {
                action: &a,
                pid,
                signal,
            });
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
        self.session_rec(&session.session_id)
            .record(&fact::driver::DriverResumes { execution: &e });
        let (live, _) = self.live_profile();
        let target = self.runner.target_for_session(&session, &live)?;
        let ran_on = (
            target.profile.clone(),
            target.provider.clone(),
            target.model.clone(),
        );
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
            .await
            .inspect_err(|e| {
                if let Some(te) = e.downcast_ref::<crate::turn::TurnError>() {
                    self.count_failed_turn(&ran_on.0, &ran_on.1, &ran_on.2, te);
                }
            })?;
        self.telemetry().record_turn(&res);
        Ok(Some(res))
    }
}
