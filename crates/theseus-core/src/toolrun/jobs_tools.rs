//! A job's handle (theseus-n8gk): `job.read`, `job.wait`, and `job.stop`
//! run here, over the jobs `proc.run` left in the background, each named by
//! its short id (`handles.rs`) or its long one, and only in the session that
//! started it. Their schemas are theseus-tools' (`jobs.rs`).
//!
//! - `job.read`: the job's latest lines, through the one tail reader
//!   (`peek.rs`), or with no job the session's running jobs.
//! - `job.wait`: a wait on the job's end as an event (the drain's settle, a
//!   stop's, the reconciler's, `JobWaits::settled`), at most
//!   `proc_sync_secs`; the job's result as a blocking `proc.run` gives it,
//!   or that it still runs.
//! - `job.stop`: the job's stop as `/stop` stops one (`Kernel::stop_call`,
//!   then `terminate_all`: its tree, verified by its cgroup where it has
//!   one), and its output so far. The gate judges it as a run of the job's
//!   own program (`job_planned`).
//!
//! A result that gives the job's end names the job it delivers (`DELIVERS`):
//! the late result its end still writes then says only that, since the
//! model has it already (`late.rs`).

use std::sync::{Arc, OnceLock, Weak};
use std::time::{Duration, Instant};

use serde_json::{json, Value};
use theseus_kernel::{Action, ActionState};
use theseus_tools::jobs::{self as names, ReadArgs, StopArgs, WaitArgs};
use theseus_tools::{
    parse, Access, AsyncMediaResult, AsyncMediaRun, Backend, External, Plan, Resource, ToolFailure,
    ToolOutput,
};

use super::handles::{Handles, KEY};
use super::{ToolRuntime, TurnCtx};
use crate::narrative;
use crate::Core;

/// The meta key of a result that gives a job's end: the job's correlation id.
pub const DELIVERS: &str = "delivers";

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
fn argv_of(store: &crate::store::Store, a: &Action) -> Vec<String> {
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
        Ok(plan)
    }

    /// A job tool's run, in its call's own task.
    pub(super) fn run_job_tool(
        &self,
        tc: &TurnCtx<'_>,
        tool: &str,
        input: &Value,
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
                let secs = a
                    .timeout_secs
                    .unwrap_or(self.proc_sync_secs)
                    .clamp(1, self.proc_sync_secs.max(1));
                Box::pin(wait(core, f, secs))
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
            other => fail(format!("{other} is not a job tool")),
        }
    }
}

/// A job's action now, or as it was found. A completion its wrapper has
/// spooled is taken first, with the drain, whichever is first, as a turn's
/// look at its own job takes one (`job_settled`): where no notice reaches
/// the drain (heartbeat only), a job's end is still seen at the next look.
fn now(core: &Core, f: &Found) -> Action {
    if let Some(spool) = &core.tools.spool {
        if let Ok(Some(a)) = ToolRuntime::job_settled(&core.kernel, spool, &f.corr) {
            return a;
        }
    }
    core.kernel
        .action(&f.corr)
        .ok()
        .flatten()
        .unwrap_or_else(|| f.action.clone())
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
/// the turns' own waits have.
async fn wait(core: Arc<Core>, f: Found, secs: u64) -> AsyncMediaResult {
    let rt = &core.tools;
    let until = Instant::now() + Duration::from_secs(secs);
    loop {
        let settled = rt.job_waits.on_settle();
        tokio::pin!(settled);
        settled.as_mut().enable();
        let a = theseus_store::blocking(|| now(&core, &f));
        if ended(&a) {
            return theseus_store::blocking(|| delivered(&core, &f, &a, ""));
        }
        let left = until.saturating_duration_since(Instant::now());
        if left.is_zero() {
            let text = format!(
                "Still running after {secs} s. job_read {} shows its latest output; its result \
                 also arrives by itself when it ends.",
                f.name
            );
            let meta = json!({KEY: f.name, "correlation_id": f.corr, "state": "running"});
            return Ok((ToolOutput { text, meta }, None, None));
        }
        tokio::select! {
            () = &mut settled => {}
            () = tokio::time::sleep(left.min(super::waits::LOOK)) => {}
        }
    }
}

/// `job.stop`: the job told to stop, as a disk floor's stop tells one
/// (`Kernel::stop_call`), its tree stopped through the one stop
/// (`terminate_all`: its cgroup where it has one, else its tree), each
/// verdict recorded; then its output so far, as its result.
async fn stop(core: Arc<Core>, f: Found) -> AsyncMediaResult {
    let rt = &core.tools;
    let a = theseus_store::blocking(|| now(&core, &f));
    if ended(&a) {
        return theseus_store::blocking(|| delivered(&core, &f, &a, "It had already ended.\n"));
    }
    let by = "a job_stop call";
    if let Err(e) = theseus_store::blocking(|| core.kernel.stop_call(&f.corr, by)) {
        return Err(ToolFailure::new(format!(
            "job {} was not stopped: {e:#}",
            f.name
        )));
    }
    let ended_all = rt
        .terminate_all(
            &core.kernel,
            &core.store,
            std::slice::from_ref(&f.corr),
            true,
        )
        .await;
    for e in ended_all.iter().filter(|e| e.written) {
        e.record(&core.session_rec(&e.action.session_id));
    }
    let a = theseus_store::blocking(|| now(&core, &f));
    let gone = a
        .verdict
        .as_ref()
        .is_some_and(theseus_kernel::Verdict::verified);
    let line = match (ended(&a), gone) {
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
