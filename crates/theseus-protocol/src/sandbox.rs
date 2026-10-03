//! L1, the native sandbox (M4 17b; design §2.2, §2.11): what health says of
//! it. A `proc.run` call runs in L1 when the model asks (`sandbox: true`),
//! when `[sandbox] l1_argv` names its program, or when `[sandbox] default`
//! is `"l1"`; the class is in its gate record, the digest a confirm binds,
//! and `tool.started`. Since 18c, so is an L1 job's egress list: `[sandbox]
//! egress`, and the hosts its call named.

use serde::{Deserialize, Serialize};

/// Health's `sandbox` block.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct SandboxHealth {
    /// `[sandbox] default`: `l0` or `l1`.
    pub default: String,
    /// `[sandbox] l1_argv`: the programs that always run in L1.
    pub l1_argv: Vec<String>,
    /// What an L1 job is given: `[sandbox]`'s limits, in processes and MB.
    pub pids: u64,
    pub scratch_mb: u64,
    pub output_mb: u64,
    /// The last real L1 launch since the daemon started (theseus-gyin):
    /// absent until the first L1 job has reported.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub last_launch: Option<SandboxLaunch>,
    /// Why L1 refuses every job here, when it does (theseus-pv6i): the
    /// daemon runs as root, whom Linux exempts from `RLIMIT_NPROC`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub refuses: Option<String>,
    /// Jobs started since the daemon started, by class.
    pub jobs_l0: u64,
    pub jobs_l1: u64,
    /// `[sandbox] egress` (M4 18c): the hosts every L1 job may reach through
    /// its proxy. Empty: an L1 job has no network unless its call names
    /// hosts, and is approved.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub egress: Vec<String>,
    /// Since the daemon started: L1 jobs' connections out through their
    /// proxies, the bytes they carried each way, and the `CONNECT`s refused.
    #[serde(default, skip_serializing_if = "crate::is_zero")]
    pub egress_connections: u64,
    #[serde(default, skip_serializing_if = "crate::is_zero")]
    pub egress_up: u64,
    #[serde(default, skip_serializing_if = "crate::is_zero")]
    pub egress_down: u64,
    #[serde(default, skip_serializing_if = "crate::is_zero")]
    pub egress_refused: u64,
    /// The latest refusal's words, `pypi.org:443 is not on this job's egress
    /// list`. Absent until one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub egress_last_refused: Option<String>,
}

/// An L1 job's reach, as every surface says it (M4 18c): `no network`, or
/// `egress: github.com:443, *.crates.io:443`.
pub fn reach(egress: &[String]) -> String {
    if egress.is_empty() {
        "no network".into()
    } else {
        format!("egress: {}", egress.join(", "))
    }
}

/// How an L1 launch went, as every surface says it (theseus-gyin): `worked
/// (start 8.1 ms)`, with what its start found missing, or `failed: <why>
/// (an L1 call fails, and never runs at L0)`.
pub fn launch_words(l: &SandboxLaunch) -> String {
    if !l.ok {
        return format!(
            "failed: {} (an L1 call fails, and never runs at L0)",
            l.why.as_deref().unwrap_or("no reason given")
        );
    }
    let mut found: Vec<String> = l
        .start_ms
        .map(|m| format!("start {m:.1} ms"))
        .into_iter()
        .collect();
    if l.sys == Some(false) {
        found.push("no sysfs: the kernel refused it".into());
    }
    if l.lo == Some(false) {
        found.push("lo down".into());
    }
    if found.is_empty() {
        "worked".into()
    } else {
        format!("worked ({})", found.join("; "))
    }
}

/// The egress list a call's proposal binds (its policy context's `egress`,
/// M4 18c): empty at L0, and for an L1 job with no network.
pub fn egress_in(policy_context: &serde_json::Value) -> Vec<String> {
    policy_context
        .get("egress")
        .and_then(serde_json::Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(serde_json::Value::as_str)
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

/// An L1 launch, as a job's completion reports it (theseus-gyin): health's
/// last one since the start, and `theseusd check`'s self-test.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct SandboxLaunch {
    /// The job started in L1.
    pub ok: bool,
    /// When it was launched (unix ms).
    pub at_ms: u64,
    /// Why not, when it did not: the stage, and the error.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub why: Option<String>,
    /// The start, from the spawn to the command's exec, in ms.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub start_ms: Option<f64>,
    /// A fresh sysfs was mounted (the kernel may refuse it).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub sys: Option<bool>,
    /// `lo` came up in the job's network namespace.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub lo: Option<bool>,
    /// `[sandbox] ro_paths` that do not exist, so a job is given none of
    /// them.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub skipped: Vec<String>,
}

/// `sandbox.usage`: the L1 jobs running now, read when asked. An L1 job has
/// no cgroup of its own to read (theseus-gyin).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct SandboxUsage {
    /// When it was read (unix ms).
    pub at_ms: u64,
    /// The L1 jobs running now whose `tool.job_started` row is not written
    /// yet (theseus-kpz1): it rides the turn's next frame, after the job ends
    /// or its turn stops waiting for it, so until then their commands are
    /// here, from the daemon's memory.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub running: Vec<RunningJob>,
}

/// An L1 job that runs, before its `tool.job_started` row is written: what
/// that row will say it ran (theseus-kpz1).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct RunningJob {
    pub correlation_id: String,
    pub session_id: String,
    pub tool: String,
    /// Its command, as its row's `argv` will have it.
    pub argv: Vec<String>,
    /// When its wrapper was launched (unix ms).
    pub started_at_ms: u64,
}
