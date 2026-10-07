//! A batch's run (`proc.run`'s `steps`, theseus-7gir.3): each step a job
//! through today's path (`launch`), started when the one before exits 0,
//! all under the call's one action and correlation id. So a `/stop` or a
//! cancel reaches the running step by that id (its pid is in the spool under
//! it), and the next step is never started once one has come (`launch`
//! checks the action first). The turn waits on the id from before the first
//! launch to the batch's end, so the drain leaves every step's completion
//! to it: a step that exits 0 before the last is taken here, never by the
//! kernel, and the action stays dispatched between two steps. The step that
//! ends the batch (the last, one that fails, or one a stop killed) settles
//! the call as one job does, its result headed by the steps before it and
//! followed by the steps not run.
//!
//! The wait is the batch's: `proc_sync_secs` from the first launch. A step
//! still running at its end goes on in the background as one job does, and
//! its completion settles the call (the drain's, or a restart's reconcile
//! from its wrapper's evidence): the steps after it are not run, and the
//! answer says so.
//!
//! The result's room: each step before the one that ends the batch keeps
//! the end of its output, at most a quarter of the result's room shared
//! between them; the step that ends it, the failing one, gets the rest, and
//! the whole is capped as one job's result is.

use std::time::{Duration, Instant};

use anyhow::{anyhow, Result};
use serde_json::{json, Value};
use theseus_kernel::{ActionState, Completion, Outcome, Spool};
use theseus_tools::JobSpec;

use super::job::{read_result_file, Job, Launched, Look};
use super::{CallOutcome, ResultNode, ToolRuntime, TurnCtx};
use crate::node::ResultStatus;

/// How long a step's wrapper may take, past its report, to remove its pid
/// file before the next step starts anyway.
const WRAPPER_EXIT: Duration = Duration::from_secs(2);

/// How a look at a running step found it.
enum Step {
    /// It exited 0 before the last: its completion, taken off the spool.
    Passed(Completion),
    /// It ended the batch, which is answered.
    Ended(ResultStatus),
}

/// `step 2 of 3 (`make test` in /w)`.
fn named(i: usize, n: usize, spec: &JobSpec) -> String {
    format!(
        "step {} of {n}: `{}` in {}",
        i + 1,
        spec.argv.join(" "),
        spec.cwd.display()
    )
}

/// The steps after `from`, each named as not run.
fn not_run(specs: &[JobSpec], from: usize) -> String {
    let n = specs.len();
    specs
        .iter()
        .enumerate()
        .skip(from)
        .map(|(i, s)| format!("\n[{}: not run]", named(i, n, s)))
        .collect()
}

/// Every step's row for the result's meta: the ones that passed with their
/// exits and times, the one running, and the rest not run.
fn rows(specs: &[JobSpec], passed: &[Value], at: usize) -> Value {
    let mut out = passed.to_vec();
    out.push(json!({"step": at + 1, "argv": specs[at].argv, "cwd": specs[at].cwd, "ran": true}));
    for (i, s) in specs.iter().enumerate().skip(at + 1) {
        out.push(json!({"step": i + 1, "argv": s.argv, "cwd": s.cwd, "ran": false}));
    }
    Value::Array(out)
}

/// The last `room` characters of `out`, saying what it left out.
fn tail_of(out: &str, room: usize) -> String {
    let n = out.chars().count();
    if n <= room {
        return out.to_string();
    }
    let cut = out
        .char_indices()
        .nth(n - room)
        .map_or(out.len(), |(i, _)| i);
    format!(
        "[its first {} characters left out; its last {room} follow]\n{}",
        n - room,
        &out[cut..]
    )
}

/// Whether a step's wrapper `pid` is done with job `id`'s pid file, so the
/// next step's launch may write its own: it removed the file just after its
/// report, or it is gone. The file itself says so: a wrapper that will
/// linger writes its marker before its report, with its pid file still
/// there (theseus-v18k), so the marker says nothing of the file.
fn unlisted(spool: &Spool, id: &str, pid: u32) -> bool {
    !theseus_kernel::job::wrapper_alive(pid, id) || spool.read_pid(id) != Some(pid)
}

/// It exited 0, and was not killed by its timeout.
fn passes(c: &Completion) -> bool {
    let d = c.detail.as_ref();
    c.outcome == Outcome::Succeeded
        && d.and_then(|d| d.get("exit_code")).and_then(Value::as_i64) == Some(0)
        && d.and_then(|d| d.get("timed_out")).and_then(Value::as_bool) != Some(true)
}

impl ToolRuntime {
    /// Run a batch's jobs in turn, stopping at the first that does not exit
    /// 0, and answer the call once.
    pub(super) async fn run_steps(
        &self,
        tc: &TurnCtx<'_>,
        job: &Job<'_>,
        specs: &[JobSpec],
    ) -> Result<CallOutcome> {
        let n = specs.len();
        let id = job.correlation_id;
        let total: u64 = specs.iter().map(|s| s.timeout_secs).sum();
        let bound = Duration::from_secs(self.proc_sync_secs.min(total + 5));
        let room = self.result_max_chars / 4 / n.saturating_sub(1).max(1);
        let t0 = Instant::now();
        // The turn waits on the id from before the first launch to the end,
        // so the drain leaves every step's completion to it (7.1).
        let waiting = self.job_waits.wait(id);
        let mut before = String::new();
        let mut passed: Vec<Value> = Vec::new();
        for (i, spec) in specs.iter().enumerate() {
            let launched = match self.launch(tc, job, spec, &waiting, bound).await? {
                Ok(l) => l,
                Err(answered) => return Ok(answered),
            };
            let head = format!("{before}[{}]\n", named(i, n, spec));
            let after = not_run(specs, i + 1);
            let steps = rows(specs, &passed, i);
            let look = Look {
                spool: job.spool,
                correlation_id: id,
                call: job.call,
                tool: job.tool.name(),
                t0,
                note: launched.note.as_deref(),
                before: &head,
                after: &after,
                steps: Some(&steps),
            };
            let last = i + 1 == n;
            let c = loop {
                match self.look_at_step(tc, &look, last)? {
                    Some(Step::Passed(c)) => break c,
                    Some(Step::Ended(status)) => return Ok(CallOutcome::Done { status }),
                    None => {}
                }
                let left = bound.saturating_sub(t0.elapsed());
                if left.is_zero() {
                    // Past the batch's wait the step goes on in the
                    // background, and its completion is the drain's: one
                    // the drain left to this turn as the wait ended is
                    // taken here, and settles the call.
                    drop(waiting);
                    if let Some(status) = self.look_at_job(tc, &look)? {
                        return Ok(CallOutcome::Done { status });
                    }
                    return self.steps_background(tc, job, &look, &launched, spec);
                }
                waiting.woken(left.min(super::waits::LOOK)).await;
            };
            let ms = c
                .detail
                .as_ref()
                .and_then(|d| d.get("duration_ms"))
                .and_then(Value::as_u64)
                .unwrap_or(c.finished_at_ms.saturating_sub(c.started_at_ms));
            let out = self
                .scrubber
                .scrub(&read_result_file(c.result_ref.as_deref()).text)
                .0;
            let out = match out.is_empty() {
                true => "(no output)".to_string(),
                false => tail_of(&out, room),
            };
            before = format!("{head}[exit code 0, {ms} ms]\n{}\n", out.trim_end());
            passed.push(json!({"step": i + 1, "argv": spec.argv, "cwd": spec.cwd, "ran": true, "exit_code": 0, "duration_ms": ms}));
            // The step's wrapper removes the pid file just after its report:
            // the next step's launch waits for that, or it would lose its own
            // (`unlisted`; the look leaves the file to the wrapper).
            let pid = launched.pid;
            let gone = || unlisted(job.spool, id, pid);
            let t = Instant::now();
            while !gone() && t.elapsed() < WRAPPER_EXIT {
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
            // Unlinked, never truncated: a process the step left holding
            // its output writes on to its own file, and the next step's
            // wrapper makes a new one.
            if let Some(p) = &c.result_ref {
                self.remove_raw_output(p);
            }
        }
        Err(anyhow!("a batch's last step ended it without an answer"))
    }

    /// A look at a step that is not the last (7.1): one that exited 0 is
    /// taken off the spool and the batch goes on; any other end is the
    /// batch's, answered by the turn's look at its job, which settles the
    /// call. The last step is looked at as one job is.
    fn look_at_step(&self, tc: &TurnCtx<'_>, look: &Look<'_>, last: bool) -> Result<Option<Step>> {
        if !last {
            let id = look.correlation_id;
            let a = tc
                .kernel
                .action(id)?
                .ok_or_else(|| anyhow!("action {id} vanished"))?;
            let going = a.state == ActionState::Dispatched && a.cancel.is_none();
            match look.spool.read_completion(id)? {
                // Its pid file is its wrapper's to remove: the next step's
                // launch waits for that (`run_steps`).
                Some(c) if going && passes(&c) => {
                    look.spool.remove(&look.spool.completion_path(id))?;
                    return Ok(Some(Step::Passed(c)));
                }
                // It failed: the look below settles the call with it.
                Some(_) => {}
                // Still running, unless something else settled the call.
                None if !a.state.is_settled() => return Ok(None),
                None => {}
            }
        }
        Ok(self.look_at_job(tc, look)?.map(Step::Ended))
    }

    /// The batch's answer when a step runs on past the wait: the steps done,
    /// the running step's background id, and the rest as not run.
    fn steps_background(
        &self,
        tc: &TurnCtx<'_>,
        job: &Job<'_>,
        look: &Look<'_>,
        launched: &Launched<'_>,
        spec: &JobSpec,
    ) -> Result<CallOutcome> {
        let id = job.correlation_id;
        let mut text = format!(
            "{}Still running as background job {id} after {} seconds (timeout {} seconds). Its result will arrive in a later message; you can keep working or tell the operator you are waiting. A batch stops at a step that goes on in the background: the steps after it will not run.{}",
            look.before, self.proc_sync_secs, spec.timeout_secs, look.after
        );
        if let Some(n) = &launched.note {
            text.push_str(&format!("\n{n}"));
        }
        self.answer(
            tc,
            ResultNode {
                correlation_id: Some(id),
                meta: json!({"pid": launched.pid, "steps": look.steps}),
                ..ResultNode::new(
                    &job.call.id,
                    job.tool.name(),
                    ResultStatus::Background,
                    text,
                )
            },
        )?;
        Ok(CallOutcome::Background {
            correlation_id: id.into(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// theseus-v18k: a step's wrapper that will linger writes its marker
    /// before its report, then removes its pid file. The next step's launch
    /// waits for that removal, or the old wrapper's would take the new pid
    /// file: so a marker that names the wrapper is no sign the pid file has
    /// gone. It was, while the marker came after the removal. A wrapper
    /// killed before its removal is done with the file too. The wrapper is
    /// a stand-in whose command line names the job.
    #[test]
    fn a_steps_wrapper_is_done_with_its_pid_file_once_it_removes_it_or_is_gone() {
        let d = tempfile::tempdir().unwrap();
        let spool = Spool::open(d.path()).unwrap();
        let id = "act_steps";
        let wrapper = crate::peer::Standin::start(id);
        let pid = wrapper.wrapper;
        spool.write_pid(id, pid).unwrap();
        assert!(!unlisted(&spool, id, pid), "its command runs");
        spool.write_lingering(id, pid).unwrap();
        assert!(
            !unlisted(&spool, id, pid),
            "marked before its report, its pid file still there"
        );
        spool.remove_pid(&id.to_string());
        assert!(unlisted(&spool, id, pid), "it removed its pid file");
        spool.write_pid(id, pid).unwrap();
        drop(wrapper);
        let t0 = Instant::now();
        while theseus_kernel::job::wrapper_alive(pid, id) {
            assert!(
                t0.elapsed() < Duration::from_secs(10),
                "the stand-in lives on"
            );
            std::thread::sleep(Duration::from_millis(5));
        }
        assert!(unlisted(&spool, id, pid), "killed before its removal");
    }
}
