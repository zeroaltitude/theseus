//! L1 for `proc.run` (M4 17b; design §2.2): `[sandbox]`, which class a job
//! runs in, L1's posture, the view an L1 job gets, and what health says of
//! it. The sandbox itself is `theseus_sandbox`, which the job wrapper runs
//! (`theseus_kernel::job`'s L1 path).
//!
//! - **The class** is chosen at plan time, at most one way, toward L1:
//!   `[sandbox] default`, then `l1_argv`, then the model's `sandbox: true`.
//!   `sandbox: false` overrides neither (the owner's decision 2, 2026-10-02).
//! - **L1's posture is notify** (decision 1): an L1 job can reach nothing
//!   (no capabilities, no network, scratch writes, and Theseus's floor and
//!   the approve list's paths covered in its view), so neither the floor nor
//!   the approve lists wait for it. A secret the broker grants its program
//!   it takes at its launch, as at L0: the call runs at no looser a posture
//!   than the secret's, so any approval comes before the launch (decision
//!   15; theseus-w5op). A session holding external text still holds it, as
//!   T1 holds any call that acts (20a lifts that for L1).
//! - **The class is bound**: an L1 call's proposal names it, so the digest a
//!   confirm binds covers it, and a confirmed call runs in the class its
//!   proposal names, never in one worked out again. So is an L1 job's egress
//!   list (18c, `crate::egress`): `[sandbox] egress`, and the hosts its call
//!   named, which make it wait when they go beyond that list.
//! - **Nothing at the start** (theseus-gyin): health reports the last real
//!   L1 launch, from its job's completion, and `theseusd check` runs the
//!   self-test on demand (`self_test`). An L1 job has no cgroup: its
//!   processes are capped by `RLIMIT_NPROC`, so a daemon that runs as root,
//!   whom Linux exempts from it, gets no L1 job at all (theseus-pv6i).

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;

use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use theseus_kernel::job::L1;
use theseus_protocol::sandbox::{RunningJob, SandboxHealth, SandboxLaunch};
use theseus_protocol::{Notice, Proposal};

use crate::policy::{Decision, Posture};
use crate::toolrun::ToolRuntime;
use theseus_tools::{Backend, Plan, Tool};

/// The tool whose jobs run in a class: the only one with a job (`Backend::Job`).
pub const PROC_RUN: &str = "proc.run";

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

/// `[sandbox]` (design §2.12; the owner's decisions 2 and 3).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
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
    /// Retired (theseus-gyin): an L1 job's memory, from the delegated cgroup
    /// L1 no longer has. Still loads, so an older config starts, with one
    /// warning, and is never honored.
    #[serde(default, skip_serializing)]
    pub memory_mb: Option<toml::Value>,
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
    /// The hosts every L1 job may reach through its egress proxy (M4 18c),
    /// each `host:port` with a glob on the host (`*.crates.io:443`). Empty
    /// by default: no network. A call may name more, and then it waits.
    #[serde(default)]
    pub egress: Vec<String>,
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
            memory_mb: None,
            pids: default_pids(),
            scratch_mb: default_scratch_mb(),
            output_mb: default_output_mb(),
            egress: Vec::new(),
        }
    }
}

impl SandboxConfig {
    pub fn validate(&self) -> Result<()> {
        for (name, v, min) in [
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
        if let Err(e) = crate::egress::check(&self.egress) {
            bail!("sandbox.egress has {e}, or *.crates.io:443 for every name under crates.io");
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

    /// The warning for a retired key this section still sets, which
    /// `Config::parse` gives once per load (theseus-gyin).
    pub fn retired(&self) -> Option<String> {
        self.memory_mb.is_some().then(|| {
            "sandbox.memory_mb is retired and ignored (theseus-gyin): an L1 job has no cgroup, so \
             no memory limit, as an L0 job has none; remove it"
                .into()
        })
    }
}

/// What a call's proposal binds about its job, so a confirmed call runs as
/// it was approved: its class (17b) and, in L1, its egress list (18c):
/// `[sandbox] egress`, and the hosts the call named.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Bound {
    pub class: Class,
    /// Empty at L0, and for an L1 job with no network.
    pub egress: Vec<String>,
}

impl Bound {
    /// What `p` binds.
    pub fn of(p: &Proposal) -> Self {
        Self {
            class: class_in(p),
            egress: crate::egress::bound(p),
        }
    }

    /// Names it in `p`'s policy context: the class for L1, and a list that
    /// is not empty.
    pub fn bind(&self, p: &mut Proposal) {
        bind_class(p, self.class);
        crate::egress::bind(p, &self.egress);
    }

    pub fn l1(&self) -> bool {
        self.class == Class::L1
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

/// The gate's decision for a call, before the external-text hold, and what
/// its proposal binds (the gate in `toolrun`): its class, and an L1 job's
/// egress list, whose hosts beyond `[sandbox] egress` make it wait (18c). L1
/// runs at notify: the floor and the approve lists guard what an L1 job
/// cannot reach, so neither is asked. The operator's own word about the
/// tool still is (the owner, 2026-10-02, theseus-jfs6): a `[policy.tools]` line
/// for it, or a tightening, that asks makes an L1 call wait too, so the model
/// cannot step around it with `sandbox: true`. The inherited
/// `[policy].enforcement` is not that word, and never makes L1 wait. The
/// broker's grant comes next in the gate's order (`toolrun::order`), in both
/// classes: a call given a secret runs at no looser a posture than the
/// secret's (decision 15, theseus-w5op).
pub(crate) fn unbrokered(
    rt: &ToolRuntime,
    tool: &dyn Tool,
    plan: &Plan,
    input: &Value,
    tightened: Option<crate::policy::Tightened<'_>>,
) -> (Decision, Bound) {
    let l1 = (tool.backend() == Backend::Job)
        .then(|| {
            rt.sandbox
                .l1_for(plan.argv.as_deref().unwrap_or(&[]), input)
        })
        .flatten();
    match l1 {
        Some(why) => {
            let asked = crate::egress::asked(input);
            let (egress, beyond) = crate::egress::list(&rt.sandbox.cfg.egress, &asked);
            let d = l1_decision(rt, tool.name(), plan, &why, &egress, tightened);
            let d = crate::egress::gate(d, &beyond, tool.name(), &plan.summary);
            let bound = Bound {
                class: Class::L1,
                egress,
            };
            (d, bound)
        }
        None => (
            rt.policy.decide_with(tool, plan, tightened),
            Bound::default(),
        ),
    }
}

/// A planned call's decision as the gate's record keeps it, with an L1
/// call's class.
pub(crate) fn record((_, d, bound): &(Plan, Decision, Bound)) -> theseus_protocol::GateDecision {
    theseus_protocol::GateDecision {
        class: bound.l1().then(|| Class::L1.as_str().into()),
        ..d.record()
    }
}

/// What a job is given at its spawn (`toolrun`'s `run_job`): what the broker
/// grants its program, in both classes (theseus-w5op), the values in its
/// environment; and in L1 its view and limits.
pub(crate) async fn for_job(
    rt: &ToolRuntime,
    bound: &Bound,
    spec: &theseus_tools::JobSpec,
    set: &[&str],
    path: Option<&str>,
    ran_at: Posture,
    correlation_id: &str,
) -> (crate::broker::ForJob, Option<L1>) {
    // An AWS job session lasts the job's deadline (AWS design §3.5).
    let job = crate::broker::JobAws {
        correlation_id,
        lasts: std::time::Duration::from_secs(spec.timeout_secs + 60),
    };
    let granted = rt
        .broker
        .for_job_of(&spec.argv, set, &spec.cwd, path, ran_at, Some(job))
        .await;
    if !bound.l1() {
        return (granted, None);
    }
    let mut view = rt.sandbox.job_view();
    // The list its proposal binds, and no other (18c).
    view.egress.clone_from(&bound.egress);
    (granted, Some(view))
}

/// A job was launched: counted by class, and an L1 job's `sandbox.started`
/// recorded (its limits and read-only paths).
/// An L1 job is also listed as running, with its command, until the
/// returned guard drops (theseus-kpz1): its call holds it until the call
/// returns, by when the frame that carries its `tool.job_started` row is
/// written.
pub(crate) fn started<'a>(
    rt: &'a ToolRuntime,
    tc: &crate::toolrun::TurnCtx<'_>,
    correlation_id: &str,
    tool: &str,
    spec: &theseus_tools::JobSpec,
    args: &theseus_kernel::job::WrapperArgs,
) -> Option<Running<'a>> {
    let class = if args.sandbox.is_some() {
        Class::L1
    } else {
        Class::L0
    };
    rt.sandbox.count(class);
    let view = args.sandbox.as_ref()?;
    let given: Vec<&str> = args.redact.iter().map(|(var, _)| var.as_str()).collect();
    tc.record(&crate::fact::sandbox::SandboxStarted {
        correlation_id,
        tool,
        argv: &spec.argv,
        cwd: &spec.cwd,
        view,
        sandbox: &rt.sandbox,
        scrubber: &rt.scrubber,
        given: &given,
    });
    Some(rt.sandbox.running(RunningJob {
        correlation_id: correlation_id.into(),
        session_id: tc.session_id.into(),
        tool: tool.into(),
        argv: spec.argv.clone(),
        started_at_ms: theseus_protocol::now_unix_ms(),
    }))
}

/// An L1 job listed as running (`Sandbox::running`), until this drops.
pub(crate) struct Running<'a> {
    sandbox: &'a Sandbox,
    correlation_id: String,
}

impl Drop for Running<'_> {
    fn drop(&mut self) {
        self.sandbox
            .running
            .lock()
            .unwrap()
            .remove(&self.correlation_id);
    }
}

/// An L1 call's decision: notify (decision 1), unless the operator's own
/// word about the tool asks: its `[policy.tools]` line, or a tightening
/// (theseus-jfs6). Neither makes it looser than notify.
fn l1_decision(
    rt: &ToolRuntime,
    tool: &str,
    plan: &Plan,
    why: &str,
    egress: &[String],
    tightened: Option<crate::policy::Tightened<'_>>,
) -> Decision {
    let own = rt
        .policy
        .tools
        .get(tool)
        .map(|p| (*p, format!("[policy.tools] \"{tool}\" = {}", p.as_str())));
    let base = own.unwrap_or((Posture::Notify, format!("L1: {why}")));
    let now = crate::policy::ToolPolicy::now_from(base, tightened);
    if now.posture < Posture::Approve {
        return decision(tool, why, egress);
    }
    Decision {
        posture: Posture::Approve,
        reason: format!(
            "{}: {tool} — approve (L1: {why}; {})",
            plan.summary,
            now.why()
        ),
        notify: None,
        floor: false,
        granted: None,
        external: None,
    }
}

/// L1's posture (decision 1): notify, whatever the floor and the lists say,
/// since an L1 job can reach none of what they guard. `why` is what chose
/// L1; `egress` is the job's list, which the rule names (18c). A secret the
/// broker grants it comes after (`decide`), as L0's does.
pub fn decision(tool: &str, why: &str, egress: &[String]) -> Decision {
    let reach = theseus_protocol::sandbox::reach(egress);
    let rule = format!("{tool} — notify (L1: {why}; {reach}, writes to scratch)");
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

/// What an L1 job started with, as its result's head and its narrative say
/// it: `no secret`, or `given GH_TOKEN`, the variables its program's grants
/// gave it at its launch. Names only, never a value.
pub fn given(vars: &[&str]) -> String {
    match vars {
        [] => "no secret".into(),
        _ => format!("given {}", crate::broker::and(vars)),
    }
}

/// An L1 job's lines at the head of its result (design §2.11): where it
/// ran, what it was given at its launch (the wrapper's `granted`, names
/// only), and what it wrote to scratch, the size limit it met, or why it
/// could not start. Empty for an L0 job.
pub fn result_lines(detail: &Value) -> String {
    let Some(sb) = detail.get("sandbox").filter(|s| s["class"] == "l1") else {
        return String::new();
    };
    let granted: Vec<&str> = sb["granted"]
        .as_array()
        .map(|a| a.iter().filter_map(Value::as_str).collect())
        .unwrap_or_default();
    let mut lines = Vec::new();
    match sb.get("error") {
        Some(e) => lines.push(format!(
            "[it could not start in L1, the sandbox: {}: {}. It did not run, in L1 or at L0]",
            e["stage"].as_str().unwrap_or("?"),
            e["error"].as_str().unwrap_or("?")
        )),
        None => lines.push(format!(
            "[ran in L1, the sandbox: {}, {}; {}]",
            crate::egress::reach_words(detail),
            given(&granted),
            detail
                .pointer("/scratch/summary")
                .and_then(Value::as_str)
                .unwrap_or("what it wrote to scratch was not reported, and is discarded")
        )),
    }
    lines.extend(crate::egress::lines(detail));
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
    lines.join("\n") + "\n"
}

/// What an L1 job's completion `detail` says of its launch at `at_ms`, as
/// health keeps the last one and `theseusd check` reports its self-test's
/// (theseus-gyin): whether it started in L1, or why not; how long the start
/// took; and what it found. None for an L0 job.
pub fn launch_of(detail: &Value, at_ms: u64) -> Option<SandboxLaunch> {
    let sb = detail.get("sandbox").filter(|s| s["class"] == "l1")?;
    let why = sb.get("error").map(|e| {
        format!(
            "{}: {}",
            e["stage"].as_str().unwrap_or("?"),
            e["error"].as_str().unwrap_or("?")
        )
    });
    Some(SandboxLaunch {
        ok: why.is_none(),
        at_ms,
        why,
        start_ms: sb["start_us"].as_u64().map(|us| us as f64 / 1000.0),
        sys: sb["sys"].as_bool(),
        lo: sb["lo"].as_bool(),
        skipped: sb["skipped"]
            .as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|s| s.as_str().map(str::to_string))
                    .collect()
            })
            .unwrap_or_default(),
    })
}

/// Credentials an `ro_paths` entry or a workspace root could bind: covered in
/// every view, as the floor is, whatever the approve list says, since an L1
/// job reads at notify what L0 would ask for. `~/.cargo` holds crates.io's
/// token after `cargo login`; `~/.aws` the operator's AWS keys, profiles, and
/// cached sessions, so the job session an `aws` grant gives an L1 job is the
/// only AWS credential it holds (18e). Each at its real path, as the view
/// binds what holds it.
const CREDENTIALS: [&str; 3] = [
    "~/.cargo/credentials",
    "~/.cargo/credentials.toml",
    "~/.aws",
];

/// L1's state in a daemon: its settings, the view a job gets, the last L1
/// launch heard of, why L1 refuses every job here (a root daemon), and the
/// jobs by class.
pub struct Sandbox {
    pub cfg: SandboxConfig,
    view: Mutex<L1>,
    /// The newest L1 launch whose job's completion this daemon has read
    /// (theseus-gyin), by its launch time.
    last: Mutex<Option<SandboxLaunch>>,
    /// `theseus_sandbox::refused_here`, asked once: L1 refuses every job of
    /// a daemon that runs as root (theseus-pv6i).
    refuses: Option<&'static str>,
    started: [AtomicU64; 2],
    /// Since the start (18c): connections out, bytes up and down, and
    /// refusals; and the latest refusal's words.
    egress: [AtomicU64; 4],
    refused_last: Mutex<Option<String>>,
    /// The L1 jobs that run before their `tool.job_started` row is written,
    /// by correlation id, for `sandbox.usage` (theseus-kpz1). In memory
    /// only: the row is the record, and this covers the time before it.
    running: Mutex<std::collections::BTreeMap<String, RunningJob>>,
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
        // The sandbox takes absolute paths alone, each once.
        let mut hidden: Vec<PathBuf> = Vec::new();
        for p in floor
            .iter()
            .chain(approve)
            .filter_map(|p| std::path::absolute(p).ok())
            .chain(CREDENTIALS.iter().map(|p| canon(p)))
            .filter(|p| p.is_absolute())
        {
            if !hidden.contains(&p) {
                hidden.push(p);
            }
        }
        let view = L1 {
            workspace: roots.to_vec(),
            ro_paths: cfg.ro_paths.iter().map(|p| canon(p)).collect(),
            hidden,
            limits: theseus_sandbox_limits(cfg),
            // Each job's own, from its proposal (`for_job`).
            egress: Vec::new(),
            egress_dns: crate::egress::test_dns(),
        };
        Self {
            cfg: cfg.clone(),
            view: Mutex::new(view),
            last: Mutex::default(),
            refuses: theseus_sandbox::refused_here(),
            started: [AtomicU64::new(0), AtomicU64::new(0)],
            egress: Default::default(),
            refused_last: Mutex::default(),
            running: Mutex::default(),
        }
    }

    /// List `job` as running until the returned guard drops.
    pub(crate) fn running(&self, job: RunningJob) -> Running<'_> {
        let correlation_id = job.correlation_id.clone();
        self.running
            .lock()
            .unwrap()
            .insert(correlation_id.clone(), job);
        Running {
            sandbox: self,
            correlation_id,
        }
    }

    /// A job's egress, counted for health (18c), once its result is written.
    pub fn egress_seen(&self, s: &theseus_sandbox::egress::Summary) {
        let sum = |f: fn(&theseus_sandbox::egress::Reached) -> u64| -> u64 {
            s.hosts.iter().map(f).sum()
        };
        let refused: u64 = s.refused.iter().map(|r| r.count).sum();
        for (i, n) in [
            sum(|h| h.connections),
            sum(|h| h.up),
            sum(|h| h.down),
            refused,
        ]
        .into_iter()
        .enumerate()
        {
            self.egress[i].fetch_add(n, Ordering::Relaxed);
        }
        if let Some(r) = s.refused.last() {
            *self.refused_last.lock().unwrap() = Some(r.why.clone());
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
        match input.get("sandbox") {
            Some(Value::Bool(true)) => Some("the call asked for it with sandbox: true".into()),
            // `sandbox: { egress: [...] }` (18c) asks for L1 too.
            Some(Value::Object(_)) => Some("the call asked for it with sandbox: { … }".into()),
            _ => None,
        }
    }

    /// A job started, by class (health's `jobs_l0`, `jobs_l1`).
    pub fn count(&self, class: Class) {
        self.started[class as usize].fetch_add(1, Ordering::Relaxed);
    }

    /// The view and limits for an L1 job.
    pub fn job_view(&self) -> L1 {
        self.view.lock().unwrap().clone()
    }

    /// What a job's limits are, in words: `512 processes, 1024 MB of
    /// scratch`.
    pub fn limits_line(&self) -> String {
        format!(
            "{} processes, {} MB of scratch",
            self.cfg.pids, self.cfg.scratch_mb
        )
    }

    /// An L1 job's completion, read (`launch_of`, launched at `at_ms`):
    /// health's last launch when it is the newest heard of (theseus-gyin).
    /// An L0 job's says nothing.
    pub fn launched(&self, detail: &Value, at_ms: u64) {
        let Some(l) = launch_of(detail, at_ms) else {
            return;
        };
        let mut last = self.last.lock().unwrap();
        if last.as_ref().is_none_or(|p| p.at_ms <= l.at_ms) {
            *last = Some(l);
        }
    }

    /// `theseusd check`'s self-test, on demand (theseus-gyin): `/bin/true`
    /// in L1 over the view a job gets, started as a job starts, on a thread
    /// that lives until it ends. A serving daemon never runs it.
    pub async fn self_test(&self) -> SandboxLaunch {
        let view = self.job_view();
        let at_ms = theseus_protocol::now_unix_ms();
        let detail = tokio::task::spawn_blocking(move || theseus_kernel::job::self_test(&view))
            .await
            .unwrap_or_else(|e| {
                serde_json::json!({"sandbox": {"class": "l1",
                    "error": {"stage": "running the self-test", "error": e.to_string()}}})
            });
        launch_of(&detail, at_ms).unwrap_or_default()
    }

    pub fn health(&self) -> SandboxHealth {
        SandboxHealth {
            default: self.cfg.default.as_str().into(),
            l1_argv: self.cfg.l1_argv.iter().map(|a| a.join(" ")).collect(),
            pids: self.cfg.pids,
            scratch_mb: self.cfg.scratch_mb,
            output_mb: self.cfg.output_mb,
            last_launch: self.last.lock().unwrap().clone(),
            refuses: self.refuses.map(str::to_string),
            jobs_l0: self.started[0].load(Ordering::Relaxed),
            jobs_l1: self.started[1].load(Ordering::Relaxed),
            egress: crate::egress::health_list(&self.cfg.egress),
            egress_connections: self.egress[0].load(Ordering::Relaxed),
            egress_up: self.egress[1].load(Ordering::Relaxed),
            egress_down: self.egress[2].load(Ordering::Relaxed),
            egress_refused: self.egress[3].load(Ordering::Relaxed),
            egress_last_refused: self.refused_last.lock().unwrap().clone(),
        }
    }

    /// `sandbox.usage`: the L1 jobs whose rows are not written yet, with
    /// their commands (theseus-kpz1), read now.
    pub fn usage(&self) -> theseus_protocol::sandbox::SandboxUsage {
        theseus_protocol::sandbox::SandboxUsage {
            at_ms: theseus_protocol::now_unix_ms(),
            running: self.running.lock().unwrap().values().cloned().collect(),
        }
    }
}

/// The template's `[sandbox]`, un-commented (`config.rs`'s
/// `example_template_uncommented_still_parses`): L0 by default (17b), the
/// commented `l1_argv` real, and the commented egress list (18c) that reaches
/// Rust's registry and GitHub, every entry one the proxy matches.
#[cfg(test)]
pub(crate) fn the_templates_sandbox_section(s: &SandboxConfig) {
    assert_eq!(s.default, Class::L0);
    assert_eq!(s.l1_argv[0], ["npm", "install"]);
    assert_eq!(s.ro_paths, ["~/.cargo", "~/.rustup"]);
    assert_eq!((s.memory_mb.as_ref(), s.pids), (None, 512));
    let egress = crate::egress::check(&s.egress).unwrap();
    for (host, port) in [
        ("index.crates.io", 443),
        ("static.crates.io", 443),
        ("api.github.com", 443),
    ] {
        assert!(egress.iter().any(|a| a.permits(host, port)), "{host}");
    }
    assert!(!egress.iter().any(|a| a.permits("pypi.org", 443)));
    // As written, with the line commented, an L1 job has no network.
    assert!(crate::Config::example().sandbox.egress.is_empty());
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

    /// Health keeps the newest L1 launch heard of, from its job's
    /// completion: how long it took, or why it failed (theseus-gyin). An L0
    /// job's, and an older launch read later, change nothing.
    #[test]
    fn health_keeps_the_newest_l1_launch() {
        let sb = Sandbox::new(&SandboxConfig::default(), &[], &[], &[]);
        assert_eq!(sb.health().last_launch, None, "no L1 job yet");
        sb.launched(&json!({"exit_code": 0}), 10);
        assert_eq!(sb.health().last_launch, None, "an L0 job's");
        let ran = json!({"sandbox": {"class": "l1", "start_us": 8100, "sys": true,
            "lo": true, "skipped": ["/nope"]}});
        sb.launched(&ran, 20);
        let l = sb.health().last_launch.unwrap();
        assert_eq!(
            (l.ok, l.at_ms, l.start_ms, l.why, l.skipped),
            (true, 20, Some(8.1), None, vec!["/nope".to_string()])
        );
        let failed = json!({"sandbox": {"class": "l1", "start_us": 300,
            "error": {"stage": "checking the job's process limit", "error": "root"}}});
        sb.launched(&failed, 15);
        assert!(
            sb.health().last_launch.unwrap().ok,
            "an older launch read later"
        );
        sb.launched(&failed, 30);
        let l = sb.health().last_launch.unwrap();
        assert_eq!(
            (l.ok, l.why.as_deref()),
            (false, Some("checking the job's process limit: root"))
        );
    }

    /// cargo's token never shows in a view, whatever `ro_paths` binds.
    #[test]
    fn cargos_credentials_are_hidden_in_every_view() {
        let cfg = SandboxConfig {
            ro_paths: vec!["~/.cargo".into()],
            ..Default::default()
        };
        let hidden = Sandbox::new(&cfg, &[], &[], &[])
            .view
            .lock()
            .unwrap()
            .hidden
            .clone();
        for f in CREDENTIALS {
            let real = theseus_tools::paths::canonical_best_effort(&crate::config::expand(f));
            assert!(hidden.contains(&real), "{f}: {hidden:?}");
        }
    }

    /// `~/.aws` never shows in a view (18e), whatever `ro_paths` binds and
    /// whatever the approve list says: an L1 job's AWS session is the only AWS
    /// credential it holds. Each path is hidden once, though the approve list
    /// names it too.
    #[test]
    fn the_operators_aws_files_are_hidden_in_every_view_whatever_the_approve_list_says() {
        let cfg = SandboxConfig {
            ro_paths: vec!["~/.aws".into()],
            ..Default::default()
        };
        let aws = theseus_tools::paths::canonical_best_effort(&crate::config::expand("~/.aws"));
        let hidden = |approve: &[PathBuf]| {
            Sandbox::new(&cfg, &[], &[], approve)
                .view
                .lock()
                .unwrap()
                .hidden
                .clone()
        };
        assert!(hidden(&[]).contains(&aws), "{:?}", hidden(&[]));
        let both = hidden(std::slice::from_ref(&aws));
        assert_eq!(both.iter().filter(|p| **p == aws).count(), 1, "{both:?}");
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

    /// A config pasted from an older template still sets `[sandbox]
    /// memory_mb`, which L1's cgroup took (theseus-gyin). It loads with one
    /// warning, and `theseusd config` no longer shows it.
    #[test]
    fn the_retired_memory_key_loads_with_one_warning() {
        let text = "[secrets]\nanthropic_api_key = \"op://v/i/f\"\n\n\
                    [sandbox]\nmemory_mb = 2048\npids = 512\n";
        let (cfg, warnings) = crate::Config::parse(text).unwrap();
        assert_eq!(warnings.len(), 1, "{warnings:?}");
        assert!(
            warnings[0].starts_with("sandbox.memory_mb is retired and ignored (theseus-gyin)"),
            "{warnings:?}"
        );
        let shown = toml::to_string(&cfg.sandbox).unwrap();
        assert!(!shown.contains("memory_mb"), "{shown}");
        assert_eq!(cfg.sandbox.pids, 512);
    }
}
