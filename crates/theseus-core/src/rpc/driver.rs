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
        // A hand is its own reconciler's, which asks AWS first (step 40).
        let ev = crate::aws::hands::overdue::Evidence(&ev);
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
                        // A job the reconciler settled: its turn, if one
                        // waits, looks again (7.1).
                        self.tools.job_waits.wake_all();
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
    /// A job a turn waits on is that turn's: woken, it takes the completion
    /// with its result's node in one frame (7.1). Each other is taken here,
    /// so one a turn took first writes nothing.
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
                    if self.tools.job_waits.wake(&c.correlation_id) {
                        continue;
                    }
                    match self.kernel.take_completion_with(&c, vec![]) {
                        Ok(theseus_kernel::Accepted::Taken { .. }) => {
                            if let Err(e) = self.spool.remove(&path) {
                                tracing::warn!(error = %e, "spool remove failed");
                            }
                        }
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
        self.cancel_execution_judged(id, by)
            .await
            .map(|(e, to_kill, _)| (e, to_kill))
    }

    /// `cancel_execution`, with how each call it stopped is known to have
    /// stopped (M4 18a), as `execution.cancel` and `task.cancel` answer.
    pub async fn cancel_execution_judged(
        &self,
        id: &str,
        by: &str,
    ) -> Result<(Execution, Vec<String>, Vec<theseus_protocol::CancelVerdict>)> {
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
        let verdicts = self.terminate_all(&to_kill).await;
        self.admission.notify_waiters();
        let e = self
            .kernel
            .execution(id)?
            .ok_or_else(|| anyhow::anyhow!("execution {id} vanished"))?;
        // What the cancel left unanswered in the transcript (theseus-0o8): the
        // calls it stopped, now that they have settled. A turn that holds the
        // execution answers its own at its end, and this finds it held.
        match self
            .tools
            .answer_after_cancel(&self.kernel, &self.store, &e.session_id, id)
        {
            Ok(nodes) => crate::toolrun::announce_cancelled(
                &self.session_rec(&e.session_id),
                &e.session_id,
                &nodes,
            ),
            Err(err) => {
                tracing::warn!(error = %format!("{err:#}"), execution_id = %id, "a cancelled execution's unanswered calls were not answered");
            }
        }
        self.session_rec(&e.session_id)
            .record(&fact::driver::ExecutionCancelled {
                execution: &e,
                by,
                stopped: to_kill.len(),
            });
        Ok((e, to_kill, verdicts))
    }

    /// Terminate the backends of actions a cancel or a stop told to stop,
    /// through the one stop (`ToolRuntime::terminate_all`, M4 18a), and
    /// record how each one ended: its fact, in its session. Each verdict, as
    /// the wire carries it.
    async fn terminate_all(&self, to_kill: &[String]) -> Vec<theseus_protocol::CancelVerdict> {
        let ended = self
            .tools
            .terminate_all(&self.kernel, &self.store, to_kill)
            .await;
        let written: Vec<_> = ended.iter().filter(|e| e.written).collect();
        for e in &written {
            e.record(&self.session_rec(&e.action.session_id));
        }
        written.iter().map(|e| e.wire()).collect()
    }

    /// Watch the disk's floor while jobs run (theseus-ht82). The floor refuses
    /// the next job, and a job already running can still fill the disk through
    /// files it writes itself (a build's target dir, a log it opens, a
    /// download); on WSL that is the file on C:, which pauses the whole VM
    /// when it fills. So below the floor every running job is stopped, with
    /// its reason: its result reads "stopped by the disk floor (812 MB free,
    /// below the floor of 1,024 MB)", it has a `job.stopped_below_floor` row,
    /// and its execution is woken, so the model reads that and can tell the
    /// operator. The heartbeat's timer calls it. It reads the spool's pid
    /// files first, so with no job running it costs one directory read, and
    /// the disk is looked at only while one is.
    ///
    /// The writer cannot be told from the others: nothing meters a job's own
    /// writes (WSL has no per-process io counters, and no quota or cgroup
    /// holds a job), so all are stopped. Returns how many it stopped.
    pub async fn stop_jobs_below_floor(&self) -> usize {
        let running = self.spool.running();
        if running.is_empty() {
            return 0;
        }
        let Some(refusal) = self.tools.disk.refusal() else {
            return 0;
        };
        let by = format!(
            "the disk floor ({} MB free, below the floor of {} MB)",
            crate::narrative::thousands(refusal.free_mb),
            crate::narrative::thousands(refusal.floor_mb)
        );
        let mut stopped = Vec::new();
        for (id, _) in &running {
            match self.kernel.stop_call(id, &by) {
                Ok(Some(a)) => stopped.push(a),
                Ok(None) => {}
                Err(e) => {
                    tracing::warn!(job = %id, error = %format!("{e:#}"), "a job below the disk floor was not stopped");
                }
            }
        }
        if stopped.is_empty() {
            return 0;
        }
        for a in &stopped {
            self.session_rec(&a.session_id)
                .record(&fact::tool::JobStoppedBelowFloor {
                    correlation_id: &a.correlation_id,
                    tool: &a.tool,
                    free_mb: refusal.free_mb,
                    floor_mb: refusal.floor_mb,
                });
        }
        tracing::warn!(
            jobs = stopped.len(),
            free_mb = refusal.free_mb,
            floor_mb = refusal.floor_mb,
            "the disk is below its floor: the running jobs are stopped"
        );
        let to_kill: Vec<String> = stopped.iter().map(|a| a.correlation_id.clone()).collect();
        self.terminate_all(&to_kill).await;
        // Each job's result reaches its conversation: a turn waiting on it
        // hears so at once, and one that left it running is woken to read it.
        for a in &stopped {
            let _ = self.kernel.wake(&a.execution_id, "disk");
        }
        self.admission.notify_waiters();
        stopped.len()
    }

    /// `/stop` (W1, theseus-lji): halt a conversation's work and keep the
    /// conversation. The kernel tells its running jobs and calls to stop and
    /// declines what waits on the operator (`Kernel::stop_execution`); here
    /// each job's backend is terminated, and each declined question's card
    /// settles where it was posted. A running turn ends at its next step,
    /// and so does the turn of an input sent before the stop and admitted
    /// after it (theseus-hmwv). The session's tasks and pending wakes go on.
    pub async fn stop_execution(
        &self,
        id: &str,
        by: &str,
    ) -> Result<theseus_protocol::ExecutionStopResult> {
        self.runner.stop_landed(id, by);
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
                verdicts: vec![],
                declined: vec![],
                turn_running: false,
                tasks_running: 0,
                wakes_pending: 0,
            });
        };
        // The model call the running turn waits on is cut now, and not left to
        // finish (theseus-yey): the kernel's record of the stop is written, so
        // the turn finds it when this wakes it.
        if stop.turn_running {
            self.runner.stops.signal(id);
        }
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
        let verdicts = self.terminate_all(&stop.to_kill).await;
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
            verdicts,
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
        // A turn waiting on the job hears at once (7.1).
        self.tools.job_waits.wake(job);
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
