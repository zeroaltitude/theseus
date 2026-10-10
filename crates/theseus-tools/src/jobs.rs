//! A job's handle (theseus-n8gk): `job.read`, `job.wait`, and `job.stop`,
//! beside `proc.run`, whose job they name. Each plans here, by its input
//! alone; the core finds the job in the calling session, judges `job.stop`
//! as a run of the job's own program, and runs all three (they need the
//! session, the kernel, and the spool).

use serde::Deserialize;
use serde_json::{json, Value};

use crate::{parse, Access, AsyncRun, Backend, Plan, Resource, Retry, Tool, ToolClass, ToolCtx};

pub const READ: &str = "job.read";
pub const WAIT: &str = "job.wait";
pub const STOP: &str = "job.stop";
/// The family the core runs them by.
pub const FAMILY: &str = "job";

/// `job.read`'s lines when the call names none, and the most it may name.
pub const READ_LINES: u64 = 40;
pub const READ_LINES_MAX: u64 = 400;

/// The longest `job.wait` any config allows: `proc.run`'s longest timeout.
/// The core holds a call to the configured `proc_sync_secs`.
pub const WAIT_SECS_MAX: u64 = 3600;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReadArgs {
    #[serde(default)]
    pub job: Option<String>,
    #[serde(default)]
    pub lines: Option<u64>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WaitArgs {
    pub job: String,
    #[serde(default)]
    pub timeout_secs: Option<u64>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StopArgs {
    pub job: String,
}

/// The three tools.
pub fn all() -> [std::sync::Arc<dyn Tool>; 3] {
    [
        std::sync::Arc::new(JobRead),
        std::sync::Arc::new(JobWait),
        std::sync::Arc::new(JobStop),
    ]
}

fn named(job: &str) -> Result<&str, String> {
    let j = job.trim();
    if j.is_empty() {
        return Err("job must name a job: its short id (j1) or its long one".into());
    }
    Ok(j)
}

fn a_read(summary: String, ctx: &ToolCtx) -> Plan {
    Plan {
        summary,
        resources: vec![Resource {
            path: ctx.cwd.clone(),
            access: Access::Read,
        }],
        ..Default::default()
    }
}

fn through_the_core() -> AsyncRun {
    Box::pin(std::future::ready(Err(crate::ToolFailure::new(
        "a job tool runs through its session's jobs",
    ))))
}

pub struct JobRead;

impl Tool for JobRead {
    fn name(&self) -> &'static str {
        READ
    }
    fn description(&self) -> &'static str {
        "Show a background job's latest output while it runs: how long it has run, how many \
         lines it has printed, and its last lines (40 by default). With no job, list this \
         session's running jobs."
    }
    fn input_schema(&self) -> Value {
        json!({"type": "object", "additionalProperties": false,
        "properties": {
            "job": {"type": "string", "description": "the job's id, as proc_run gave it (j1)"},
            "lines": {"type": "integer", "minimum": 1, "maximum": READ_LINES_MAX, "description": "how many of its last lines (default 40)"}
        }})
    }
    fn class(&self) -> ToolClass {
        ToolClass::Read
    }
    fn backend(&self) -> Backend {
        Backend::Async
    }
    fn retry(&self) -> Retry {
        Retry::SafeToRepeat
    }
    fn plan(&self, input: &Value, ctx: &ToolCtx) -> Result<Plan, String> {
        let a: ReadArgs = parse(input)?;
        if a.lines.is_some_and(|n| n == 0 || n > READ_LINES_MAX) {
            return Err(format!("lines must be 1 to {READ_LINES_MAX}"));
        }
        let summary = match &a.job {
            Some(j) => format!("read job {}'s output", named(j)?),
            None => "list this session's running jobs".into(),
        };
        Ok(a_read(summary, ctx))
    }
    fn run_async(&self, _: &Value, _: &ToolCtx) -> AsyncRun {
        through_the_core()
    }
}

pub struct JobWait;

impl Tool for JobWait {
    fn name(&self) -> &'static str {
        WAIT
    }
    fn description(&self) -> &'static str {
        "Wait for a background job to end and get its result, as proc_run gives a run that ends \
         in time; or, if it is still running when the wait ends, say so. timeout_secs is at most \
         proc_run's own wait (its default)."
    }
    fn input_schema(&self) -> Value {
        json!({"type": "object", "required": ["job"], "additionalProperties": false,
        "properties": {
            "job": {"type": "string", "description": "the job's id, as proc_run gave it (j1)"},
            "timeout_secs": {"type": "integer", "minimum": 1}
        }})
    }
    fn class(&self) -> ToolClass {
        ToolClass::Read
    }
    fn backend(&self) -> Backend {
        Backend::Async
    }
    fn retry(&self) -> Retry {
        Retry::SafeToRepeat
    }
    /// Past the longest wait any config allows, and a margin.
    fn deadline(&self) -> Option<std::time::Duration> {
        Some(std::time::Duration::from_secs(WAIT_SECS_MAX + 30))
    }
    fn plan(&self, input: &Value, ctx: &ToolCtx) -> Result<Plan, String> {
        let a: WaitArgs = parse(input)?;
        if a.timeout_secs == Some(0) {
            return Err("timeout_secs must be at least 1".into());
        }
        Ok(a_read(format!("wait for job {}", named(&a.job)?), ctx))
    }
    fn run_async(&self, _: &Value, _: &ToolCtx) -> AsyncRun {
        through_the_core()
    }
}

pub struct JobStop;

impl Tool for JobStop {
    fn name(&self) -> &'static str {
        STOP
    }
    fn description(&self) -> &'static str {
        "Stop a background job: its program and everything it started, then its output so far \
         and how it ended. Stopping a job counts as running its program."
    }
    fn input_schema(&self) -> Value {
        json!({"type": "object", "required": ["job"], "additionalProperties": false,
        "properties": {
            "job": {"type": "string", "description": "the job's id, as proc_run gave it (j1)"}
        }})
    }
    fn class(&self) -> ToolClass {
        ToolClass::Run
    }
    fn backend(&self) -> Backend {
        Backend::Async
    }
    fn retry(&self) -> Retry {
        Retry::SafeToRepeat
    }
    /// The input alone: the core makes it the job's own run (its argv and
    /// directory) once it has found the job in the calling session.
    fn plan(&self, input: &Value, ctx: &ToolCtx) -> Result<Plan, String> {
        let a: StopArgs = parse(input)?;
        Ok(Plan {
            summary: format!("stop job {}", named(&a.job)?),
            resources: vec![Resource {
                path: ctx.cwd.clone(),
                access: Access::Exec,
            }],
            ..Default::default()
        })
    }
    fn run_async(&self, _: &Value, _: &ToolCtx) -> AsyncRun {
        through_the_core()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The gate's classes (theseus-n8gk): a read and a wait are reads, a stop
    /// is a run; each checks its input, and a stop plans as a run until the
    /// core names the job's program.
    #[test]
    fn a_read_and_a_wait_are_reads_and_a_stop_is_a_run() {
        let [read, wait, stop] = all();
        assert_eq!(read.class(), ToolClass::Read);
        assert_eq!(wait.class(), ToolClass::Read);
        assert_eq!(stop.class(), ToolClass::Run);
        for t in [&read, &wait, &stop] {
            assert_eq!(t.backend(), Backend::Async);
            assert_eq!(t.family(), FAMILY);
        }
        assert_eq!(read.wire_name(), "job_read");
        let d = tempfile::tempdir().unwrap();
        let c = ToolCtx::for_tests(d.path());
        assert_eq!(
            read.plan(&json!({}), &c).unwrap().summary,
            "list this session's running jobs"
        );
        assert!(read.plan(&json!({"job": "j1", "lines": 0}), &c).is_err());
        assert!(read.plan(&json!({"job": "j1", "lines": 401}), &c).is_err());
        assert!(wait.plan(&json!({}), &c).is_err(), "a wait names its job");
        assert!(wait
            .plan(&json!({"job": "j1", "timeout_secs": 0}), &c)
            .is_err());
        assert!(stop.plan(&json!({"job": " "}), &c).is_err());
        let p = stop.plan(&json!({"job": "j2"}), &c).unwrap();
        assert_eq!(p.summary, "stop job j2");
        assert_eq!(p.resources[0].access, Access::Exec);
        assert!(wait.deadline().unwrap().as_secs() > WAIT_SECS_MAX);
    }
}
