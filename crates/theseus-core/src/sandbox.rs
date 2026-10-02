//! L1 for `proc.run` (M4 17b; design §2.2): `[sandbox]`, which class a job
//! runs in, L1's posture, the view an L1 job gets, the probe after serving,
//! and the delegated cgroup. The sandbox itself is `theseus_sandbox`, which
//! the job wrapper runs (`theseus_kernel::job`'s L1 path).
//!
//! - **The class** is chosen at plan time, at most one way, toward L1:
//!   `[sandbox] default`, then `l1_argv`, then the model's `sandbox: true`.
//!   `sandbox: false` overrides neither (Eddie's decision 2, 2026-10-02).
//! - **L1's posture is notify** (decision 1): an L1 job can reach nothing
//!   (no capabilities, no network, scratch writes, and Theseus's floor and
//!   the approve list's paths covered in its view), so neither the floor nor
//!   the approve lists wait for it, and no secret is granted to it (18d's).
//!   A session holding external text still holds it, as T1 holds any call
//!   that acts (20a lifts that for L1).
//! - **The class is bound**: an L1 call's proposal names it, so the digest a
//!   confirm binds covers it, and a confirmed call runs in the class its
//!   proposal names, never in one worked out again.
//! - **Nothing new before serving**: the probe runs after serving, and the
//!   cgroup is found by it, or by the first L1 job, and readied at that job.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::Duration;

use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use theseus_kernel::job::L1;
use theseus_protocol::sandbox::{SandboxHealth, SandboxProbe};
use theseus_protocol::{Notice, Proposal};

use crate::policy::{Decision, Posture};
use crate::toolrun::ToolRuntime;
use theseus_tools::{Backend, Plan, Tool};

/// A job's class: L0 runs it as the operator's process; L1 in the sandbox.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Class {
    #[default]
    L0,
    L1,
}

impl Class {
    pub fn as_str(self) -> &'static str {
        match self {
            Class::L0 => "l0",
            Class::L1 => "l1",
        }
    }
}

/// `[sandbox]` (design §2.12; Eddie's decisions 2 and 3).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SandboxConfig {
    /// The class of every `proc.run`: `l0` built in, so an upgrade changes
    /// nothing; `l1` runs every job in the sandbox.
    #[serde(default)]
    pub default: Class,
    /// Programs that always run in L1, as argv prefixes matched as
    /// `[policy] allow_argv` matches (`["npm", "install"]`).
    #[serde(default)]
    pub l1_argv: Vec<Vec<String>>,
    /// More read-only paths in an L1 job's view (`~/.cargo`, `~/.rustup`),
    /// at the same paths. The system's directories are always there. One
    /// that does not exist is skipped, and the result says so.
    #[serde(default)]
    pub ro_paths: Vec<String>,
    /// An L1 job's memory, where the daemon's cgroup is delegated.
    #[serde(default = "default_memory_mb")]
    pub memory_mb: u64,
    /// An L1 job's processes.
    #[serde(default = "default_pids")]
    pub pids: u64,
    /// What an L1 job may write to scratch (its writes to the workspace), and
    /// to each of its `/tmp` and HOME.
    #[serde(default = "default_scratch_mb")]
    pub scratch_mb: u64,
    /// The largest file an L1 job may write (`RLIMIT_FSIZE`). What it prints
    /// is capped by `[tools] job_output_max_bytes`, as at L0.
    #[serde(default = "default_output_mb")]
    pub output_mb: u64,
}

fn default_memory_mb() -> u64 {
    2048
}
fn default_pids() -> u64 {
    512
}
fn default_scratch_mb() -> u64 {
    1024
}
fn default_output_mb() -> u64 {
    64
}

impl Default for SandboxConfig {
    fn default() -> Self {
        Self {
            default: Class::L0,
            l1_argv: Vec::new(),
            ro_paths: Vec::new(),
            memory_mb: default_memory_mb(),
            pids: default_pids(),
            scratch_mb: default_scratch_mb(),
            output_mb: default_output_mb(),
        }
    }
}

impl SandboxConfig {
    pub fn validate(&self) -> Result<()> {
        for (name, v, min) in [
            ("memory_mb", self.memory_mb, 16),
            ("pids", self.pids, 1),
            ("scratch_mb", self.scratch_mb, 1),
            ("output_mb", self.output_mb, 1),
        ] {
            if v < min {
                bail!("sandbox.{name} = {v} is under {min}");
            }
        }
        if self.l1_argv.iter().any(|p| p.is_empty()) {
            bail!("sandbox.l1_argv has an empty entry: name a program");
        }
        for p in &self.ro_paths {
            let e = crate::config::expand(p);
            let owned = ["/proc", "/sys", "/dev"].iter().any(|o| e.starts_with(o));
            if !e.is_absolute() || e == Path::new("/") || owned {
                bail!(
                    "sandbox.ro_paths has {p:?}: name an absolute path, not the root, /proc, /sys, \
                     or /dev"
                );
            }
        }
        Ok(())
    }
}

/// The proposal's key that names an L1 call's class. An L0 call's proposal
/// has none, so its digest is what it was before 17b.
const CLASS_KEY: &str = "class";

/// The class a proposal names: what a confirmed call runs in.
pub fn class_in(p: &Proposal) -> Class {
    match p.policy_context.get(CLASS_KEY).and_then(Value::as_str) {
        Some("l1") => Class::L1,
        _ => Class::L0,
    }
}

/// Names `class` in a proposal's policy context, for L1 alone.
pub fn bind_class(p: &mut Proposal, class: Class) {
    if class == Class::L1 {
        if let Some(m) = p.policy_context.as_object_mut() {
            m.insert(CLASS_KEY.into(), Value::String(class.as_str().into()));
        }
    }
}

/// The gate's decision for a call, before the external-text hold, and its
/// class (the gate in `toolrun`). L1 runs at notify: the floor, the approve
/// lists, and the tool's posture guard what an L1 job cannot reach, and no
/// secret is granted to one (18d's), so none of them is asked. Any other
/// call takes L0's order: the policy, then the broker's grant.
pub(crate) fn decide(
    rt: &ToolRuntime,
    tool: &dyn Tool,
    plan: &Plan,
    input: &Value,
    tightened: Option<crate::policy::Tightened<'_>>,
) -> (Decision, Class) {
    let l1 = (tool.backend() == Backend::Job)
        .then(|| {
            rt.sandbox
                .l1_for(plan.argv.as_deref().unwrap_or(&[]), input)
        })
        .flatten();
    match l1 {
        Some(why) => (decision(tool.name(), &why), Class::L1),
        None => {
            let d = rt.policy.decide_with(tool, plan, tightened);
            (rt.brokered(tool.name(), plan, input, d), Class::L0)
        }
    }
}

/// A planned call's decision as the gate's record keeps it, with an L1
/// call's class.
pub(crate) fn record((_, d, class): &(Plan, Decision, Class)) -> theseus_protocol::GateDecision {
    theseus_protocol::GateDecision {
        class: (*class == Class::L1).then(|| class.as_str().into()),
        ..d.record()
    }
}

/// What a job is given at its spawn (`toolrun`'s `run_job`): at L0, what
/// the broker grants its program; in L1, no secret (decision 4; 18d brings
/// them), what L0 would grant named as withheld and nothing resolved, and
/// its view and limits, the cgroup readied at the first L1 job.
pub(crate) async fn for_job(
    rt: &ToolRuntime,
    class: Class,
    spec: &theseus_tools::JobSpec,
    set: &[&str],
    path: Option<&str>,
    ran_at: Posture,
) -> (crate::broker::ForJob, Option<L1>) {
    match class {
        Class::L0 => (
            rt.broker
                .for_job(&spec.argv, set, &spec.cwd, path, ran_at)
                .await,
            None,
        ),
        Class::L1 => {
            let would = rt
                .broker
                .at_gate("proc.run", Some(&spec.argv), set, &spec.cwd, path);
            (no_grant(would), Some(rt.sandbox.job_view().await))
        }
    }
}

/// A job was launched: counted by class, and an L1 job's `sandbox.started`
/// recorded (its limits, cgroup, and read-only paths).
pub(crate) fn started(
    rt: &ToolRuntime,
    tc: &crate::toolrun::TurnCtx<'_>,
    correlation_id: &str,
    tool: &str,
    spec: &theseus_tools::JobSpec,
    args: &theseus_kernel::job::WrapperArgs,
) {
    let class = if args.sandbox.is_some() {
        Class::L1
    } else {
        Class::L0
    };
    rt.sandbox.count(class);
    if let Some(view) = &args.sandbox {
        tc.record(&crate::fact::sandbox::SandboxStarted {
            correlation_id,
            tool,
            argv: &spec.argv,
            cwd: &spec.cwd,
            view,
            sandbox: &rt.sandbox,
            scrubber: &rt.scrubber,
        });
    }
}

/// L1's posture (decision 1): notify, whatever the floor, the lists, and the
/// tool's own posture say, since an L1 job can reach none of what they
/// guard. `why` is what chose L1.
pub fn decision(tool: &str, why: &str) -> Decision {
    let rule =
        format!("{tool} — notify (L1, since {why}: no network, no secret, writes to scratch)");
    Decision {
        posture: Posture::Notify,
        reason: rule.clone(),
        notify: Some(Notice {
            kind: "notify".into(),
            setting: format!("L1: {why}"),
            rule,
        }),
        floor: false,
        granted: None,
        external: None,
    }
}

/// Why an L1 job got none of the secrets L0 would grant it (decision 4).
pub const NO_SECRET: &str = "no secret reaches a job in L1, the sandbox (step 18d brings them)";

/// What the broker gives an L1 job: nothing. Each grant of a program that
/// L0 would make is withheld, with why, so the result and the ledger's
/// `secret.withheld` say so; no secret is resolved.
pub fn no_grant(would: Vec<crate::broker::Grant>) -> crate::broker::ForJob {
    crate::broker::ForJob {
        withheld: would
            .into_iter()
            .filter(|g| g.variable.is_some())
            .map(|g| (g, NO_SECRET.to_string()))
            .collect(),
        ..Default::default()
    }
}

/// An L1 job's lines at the head of its result (design §2.11): where it
/// ran and what it wrote to scratch, the limits it met, or why it could not
/// start. Empty for an L0 job.
pub fn result_lines(detail: &Value) -> String {
    let Some(sb) = detail.get("sandbox").filter(|s| s["class"] == "l1") else {
        return String::new();
    };
    let mut lines = Vec::new();
    match sb.get("error") {
        Some(e) => lines.push(format!(
            "[it could not start in L1, the sandbox: {}: {}. It did not run, in L1 or at L0]",
            e["stage"].as_str().unwrap_or("?"),
            e["error"].as_str().unwrap_or("?")
        )),
        None => lines.push(format!(
            "[ran in L1, the sandbox: no network, no secret; {}]",
            detail
                .pointer("/scratch/summary")
                .and_then(Value::as_str)
                .unwrap_or("what it wrote to scratch was not reported, and is discarded")
        )),
    }
    if let Some(n) = sb["pids_refused"].as_u64() {
        lines.push(format!(
            "[L1: {n} of its forks were refused at its process limit]"
        ));
    }
    if let Some(n) = sb["oom_kills"].as_u64() {
        lines.push(format!(
            "[L1: its memory limit killed {n} of its processes]"
        ));
    }
    if sb["output_capped"] == true {
        lines.push(
            "[L1: it wrote a file past its size limit ([sandbox] output_mb), and was stopped \
             there (SIGXFSZ)]"
                .into(),
        );
    }
    if let Some(a) = sb["skipped"].as_array().filter(|a| !a.is_empty()) {
        let names: Vec<&str> = a.iter().filter_map(Value::as_str).collect();
        lines.push(format!(
            "[L1: [sandbox] ro_paths it was not given, since they do not exist: {}]",
            names.join(", ")
        ));
    }
    if let Some(e) = sb["cgroup_error"].as_str() {
        lines.push(format!("[L1: it had no cgroup, so no memory limit: {e}]"));
    }
    lines.join("\n") + "\n"
}

/// Where an L1 job's memory and pids limits come from.
#[derive(Debug, Clone, PartialEq)]
enum Cgroup {
    /// The daemon's own systemd service, with `Delegate=yes`: each job gets
    /// `<dir>/jobs/<corr>`.
    Delegated(PathBuf),
    /// None, and why: the namespaces and seccomp still hold, and
    /// `RLIMIT_NPROC` still caps a job's processes.
    None(String),
}

/// L1's state in a daemon: its settings, the view a job gets, the probe's
/// last answer, the cgroup, and the jobs by class.
pub struct Sandbox {
    pub cfg: SandboxConfig,
    view: Mutex<L1>,
    probe: Mutex<Option<SandboxProbe>>,
    cgroup: tokio::sync::OnceCell<Cgroup>,
    /// The delegated cgroup's `jobs`, readied at the first L1 job, or why
    /// not.
    jobs: Mutex<Option<Result<PathBuf, String>>>,
    started: [AtomicU64; 2],
}

impl Sandbox {
    /// `roots` are the workspace's; no view shows the `floor`'s paths or the
    /// `approve` list's, whatever binds them.
    pub fn new(
        cfg: &SandboxConfig,
        roots: &[PathBuf],
        floor: &[PathBuf],
        approve: &[PathBuf],
    ) -> Self {
        let canon =
            |p: &str| theseus_tools::paths::canonical_best_effort(&crate::config::expand(p));
        let view = L1 {
            workspace: roots.to_vec(),
            ro_paths: cfg.ro_paths.iter().map(|p| canon(p)).collect(),
            // The sandbox takes absolute paths alone.
            hidden: floor
                .iter()
                .chain(approve)
                .filter_map(|p| std::path::absolute(p).ok())
                .collect(),
            limits: theseus_sandbox_limits(cfg),
            memory_mb: cfg.memory_mb,
            cgroup: None,
        };
        Self {
            cfg: cfg.clone(),
            view: Mutex::new(view),
            probe: Mutex::default(),
            cgroup: tokio::sync::OnceCell::new(),
            jobs: Mutex::default(),
            started: [AtomicU64::new(0), AtomicU64::new(0)],
        }
    }

    /// One more path no view may show: the daemon's socket, once known.
    pub fn hide(&self, p: PathBuf) {
        let Ok(p) = std::path::absolute(&p) else {
            return;
        };
        let mut v = self.view.lock().unwrap();
        if !v.hidden.contains(&p) {
            v.hidden.push(p);
        }
    }

    /// Why a `proc.run` of `argv` with `input` runs in L1, or `None`: L0.
    pub fn l1_for(&self, argv: &[String], input: &Value) -> Option<String> {
        if self.cfg.default == Class::L1 {
            return Some("[sandbox] default = \"l1\"".into());
        }
        let n = crate::policy::normalized_argv(argv);
        if let Some(p) = self
            .cfg
            .l1_argv
            .iter()
            .find(|p| crate::policy::prefix_match(&n, p))
        {
            return Some(format!("[sandbox] l1_argv names `{}`", p.join(" ")));
        }
        (input.get("sandbox").and_then(Value::as_bool) == Some(true))
            .then(|| "the call asked for it (sandbox: true)".into())
    }

    /// A job started, by class (health's `jobs_l0`, `jobs_l1`).
    pub fn count(&self, class: Class) {
        self.started[class as usize].fetch_add(1, Ordering::Relaxed);
    }

    /// The view and limits for an L1 job, with the job's cgroup's parent
    /// when the daemon's is delegated: the first L1 job finds the cgroup if
    /// the probe has not, and readies it (the daemon moves into its leaf).
    pub async fn job_view(&self) -> L1 {
        let cg = self.cgroup.get_or_init(find_cgroup).await;
        let mut v = self.view.lock().unwrap().clone();
        v.cgroup = self.jobs_dir(cg).ok();
        v
    }

    /// What a job's limits are, in words: `memory 2048 MB, 512 processes`,
    /// or why there is no memory limit.
    pub fn limits_line(&self) -> String {
        let memory = match self.jobs.lock().unwrap().as_ref() {
            Some(Ok(_)) => format!("{} MB of memory, ", self.cfg.memory_mb),
            Some(Err(why)) => format!("no memory limit ({why}), "),
            None => String::new(),
        };
        format!(
            "{memory}{} processes, {} MB of scratch",
            self.cfg.pids, self.cfg.scratch_mb
        )
    }

    fn jobs_dir(&self, cg: &Cgroup) -> Result<PathBuf, String> {
        let mut j = self.jobs.lock().unwrap();
        if let Some(r) = &*j {
            return r.clone();
        }
        let r = match cg {
            Cgroup::Delegated(dir) => theseus_sandbox::cgroup::delegate(dir)
                .map_err(|e| format!("readying {} for jobs: {e}", dir.display())),
            Cgroup::None(why) => Err(why.clone()),
        };
        *j = Some(r.clone());
        r
    }

    /// The probe (design §2.2): `/bin/true` in L1, through this binary's
    /// `sandbox-probe` role (`exe`), over the view a job gets. It also finds
    /// the cgroup, off every job's path. Kept for health, and returned.
    pub async fn probe(&self, exe: &Path) -> SandboxProbe {
        let mut view = self.view.lock().unwrap().clone();
        view.cgroup = None;
        let at_ms = theseus_protocol::now_unix_ms();
        let p = match run_probe(exe, &view).await {
            Ok(v) => read_probe(&v, at_ms),
            Err(why) => SandboxProbe {
                ok: false,
                at_ms,
                why: Some(why),
                ..Default::default()
            },
        };
        let _ = self.cgroup.get_or_init(find_cgroup).await;
        *self.probe.lock().unwrap() = Some(p.clone());
        p
    }

    /// The cgroup, in words: `delegated: <dir>`, or `none: <why>`.
    pub fn cgroup_line(&self) -> Option<String> {
        let cg = self.cgroup.get()?;
        Some(match (cg, self.jobs.lock().unwrap().as_ref()) {
            (_, Some(Err(why))) | (Cgroup::None(why), _) => format!("none: {why}"),
            (Cgroup::Delegated(dir), Some(Ok(_))) => format!("delegated: {}", dir.display()),
            (Cgroup::Delegated(dir), None) => {
                format!("delegated: {} (readied at the first L1 job)", dir.display())
            }
        })
    }

    pub fn health(&self) -> SandboxHealth {
        SandboxHealth {
            default: self.cfg.default.as_str().into(),
            l1_argv: self.cfg.l1_argv.iter().map(|a| a.join(" ")).collect(),
            memory_mb: self.cfg.memory_mb,
            pids: self.cfg.pids,
            scratch_mb: self.cfg.scratch_mb,
            output_mb: self.cfg.output_mb,
            probe: self.probe.lock().unwrap().clone(),
            cgroup: self.cgroup_line(),
            jobs_l0: self.started[0].load(Ordering::Relaxed),
            jobs_l1: self.started[1].load(Ordering::Relaxed),
        }
    }
}

/// The probe waits this long after serving (design §2.10): its row lands
/// outside the start's aftermath, and a second after the index tender's
/// start (`tender::START_AFTER`), so the two never share a moment.
pub const PROBE_AFTER: Duration = Duration::from_secs(3);

/// The probe after serving (design §2.2), once per image, so a restart in
/// place probes again: it waits [`PROBE_AFTER`], then runs this binary's
/// `sandbox-probe` role and records `sandbox.probe`. It holds the core by
/// `Weak` while it waits, so a stop never waits on it. A job never waits
/// for it: one that finds L1 broken says why itself.
pub async fn probe_after_serving(core: std::sync::Weak<crate::rpc::Core>) {
    tokio::time::sleep(PROBE_AFTER).await;
    let Some(c) = core.upgrade() else {
        return;
    };
    let sandbox = c.tools.sandbox.clone();
    drop(c);
    let probe = sandbox.probe(Path::new("/proc/self/exe")).await;
    let cgroup = sandbox.cgroup_line();
    tracing::info!(ok = probe.ok, why = ?probe.why, start_ms = ?probe.start_ms, cgroup = ?cgroup, "sandbox probe");
    let Some(c) = core.upgrade() else {
        return;
    };
    // Its row is a frame of its own, written off the runtime's workers.
    let _ = tokio::task::spawn_blocking(move || {
        c.rec(None).record(&crate::fact::sandbox::SandboxProbed {
            probe: &probe,
            cgroup: cgroup.as_deref(),
        });
    })
    .await;
}

/// `[sandbox]`'s limits as the sandbox takes them. `/tmp`, `/dev/shm`, and
/// HOME are each capped as scratch is (17a's `tmp_mb`, no key of its own).
fn theseus_sandbox_limits(cfg: &SandboxConfig) -> theseus_sandbox::Limits {
    theseus_sandbox::Limits {
        scratch_mb: cfg.scratch_mb,
        tmp_mb: cfg.scratch_mb,
        output_mb: cfg.output_mb,
        pids: cfg.pids,
    }
}

/// The probe's process: `<exe> sandbox-probe`, with the view on its stdin
/// and nothing of the daemon's environment but HOME and PATH. Its one line.
async fn run_probe(exe: &Path, view: &L1) -> Result<Value, String> {
    use std::process::Stdio;
    use theseus_kernel::children::{self, Kind};
    use tokio::io::AsyncWriteExt;
    let mut cmd = tokio::process::Command::new(exe);
    cmd.arg(theseus_kernel::job::PROBE_MODE)
        .env_clear()
        .envs(
            ["HOME", "PATH"]
                .iter()
                .filter_map(|k| std::env::var(k).ok().map(|v| (*k, v))),
        )
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true);
    let mut child = children::spawn(Kind::Owned, || cmd.spawn(), tokio::process::Child::id)
        .map_err(|e| format!("starting {}: {e}", exe.display()))?;
    let input = serde_json::to_vec(view).map_err(|e| e.to_string())?;
    if let Some(mut stdin) = child.stdin.take() {
        stdin
            .write_all(&input)
            .await
            .map_err(|e| format!("handing the probe its view: {e}"))?;
    }
    let out = tokio::time::timeout(Duration::from_secs(20), child.wait_with_output())
        .await
        .map_err(|_| "the probe gave no answer in 20 s".to_string())?
        .map_err(|e| format!("waiting for the probe: {e}"))?;
    serde_json::from_slice(&out.stdout).map_err(|e| {
        format!(
            "the probe's answer is not JSON ({e}); it exited with {}",
            out.status
        )
    })
}

/// The probe's line as health keeps it.
fn read_probe(v: &Value, at_ms: u64) -> SandboxProbe {
    let ok = v["ok"].as_bool().unwrap_or(false);
    let why = (!ok).then(|| {
        format!(
            "{}: {}",
            v["stage"].as_str().unwrap_or("?"),
            v["error"].as_str().unwrap_or("?")
        )
    });
    SandboxProbe {
        ok,
        at_ms,
        why,
        start_ms: v["start_us"].as_u64().map(|us| us as f64 / 1000.0),
        sys: v["sys"].as_bool(),
        lo: v["lo"].as_bool(),
        skipped: v["skipped"]
            .as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|s| s.as_str().map(str::to_string))
                    .collect()
            })
            .unwrap_or_default(),
    }
}

/// Whether the daemon's cgroup is its own and delegated (design §2.2,
/// "Limits"): systemd's own answer for the unit whose cgroup it is in.
/// Asked once, by the probe or the first L1 job, never on the start path.
async fn find_cgroup() -> Cgroup {
    let own = match theseus_sandbox::cgroup::own() {
        Ok(p) => p,
        Err(e) => return Cgroup::None(format!("no cgroup v2 here ({e})")),
    };
    // After its first L1 job the daemon lives in its own leaf, `daemon/`,
    // and a restart in place keeps it there.
    let dir = match (own.file_name(), own.parent()) {
        (Some(n), Some(p)) if n == "daemon" && p.join("jobs").is_dir() => p.to_path_buf(),
        _ => own,
    };
    let Some(unit) = dir.file_name().and_then(|n| n.to_str()).map(str::to_string) else {
        return Cgroup::None("the daemon's cgroup is the root".into());
    };
    if !unit.ends_with(".service") {
        return Cgroup::None(format!(
            "the daemon runs in {unit}, not in a service of its own (`theseusd install --user` \
             makes one)"
        ));
    }
    let user = dir.to_string_lossy().contains("/user@");
    match systemctl_show(&unit, user).await {
        Ok(out) => match judge(&unit, &out, std::process::id(), &dir) {
            Ok(()) => Cgroup::Delegated(dir),
            Err(why) => Cgroup::None(why),
        },
        Err(why) => Cgroup::None(format!("could not ask systemd about {unit}: {why}")),
    }
}

/// `systemctl [--user] show <unit> --property=Delegate --property=MainPID`.
async fn systemctl_show(unit: &str, user: bool) -> Result<String, String> {
    use std::process::Stdio;
    use theseus_kernel::children::{self, Kind};
    let mut cmd = tokio::process::Command::new("systemctl");
    if user {
        cmd.arg("--user");
    }
    cmd.args(["show", unit, "--property=Delegate", "--property=MainPID"])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true);
    let child = children::spawn(Kind::Owned, || cmd.spawn(), tokio::process::Child::id)
        .map_err(|e| e.to_string())?;
    let out = tokio::time::timeout(Duration::from_secs(5), child.wait_with_output())
        .await
        .map_err(|_| "no answer in 5 s".to_string())?
        .map_err(|e| e.to_string())?;
    if !out.status.success() {
        return Err(format!("systemctl exited with {}", out.status));
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

/// systemd's answer for `unit`, judged: delegated, its main process this
/// daemon, and its cgroup's files writable by it. Why not, otherwise.
fn judge(unit: &str, show: &str, pid: u32, dir: &Path) -> Result<(), String> {
    let prop = |k: &str| {
        show.lines()
            .find_map(|l| l.strip_prefix(k)?.strip_prefix('='))
            .map(str::trim)
    };
    if prop("Delegate") != Some("yes") {
        return Err(format!(
            "{unit} is not delegated (its unit needs Delegate=yes, as `theseusd install` writes)"
        ));
    }
    let main: Option<u32> = prop("MainPID").and_then(|p| p.parse().ok());
    if main != Some(pid) {
        return Err(format!(
            "{unit}'s main process is {}, not this daemon ({pid})",
            main.map_or("unknown".into(), |m| m.to_string())
        ));
    }
    for f in ["cgroup.procs", "cgroup.subtree_control"] {
        let p = dir.join(f);
        let c =
            std::ffi::CString::new(p.as_os_str().as_encoded_bytes()).map_err(|e| e.to_string())?;
        if unsafe { libc::access(c.as_ptr(), libc::W_OK) } != 0 {
            return Err(format!("{} is not writable by this daemon", p.display()));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// The class goes toward L1 only: the default, `l1_argv`, or the call's
    /// own ask, and `sandbox: false` undoes neither of the first two
    /// (decision 2).
    #[test]
    fn the_class_is_chosen_toward_l1_alone() {
        let argv = |a: &[&str]| a.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        let sb = |cfg: SandboxConfig| Sandbox::new(&cfg, &[], &[], &[]);
        let plain = sb(SandboxConfig::default());
        assert_eq!(plain.l1_for(&argv(&["ls"]), &json!({})), None);
        assert_eq!(
            plain.l1_for(&argv(&["ls"]), &json!({"sandbox": false})),
            None
        );
        assert!(plain
            .l1_for(&argv(&["ls"]), &json!({"sandbox": true}))
            .unwrap()
            .contains("sandbox: true"));
        let listed = sb(SandboxConfig {
            l1_argv: vec![argv(&["npm", "install"])],
            ..Default::default()
        });
        let why = listed
            .l1_for(
                &argv(&["/usr/bin/npm", "install", "x"]),
                &json!({"sandbox": false}),
            )
            .unwrap();
        assert!(why.contains("l1_argv names `npm install`"), "{why}");
        assert_eq!(listed.l1_for(&argv(&["npm", "test"]), &json!({})), None);
        let all = sb(SandboxConfig {
            default: Class::L1,
            ..Default::default()
        });
        assert!(all
            .l1_for(&argv(&["ls"]), &json!({"sandbox": false}))
            .unwrap()
            .contains("default"));
    }

    /// An L1 call's proposal names its class; an L0 call's is as before, so
    /// a confirm parked before 17b still binds.
    #[test]
    fn only_an_l1_proposal_names_its_class() {
        let mut p = Proposal {
            tool: "proc.run".into(),
            args: json!({"argv": ["true"]}),
            resource: None,
            policy_context: json!({"roots": [], "cwd": "/"}),
        };
        let before = theseus_kernel::gate::digest_proposal(&p);
        bind_class(&mut p, Class::L0);
        assert_eq!(theseus_kernel::gate::digest_proposal(&p), before);
        assert_eq!(class_in(&p), Class::L0);
        bind_class(&mut p, Class::L1);
        assert_ne!(theseus_kernel::gate::digest_proposal(&p), before);
        assert_eq!(class_in(&p), Class::L1);
    }

    /// systemd's answer: only a delegated unit whose main process is this
    /// daemon gives jobs a cgroup.
    #[test]
    fn a_cgroup_is_delegated_only_by_systemds_own_answer() {
        let dir = tempfile::tempdir().unwrap();
        for f in ["cgroup.procs", "cgroup.subtree_control"] {
            std::fs::write(dir.path().join(f), "").unwrap();
        }
        let ok = judge("t.service", "Delegate=yes\nMainPID=42\n", 42, dir.path());
        assert_eq!(ok, Ok(()));
        let not = judge("t.service", "Delegate=no\nMainPID=42\n", 42, dir.path()).unwrap_err();
        assert!(not.contains("not delegated"), "{not}");
        let other = judge("t.service", "MainPID=7\nDelegate=yes\n", 42, dir.path()).unwrap_err();
        assert!(other.contains("main process is 7"), "{other}");
        let gone = judge(
            "t.service",
            "Delegate=yes\nMainPID=42\n",
            42,
            &dir.path().join("x"),
        );
        assert!(gone.unwrap_err().contains("not writable"));
    }

    #[test]
    fn the_settings_are_checked() {
        let bad = |cfg: SandboxConfig| cfg.validate().unwrap_err().to_string();
        assert!(bad(SandboxConfig {
            pids: 0,
            ..Default::default()
        })
        .contains("sandbox.pids"));
        assert!(bad(SandboxConfig {
            ro_paths: vec!["/proc/self".into()],
            ..Default::default()
        })
        .contains("ro_paths"));
        assert!(bad(SandboxConfig {
            l1_argv: vec![vec![]],
            ..Default::default()
        })
        .contains("l1_argv"));
        SandboxConfig {
            ro_paths: vec!["~/.cargo".into()],
            ..Default::default()
        }
        .validate()
        .unwrap();
    }
}
