//! A job's handle (theseus-n8gk): `job.read`, `job.wait`, and `job.stop`
//! run here, over the jobs `proc.run` left in the background, each named by
//! its short id (`handles.rs`) or its long one, and only in the session that
//! started it. Their schemas are theseus-tools' (`jobs.rs`).
//!
//! - `job.read`: the job's latest lines, through the one tail reader
//!   (`peek.rs`), or with no job the session's running jobs.
//! - `job.wait`: a wait on the job's end as an event (the drain's settle, a
//!   stop's, the reconciler's, `JobWaits::settled`), as long as the call
//!   asked and at most `[tools] job_wait_max_secs` (theseus-7m7w.3), ended
//!   at once by the job's end, a person's message (`Steer`) and a stop; the
//!   job's result as a blocking `proc.run` gives it, or that it still runs.
//!   A wait past the cache's lifetime keeps the turn's cache warm
//!   (`keep_warm_wait`).
//! - `job.stop`: the job's stop as `/stop` stops one (`Kernel::stop_call`,
//!   then `terminate_all`: its tree, verified by its cgroup where it has
//!   one), and its output so far. The gate judges it as a run of the job's
//!   own program (`job_planned`).
//!
//! A result that gives the job's end names the job it delivers (`DELIVERS`):
//! the late result its end still writes then says only that, since the
//! model has it already (`late.rs`).

use std::sync::{Arc, OnceLock, Weak};
use std::time::Duration;

use serde_json::{json, Value};
use theseus_kernel::{Action, ActionState};
use theseus_tools::jobs::{self as names, ReadArgs, StopArgs, WaitArgs};
use theseus_tools::{
    parse, Access, AsyncMediaResult, AsyncMediaRun, Backend, External, Plan, Resource, ToolFailure,
    ToolOutput,
};
use tokio::time::Instant;

mod wait_facts;
mod wait_status;

use super::handles::{Handles, KEY};
use super::{ToolRuntime, TurnCtx};
use crate::keep_warm::Outcome;
use crate::narrative;
use crate::Core;

/// The meta key of a result that gives a job's end: the job's correlation id.
pub const DELIVERS: &str = "delivers";

/// The meta key of a wait that returned because a person's message arrived
/// (theseus-qu1lc); its value is `"message"`.
pub const INTERRUPTED_BY: &str = "interrupted_by";

/// What the job tools read: the core, once built, and the sessions' short
/// ids.
#[derive(Default)]
pub struct Board {
    core: OnceLock<Weak<Core>>,
    pub handles: Handles,
}

impl Board {
    pub fn attach(&self, core: &Arc<Core>) {
        let _ = self.core.set(Arc::downgrade(core));
    }

    /// The core, once built and while it lives.
    pub fn core(&self) -> Option<Arc<Core>> {
        self.core.get().and_then(Weak::upgrade)
    }
}

/// A job a call names, found in the calling session.
#[derive(Clone)]
struct Found {
    corr: String,
    /// Its short id, or its long one when it has none.
    name: String,
    argv: Vec<String>,
    action: Action,
}

impl Found {
    /// `j7 (cargo test --workspace)`.
    fn called(&self) -> String {
        format!("{} ({})", self.name, label(&self.argv))
    }
}

/// A command as a job's line names it: its argv, cut at 60 characters.
pub(super) fn label(argv: &[String]) -> String {
    let all = argv.join(" ");
    if all.chars().count() <= 60 {
        return all;
    }
    let cut: String = all.chars().take(59).collect();
    format!("{cut}…")
}

/// Has the job ended, however it did?
fn ended(a: &Action) -> bool {
    matches!(
        a.state,
        ActionState::Succeeded
            | ActionState::Failed
            | ActionState::OutcomeUnknown
            | ActionState::Cancelled
    )
}

/// How an ended job ended, in a few words.
fn how(a: &Action) -> &'static str {
    match a.state {
        ActionState::Succeeded => "succeeded",
        ActionState::Failed => "failed",
        ActionState::Cancelled => "was stopped",
        _ => "ended, its outcome unknown",
    }
}

/// What `proc.run {background: true}` answers at once: the job's handle,
/// where its output goes, and what the job tools do with it.
pub(super) fn started(
    name: &str,
    spec: &theseus_tools::JobSpec,
    output: &std::path::Path,
) -> String {
    let at = crate::wake::local(theseus_protocol::now_unix_ms()).hms();
    format!(
        "Started job {name} ({}) in {} at {at}; its output goes to {}.\n\
         job_read {name} shows its latest output, job_wait {name} waits for it, job_stop {name} \
         stops it. When it ends, you get a notice between your calls.",
        label(&spec.argv),
        spec.cwd.display(),
        output.display()
    )
}

/// A job's argv, from its call's proposal: for a job named by its long id,
/// or one whose short id a restart read again.
pub(super) fn argv_of(store: &crate::store::Store, a: &Action) -> Vec<String> {
    super::confirm_proposal(store, a, None)
        .ok()
        .and_then(|p| serde_json::from_value(p.args["argv"].clone()).ok())
        .unwrap_or_default()
}

fn is_short(job: &str) -> bool {
    job.strip_prefix('j')
        .is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()))
}

impl ToolRuntime {
    /// The job `job` names in `session`, or why there is none: a short id
    /// this session never gave, a long one that is not a job, or another
    /// session's job, refused by name.
    fn find_job(
        &self,
        kernel: &theseus_kernel::Kernel,
        store: &crate::store::Store,
        session: &str,
        job: &str,
    ) -> Result<Found, String> {
        let job = job.trim();
        let handles = &self.jobs.handles;
        let (corr, mut argv) = match handles.resolve(store, session, job) {
            Some(e) => (e.corr, e.argv),
            None if is_short(job) => {
                return Err(format!(
                    "this session has no job {job}: proc_run names a job when it goes on in the \
                     background"
                ))
            }
            None => (job.to_string(), Vec::new()),
        };
        let a = kernel
            .action(&corr)
            .map_err(|e| format!("{e:#}"))?
            .ok_or_else(|| format!("there is no job {job}"))?;
        if a.session_id != session {
            return Err(format!(
                "job {job} is another session's: only the session that started a job reads, \
                 waits for, or stops it"
            ));
        }
        if self.registry.get(&a.tool).map(|t| t.backend()) != Some(Backend::Job) {
            return Err(format!("{job} is not a job: it is a {} call", a.tool));
        }
        let name = match handles.name_of(store, session, &corr) {
            Some((n, e)) => {
                if argv.is_empty() {
                    argv = e.argv;
                }
                n
            }
            None => corr.clone(),
        };
        if argv.is_empty() {
            argv = argv_of(store, &a);
        }
        Ok(Found {
            corr,
            name,
            argv,
            action: a,
        })
    }

    /// The gate's look at a job tool's call (theseus-n8gk): the job it names
    /// must be the calling session's, and `job.stop` is judged as a run of
    /// the job's own program, its argv and directory, as `term.send` is its
    /// terminal's.
    pub(super) fn job_planned(
        &self,
        tc: &TurnCtx<'_>,
        tool: &str,
        input: &Value,
        mut plan: Plan,
    ) -> Result<Plan, String> {
        if !matches!(tool, names::READ | names::WAIT | names::STOP) {
            return Ok(plan);
        }
        let Some(job) = input.get("job").and_then(Value::as_str) else {
            self.own_job_outside_roots(&mut plan);
            return Ok(plan);
        };
        let f = self.find_job(tc.kernel, tc.store, tc.session_id, job)?;
        if tool == names::STOP {
            let cwd = f
                .action
                .resource
                .clone()
                .unwrap_or_else(|| self.ctx.cwd.display().to_string());
            plan.summary = format!("stop job {} (`{}`) in {cwd}", f.name, f.argv.join(" "));
            plan.resources = vec![Resource {
                path: cwd.into(),
                access: Access::Exec,
            }];
            plan.argv = (!f.argv.is_empty()).then_some(f.argv);
        }
        self.own_job_outside_roots(&mut plan);
        Ok(plan)
    }

    /// A session's own job is its own wherever its directory is (theseus-zntlw):
    /// the directory a job tool's plan names is no file the call reads and no
    /// program it starts, so one outside the workspace roots is left off the
    /// plan, and the call carries no outside-your-project-folder label for it,
    /// as a read or run of that directory would. The job itself was gated when `proc.run` started it. The
    /// conversation's directory is not made a root for it: that is the owner's
    /// to decide.
    fn own_job_outside_roots(&self, plan: &mut Plan) {
        plan.resources.retain(|r| {
            let at = theseus_tools::paths::canonical_best_effort(&r.path);
            self.ctx
                .roots
                .iter()
                .any(|root| theseus_tools::paths::within(&at, root))
        });
    }

    /// A job tool's run, in its call's own task.
    pub(super) fn run_job_tool(
        &self,
        tc: &TurnCtx<'_>,
        tool: &str,
        input: &Value,
        correlation_id: &str,
    ) -> AsyncMediaRun {
        let fail = |why: String| -> AsyncMediaRun {
            Box::pin(std::future::ready(Err(ToolFailure::new(why))))
        };
        let Some(core) = self.jobs.core.get().and_then(Weak::upgrade) else {
            return fail("the job tools are not ready: the core is not built".into());
        };
        let find = |job: &str| self.find_job(tc.kernel, tc.store, tc.session_id, job);
        let session = tc.session_id.to_string();
        match tool {
            names::READ => {
                let a: ReadArgs = match parse(input) {
                    Ok(a) => a,
                    Err(e) => return fail(e),
                };
                let lines = a
                    .lines
                    .unwrap_or(names::READ_LINES)
                    .min(names::READ_LINES_MAX);
                let found = match a.job.as_deref().map(find).transpose() {
                    Ok(f) => f,
                    Err(e) => return fail(e),
                };
                Box::pin(async move {
                    let out = theseus_store::blocking(|| match &found {
                        Some(f) => read_one(&core, f, lines),
                        None => running(&core, &session),
                    });
                    Ok((out, None, None))
                })
            }
            names::WAIT => {
                let a: WaitArgs = match parse(input) {
                    Ok(a) => a,
                    Err(e) => return fail(e),
                };
                let f = match find(&a.job) {
                    Ok(f) => f,
                    Err(e) => return fail(e),
                };
                // As long as the model asked, up to `[tools] job_wait_max_secs`: a
                // person's message, the job's end and a stop end the wait at once.
                let asked = a.timeout_secs.unwrap_or(self.proc_sync_secs);
                let secs = asked.clamp(1, self.job_wait_max_secs.max(self.proc_sync_secs).max(1));
                Box::pin(wait(core, f, secs, asked))
            }
            names::STOP => {
                let a: StopArgs = match parse(input) {
                    Ok(a) => a,
                    Err(e) => return fail(e),
                };
                match find(&a.job) {
                    Ok(f) => Box::pin(stop(core, f)),
                    Err(e) => fail(e),
                }
            }
            crate::task::WAIT => crate::task_wait::run(core, tc, input),
            crate::task::FAN_OUT => crate::task_fan_out::run(core, tc, correlation_id, input),
            other => fail(format!("{other} is not a job tool")),
        }
    }
}

/// A job's action now, or as it was found. A completion its wrapper has
/// spooled is taken first, with the drain, whichever is first, as a turn's
/// look at its own job takes one (`job_settled`): where no notice reaches
/// the drain (heartbeat only), a job's end is still seen at the next look.
fn now(core: &Core, f: &Found) -> Action {
    job_now(core, &f.corr, &f.action)
}

/// `job.read` of one job: how long it has run (or how it ended), how much it
/// printed, and its last `lines` lines.
fn read_one(core: &Core, f: &Found, lines: u64) -> ToolOutput {
    let rt = &core.tools;
    let a = now(core, f);
    let since = a.dispatched_at_ms.unwrap_or(a.planned_at_ms);
    let at = match ended(&a) {
        true => a.settled_at_ms.unwrap_or(since),
        false => theseus_protocol::now_unix_ms(),
    };
    let ran = narrative::duration(at.saturating_sub(since));
    let head = match ended(&a) {
        false => format!("Job {}, running {ran}", f.called()),
        true => format!("Job {} {} after {ran}", f.called(), how(&a)),
    };
    let path = rt.spool.as_ref().map(|s| s.result_path(&f.corr));
    let there = path.as_ref().is_some_and(|p| p.exists());
    let p = path
        .as_deref()
        .map(|p| super::peek::peek(p, lines, rt.output_max_bytes))
        .unwrap_or_default();
    let so_far = match (ended(&a), p.lines) {
        (false, Some(n)) => format!("{} so far", narrative::count(n, "line", "lines")),
        (true, Some(n)) => narrative::count(n, "line", "lines"),
        (_, None) => format!("{} so far", narrative::bytes(p.bytes)),
    };
    let mut text = match (there, p.shown) {
        // Its output went with its result, which this conversation holds.
        (false, _) if ended(&a) => format!("{head}; its result is in this conversation."),
        (_, 0) => format!("{head}; no output yet."),
        (_, n) if Some(n) == p.lines => format!("{head}; {so_far}:\n{}", p.text),
        (_, n) => format!(
            "{head}; {so_far}; the last {}:\n{}",
            narrative::count(n, "line", "lines"),
            p.text
        ),
    };
    if p.held && !ended(&a) {
        text.push_str(
            "\n[its output has passed what its file keeps from the start: its newest lines wait \
             in its wrapper until it ends]",
        );
    }
    if ended(&a) && there {
        text.push_str(&format!("\njob_wait {} gives its result.", f.name));
    }
    ToolOutput {
        text,
        meta: json!({
            KEY: f.name, "correlation_id": f.corr, "state": a.state.as_str(),
            "lines": p.lines, "bytes": p.bytes, "shown": p.shown,
        }),
    }
}

/// `job.read` with no job: the session's jobs that run, each with its
/// directory and how long it has run.
fn running(core: &Core, session: &str) -> ToolOutput {
    let t = theseus_protocol::now_unix_ms();
    let mut rows = Vec::new();
    for (name, e) in core.tools.jobs.handles.of_session(&core.store, session) {
        let Ok(Some(a)) = core.kernel.action(&e.corr) else {
            continue;
        };
        if ended(&a) {
            continue;
        }
        let since = a.dispatched_at_ms.unwrap_or(a.planned_at_ms);
        let argv = match e.argv.is_empty() {
            true => argv_of(&core.store, &a),
            false => e.argv,
        };
        rows.push(format!(
            "{name} ({}) in {}, running {}",
            label(&argv),
            a.resource.as_deref().unwrap_or("?"),
            narrative::duration(t.saturating_sub(since))
        ));
    }
    let text = match rows.len() {
        0 => "No job of this session is running.".to_string(),
        n => format!(
            "This session's {} running:\n{}",
            narrative::count(n as u64, "job", "jobs"),
            rows.join("\n")
        ),
    };
    ToolOutput {
        text,
        meta: json!({"running": rows.len()}),
    }
}

/// `job.wait`: until the job ends or `secs` pass, woken by each settle
/// (`JobWaits::settled`); the backstop's look (`waits::LOOK`) is the one
/// the turns' own waits have. `asked` is what the call asked: a longer ask
/// than `secs` is said in the result. A wait that outlasts its turn's cache
/// reads it again before it lapses (`keep_warm_wait`), beside the wait's
/// other events.
async fn wait(core: Arc<Core>, f: Found, secs: u64, asked: u64) -> AsyncMediaResult {
    let rt = &core.tools;
    let until = Instant::now() + Duration::from_secs(secs);
    // A message queued behind this turn ends the wait at once, the job
    // untouched (theseus-qu1lc).
    let steer = rt.job_waits.steers.watch(&f.action.session_id);
    let warmth = core.runner.keep_warm.warmth(&f.action.session_id);
    let mut reading: Option<
        std::pin::Pin<Box<dyn std::future::Future<Output = Outcome> + Send + '_>>,
    > = None;
    let mut reads = 0u32;
    // `[tools] job_wait_beat_secs`: a job whose output stands still that long
    // returns the wait with what it saw. 0 watches nothing.
    let beat = Duration::from_secs(core.cfg.tools.job_wait_beat_secs);
    let clock = wait_facts::Clock::start(&f.action);
    let mut silence = (!beat.is_zero()).then(|| {
        let path = rt.spool.as_ref().map(|s| s.result_path(&f.corr));
        theseus_store::blocking(|| wait_facts::Silence::start(path))
    });
    let mut held = false;
    loop {
        let settled = rt.job_waits.on_settle();
        tokio::pin!(settled);
        settled.as_mut().enable();
        let a = theseus_store::blocking(|| {
            if let Some(s) = silence.as_mut() {
                s.look();
            }
            now(&core, &f)
        });
        if ended(&a) {
            return theseus_store::blocking(|| delivered(&core, &f, &a, ""));
        }
        if steer.pending() {
            let text = format!(
                "Job {} is still running. A message arrived, so this wait returned early; its \
                 result also arrives by itself when it ends.",
                f.called()
            );
            let meta = json!({KEY: f.name, "correlation_id": f.corr, "state": "running",
                INTERRUPTED_BY: "message", "warm_reads": reads});
            return Ok((ToolOutput { text, meta }, None, None));
        }
        let left = until.saturating_duration_since(Instant::now());
        let silent = silence.as_ref().filter(|s| !held && s.quiet() >= beat);
        if let (Some(s), false) = (silent, left.is_zero()) {
            let beat_out = theseus_store::blocking(|| {
                let out = clock.beat(&core, &f, s, asked, reads)?;
                wait_status::post(&core, &f, &clock, s);
                Some(out)
            });
            match beat_out {
                Some(out) => return Ok((out, None, None)),
                // A job past its file's head prints into its wrapper: not silent.
                None => held = true,
            }
        }
        if left.is_zero() {
            let cap = match asked > secs {
                true => format!(" (the longest one wait lasts; {asked} s was asked)"),
                false => String::new(),
            };
            let mut text = format!(
                "Still running after {secs} s{cap}. job_read {} shows its latest output; its \
                 result also arrives by itself when it ends.",
                f.name
            );
            if let Some(s) = &silence {
                text.push('\n');
                text.push_str(&theseus_store::blocking(|| clock.said(&core, &f, s)));
            }
            let meta = json!({KEY: f.name, "correlation_id": f.corr, "state": "running",
                "warm_reads": reads});
            return Ok((ToolOutput { text, meta }, None, None));
        }
        // The next read of the turn's cache, if one falls inside this wait.
        let due = warmth.as_ref().and_then(|w| w.due_before(until));
        // The look that finds the beat is at the beat, not up to a look later.
        let look = match silence.as_ref().filter(|_| !held) {
            Some(s) => super::waits::LOOK.min(s.until(beat).max(Duration::from_millis(1))),
            None => super::waits::LOOK,
        };
        tokio::select! {
            () = &mut settled => {}
            () = steer.raised() => {}
            () = tokio::time::sleep(left.min(look)) => {}
            () = async { tokio::time::sleep_until(due.unwrap_or(until)).await },
                if due.is_some() && reading.is_none() => {
                if let Some(w) = warmth.as_ref().filter(|w| w.claim(Instant::now())) {
                    reads += 1;
                    let session = f.action.session_id.clone();
                    let runner = &core.runner;
                    reading = Some(Box::pin(async move { w.read(runner, &session).await }));
                }
            }
            out = async { reading.as_mut().unwrap().await }, if reading.is_some() => {
                reading = None;
                tracing::debug!(?out, job = %f.name, "job_wait: the turn's cache was read");
            }
        }
    }
}

/// What stopping a job did (`stop_job`): the job as it stands after, whether it had ended before the stop, and
/// whether it has ended now and its end was verified (nothing of it is left).
pub(crate) struct Stopped {
    pub action: Action,
    pub already: bool,
    pub ended: bool,
    pub verified: bool,
}

/// A job's action as it stands: settled from its spool if its wrapper has finished, else the kernel's, else `fallback`.
fn job_now(core: &Core, corr: &str, fallback: &Action) -> Action {
    if let Some(spool) = &core.tools.spool {
        if let Ok(Some(a)) = ToolRuntime::job_settled(&core.kernel, spool, corr) {
            return a;
        }
    }
    core.kernel
        .action(corr)
        .ok()
        .flatten()
        .unwrap_or_else(|| fallback.clone())
}

/// Stop the job `corr`, as a disk floor's stop tells one (`Kernel::stop_call`), its tree stopped through the one
/// stop (`terminate_all`: its cgroup where it has one, else its tree), each verdict recorded. The one body of the
/// model's `job_stop` and of `job.stop`; a job that had ended already is not stopped again. `by` names who stopped it.
pub(crate) async fn stop_job(
    core: &Core,
    corr: &str,
    fallback: &Action,
    by: &str,
) -> anyhow::Result<Stopped> {
    let a = theseus_store::blocking(|| job_now(core, corr, fallback));
    if ended(&a) {
        return Ok(Stopped {
            action: a,
            already: true,
            ended: true,
            verified: false,
        });
    }
    theseus_store::blocking(|| core.kernel.stop_call(corr, by))?;
    let ended_all = core
        .tools
        .terminate_all(
            &core.kernel,
            &core.store,
            std::slice::from_ref(&corr.to_string()),
            true,
        )
        .await;
    for e in ended_all.iter().filter(|e| e.written) {
        e.record(&core.session_rec(&e.action.session_id));
    }
    let a = theseus_store::blocking(|| job_now(core, corr, fallback));
    let verified = a
        .verdict
        .as_ref()
        .is_some_and(theseus_kernel::Verdict::verified);
    Ok(Stopped {
        ended: ended(&a),
        action: a,
        already: false,
        verified,
    })
}

/// `job.stop`: the job stopped (`stop_job`); then its output so far, as its result.
async fn stop(core: Arc<Core>, f: Found) -> AsyncMediaResult {
    let s = stop_job(&core, &f.corr, &f.action, "a job_stop call")
        .await
        .map_err(|e| ToolFailure::new(format!("job {} was not stopped: {e:#}", f.name)))?;
    if s.already {
        return theseus_store::blocking(|| {
            delivered(&core, &f, &s.action, "It had already ended.\n")
        });
    }
    let a = s.action;
    let line = match (s.ended, s.verified) {
        (true, true) => format!("Stopped job {}: nothing of it is left.\n", f.called()),
        (true, false) => format!(
            "Told job {} to stop; its end was not verified, so some of it may still run.\n",
            f.called()
        ),
        (false, _) => format!(
            "Told job {} to stop; it has not ended yet: job_wait {} hears its end.\n",
            f.called(),
            f.name
        ),
    };
    let (mut out, external, media) = theseus_store::blocking(|| delivered(&core, &f, &a, &line))?;
    if let Some(v) = &a.verdict {
        out.meta["verdict"] = json!(v);
    }
    Ok((out, external, media))
}

/// A job's result as a blocking `proc.run` gives it (`job_result`), after
/// `before`, naming the job it delivers. One whose result this conversation
/// holds already (its late result took its output) says so instead.
fn delivered(core: &Core, f: &Found, a: &Action, before: &str) -> AsyncMediaResult {
    let rt = &core.tools;
    if !ended(a) {
        let text = format!("{before}Job {} is still running.", f.called());
        let meta = json!({KEY: f.name, "correlation_id": f.corr, "state": a.state.as_str()});
        return Ok((ToolOutput { text, meta }, None, None));
    }
    if rt.raw_output(a).is_none() && late_written(core, a) {
        let text = format!(
            "{before}Job {} {}; its result is in this conversation already.",
            f.called(),
            how(a)
        );
        let meta = json!({KEY: f.name, "correlation_id": f.corr, "state": a.state.as_str()});
        return Ok((ToolOutput { text, meta }, None, None));
    }
    let input = json!({"argv": f.argv});
    let r = rt.job_result(&core.store, a, "", "proc.run", Some(&input));
    let mut meta = r.meta;
    meta[KEY] = json!(f.name);
    meta[DELIVERS] = json!(f.corr);
    let external: Option<External> = r.external;
    let text = format!("{before}{}", r.text);
    Ok((ToolOutput { text, meta }, external, None))
}

/// Has a late result of `a`'s been written to its session?
fn late_written(core: &Core, a: &Action) -> bool {
    use crate::node::Body;
    let Ok(nodes) = core.store.transcript(&a.session_id) else {
        return false;
    };
    nodes
        .iter()
        .rev()
        .filter(|(_, n)| n.kind == crate::stub::Kind::ToolResult)
        .any(|(_, n)| {
            matches!(&n.body, Body::ToolResult { late: true, correlation_id: Some(c), .. }
                if *c == a.correlation_id)
        })
}

/// Say the longest wait in `job_wait`'s description (`[tools] job_wait_max_secs`):
/// the tool's own text is static, and the cap is the operator's.
pub(super) fn say_wait_cap(defs: &mut [Value], max_secs: u64) {
    let wire = theseus_tools::wire_name(names::WAIT);
    for d in defs.iter_mut().filter(|d| d["name"] == wire.as_str()) {
        if let Some(text) = d["description"].as_str() {
            d["description"] = json!(format!("{text} A wait lasts at most {max_secs} s."));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_label_is_the_argv_cut_at_sixty_characters() {
        assert_eq!(label(&["cargo".into(), "test".into()]), "cargo test");
        let long: Vec<String> = vec!["x".repeat(80)];
        let l = label(&long);
        assert_eq!(l.chars().count(), 60);
        assert!(l.ends_with('…'));
        assert!(is_short("j12") && !is_short("j") && !is_short("job") && !is_short("act_1"));
    }
}
