//! A job's call (`proc.run`, through the detached job wrapper): its
//! launch, the wait up to `proc_sync_secs`, its result read from the
//! tail of its raw output, and that output's removal once the result is
//! written. Split from `toolrun.rs` (theseus-5gw9).

use std::io::{Read, Seek, SeekFrom};
use std::time::{Duration, Instant};

use anyhow::{anyhow, Result};
use serde_json::{json, Value};
use theseus_kernel::job::WrapperArgs;
use theseus_kernel::{Accepted, Action, ActionState, Completion, Kernel, Outcome, Spool};
use theseus_store::Store as _;
use theseus_tools::{JobSpec, Tool};
use zeroize::Zeroize;

use super::{
    confirm_proposal, forbidden_env, not_run_answer, CallOutcome, ResultNode, ToolRuntime, TurnCtx,
};
use crate::egress;
use crate::fact;
use crate::narrative;
use crate::node::{Node, ResultStatus};
use crate::policy::Posture;
use crate::provider::ToolUse;
use crate::sandbox;
use crate::store::Store;

const MAX_RESULT_READ: usize = 4 * 1024 * 1024;

/// `4 MiB` for a whole number of MiB, else the bytes: the sizes a job's
/// result names.
fn size(n: u64) -> String {
    const MIB: u64 = 1024 * 1024;
    if n >= MIB && n.is_multiple_of(MIB) {
        format!("{} MiB", n / MIB)
    } else {
        narrative::count(n, "byte", "bytes")
    }
}

/// The end of a job's raw output, as the result reads it.
#[derive(Debug, Default, PartialEq)]
struct Tail {
    text: String,
    /// The file's whole length.
    total: u64,
    /// The bytes before what was read.
    unread: u64,
}

/// What the turn's look at its job needs (7.1).
struct Look<'a> {
    spool: &'a Spool,
    correlation_id: &'a str,
    call: &'a ToolUse,
    tool: &'a str,
    /// The launch, from which the result's duration counts.
    t0: Instant,
    /// The broker's note, which heads the result.
    note: Option<&'a str>,
}

impl Look<'_> {
    /// `r` with the wait's duration and the broker's note.
    fn dressed<'r>(&self, mut r: ResultNode<'r>) -> ResultNode<'r> {
        r.duration_ms = Some(self.t0.elapsed().as_millis() as u64);
        if let Some(n) = self.note {
            r.text = format!("{n}\n{}", r.text);
        }
        r
    }
}

/// The last `MAX_RESULT_READ` bytes of a job's raw output, read by seek
/// (theseus-102): whatever the job printed, the daemon holds no more than
/// that. A cut through a UTF-8 character moves to the character's end.
/// Its cgroup's cap refused it new processes (theseus-a5nv): what it did may
/// have failed for that, and the result says so.
fn cap_line(detail: &Value) -> Option<String> {
    let n = detail
        .get("pids_refused")
        .and_then(Value::as_u64)
        .filter(|n| *n > 0)?;
    Some(format!(
        "[its cap of {} processes and threads ([tools] job_pids_max) refused it {}: what it did \
         may have failed for that]\n",
        detail["pids_max"],
        narrative::count(n, "new one", "new ones"),
    ))
}

fn read_result_file(path: Option<&str>) -> Tail {
    let Some(p) = path else {
        return Tail::default();
    };
    std::fs::File::open(p)
        .and_then(|mut f| read_tail(&mut f, MAX_RESULT_READ as u64))
        .unwrap_or_default()
}

/// `read_result_file` over any reader that seeks.
fn read_tail<R: Read + Seek>(r: &mut R, max: u64) -> std::io::Result<Tail> {
    let total = r.seek(SeekFrom::End(0))?;
    let mut unread = total.saturating_sub(max);
    r.seek(SeekFrom::Start(unread))?;
    let mut b = Vec::with_capacity((total - unread) as usize);
    // Bounded by the length seen: a file that grows meanwhile is not followed.
    r.take(total - unread).read_to_end(&mut b)?;
    let skip = if unread > 0 {
        b.iter().take(3).take_while(|&&c| c & 0xC0 == 0x80).count()
    } else {
        0
    };
    unread += skip as u64;
    Ok(Tail {
        text: String::from_utf8_lossy(&b[skip..]).into_owned(),
        total,
        unread,
    })
}

impl ToolRuntime {
    /// A job: started through the wrapper, waited for up to `proc_sync_secs`,
    /// then left to run in the background with a placeholder result.
    #[expect(clippy::too_many_lines, reason = "shape budget: split it")]
    pub(super) async fn run_job(
        &self,
        tc: &TurnCtx<'_>,
        correlation_id: &str,
        tool: &dyn Tool,
        call: &ToolUse,
        ran_at: Posture,
        class: &sandbox::Bound,
    ) -> Result<CallOutcome> {
        let spec: JobSpec = match tool.job(&call.input, &self.ctx) {
            Ok(s) => s,
            Err(e) => return self.settle_job_failure(tc, correlation_id, tool.name(), call, &e),
        };
        let Some(spool) = self.spool.clone() else {
            return self.settle_job_failure(
                tc,
                correlation_id,
                tool.name(),
                call,
                "no completion spool is configured",
            );
        };
        // Below the floor no job starts (theseus-102): the store keeps room
        // to write, and the result says why, to the model and every surface.
        if let Some(r) = self.disk.refusal() {
            tc.record(&fact::tool::JobRefused {
                correlation_id,
                tool: tool.name(),
                free_mb: r.free_mb,
                floor_mb: r.floor_mb,
            });
            return self.settle_job_failure(tc, correlation_id, tool.name(), call, &r.to_string());
        }
        let mut env = self.proc_env.clone();
        for (k, v) in &spec.env {
            if forbidden_env(k) {
                return self.settle_job_failure(
                    tc,
                    correlation_id,
                    tool.name(),
                    call,
                    &format!("environment variable {k} may not be set by a tool call"),
                );
            }
            env.retain(|(ek, _)| ek != k);
            env.push((k.clone(), v.clone()));
        }
        // Its session (theseus-b5cl), L0 and L1: a `theseus` it runs sends it
        // as `opened_from`, so a session it opens or writes to takes this
        // one's hold. No call sets a THESEUS variable (`forbidden_env`).
        let session = theseus_protocol::JOB_SESSION_ENV;
        env.retain(|(k, _)| k != session);
        env.push((session.into(), tc.session_id.into()));
        // The broker's variables, from the board (theseus-dcy): a program run
        // by its own argv, by a call that sets no variable of its own (review
        // 2's H7), gets its grant, and nothing stands in for a secret it does
        // not get.
        let path = env
            .iter()
            .find(|(k, _)| k == "PATH")
            .map(|(_, v)| v.clone());
        let set: Vec<&str> = spec.env.iter().map(|(k, _)| k.as_str()).collect();
        let (brokered, sandbox) = sandbox::for_job(
            self,
            class,
            &spec,
            &set,
            path.as_deref(),
            ran_at,
            correlation_id,
        )
        .await;
        for (k, v) in &brokered.env {
            env.retain(|(ek, _)| ek != k);
            env.push((k.clone(), v.expose().to_string()));
        }
        // A git given a secret, or the git a gh given one runs: no hooks and
        // no fsmonitor program (theseus-ur1t).
        brokered.pin(&mut env);
        let mut args = WrapperArgs {
            spool_dir: spool.dir().to_path_buf(),
            correlation_id: correlation_id.into(),
            deadline_ms: spec.timeout_secs * 1000,
            notify_socket: self.notify_socket.clone(),
            argv: spec.argv.clone(),
            cwd: Some(spec.cwd.clone()),
            env,
            umask: theseus_kernel::umask::operator(),
            // Each granted variable and its secret's name: the wrapper
            // withholds the value from the job's raw output (theseus-l0d).
            redact: brokered
                .granted
                .iter()
                .filter_map(|g| Some((g.variable.clone()?, g.secret.clone())))
                .filter(|(var, _)| brokered.env.iter().any(|(k, _)| k == var))
                .collect(),
            output_max_bytes: self.output_max_bytes,
            sandbox,
            // The daemon's cgroup, once it is readied (theseus-a5nv).
            cgroup: theseus_kernel::cgroup::ready_jobs().cloned(),
        };
        // The values go with the spawn, as its environment, or nowhere; the
        // copies here are wiped either way.
        let wipe = |args: &mut WrapperArgs| {
            for (k, v) in args.env.iter_mut() {
                if brokered.env.iter().any(|(g, _)| g == k) {
                    v.zeroize();
                }
            }
        };
        // Outbox: `dispatched` was durable before the process exists. A stop
        // or a cancel that came since read the spool for a pid that is not
        // there yet, and reached nothing: the job is not started
        // (theseus-36to).
        if let Some(a) = Self::told_to_stop(tc.kernel, correlation_id)? {
            wipe(&mut args);
            return self.not_started(tc, &a, call, tool.name());
        }
        // The turn waits on its job from before the launch, so the drain
        // leaves the job's completion to it, and wakes it (7.1).
        let waiting = self.job_waits.wait(correlation_id);
        let launched = self.launcher.launch(&spool, &args, waiting.done());
        wipe(&mut args);
        let pid = match launched {
            Ok(p) => p,
            Err(e) => {
                return self.settle_job_failure(
                    tc,
                    correlation_id,
                    tool.name(),
                    call,
                    &format!("could not start the job: {e}"),
                );
            }
        };
        let granted = crate::broker::got(&brokered.granted);
        let withheld: Vec<String> = brokered
            .withheld
            .iter()
            .map(|(g, _)| format!("{} got no {}", g.to, g.variable.as_deref().unwrap_or("")))
            .collect();
        let note = brokered.note();
        let t0 = Instant::now();
        let bound = Duration::from_secs(self.proc_sync_secs.min(spec.timeout_secs + 5));
        // An L1 job's command, for `sandbox.usage` until its row is written
        // (theseus-kpz1): listed until this call returns.
        let _running = sandbox::started(self, tc, correlation_id, tool.name(), &spec, &args);
        tc.record(&fact::tool::JobStarted {
            session_id: tc.session_id,
            turn_id: tc.turn_id,
            tool_use_id: &call.id,
            tool: tool.name(),
            correlation_id,
            pid,
            argv: &spec.argv,
            cwd: &spec.cwd,
            timeout_secs: spec.timeout_secs,
            granted: granted.as_deref(),
            withheld: &withheld,
            bound_ms: bound.as_millis() as u64,
            scrubber: &self.scrubber,
            class,
        });
        for g in &brokered.granted {
            tc.record(&fact::tool::SecretGranted {
                grant: g,
                correlation_id,
                tool: tool.name(),
            });
        }
        for (g, why) in &brokered.withheld {
            tc.record(&fact::tool::SecretWithheld {
                grant: g,
                why,
                correlation_id,
                tool: tool.name(),
            });
        }
        // A stop or a cancel that came during the launch read the spool
        // before the pid was in it, and reached nothing: the job is stopped
        // here, now that its pid is written (theseus-36to). The stop writes
        // `cancel = requested`, then reads the pid; the launch writes the
        // pid, then reads the cancel: one of them sees the other. When both
        // do, the job's group gets a second SIGTERM, which is harmless.
        if tc
            .kernel
            .action(correlation_id)?
            .is_some_and(|a| a.cancel.is_some())
        {
            self.stop_launched(tc, correlation_id, pid).await;
        }
        let look = Look {
            spool: &spool,
            correlation_id,
            call,
            tool: tool.name(),
            t0,
            note: note.as_deref(),
        };
        loop {
            if let Some(status) = self.look_at_job(tc, &look)? {
                return Ok(CallOutcome::Done { status });
            }
            let left = bound.saturating_sub(t0.elapsed());
            if left.is_zero() {
                break;
            }
            // The drain's word, a stop's, or the backstop's look (7.1).
            waiting.woken(left.min(super::waits::LOOK)).await;
        }
        // Past its bound the job goes on in the background, and its
        // completion is the drain's to take: one the drain left to this turn
        // as the wait ended is taken here.
        drop(waiting);
        if let Some(status) = self.look_at_job(tc, &look)? {
            return Ok(CallOutcome::Done { status });
        }
        let mut text = format!(
            "Still running as background job {correlation_id} after {} seconds (timeout {} seconds). Its result will arrive in a later message; you can keep working or tell the operator you are waiting.",
            self.proc_sync_secs, spec.timeout_secs
        );
        if let Some(n) = &note {
            text.push_str(&format!("\n{n}"));
        }
        self.answer(
            tc,
            ResultNode {
                correlation_id: Some(correlation_id),
                meta: json!({"pid": pid}),
                ..ResultNode::new(&call.id, tool.name(), ResultStatus::Background, text)
            },
        )?;
        Ok(CallOutcome::Background {
            correlation_id: correlation_id.into(),
        })
    }

    pub(super) fn settle_job_failure(
        &self,
        tc: &TurnCtx<'_>,
        correlation_id: &str,
        tool: &str,
        call: &ToolUse,
        msg: &str,
    ) -> Result<CallOutcome> {
        let node = self.result_node(
            tc,
            ResultNode {
                correlation_id: Some(correlation_id),
                ..ResultNode::new(&call.id, tool, ResultStatus::Error, msg)
            },
        );
        let now = theseus_protocol::now_unix_ms();
        let c = Completion {
            correlation_id: correlation_id.into(),
            outcome: Outcome::Failed,
            result_ref: None,
            external_op_id: None,
            started_at_ms: now,
            finished_at_ms: now,
            producer: format!("harness:{tool}"),
            signature: None,
            cost_micros: None,
            detail: Some(json!({"error": msg})),
        };
        tc.kernel.accept_completion_with(&c, vec![node.record()?])?;
        Self::announce_end(tc, &node);
        Ok(CallOutcome::Done {
            status: ResultStatus::Error,
        })
    }

    /// The job's action, when a stop or a cancel has told it to stop, or it
    /// has settled, before its launch (theseus-36to).
    fn told_to_stop(kernel: &Kernel, correlation_id: &str) -> Result<Option<Action>> {
        let a = kernel
            .action(correlation_id)?
            .ok_or_else(|| anyhow!("action {correlation_id} vanished"))?;
        Ok((a.cancel.is_some() || a.state.is_settled()).then_some(a))
    }

    /// A job a stop or a cancel reached before its launch is never started
    /// (theseus-36to). Its action settles cancelled, its termination
    /// verified, since nothing ran, unless the stop settled it first, having
    /// found no pid to signal (unsupported). Its result reads as any call a
    /// stop caught before it ran ("Not run: stopped by …"), and a
    /// `job.not_started` row says why. Nothing was written to the spool.
    fn not_started(
        &self,
        tc: &TurnCtx<'_>,
        a: &Action,
        call: &ToolUse,
        tool: &str,
    ) -> Result<CallOutcome> {
        let a = if a.state.is_settled() {
            a.clone()
        } else {
            tc.kernel.cancel_verified(&a.correlation_id, None)?
        };
        tc.record(&fact::tool::JobNotStarted { action: &a, tool });
        let (status, _) = not_run_answer(&a);
        self.answer_cancelled(tc, call, &a)?;
        Ok(CallOutcome::Done { status })
    }

    /// Stop a job a stop or a cancel reached during its launch
    /// (theseus-36to), through the one stop (`terminate_all`): its wrapper
    /// is asked to stop its tree, and its cancel walks to a verdict. A
    /// cancel step on an action the stop already settled writes nothing, so
    /// the `job.stopped_at_launch` row is what says the job was stopped.
    async fn stop_launched(&self, tc: &TurnCtx<'_>, correlation_id: &str, pid: u32) {
        let ended = self
            .terminate_all(tc.kernel, tc.store, &[correlation_id.to_string()])
            .await;
        for e in ended.iter().filter(|e| e.written) {
            e.record(&tc.rec());
        }
        let gone = ended.iter().all(|e| e.verdict.verified());
        tc.record(&fact::tool::JobStoppedAtLaunch {
            correlation_id,
            pid,
            gone,
        });
    }

    /// The turn's look at its job (7.1). A completion the wrapper has spooled
    /// is taken with the result's node in one frame: the drain leaves it to
    /// the waiting turn. A job something else settled (a stop or a cancel,
    /// W1; the drain, once the wait has ended; the reconciler) is answered
    /// from its action in a frame of its own, as before. None while it runs.
    fn look_at_job(&self, tc: &TurnCtx<'_>, look: &Look<'_>) -> Result<Option<ResultStatus>> {
        self.job_waits.looked();
        let id = look.correlation_id;
        let action = || {
            tc.kernel
                .action(id)?
                .ok_or_else(|| anyhow!("action {id} vanished"))
        };
        let mut egress = false;
        if let Some(c) = look.spool.read_completion(id)? {
            let a = action()?;
            let took = match a.state {
                ActionState::Dispatched | ActionState::OutcomeUnknown => {
                    let status = match c.outcome {
                        Outcome::Succeeded => ResultStatus::Ok,
                        Outcome::Failed => ResultStatus::Error,
                        Outcome::Unknown => ResultStatus::Unknown,
                    };
                    let input = Some(&look.call.input);
                    let ended = (Some(&c), status);
                    let r =
                        self.job_result_of(tc.store, &a, ended, &look.call.id, look.tool, input);
                    let r = look.dressed(r);
                    // Its egress rows ride in the frame that writes it (18c).
                    crate::egress::record(tc, &self.sandbox, &a, &r.meta["detail"]);
                    egress = true;
                    let node = self.result_node(tc, r);
                    self.take_with_result(tc, &c, &node)?
                        .then_some((node, status))
                }
                // Settled already: taken, which writes nothing, or late after
                // a cancel, which the action records.
                _ => {
                    tc.kernel.take_completion_with(&c, vec![])?;
                    None
                }
            };
            look.spool.remove(&look.spool.completion_path(id))?;
            look.spool.remove_pid(&id.to_string());
            if let Some((node, status)) = took {
                Self::announce_end(tc, &node);
                self.remove_job_output(&a);
                return Ok(Some(status));
            }
        }
        let a = action()?;
        if !matches!(
            a.state,
            ActionState::Succeeded
                | ActionState::Failed
                | ActionState::OutcomeUnknown
                | ActionState::Cancelled
        ) {
            return Ok(None);
        }
        let r = look.dressed(self.job_result(
            tc.store,
            &a,
            &look.call.id,
            look.tool,
            Some(&look.call.input),
        ));
        // Unless a take that lost recorded them already.
        if !egress {
            crate::egress::record(tc, &self.sandbox, &a, &r.meta["detail"]);
        }
        let status = self.answer(tc, r)?;
        self.remove_job_output(&a);
        Ok(Some(status))
    }

    /// A job's completion and its result's node, taken in one frame (7.1),
    /// with the session's hold when the result brings one, as `complete`
    /// writes an in-process call's. Whether this took it: not when the drain
    /// or the reconciler had, or a cancel had settled the call.
    fn take_with_result(&self, tc: &TurnCtx<'_>, c: &Completion, node: &Node) -> Result<bool> {
        let sid = tc.session_id;
        let (accepted, newly) =
            crate::external::with_hold(tc.store, sid, Some(tc.turn_id), node, |frame| {
                tc.kernel.take_completion_with(c, frame)
            })?;
        let took = matches!(
            accepted,
            Accepted::Settled { .. } | Accepted::ResolvedUnknown { .. }
        );
        if took {
            self.held(tc, newly);
        }
        Ok(took)
    }

    /// Has the job settled? Takes a spooled completion if it is there (with
    /// the harness's own drain, whichever is first) and returns the settled
    /// action.
    pub(super) fn job_settled(
        kernel: &Kernel,
        spool: &Spool,
        correlation_id: &str,
    ) -> Result<Option<Action>> {
        if let Some(c) = spool.read_completion(correlation_id)? {
            kernel.take_completion_with(&c, vec![])?;
            spool.remove(&spool.completion_path(correlation_id))?;
            spool.remove_pid(&correlation_id.to_string());
        }
        let a = kernel
            .action(correlation_id)?
            .ok_or_else(|| anyhow!("action {correlation_id} vanished"))?;
        // A job a stop or a cancel killed is settled too: the turn waiting on
        // it hears so at once, not at its bound (W1).
        Ok(match a.state {
            ActionState::Succeeded
            | ActionState::Failed
            | ActionState::OutcomeUnknown
            | ActionState::Cancelled => Some(a),
            _ => None,
        })
    }

    /// A settled job's result: how it ended (its exit code, a timeout, or an
    /// unknown outcome), then its output, the tail of it when it is long.
    /// Answer it with `answer_job`, which then deletes the raw output.
    /// `input` is the call's, by which a job of a listed program is marked
    /// (theseus-b5cl).
    pub(super) fn job_result<'a>(
        &self,
        store: &Store,
        a: &'a Action,
        tool_use_id: &'a str,
        tool: &'a str,
        input: Option<&Value>,
    ) -> ResultNode<'a> {
        // A hands group's aggregate (AWS design §3.3): no raw output.
        if a.tool == crate::aws::hands::RUN {
            return Self::hands_result(store, a, tool_use_id, tool);
        }
        let completion: Option<Completion> = store
            .inner()
            .as_ref()
            .latest_by_key(theseus_store::kinds::COMPLETION, &a.correlation_id)
            .ok()
            .flatten()
            .and_then(|r| r.decode().ok());
        let status = match a.state {
            ActionState::Succeeded => ResultStatus::Ok,
            ActionState::Failed => ResultStatus::Error,
            ActionState::Cancelled => ResultStatus::Cancelled,
            _ => ResultStatus::Unknown,
        };
        let ended = (completion.as_ref(), status);
        self.job_result_of(store, a, ended, tool_use_id, tool, input)
    }

    /// `job_result` from how the job `ended`: its completion and the status
    /// it settles the call as. The turn's take has them before the action
    /// does, and writes the two in one frame (7.1).
    #[expect(clippy::too_many_lines, reason = "shape budget: split it")]
    fn job_result_of<'a>(
        &self,
        store: &Store,
        a: &'a Action,
        (completion, status): (Option<&Completion>, ResultStatus),
        tool_use_id: &'a str,
        tool: &'a str,
        input: Option<&Value>,
    ) -> ResultNode<'a> {
        let detail = completion
            .and_then(|c| c.detail.clone())
            .unwrap_or(Value::Null);
        // An L1 job's launch, as health's last one (theseus-gyin).
        if let Some(c) = completion {
            self.sandbox.launched(&detail, c.started_at_ms);
        }
        let Tail {
            text: out,
            total,
            unread,
        } = read_result_file(self.raw_output(a).as_deref());
        let exit = detail.get("exit_code").and_then(Value::as_i64);
        let header = match (
            status,
            exit,
            detail.get("timed_out").and_then(Value::as_bool),
        ) {
            (_, _, Some(true)) => "[timed out and killed]\n".to_string(),
            // A `/stop` killed it (W1), or a cancel did.
            (ResultStatus::Cancelled, _, _) => format!(
                "[cancelled: {}{}]\n",
                a.resolution
                    .as_deref()
                    .unwrap_or("its execution was cancelled"),
                crate::cancel::words(a).map_or(String::new(), |w| format!("; {w}"))
            ),
            (ResultStatus::Unknown, _, _) => {
                "[outcome unknown: the harness could not establish whether this finished]\n"
                    .to_string()
            }
            (_, Some(c), _) => format!("[exit code {c}]\n"),
            _ => String::new(),
        };
        // A job that printed past the cap kept only the cap's worth: the
        // result says how much it printed, how much was dropped, and how to
        // get the rest (theseus-102), as the cap's line does (theseus-46v).
        // Since theseus-gsn9 the cap keeps the output's two ends, and the
        // bytes dropped are those between them; a wrapper from before keeps
        // the head alone.
        let dropped = detail.get("dropped").and_then(Value::as_u64).unwrap_or(0);
        let ends = match (
            detail.get("head").and_then(Value::as_u64),
            detail.get("tail").and_then(Value::as_u64),
        ) {
            (Some(h), Some(t)) => Some((h, t)),
            _ => None,
        };
        let bytes = |n: u64| narrative::count(n, "byte", "bytes");
        let header = match dropped {
            0 => header,
            d => {
                let cap = detail
                    .get("output_max_bytes")
                    .and_then(Value::as_u64)
                    .unwrap_or(total);
                let rest = self
                    .registry
                    .get(tool)
                    .map(|t| t.rest(""))
                    .filter(|r| !r.is_empty())
                    .map(|r| format!("; {r}"))
                    .unwrap_or_default();
                match ends {
                    Some((h, t)) => format!(
                        "{header}[truncated: it printed {}, more than its output cap of {}: its \
                         first {} and its last {} are kept, and the {} between them were \
                         dropped{rest}]\n",
                        bytes(h + d + t),
                        size(cap),
                        bytes(h),
                        bytes(t),
                        bytes(d),
                    ),
                    None => format!(
                        "{header}[truncated: it printed {}, and the {} past its output cap of {} \
                         were dropped{rest}]\n",
                        bytes(total + d),
                        bytes(d),
                        size(cap),
                    ),
                }
            }
        };
        // The end the copy held when the wrapper reported, which the file
        // takes only at the pipe's end (theseus-gsn9).
        let header = match detail.get("held").and_then(Value::as_u64) {
            Some(n) if n > 0 => format!(
                "{header}[when it reported, a process it started still held its output open, and \
                 its last {} were not yet written]\n",
                bytes(n)
            ),
            _ => header,
        };
        let header = format!("{header}{}", cap_line(&detail).unwrap_or_default());
        // No report, and the file stopped where the head does: the output
        // went past the head, and its end waited in the job's wrapper, which
        // was killed before the pipe's end (theseus-gsn9).
        let (head, tail) = theseus_kernel::redact::split(self.output_max_bytes);
        let header = if completion.is_none() && tail > 0 && total == head {
            format!(
                "{header}[its output reached the first {}, all the file takes before the end, and \
                 its end was lost: the job's wrapper was killed before it could write it]\n",
                bytes(head)
            )
        } else {
            header
        };
        // Only the end of a very long output is read: the cut says so, as the
        // cap's does for what it leaves out (theseus-46v).
        let header = match (unread, dropped, ends) {
            (0, _, _) => header,
            (n, 0, _) | (n, _, Some(_)) => format!(
                "{header}[the first {} of {} not read; its last {} follow]\n",
                bytes(n),
                if dropped == 0 {
                    "its output"
                } else {
                    "what was kept"
                },
                size(total - n)
            ),
            (n, _, None) => format!(
                "{header}[the first {} of what was kept not read; the {} before the cap follow]\n",
                bytes(n),
                size(total - n)
            ),
        };
        // A job that reached a host beyond `[sandbox] egress` brought back
        // outside text (18c; theseus-gyin); so did a job of a program
        // `[policy] external_programs` lists, unless its egress marked it
        // already (theseus-b5cl).
        let egress =
            crate::egress::marker(&detail, !out.is_empty(), &self.sandbox.cfg.egress, || {
                confirm_proposal(store, a, None).map_or_else(|_| vec![], |p| egress::bound(&p))
            });
        let listed = match (&egress, input) {
            (None, Some(i)) => crate::external::Listed::of(i, &self.external_programs),
            _ => None,
        };
        let header = format!(
            "{}{}{header}",
            sandbox::result_lines(&detail),
            listed
                .as_ref()
                .map_or_else(String::new, crate::external::Listed::line)
        );
        let raw = if out.is_empty() {
            format!("{header}(no output)")
        } else {
            format!("{header}{out}")
        };
        let duration_ms = detail.get("duration_ms").and_then(Value::as_u64);
        let mut meta = json!({"exit_code": exit, "detail": detail});
        if let Some(l) = &listed {
            meta[crate::external::PROGRAM_KEY] = json!(l.program);
        }
        crate::cancel::stopped_meta(&mut meta, status, a);
        ResultNode {
            correlation_id: Some(&a.correlation_id),
            duration_ms,
            bytes_total: Some(total),
            external: egress.or_else(|| listed.as_ref().map(crate::external::Listed::marker)),
            meta,
            ..ResultNode::new(tool_use_id, tool, status, raw)
        }
    }

    /// A job's result on its own frame, announced; then the job's raw output
    /// goes (theseus-wz2). The file held what the job printed before the
    /// scrubber saw it, a printed secret too; the node holds the scrubbed,
    /// capped text, and nothing reads the file again. A restart between the
    /// two leaves the file, 0600 in the private spool.
    pub(super) fn answer_job(
        &self,
        tc: &TurnCtx<'_>,
        r: ResultNode<'_>,
        a: &Action,
    ) -> Result<ResultStatus> {
        // Its egress rows ride in the frame that writes it (18c).
        crate::egress::record(tc, &self.sandbox, a, &r.meta["detail"]);
        let status = self.answer(tc, r)?;
        self.remove_job_output(a);
        Ok(status)
    }

    /// Where a job's raw output is: the file its completion names, or, for a
    /// job killed before its completion (a stop, a cancel), the spool's file
    /// for its id, when there is one (theseus-ewev). A stopped job's result
    /// reads it too, so it shows what the job printed before the stop.
    fn raw_output(&self, a: &Action) -> Option<String> {
        a.result_ref.clone().or_else(|| {
            let path = self.spool.as_ref()?.result_path(&a.correlation_id);
            path.exists().then(|| path.to_string_lossy().into_owned())
        })
    }

    /// Delete a job's raw output once its result's node is written
    /// (theseus-wz2). A job killed before its completion has no `result_ref`,
    /// so its file waited for the spool's sweep after its cancelled result was
    /// written; now it goes then too (theseus-ewev). Neither goes while the
    /// job's wrapper still lives, as the sweep checks: a job can report while a
    /// process it started holds its output open, and its wrapper then still
    /// writes the file (its end waits in the ring), so the file stays, and
    /// the sweep takes it once the wrapper has exited (theseus-5wgd).
    pub(crate) fn remove_job_output(&self, a: &Action) {
        let Some(spool) = &self.spool else {
            return;
        };
        let corr = &a.correlation_id;
        if spool.wrapper_lives(corr) {
            return;
        }
        match a.result_ref.as_deref() {
            Some(path) => self.remove_raw_output(path),
            None => self.remove_raw_output(&spool.result_path(corr).to_string_lossy()),
        }
    }

    /// Delete a job's raw output, once its result's node is written
    /// (theseus-wz2). A failure is a warning: the file stays 0600 in the
    /// private spool.
    fn remove_raw_output(&self, path: &str) {
        if let Some(spool) = &self.spool {
            if let Err(e) = spool.remove_result(std::path::Path::new(path)) {
                tracing::warn!(path, error = %format!("{e:#}"), "a job's raw output was not removed");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A reader that counts the bytes read from it.
    struct Counted<R> {
        inner: R,
        read: u64,
    }

    impl<R: Read> Read for Counted<R> {
        fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
            let n = self.inner.read(buf)?;
            self.read += n as u64;
            Ok(n)
        }
    }

    impl<R: Seek> Seek for Counted<R> {
        fn seek(&mut self, pos: SeekFrom) -> std::io::Result<u64> {
            self.inner.seek(pos)
        }
    }

    /// Review 2's R3 (theseus-102): a job's output is read by seek, so the
    /// daemon reads its last 4 MiB and no more, whatever the job printed.
    /// Here 40 MiB, of which 4 MiB are read; and a sparse 8 GiB file, which a
    /// whole read could not hold, gives its tail (no disk is filled: the file
    /// is a hole and one line).
    /// A job its cap refused processes says how many and why it may have
    /// failed (theseus-a5nv); one it refused none says nothing.
    #[test]
    fn a_job_its_cap_refused_says_so() {
        assert_eq!(cap_line(&json!({"cpu_us": 5})), None);
        assert_eq!(cap_line(&json!({"pids_refused": 0, "pids_max": 10})), None);
        assert_eq!(
            cap_line(&json!({"pids_refused": 3, "pids_max": 10})).as_deref(),
            Some(
                "[its cap of 10 processes and threads ([tools] job_pids_max) refused it 3 new ones: \
                 what it did may have failed for that]\n"
            )
        );
    }

    #[test]
    fn a_jobs_output_is_read_by_seek_and_only_its_tail() {
        let big = 40 * 1024 * 1024;
        let mut body = vec![b'.'; big];
        body[..5].copy_from_slice(b"first");
        body[big - 5..].copy_from_slice(b"final");
        let mut r = Counted {
            inner: std::io::Cursor::new(body),
            read: 0,
        };
        let t = read_tail(&mut r, MAX_RESULT_READ as u64).unwrap();
        assert_eq!(r.read, MAX_RESULT_READ as u64, "only the tail is read");
        assert_eq!(t.total, big as u64);
        assert_eq!(t.unread, (big - MAX_RESULT_READ) as u64);
        assert!(t.text.ends_with("final") && !t.text.contains("first"));

        let d = tempfile::tempdir().unwrap();
        let path = d.path().join("act_huge.out");
        let f = std::fs::File::create(&path).unwrap();
        let huge = 8u64 << 30;
        f.set_len(huge - 12).unwrap();
        drop(f);
        let mut f = std::fs::OpenOptions::new()
            .append(true)
            .open(&path)
            .unwrap();
        std::io::Write::write_all(&mut f, b"the last one").unwrap();
        drop(f);
        let t = read_result_file(path.to_str());
        assert_eq!(t.total, huge);
        assert_eq!(t.unread, huge - MAX_RESULT_READ as u64);
        assert_eq!(t.text.len(), MAX_RESULT_READ);
        assert!(t.text.ends_with("the last one"));
        assert_eq!(read_result_file(None), Tail::default());
        assert_eq!(
            read_result_file(Some("/no/such/invented.out")),
            Tail::default()
        );
    }

    /// A job whose completion the daemon's drain took, accepted and then
    /// removed, before the turn's look reads as settled from the kernel
    /// (theseus-46ya): the read finds no file, and the action the drain
    /// settled answers the call. Before the drain, it is still running.
    #[test]
    fn a_job_the_drain_settled_reads_as_settled() {
        use std::collections::BTreeMap;
        use theseus_kernel::types::{new_id, Authority, RetryClass, SessionKind};
        use theseus_kernel::{NoEvidence, Proposal};
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(&dir.path().join("store")).unwrap();
        let kernel = Kernel::new(
            store.shared(),
            std::sync::Arc::new(theseus_kernel::RealClock),
            Default::default(),
        );
        kernel.startup(None, &NoEvidence).unwrap();
        let spool = Spool::open(&dir.path().join("spool")).unwrap();
        let exec = kernel
            .open_execution(
                &new_id("ses"),
                SessionKind::Conversation,
                Authority {
                    principal: "rig".into(),
                    delegated_by: None,
                    ceilings: BTreeMap::from([("tools".into(), "proc".into())]),
                },
                Some(1_000_000_000),
                None,
            )
            .unwrap()
            .id;
        kernel.wake_input(&exec).unwrap();
        let g = kernel.admit(&exec).unwrap();
        let p = Proposal {
            tool: "proc.run".into(),
            args: json!({"argv": ["true"]}),
            resource: None,
            policy_context: json!({}),
        };
        let a = kernel
            .plan_and_dispatch(&g, &p, RetryClass::SafeToRepeat, Some(60_000), 0, |_| {
                Ok(vec![])
            })
            .unwrap();
        let corr = a.correlation_id;
        assert!(ToolRuntime::job_settled(&kernel, &spool, &corr)
            .unwrap()
            .is_none());
        let path = spool
            .write(&Completion {
                correlation_id: corr.clone(),
                outcome: Outcome::Succeeded,
                result_ref: None,
                external_op_id: None,
                started_at_ms: 1,
                finished_at_ms: 2,
                producer: "rig".into(),
                signature: None,
                cost_micros: None,
                detail: None,
            })
            .unwrap();
        // The drain, as `Driver::drain_spool` does it: accept, then remove.
        kernel
            .accept_completion(&spool.drain().unwrap().completions[0].1)
            .unwrap();
        spool.remove(&path).unwrap();
        let done = ToolRuntime::job_settled(&kernel, &spool, &corr)
            .unwrap()
            .expect("the drain settled it");
        assert_eq!(done.state, ActionState::Succeeded);
    }

    /// A cut through a UTF-8 character moves to the character's end, and the
    /// bytes it skips count as not read.
    #[test]
    fn a_tail_cut_through_a_character_starts_at_the_next_one() {
        // "é" is two bytes: a cut 3 bytes from the end lands inside it.
        let mut r = std::io::Cursor::new("aé\nbc".as_bytes().to_vec());
        let t = read_tail(&mut r, 4).unwrap();
        assert_eq!(t.text, "\nbc");
        assert_eq!((t.total, t.unread), (6, 3));
    }
}
