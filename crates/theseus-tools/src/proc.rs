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
    /// L1 (M4 17b): the core reads it from the input when it chooses the
    /// call's class; here its egress hosts are checked (18c).
    #[serde(default)]
    sandbox: Option<Sandbox>,
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
        "Run one program with a typed argv (no shell): builds, tests, linters, git commands the git tools do not cover. Returns combined stdout and stderr with the exit code. Prefer the fs, text, and git tools when they can do the job. Long runs continue in the background and report back later."
    }
    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "argv": {"type": "array", "items": {"type": "string"}, "minItems": 1, "description": "Program and arguments, e.g. [\"cargo\", \"test\", \"-p\", \"core\"]. Pass [\"bash\", \"-c\", \"...\"] only when a shell is truly needed."},
                "cwd": {"type": "string", "description": "Working directory. Default: the working directory."},
                "timeout_secs": {"type": "integer", "minimum": 1, "description": "Kill the program after this many seconds."},
                "env": {"type": "object", "additionalProperties": {"type": "string"}, "description": "Extra environment variables (no secrets; token/key names are refused)."},
                "sandbox": {"anyOf": [{"type": "boolean"}, {"type": "object", "properties": {"egress": {"type": "array", "items": {"type": "string"}}}}], "description": "Run it in the sandbox (L1): no network, an empty HOME, and its writes discarded afterwards. For untrusted code, builds, and tests. {\"egress\": [\"host:port\"]} lets it reach those hosts through the proxy HTTPS_PROXY names, once approved if the operator has not listed them; what it brings back is outside text."}
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
        if let Some(Sandbox::With(w)) = &a.sandbox {
            for e in &w.egress {
                e.parse::<crate::net::Allow>()
                    .map_err(|why| format!("sandbox.egress: {why}"))?;
            }
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
            url: None,
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
            argv: a.argv,
            cwd,
            timeout_secs: a
                .timeout_secs
                .unwrap_or(ctx.proc_timeout_secs)
                .clamp(1, ctx.proc_timeout_max_secs),
            env: a.env.into_iter().collect(),
        })
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
}
