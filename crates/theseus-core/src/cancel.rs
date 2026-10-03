//! The one stop of the calls a cancel reaches (M4 18a; design §2.3). Every
//! path that tells running calls to stop (a cancel, a task's cancel, `/stop`,
//! the disk's floor, and a stop that met a job's launch) ends their backends
//! through `ToolRuntime::terminate_all`, which says how each one stopped:
//!
//! - **A job:** its wrapper is asked to stop its whole tree, or, for a
//!   wrapper from before 18a, its process group is stopped as before
//!   (`theseus_kernel::job::Stopping`). The verdict says how it is known: the
//!   job's pid namespace (L1; its cgroup in records from before
//!   theseus-gyin), its process tree (L0), or its group.
//! - **An async tool's task** (`http.fetch`, `web.search`): aborted, and
//!   verified once its handle has finished (`task`).
//! - **Anything else** runs in process to its end, within its deadline:
//!   unsupported, and its real outcome is recorded when it ends.
//!
//! Each verdict is written on its action (ACTION schema 3), counted for
//! health, and recorded as a fact: `action.cancel_verified`,
//! `action.cancel_uncertain`, or `action.cancel_unsupported`.

use std::collections::{BTreeMap, HashMap};
use std::sync::{Mutex, PoisonError};
use std::time::{Duration, Instant};

use serde_json::{json, Value};
use theseus_kernel::{Action, ActionState, CancelState, Kernel, Verdict, VerifiedBy};
use theseus_protocol::{CancelCount, CancelVerdict};

use crate::fact;
use crate::node::ResultStatus;
use crate::toolrun::ToolRuntime;

/// Why a call with neither a job nor a task cannot be reached.
const IN_PROCESS: &str = "it runs in process to its end, within its deadline";

/// Why a job not yet launched cannot be: its launch stops it (theseus-36to).
const NOT_LAUNCHED: &str = "its job had not started; its launch stops it";

/// What a cancel can reach besides jobs, and what health counts of each
/// backend's cancels (M4 18a).
#[derive(Default)]
pub struct Stops {
    /// Each running async tool's task, by its call's correlation id.
    tasks: Mutex<HashMap<String, tokio::task::AbortHandle>>,
    /// Cancels since the daemon started, by backend and how they ended.
    counts: Mutex<BTreeMap<(&'static str, &'static str), u64>>,
}

/// An async tool's task, reachable by a cancel while this lives.
pub struct Tracked<'a> {
    stops: &'a Stops,
    correlation_id: String,
}

impl Drop for Tracked<'_> {
    fn drop(&mut self) {
        self.stops
            .tasks
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .remove(&self.correlation_id);
    }
}

impl Stops {
    /// Make call `correlation_id`'s task reachable by a cancel, until the
    /// guard drops.
    pub fn track(&self, correlation_id: &str, task: tokio::task::AbortHandle) -> Tracked<'_> {
        self.tasks
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(correlation_id.to_string(), task);
        Tracked {
            stops: self,
            correlation_id: correlation_id.to_string(),
        }
    }

    fn take(&self, correlation_id: &str) -> Option<tokio::task::AbortHandle> {
        self.tasks
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .remove(correlation_id)
    }

    fn count(&self, backend: &'static str, state: &'static str) {
        *self
            .counts
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .entry((backend, state))
            .or_default() += 1;
    }

    /// Health's counts: each backend's cancels since the start, by how they
    /// ended.
    pub fn counts(&self) -> Vec<CancelCount> {
        self.counts
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .iter()
            .map(|(&(backend, state), &n)| CancelCount {
                backend: backend.into(),
                state: state.into(),
                n,
            })
            .collect()
    }
}

/// How one call a stop reached ended: its action after the cancel's last
/// step, its backend, and the verdict. `written`: this stop's last step wrote
/// it. A job a stop at its launch ended, whose action an earlier stop had
/// settled, is stopped all the same and has its verdict, but its action
/// keeps the earlier step (theseus-36to).
pub struct Ended {
    pub action: Action,
    pub backend: &'static str,
    pub verdict: Verdict,
    pub written: bool,
}

impl Ended {
    /// The cancel's state, as health and the facts name it.
    fn state(&self) -> &'static str {
        match self.action.cancel {
            Some(CancelState::TerminationVerified) => "verified",
            Some(CancelState::Unsupported) => "unsupported",
            _ => "uncertain",
        }
    }

    /// The verdict as the wire carries it.
    pub fn wire(&self) -> CancelVerdict {
        wire(&self.action, &self.verdict)
    }

    /// Its fact, on `rec`'s channels.
    pub fn record(&self, rec: &fact::Rec<'_>) {
        let (action, verdict) = (&self.action, &self.verdict);
        match self.state() {
            "verified" => rec.record(&fact::cancel::CancelVerified { action, verdict }),
            "unsupported" => rec.record(&fact::cancel::CancelUnsupported { action, verdict }),
            _ => rec.record(&fact::cancel::CancelUncertain { action, verdict }),
        }
    }
}

/// An action's verdict as the wire carries it.
pub fn wire(a: &Action, v: &Verdict) -> CancelVerdict {
    CancelVerdict {
        correlation_id: a.correlation_id.clone(),
        tool: a.tool.clone(),
        state: match a.cancel {
            Some(CancelState::TerminationVerified) => "termination_verified",
            Some(CancelState::Unsupported) => "unsupported",
            _ => "outcome_uncertain",
        }
        .into(),
        verified_by: v.verified_by.as_str().into(),
        killed: v.killed,
        survivors: v.survivors,
        scope: v.scope.clone(),
        ms: v.ms,
        why: v.why.clone(),
    }
}

/// A cancelled call's verdict in words, from its action: "verified: pid
/// namespace, 4 processes", or "not verified: …". None for a call no cancel
/// stopped, or one cancelled before 18a.
pub fn words(a: &Action) -> Option<String> {
    let v = a.verdict.as_ref()?;
    a.cancel?;
    // A job a stop reached before its pid was written: its launch stopped it
    // (`job.stopped_at_launch` says how), and its action keeps the earlier
    // step's `unsupported`, which is no word on the job itself.
    if v.why.as_deref() == Some(NOT_LAUNCHED) {
        return None;
    }
    Some(wire(a, v).words())
}

/// The backend a job's verdict names: L1's namespace (or, in an old record,
/// its cgroup), L0's tree or group, or a job whose verdict cannot tell.
fn job_backend(v: &Verdict) -> &'static str {
    match v.verified_by {
        VerifiedBy::Pidns | VerifiedBy::Cgroup => "l1",
        VerifiedBy::Tree | VerifiedBy::Group => "l0",
        _ => "job",
    }
}

impl ToolRuntime {
    /// Terminate the backends of the actions a cancel or a stop told to
    /// stop, and walk each one's cancel to its last step, with its verdict.
    /// The jobs are stopped together (theseus-bzq): each is asked at once,
    /// and they share one grace. The waits are the runtime's timer, so no
    /// worker is held meanwhile. A job whose wrapper still runs is stopped
    /// even when its action has settled; a call its own completion settled
    /// keeps that, and its stop writes nothing.
    pub(crate) async fn terminate_all(&self, kernel: &Kernel, to_kill: &[String]) -> Vec<Ended> {
        let mut ended = Vec::new();
        let (mut jobs, mut tasks) = (Vec::new(), Vec::new());
        let open = |corr: &str| matches!(kernel.action(corr), Ok(Some(a)) if !a.state.is_settled());
        for corr in to_kill {
            let is_open = open(corr);
            if let Some(pid) = self.spool.as_ref().and_then(|s| s.read_pid(corr)) {
                if is_open {
                    let _ = kernel.cancel_acknowledged(corr);
                }
                jobs.push((pid, corr.clone()));
            } else if !is_open {
            } else if let Some(task) = self.stops.take(corr) {
                let _ = kernel.cancel_acknowledged(corr);
                task.abort();
                tasks.push((corr.clone(), task));
            } else if let Ok(a) = kernel.cancel_unsupported(corr, self.out_of_reach(kernel, corr)) {
                self.settled(&mut ended, a, None, "inproc");
            }
        }
        let t0 = Instant::now();
        let grace = theseus_kernel::job::STOP_GRACE;
        let mut stopping = match (&self.spool, jobs.is_empty()) {
            (Some(spool), false) => Some(theseus_kernel::job::Stopping::start(spool, jobs, grace)),
            _ => None,
        };
        loop {
            let next = stopping
                .as_mut()
                .and_then(theseus_kernel::job::Stopping::poll);
            // A task is gone once its handle has finished, which an abort
            // reaches at the task's next poll.
            let mut i = 0;
            while i < tasks.len() {
                let (corr, task) = &tasks[i];
                let ms = t0.elapsed().as_millis() as u64;
                let v = if task.is_finished() {
                    Verdict {
                        ms,
                        ..Verdict::verified_as(VerifiedBy::Task, None)
                    }
                } else if t0.elapsed() >= grace {
                    let why = format!(
                        "its task had not ended {} ms after its abort",
                        grace.as_millis()
                    );
                    Verdict {
                        ms,
                        ..Verdict::uncertain(VerifiedBy::Task, why)
                    }
                } else {
                    i += 1;
                    continue;
                };
                if open(corr) {
                    let a = if v.verified() {
                        kernel.cancel_verified(corr, Some(&v))
                    } else {
                        kernel.cancel_uncertain(corr, &v)
                    };
                    if let Ok(a) = a {
                        self.settled(&mut ended, a, None, "async");
                    }
                }
                tasks.swap_remove(i);
            }
            if next.is_none() && tasks.is_empty() {
                break;
            }
            let wait = match (next, tasks.is_empty()) {
                (Some(w), true) => w,
                (Some(w), false) => w.min(Duration::from_millis(5)),
                (None, _) => Duration::from_millis(5),
            };
            tokio::time::sleep(wait).await;
        }
        for (corr, v) in stopping
            .iter()
            .flat_map(theseus_kernel::job::Stopping::verdicts)
        {
            let backend = job_backend(&v);
            if !open(corr) {
                if let Ok(Some(a)) = kernel.action(corr) {
                    self.settled(&mut ended, a, Some(v), backend);
                }
                continue;
            }
            let a = if v.verified() {
                kernel.cancel_verified(corr, Some(&v))
            } else {
                kernel.cancel_uncertain(corr, &v)
            };
            if let Ok(a) = a {
                self.settled(&mut ended, a, None, backend);
            }
        }
        ended
    }

    /// Why a call with no wrapper and no task is out of a cancel's reach: a
    /// job not yet launched (its launch stops it, theseus-36to), or a call
    /// that runs in process to its end.
    fn out_of_reach(&self, kernel: &Kernel, corr: &str) -> &'static str {
        let tool = kernel.action(corr).ok().flatten().map(|a| a.tool);
        match tool
            .and_then(|t| self.registry.get(&t))
            .map(|t| t.backend())
        {
            Some(theseus_tools::Backend::Job) => NOT_LAUNCHED,
            _ => IN_PROCESS,
        }
    }

    /// Keep a call the stop reached, with its verdict, and count it when
    /// this stop's step wrote it. `seen`: the verdict of a job whose action
    /// an earlier step had settled.
    fn settled(
        &self,
        ended: &mut Vec<Ended>,
        a: Action,
        seen: Option<Verdict>,
        backend: &'static str,
    ) {
        let written = seen.is_none();
        let Some(verdict) = seen.or_else(|| a.verdict.clone()) else {
            return;
        };
        let e = Ended {
            action: a,
            backend,
            verdict,
            written,
        };
        if written {
            self.stops.count(backend, e.state());
        }
        ended.push(e);
    }
}

/// A call a `/stop` ended (W1) says who stopped it, in its result's `meta`
/// (`stopped_by`), which `tool.ended` carries: every surface then shows it as
/// a stop the operator asked for, `⏹️ stopped by …`, never as a failure, and
/// apart from a cancel's `not run` (theseus-4uw). A call a cancel or a stop
/// ended also says how it is known to have stopped (`verified`, M4 18a):
/// Discord's line adds "(verified)" or "(not verified: …)". Only a call that
/// did not finish: one that finished before the stop reached it keeps its
/// result.
pub(crate) fn stopped_meta(meta: &mut Value, status: ResultStatus, a: &Action) {
    let by = a.stopped_by().filter(|_| status == ResultStatus::Cancelled);
    let verified = words(a).filter(|_| a.state == ActionState::Cancelled);
    if by.is_none() && verified.is_none() {
        return;
    }
    if !meta.is_object() {
        *meta = json!({});
    }
    if let Some(by) = by {
        meta["stopped_by"] = json!(by);
    }
    if let Some(w) = verified {
        meta["verified"] = json!(w);
    }
}

/// How long a turn whose async call a cancel aborted waits for that cancel
/// to settle it: the stop looks at the task every few milliseconds.
const SETTLE_WAIT: Duration = Duration::from_secs(3);

/// The action of a call whose task a cancel aborted, once the cancel has
/// settled it (18a); `None` if it did not within `SETTLE_WAIT`, and the turn
/// settles it itself.
pub(crate) async fn after_abort(kernel: &Kernel, correlation_id: &str) -> Option<Action> {
    let t0 = Instant::now();
    loop {
        match kernel.action(correlation_id) {
            Ok(Some(a)) if a.state.is_settled() => {
                return (a.state == ActionState::Cancelled).then_some(a);
            }
            Ok(Some(_)) if t0.elapsed() < SETTLE_WAIT => {
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
            _ => return None,
        }
    }
}

/// An aborted call's result, from its settled action: cancelled, with who
/// stopped it and how it is known.
pub(crate) fn aborted_result(a: &Action) -> (ResultStatus, String, Value) {
    let text = format!(
        "[{}; {}]",
        a.resolution.as_deref().unwrap_or("cancelled"),
        words(a).unwrap_or_else(|| "its task was aborted".into())
    );
    let mut meta = Value::Null;
    stopped_meta(&mut meta, ResultStatus::Cancelled, a);
    (ResultStatus::Cancelled, text, meta)
}
