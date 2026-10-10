//! `proc.run` (spec §3.23): the escape hatch. One program, a typed argv, no
//! shell (a shell is `["bash", "-c", "..."]`, visible as such), run by the
//! detached job wrapper with a cleared environment, its own deadline, and a
//! spooled result. It is counted: the shell-fallback ratio watches it.

use std::collections::BTreeMap;

use serde::Deserialize;
use serde_json::{json, Value};

use crate::{parse, Access, Backend, JobSpec, Plan, Resource, Retry, Tool, ToolClass, ToolCtx};

pub struct Run;

/// The most steps a call may give (theseus-7gir.3): a batch is a handful of
/// shell steps, and its card lists every one of them whole.
pub const MAX_STEPS: usize = 16;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RunArgs {
    #[serde(default)]
    argv: Option<Vec<String>>,
    /// Programs run in turn, stopping at the first that fails
    /// (theseus-7gir.3): exactly one of `argv` and `steps`.
    #[serde(default)]
    steps: Option<Vec<StepArgs>>,
    #[serde(default)]
    cwd: Option<String>,
    #[serde(default)]
    timeout_secs: Option<u64>,
    #[serde(default)]
    env: BTreeMap<String, String>,
    /// L1 (M4 17b): the core reads it from the input when it chooses the
    /// call's class; here its egress hosts are checked (18c).
    #[serde(default)]
    sandbox: Option<Sandbox>,
    /// Answer at once with the job's handle, and leave it running
    /// (theseus-n8gk): one program, not a batch.
    #[serde(default)]
    background: bool,
}

/// One step of a batch: its own program, and its own directory and timeout,
/// each the call's when it gives none. `env` and `sandbox` are the call's.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct StepArgs {
    argv: Vec<String>,
    #[serde(default)]
    cwd: Option<String>,
    #[serde(default)]
    #[expect(
        dead_code,
        reason = "read from the input, as each step alone (`steps`)"
    )]
    timeout_secs: Option<u64>,
}

impl RunArgs {
    /// The one program, or the steps: exactly one, each naming a program.
    fn checked(&self) -> Result<(), String> {
        match (&self.argv, &self.steps) {
            (None, Some(_)) if self.background => Err(
                "background runs one program (argv); a batch's steps run in turn and are waited for"
                    .into(),
            ),
            (Some(_), Some(_)) => {
                Err("give argv (one program) or steps (programs run in turn), not both".into())
            }
            (None, None) => Err("argv must name a program (or steps, programs run in turn)".into()),
            (Some(argv), None) => named(argv)
                .then_some(())
                .ok_or_else(|| "argv must name a program".into()),
            (None, Some(steps)) if steps.is_empty() => {
                Err("steps must hold at least one step".into())
            }
            (None, Some(steps)) if steps.len() > MAX_STEPS => Err(format!(
                "steps holds {} steps, more than {MAX_STEPS}: split the batch",
                steps.len()
            )),
            (None, Some(steps)) => match steps.iter().position(|s| !named(&s.argv)) {
                Some(i) => Err(format!("step {}'s argv must name a program", i + 1)),
                None => Ok(()),
            },
        }
    }
}

fn named(argv: &[String]) -> bool {
    argv.first().is_some_and(|p| !p.trim().is_empty())
}

/// `sandbox: true`, or `sandbox: { egress: [...] }` (M4 18c): L1, with hosts
/// beyond `[sandbox] egress` that its job may reach once the call is
/// approved.
#[derive(Deserialize)]
#[serde(untagged)]
enum Sandbox {
    Asked(#[expect(dead_code, reason = "read by the core, from the call's input")] bool),
    With(SandboxWith),
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SandboxWith {
    #[serde(default)]
    egress: Vec<String>,
}

impl Tool for Run {
    fn name(&self) -> &'static str {
        "proc.run"
    }
    fn description(&self) -> &'static str {
        "Run one program with a typed argv (no shell): builds, tests, linters, git commands the git tools do not cover. Returns combined stdout and stderr with the exit code. Several programs in a row go in one call as `steps`. Prefer the fs, text, and git tools when they can do the job. It waits for the program's end up to a time limit; a run past it goes on in the background as a job, which job_read, job_wait and job_stop take, and its result also arrives by itself when it ends. background: true starts the program as such a job at once."
    }
    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "argv": {"type": "array", "items": {"type": "string"}, "minItems": 1, "description": "Program and arguments, e.g. [\"cargo\", \"test\", \"-p\", \"core\"]. Pass [\"bash\", \"-c\", \"...\"] only when a shell is truly needed. Give argv or steps, not both."},
                "steps": {"type": "array", "minItems": 1, "maxItems": MAX_STEPS, "items": {"type": "object", "properties": {"argv": {"type": "array", "items": {"type": "string"}, "minItems": 1}, "cwd": {"type": "string"}, "timeout_secs": {"type": "integer", "minimum": 1}}, "required": ["argv"], "additionalProperties": false}, "description": "Programs run in order, in place of argv, each with its own argv and optional cwd and timeout_secs (the call's are the default; env and sandbox apply to every step). The run stops at the first step that exits non-zero, and one result gives each step's exit code, output tail, and time, and names the steps not run."},
                "cwd": {"type": "string", "description": "Working directory. Default: the working directory."},
                "timeout_secs": {"type": "integer", "minimum": 1, "description": "Kill the program after this many seconds."},
                "background": {"type": "boolean", "description": "Answer at once with the job's id and leave it running (a server, a long build): job_read shows its latest output, job_wait waits for its end, job_stop stops it. Not with steps."},
                "env": {"type": "object", "additionalProperties": {"type": "string"}, "description": "Extra environment variables (no secrets; token/key names are refused)."},
                "sandbox": {"anyOf": [{"type": "boolean"}, {"type": "object", "properties": {"egress": {"type": "array", "items": {"type": "string"}}}}], "description": "Run it in the sandbox (L1): no network, an empty HOME, and its writes discarded afterwards. For untrusted code, builds, and tests. {\"egress\": [\"host:port\"]} lets it reach those hosts through the proxy HTTPS_PROXY names, once approved if the operator has not listed them; what it brings back is outside text."}
            },
            "additionalProperties": false
        })
    }
    fn class(&self) -> ToolClass {
        ToolClass::Run
    }
    fn backend(&self) -> Backend {
        Backend::Job
    }
    fn retry(&self) -> Retry {
        Retry::NonRepeatable
    }
    fn plan(&self, input: &Value, ctx: &ToolCtx) -> Result<Plan, String> {
        let a: RunArgs = parse(input)?;
        a.checked()?;
        if let Some(Sandbox::With(w)) = &a.sandbox {
            for e in &w.egress {
                e.parse::<crate::net::Allow>()
                    .map_err(|why| format!("sandbox.egress: {why}"))?;
            }
        }
        let dir = |cwd: Option<&str>| {
            cwd.or(a.cwd.as_deref())
                .map(|p| ctx.resolve(p))
                .unwrap_or_else(|| ctx.cwd.clone())
        };
        let Some(steps) = &a.steps else {
            let cwd = dir(None);
            let argv = a.argv.unwrap_or_default();
            return Ok(Plan {
                summary: format!("run `{}` in {}", argv.join(" "), cwd.display()),
                resources: vec![Resource {
                    path: cwd,
                    access: Access::Exec,
                }],
                argv: Some(argv),
                url: None,
                ..Default::default()
            });
        };
        // The card shows the batch whole: every step's argv and directory.
        let mut resources: Vec<Resource> = Vec::new();
        let mut listed = Vec::new();
        for (i, step) in steps.iter().enumerate() {
            let cwd = dir(step.cwd.as_deref());
            listed.push(format!(
                "{}. `{}` in {}",
                i + 1,
                step.argv.join(" "),
                cwd.display()
            ));
            if !resources.iter().any(|r| r.path == cwd) {
                resources.push(Resource {
                    path: cwd,
                    access: Access::Exec,
                });
            }
        }
        Ok(Plan {
            summary: format!(
                "run {} steps in turn, stopping at the first that fails: {}",
                steps.len(),
                listed.join("; ")
            ),
            resources,
            steps: Some(steps.iter().map(|s| s.argv.clone()).collect()),
            ..Default::default()
        })
    }
    fn job(&self, input: &Value, ctx: &ToolCtx) -> Result<JobSpec, String> {
        let a: RunArgs = parse(input)?;
        let cwd = a
            .cwd
            .as_deref()
            .map(|p| ctx.resolve(p))
            .unwrap_or_else(|| ctx.cwd.clone());
        if !cwd.is_dir() {
            return Err(format!(
                "working directory {} does not exist",
                cwd.display()
            ));
        }
        Ok(JobSpec {
            argv: a.argv.ok_or("a batch's jobs are its steps'")?,
            cwd,
            timeout_secs: a
                .timeout_secs
                .unwrap_or(ctx.proc_timeout_secs)
                .clamp(1, ctx.proc_timeout_max_secs),
            env: a.env.into_iter().collect(),
        })
    }
    /// Each step's job, every directory checked before the first starts.
    fn jobs(&self, input: &Value, ctx: &ToolCtx) -> Result<Vec<JobSpec>, String> {
        match self.steps(input) {
            None => self.job(input, ctx).map(|j| vec![j]),
            Some(steps) => steps
                .iter()
                .enumerate()
                .map(|(i, s)| self.job(s, ctx).map_err(|e| format!("step {}: {e}", i + 1)))
                .collect(),
        }
    }
    /// Each step as the call it would be alone: its argv, its directory and
    /// timeout or the call's, and the call's `env` and `sandbox`.
    fn steps(&self, input: &Value) -> Option<Vec<Value>> {
        let steps = input.get("steps")?.as_array()?;
        let mut alone = input.as_object()?.clone();
        alone.remove("steps");
        Some(
            steps
                .iter()
                .map(|s| {
                    let mut one = alone.clone();
                    for k in ["argv", "cwd", "timeout_secs"] {
                        if let Some(v) = s.get(k) {
                            one.insert(k.into(), v.clone());
                        }
                    }
                    Value::Object(one)
                })
                .collect(),
        )
    }
    /// A job's raw output is deleted once its result is written
    /// (theseus-wz2): only a run that prints less, or keeps its output in a
    /// file, gets the rest.
    fn rest(&self, _left_out: &str) -> String {
        "its output is not kept: run it again printing less, or with its output sent to a file \
         that fs_read then reads in ranges"
            .into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plans_argv_and_clamps_timeouts() {
        let d = tempfile::tempdir().unwrap();
        let c = ToolCtx::for_tests(d.path());
        let p = Run.plan(&json!({"argv": ["cargo", "test"]}), &c).unwrap();
        assert_eq!(p.argv.unwrap(), vec!["cargo", "test"]);
        assert_eq!(p.resources[0].access, Access::Exec);
        let j = Run
            .job(&json!({"argv": ["sleep", "1"], "timeout_secs": 999999}), &c)
            .unwrap();
        assert_eq!(j.timeout_secs, 3600);
        assert!(Run.plan(&json!({"argv": []}), &c).is_err());
        assert!(Run
            .job(&json!({"argv": ["ls"], "cwd": "no/such/dir"}), &c)
            .is_err());
    }

    /// `argv` or `steps`, exactly one, each naming a program, at most
    /// `MAX_STEPS` of them (theseus-7gir.3); the plan lists every step's
    /// argv and directory, and each step alone is the call with its own.
    #[test]
    fn steps_or_argv_exactly_one_and_the_plan_lists_every_step() {
        let d = tempfile::tempdir().unwrap();
        std::fs::create_dir(d.path().join("sub")).unwrap();
        let c = ToolCtx::for_tests(d.path());
        let err = |i: Value| Run.plan(&i, &c).unwrap_err();
        assert!(err(json!({"argv": ["a"], "steps": [{"argv": ["b"]}]})).contains("not both"));
        assert!(err(json!({})).contains("argv must name a program"));
        assert!(err(json!({"steps": []})).contains("at least one step"));
        assert!(err(json!({"steps": [{"argv": ["a"]}, {"argv": []}]})).contains("step 2's argv"));
        assert!(err(json!({"steps": [{"argv": ["a"], "env": {}}]})).contains("unknown field"));
        let many: Vec<Value> = (0..=MAX_STEPS).map(|_| json!({"argv": ["true"]})).collect();
        assert!(err(json!({"steps": many})).contains("more than 16"));
        let batch = json!({"steps": [{"argv": ["make", "x"]}, {"argv": ["ls"], "cwd": "sub", "timeout_secs": 5}], "env": {"A": "1"}, "timeout_secs": 9});
        let p = Run.plan(&batch, &c).unwrap();
        assert_eq!(p.argv, None);
        assert_eq!(
            p.steps,
            Some(vec![
                vec!["make".to_string(), "x".into()],
                vec!["ls".into()]
            ])
        );
        assert!(
            p.summary.contains("1. `make x` in ") && p.summary.contains("2. `ls` in "),
            "{}",
            p.summary
        );
        assert!(p.summary.contains("/sub"), "{}", p.summary);
        assert_eq!(p.resources.len(), 2);
        let alone = Run.steps(&batch).unwrap();
        assert_eq!(
            alone[1],
            json!({"argv": ["ls"], "cwd": "sub", "timeout_secs": 5, "env": {"A": "1"}})
        );
        assert_eq!(alone[0]["timeout_secs"], 9);
        let jobs = Run.jobs(&batch, &c).unwrap();
        assert_eq!((jobs[0].timeout_secs, jobs[1].timeout_secs), (9, 5));
        assert!(jobs[1].cwd.ends_with("sub"));
        let gone = json!({"steps": [{"argv": ["ls"]}, {"argv": ["ls"], "cwd": "nowhere"}]});
        assert!(Run.jobs(&gone, &c).unwrap_err().starts_with("step 2: "));
        assert!(Run.steps(&json!({"argv": ["ls"]})).is_none());
    }

    /// `sandbox` is `true`, or `{ egress: [...] }` whose every entry is
    /// `host:port` (18c): a bad entry, or another key, is invalid input.
    #[test]
    fn a_sandbox_with_egress_names_hosts_as_host_and_port() {
        let d = tempfile::tempdir().unwrap();
        let c = ToolCtx::for_tests(d.path());
        let plan = |sandbox: Value| Run.plan(&json!({"argv": ["true"], "sandbox": sandbox}), &c);
        assert!(plan(json!(true)).is_ok());
        assert!(plan(json!({})).is_ok());
        assert!(plan(json!({"egress": ["api.github.com:443", "*.crates.io:443"]})).is_ok());
        let bad = plan(json!({"egress": ["api.github.com"]})).unwrap_err();
        assert!(bad.starts_with("sandbox.egress: "), "{bad}");
        assert!(plan(json!({"egress": ["https://pypi.org/"]})).is_err());
        assert!(plan(json!({"hosts": ["a.test:443"]})).is_err());
        assert!(plan(json!("yes")).is_err());
    }

    /// `background: true` (theseus-n8gk) starts one program as a job and
    /// plans as the same run; a batch's steps are waited for, so a batch
    /// with it is invalid input.
    #[test]
    fn background_takes_one_program_and_never_a_batch() {
        let d = tempfile::tempdir().unwrap();
        let c = ToolCtx::for_tests(d.path());
        let p = Run
            .plan(&json!({"argv": ["sleep", "20"], "background": true}), &c)
            .unwrap();
        assert_eq!(p.argv.unwrap(), vec!["sleep", "20"]);
        let j = Run
            .job(&json!({"argv": ["sleep", "20"], "background": true}), &c)
            .unwrap();
        assert_eq!(j.argv, vec!["sleep", "20"]);
        let bad = Run
            .plan(
                &json!({"steps": [{"argv": ["ls"]}], "background": true}),
                &c,
            )
            .unwrap_err();
        assert!(bad.starts_with("background runs one program"), "{bad}");
        assert!(Run
            .plan(&json!({"argv": ["ls"], "background": "yes"}), &c)
            .is_err());
    }
}
