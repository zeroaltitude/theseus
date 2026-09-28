//! `proc.run` (spec §3.23): the escape hatch. One program, a typed argv, no
//! shell (a shell is `["bash", "-c", "..."]`, visible as such), run by the
//! detached job wrapper with a cleared environment, its own deadline, and a
//! spooled result. It is counted: the shell-fallback ratio watches it.

use std::collections::BTreeMap;

use serde::Deserialize;
use serde_json::{json, Value};

use crate::{parse, Access, Backend, JobSpec, Plan, Resource, Retry, Tool, ToolClass, ToolCtx};

pub struct Run;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RunArgs {
    argv: Vec<String>,
    #[serde(default)]
    cwd: Option<String>,
    #[serde(default)]
    timeout_secs: Option<u64>,
    #[serde(default)]
    env: BTreeMap<String, String>,
}

impl Tool for Run {
    fn name(&self) -> &'static str {
        "proc.run"
    }
    fn description(&self) -> &'static str {
        "Run one program with a typed argv (no shell): builds, tests, linters, git commands the git tools do not cover. Returns combined stdout and stderr with the exit code. Prefer the fs, text, and git tools when they can do the job. Long runs continue in the background and report back later."
    }
    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "argv": {"type": "array", "items": {"type": "string"}, "minItems": 1, "description": "Program and arguments, e.g. [\"cargo\", \"test\", \"-p\", \"core\"]. Pass [\"bash\", \"-c\", \"...\"] only when a shell is truly needed."},
                "cwd": {"type": "string", "description": "Working directory. Default: the working directory."},
                "timeout_secs": {"type": "integer", "minimum": 1, "description": "Kill the program after this many seconds."},
                "env": {"type": "object", "additionalProperties": {"type": "string"}, "description": "Extra environment variables (no secrets; token/key names are refused)."}
            },
            "required": ["argv"],
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
        if a.argv.is_empty() || a.argv[0].trim().is_empty() {
            return Err("argv must name a program".into());
        }
        let cwd = a
            .cwd
            .as_deref()
            .map(|p| ctx.resolve(p))
            .unwrap_or_else(|| ctx.cwd.clone());
        Ok(Plan {
            summary: format!("run `{}` in {}", a.argv.join(" "), cwd.display()),
            resources: vec![Resource {
                path: cwd,
                access: Access::Exec,
            }],
            argv: Some(a.argv),
            consequences: vec![],
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
            argv: a.argv,
            cwd,
            timeout_secs: a
                .timeout_secs
                .unwrap_or(ctx.proc_timeout_secs)
                .clamp(1, ctx.proc_timeout_max_secs),
            env: a.env.into_iter().collect(),
        })
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
}
