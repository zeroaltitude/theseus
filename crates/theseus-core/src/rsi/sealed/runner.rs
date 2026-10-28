//! The sealed scorer's production [`Runner`] (theseus-pw1q.21.5, the hard
//! keel): a scratch daemon of the candidate's own build, run in L1 under a
//! view that holds nothing of Theseus, reaching a model only through the
//! metered relay (`relay.rs`).
//!
//! **The build.** A candidate is the `theseusd` and `theseus` its self build
//! left in `<state>/self/target/<slug>/debug/` (main is scored from its
//! installed binaries: any directory holding the two). They are copied into
//! `<state>/self/runner/<branch>/bin/`, read-only, with their SHA-256
//! digests ([`ScratchRunner::digests`]); the generated config sits beside
//! them in `cfg/`.
//!
//! **The daemon.** Each item is one L1 job: `theseus --spawn theseusd ask`
//! over that config, so every item starts a daemon of its own, on a state dir
//! in L1's tmpfs that goes with the job's pid namespace: nothing an item
//! caused outlives it, so no item's text reaches a memory or a ledger anyone
//! reads later. The config turns off memory, the judge, the channels, the
//! web UI, the index and the MCP server, names no AWS account (so no hands)
//! and no extension, and points `model.api_base` at the relay; its key is a
//! placeholder, the real one never enters L1. The job's environment is bare:
//! nothing of the production daemon's `THESEUS_*` is inherited, and the only
//! ones set are the two that name the config and the state dir
//! ([`DAEMON_ENV`]).
//!
//! **The view** ([`runner_view`]): the system's directories and the runner's
//! own directory, read-only; no workspace, no production state, no sealed
//! dir (a floor path, hidden in every view); the relay's name as the egress
//! list.
//!
//! **Per item**: the relay opens an item line (`[self.sealed] item_usd`), the
//! job runs under `[route_bench] whole_seconds` and its profile's
//! `whole_loops`, and the answer is the reply's text with the cost the relay
//! metered, never the candidate's report. A timeout, a crash, a turn that did
//! not end, or a call the relay refused is [`RunFailed`].

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{bail, Context, Result};
use serde_json::Value;
use sha2::{Digest, Sha256};
use theseus_kernel::job::L1;
use theseus_kernel::job_egress::Registered;
use theseus_kernel::probe_run::{self, ProbeRun};

use super::relay::Relay;
use super::{Answer, RunFailed, Runner, Task};
use crate::config::ProfileConfig;
use crate::rpc::Core;

/// The programs a candidate build must hold.
pub const BINARIES: [&str; 2] = ["theseusd", "theseus"];

/// The variables a scratch daemon is never handed from the environment it is
/// started in: the ones that name a config, a state dir, a socket, or the
/// session of a job. theseus-exam's `start` takes them out of its daemons'
/// environment; the runner's job has a bare one and sets only [`DAEMON_ENV`].
pub const SCRATCH_UNSET: [&str; 4] = [
    "THESEUS_CONFIG",
    "THESEUS_STATE_DIR",
    "THESEUS_SOCKET",
    "THESEUS_SESSION",
];

/// The only `THESEUS_*` variables the job has, set to its config and to a
/// state dir in L1's tmpfs.
pub const DAEMON_ENV: [&str; 2] = ["THESEUS_CONFIG", "THESEUS_STATE_DIR"];

/// The most of an item's output the runner keeps: a reply and its envelope.
const OUTPUT_MAX: usize = 4 * 1024 * 1024;
/// The output cap a scratch profile asks of each call, which the relay's
/// worst-case reservation is made from.
const MAX_OUTPUT_TOKENS: u32 = 4096;

/// What a [`ScratchRunner`] scores: a build of the candidate, and the profile
/// it runs on.
#[derive(Debug, Clone)]
pub struct Candidate {
    /// The self branch (or `main`), which names the runner's directory and
    /// the budget line the relay spends on.
    pub branch: String,
    /// A directory holding the candidate's `theseusd` and `theseus`.
    pub bins: PathBuf,
    /// The profile under test: the only model the relay serves.
    pub profile: String,
}

/// How an item's job runs. L1 in production; a test's seam.
pub trait Launch: Send + Sync {
    fn run(&self, job: &Job) -> ProbeRun;
}

/// One item's job.
#[derive(Debug, Clone)]
pub struct Job {
    pub argv: Vec<String>,
    pub cwd: PathBuf,
    pub env: Vec<(String, String)>,
    pub id: String,
    pub deadline_ms: u64,
}

/// The job in L1, over the runner's view.
pub fn in_l1(view: &L1) -> Box<dyn Launch> {
    Box::new(InL1(view.clone()))
}

struct InL1(L1);

impl Launch for InL1 {
    fn run(&self, j: &Job) -> ProbeRun {
        probe_run::run_in(
            &self.0,
            &j.argv,
            (&j.cwd, &j.env),
            &j.id,
            j.deadline_ms,
            OUTPUT_MAX,
        )
    }
}

/// A candidate's build, scored one item at a time. Dropping it removes its
/// directory and ends the relay's service.
pub struct ScratchRunner {
    relay: Arc<Relay>,
    _serving: Registered,
    root: PathBuf,
    view: L1,
    launch: Box<dyn Launch>,
    digests: Vec<(String, String)>,
    item_usd: f64,
    deadline_ms: u64,
}

impl ScratchRunner {
    /// The runner for `candidate` on `core`: its build copied and its relay
    /// serving, its items run in L1. Blocking (it copies the binaries); call
    /// it from the blocking pool, within the runtime.
    pub fn new(core: &Arc<Core>, candidate: &Candidate) -> Result<Self> {
        Self::with_launch(core, candidate, |view, _| in_l1(view))
    }

    /// [`ScratchRunner::new`] with the way a job runs chosen from the view
    /// built and the relay (a test's seam: nothing else about the runner
    /// changes).
    pub fn with_launch(
        core: &Arc<Core>,
        candidate: &Candidate,
        launch: impl FnOnce(&L1, &Arc<Relay>) -> Box<dyn Launch>,
    ) -> Result<Self> {
        let cfg = &core.cfg;
        let host = format!(
            "r{}.relay.invalid",
            &uuid::Uuid::new_v4().simple().to_string()[..12]
        );
        let relay = Relay::new(core, &candidate.profile, &candidate.branch, &host)?;
        let root = cfg
            .state_dir()
            .join("self")
            .join("runner")
            .join(slug(&candidate.branch));
        remove_tree(&root);
        let digests = install(&root, &candidate.bins)?;
        let profile = cfg.profile(&candidate.profile)?;
        let caps = (
            cfg.route_bench.whole_loops,
            cfg.self_improve.sealed.item_usd,
        );
        let config = scratch_config(&candidate.profile, profile, &relay.url(), caps);
        write_read_only(
            &root.join("cfg").join("theseus.toml"),
            toml::to_string(&config)?.as_bytes(),
        )?;
        seal(&root)?;
        let view = runner_view(&core.tools.sandbox.job_view(), &root, &relay.entry());
        let launch = launch(&view, &relay);
        Ok(Self {
            _serving: relay.serve(),
            relay,
            root,
            view,
            launch,
            digests,
            item_usd: cfg.self_improve.sealed.item_usd,
            deadline_ms: cfg.route_bench.whole_seconds * 1000,
        })
    }

    /// The model relay this runner spends through: its meter and its log.
    pub fn relay(&self) -> &Arc<Relay> {
        &self.relay
    }

    /// The L1 view its jobs run under.
    pub fn view(&self) -> &L1 {
        &self.view
    }

    /// The runner's directory (binaries and config), gone when it drops.
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Each copied program's name and SHA-256, hex.
    pub fn digests(&self) -> &[(String, String)] {
        &self.digests
    }

    fn job(&self, task: &str) -> Job {
        let bin = |n: &str| self.root.join("bin").join(n).display().to_string();
        let var = |k: &str, v: String| (k.to_string(), v);
        Job {
            argv: [
                bin("theseus"),
                "--spawn".into(),
                bin("theseusd"),
                "ask".into(),
                "--json".into(),
                "--follow-for".into(),
                "0".into(),
                "--dir".into(),
                "/tmp".into(),
                "--".into(),
                task.to_string(),
            ]
            .to_vec(),
            cwd: "/tmp".into(),
            env: vec![
                var(
                    DAEMON_ENV[0],
                    self.root
                        .join("cfg")
                        .join("theseus.toml")
                        .display()
                        .to_string(),
                ),
                var(DAEMON_ENV[1], "/dev/shm/state".into()),
                var("SEALED_RELAY_KEY", "relay".into()),
                var("TOKIO_WORKER_THREADS", "2".into()),
            ],
            id: "sealed".into(),
            deadline_ms: self.deadline_ms,
        }
    }
}

impl Runner for ScratchRunner {
    fn answer(&self, task: &Task<'_>) -> Result<Answer, RunFailed> {
        self.relay.begin(self.item_usd);
        let run = self.launch.run(&self.job(task.text()));
        let spent = self.relay.finish();
        if spent.refused {
            return Err(RunFailed);
        }
        Ok(Answer {
            reply: reply_of(&run).ok_or(RunFailed)?,
            cost_usd: spent.usd,
        })
    }
}

impl Drop for ScratchRunner {
    fn drop(&mut self) {
        remove_tree(&self.root);
    }
}

/// The reply of a finished `ask --json`: its `output`, when the job ran to
/// an exit 0.
fn reply_of(run: &ProbeRun) -> Option<String> {
    if run.failed.is_some() || run.code != Some(0) {
        return None;
    }
    let line = run.stdout.lines().rev().find(|l| !l.trim().is_empty())?;
    let v: Value = serde_json::from_str(line).ok()?;
    v["output"].as_str().map(str::to_string)
}

/// The view an item's job gets, from the daemon's own `base` view: no
/// workspace, `root` the one read-only addition, the relay's name the only
/// egress, and the floor still covering whatever it covers (never the runner's
/// own directory, which sits under the state dir).
pub fn runner_view(base: &L1, root: &Path, relay: &str) -> L1 {
    L1 {
        workspace: Vec::new(),
        ro_paths: vec![root.to_path_buf()],
        hidden: base
            .hidden
            .iter()
            .filter(|h| !root.starts_with(h))
            .cloned()
            .collect(),
        limits: base.limits,
        egress: vec![relay.to_string()],
        egress_dns: None,
        // An item's job keeps no write: only a self build's view binds
        // read-write (theseus-pw1q.21.1).
        rw: Vec::new(),
    }
}

/// The candidate's config: only what differs from the defaults and what must
/// be off. Written as plain keys so a build older or newer than this one reads
/// it.
pub fn scratch_config(
    name: &str,
    p: &ProfileConfig,
    api_base: &str,
    (loops, item_usd): (u32, f64),
) -> toml::Table {
    let tokens = p
        .max_output_tokens
        .map_or(MAX_OUTPUT_TOKENS, |t| t.min(MAX_OUTPUT_TOKENS));
    let mut profile = format!(
        "provider = \"anthropic\"\nmodel = {:?}\nmax_output_tokens = {tokens}\nmax_loops = {loops}\non_max_loops = \"end\"\n",
        p.model
    );
    if let Some(e) = p.effort {
        profile += &format!("effort = {:?}\n", format!("{e:?}").to_lowercase());
    }
    let text = format!(
        "[model]\nlive = {name:?}\nprovider = \"anthropic\"\nmodel = {:?}\napi_base = {api_base:?}\n\
         api_key_secret = \"sealed_relay\"\n[model.retries]\ntransient = 2\n\
         [profiles.{name}]\n{profile}\
         [secrets]\nsealed_relay = \"env:SEALED_RELAY_KEY\"\n\
         [server]\ndisk_warn_mb = 0\ndisk_floor_mb = 0\n\
         [kernel]\nspend_limit_usd = {item_usd}\nspend_limit_mode = \"ask\"\n\
         [tools]\nprojects_dir = \"/tmp\"\nroots = [\"/tmp\"]\napprove_paths = []\n\
         [policy]\nenforcement = \"open\"\nallow_argv = []\napprove_argv = []\n\
         private_addresses = \"open\"\n\
         [sandbox]\ndefault = \"l0\"\n\
         [discord]\nenabled = false\n[web]\nenabled = false\n[index]\nenabled = false\n\
         [judge]\nenabled = false\n[memory]\nmode = \"off\"\n",
        p.model
    );
    text.parse().expect("the scratch config is valid TOML")
}

/// A branch name as one path part.
fn slug(branch: &str) -> String {
    branch
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '-'
            }
        })
        .collect()
}

/// Copy the candidate's programs into `<root>/bin`, read-only, with their
/// digests.
fn install(root: &Path, from: &Path) -> Result<Vec<(String, String)>> {
    let bin = root.join("bin");
    std::fs::create_dir_all(&bin).with_context(|| format!("making {}", bin.display()))?;
    let mut digests = Vec::new();
    for name in BINARIES {
        let src = from.join(name);
        let bytes = std::fs::read(&src).with_context(|| format!("reading {}", src.display()))?;
        digests.push((name.to_string(), hex::encode(Sha256::digest(&bytes))));
        let to = bin.join(name);
        std::fs::write(&to, &bytes)?;
        std::fs::set_permissions(&to, std::fs::Permissions::from_mode(0o555))?;
    }
    Ok(digests)
}

fn write_read_only(path: &Path, bytes: &[u8]) -> Result<()> {
    let dir = path.parent().context("a file has a directory")?;
    std::fs::create_dir_all(dir)?;
    std::fs::write(path, bytes)?;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o444))?;
    Ok(())
}

/// Make the directory read-only, so nothing in it can change under a job.
fn seal(root: &Path) -> Result<()> {
    for d in ["bin", "cfg", ""] {
        let dir = root.join(d);
        if !dir.is_dir() {
            bail!("{} is missing", dir.display());
        }
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o555))?;
    }
    Ok(())
}

/// Remove a runner directory, read-only or not. Nothing is left, or the
/// reason is logged.
fn remove_tree(root: &Path) {
    for d in ["bin", "cfg", ""] {
        let _ = std::fs::set_permissions(root.join(d), std::fs::Permissions::from_mode(0o755));
    }
    if let Err(e) = std::fs::remove_dir_all(root) {
        if e.kind() != std::io::ErrorKind::NotFound {
            tracing::warn!(dir = %root.display(), "a sealed runner's directory was not removed: {e}");
        }
    }
}
