//! L1, the native sandbox (M4 17b; design §2.2, §2.11): what health says of
//! it. A `proc.run` call runs in L1 when the model asks (`sandbox: true`),
//! when `[sandbox] l1_argv` names its program, or when `[sandbox] default`
//! is `"l1"`; the class is in its gate record, the digest a confirm binds,
//! and `tool.started`.

use serde::{Deserialize, Serialize};

/// Health's `sandbox` block.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct SandboxHealth {
    /// `[sandbox] default`: `l0` or `l1`.
    pub default: String,
    /// `[sandbox] l1_argv`: the programs that always run in L1.
    pub l1_argv: Vec<String>,
    /// What an L1 job is given: `[sandbox]`'s limits, in MB and processes.
    pub memory_mb: u64,
    pub pids: u64,
    pub scratch_mb: u64,
    pub output_mb: u64,
    /// The probe after serving: absent until it has run.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub probe: Option<SandboxProbe>,
    /// Where a job's memory and pids limits come from: `delegated` (the
    /// daemon's own systemd unit, with `Delegate=yes`), or why there is no
    /// cgroup for jobs. Absent until it has been asked.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub cgroup: Option<String>,
    /// Jobs started since the daemon started, by class.
    pub jobs_l0: u64,
    pub jobs_l1: u64,
}

/// The probe's answer: `/bin/true` in L1, started as a job starts, once
/// after serving and again after a restart in place.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct SandboxProbe {
    /// L1 runs here.
    pub ok: bool,
    /// When it ran (unix ms).
    pub at_ms: u64,
    /// Why not, when it does not: the stage, and the error.
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
