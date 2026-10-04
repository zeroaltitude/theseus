//! `theseus-sim bench lifecycle`: the lifecycle budgets of §9, measured on a
//! real `theseusd` over its real socket (FAST, §2; P5b; theseus-qa0).
//!
//! Six phases, each run `runs` times, with p50 and p95 per phase, and per
//! start-path phase and kernel step by the daemon's own clock (`health`):
//! - `cold`: process start to the first `health` answer;
//! - `vault`: the same, with the config an `op://` note, from its
//!   last-known-good copy (theseus-2fo): each first answer must come before
//!   the vault's, and say the config is `confirming`;
//! - `shutdown`: the `shutdown` request to process exit, with executions
//!   waiting and a job running (a real `proc.run`, started by a real turn
//!   against a stand-in for the Messages API, `fake_model`);
//! - `inflight`: the same, with a turn's reply post in flight to a stand-in
//!   Discord that holds its answer, so the stop waits out its grace
//!   (theseus-ndw);
//! - `kill`: SIGKILL, then a new process to its first `health` answer;
//! - `swap`: a binary upgrade under the same load (F4b). The `shutdown`
//!   request, its answer, and at once the other build on the same store, to
//!   that process's first answer. The job's wrapper runs through every swap,
//!   and the last new daemon ends it with its own cancel: it was adopted;
//! - `restore`: `theseusd restore` from a copy of the store's WAL into a fresh
//!   state dir, with the source's pages dropped from the cache first, beside
//!   a cold sequential read of the same bytes. Measured, with no budget yet;
//!   the last restored store must serve.
//!
//! The daemon's secrets come from a fake `op` that answers only after
//! `resolver_ms`, so a start that waited for them would show: at each first
//! answer, health must still say `resolving`.

use std::collections::BTreeMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use anyhow::{bail, Context, Result};
use serde::Serialize;
use serde_json::{json, Value};

use crate::fake_model::FakeModel;

pub const PHASES: [&str; 8] = [
    "cold", "vault", "shutdown", "inflight", "kill", "swap", "restore", "seed",
];

/// The bench's vault note: the fake `op` answers it with the bench config.
pub const VAULT_REF: &str = "op://Bench/theseus-config/notesPlain";

/// The job the shutdown and kill phases keep running.
const JOB: [&str; 2] = ["sleep", "300"];

/// Unmeasured starts before the first measured one.
const WARM_UPS: usize = 2;

pub struct Opts {
    pub theseusd: PathBuf,
    /// The build the swap phase alternates with `theseusd`: both must read
    /// the store (F4a or later). None: a copy of `theseusd`.
    pub swap_to: Option<PathBuf>,
    pub runs: usize,
    /// A synthetic store of this many parked sessions (0: an empty store).
    pub sessions: u64,
    /// Or a copy of this store directory.
    pub store: Option<PathBuf>,
    /// How long the fake `op` takes to answer.
    pub resolver_ms: u64,
    pub phases: Vec<String>,
    /// The noise allowed over every budget; `None`: each phase's measured one.
    pub margin_ms: Option<f64>,
    /// Work here and keep it; otherwise in a temporary directory.
    pub dir: Option<PathBuf>,
    /// A real config instead of the bench's (the live check: the operator's
    /// own, with Discord and the web UI off), with the real `op` under the
    /// token in `op_token_file`. Cold starts only: nothing fakes a model.
    pub config: Option<PathBuf>,
    pub op_token_file: Option<PathBuf>,
    /// After each first answer of the vault phase, wait for the vault to
    /// confirm the copy, and time it.
    pub confirm: bool,
}

// ------------------------------------------------------------------ arithmetic

/// Nearest rank: the smallest sample with at least `p` % of the samples at
/// or below it. `sorted` is ascending and not empty.
pub fn percentile(sorted: &[f64], p: f64) -> f64 {
    let rank = ((p / 100.0) * sorted.len() as f64).ceil() as usize;
    sorted[rank.clamp(1, sorted.len()) - 1]
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct Summary {
    pub n: usize,
    pub p50: f64,
    pub p95: f64,
    pub min: f64,
    pub max: f64,
}

impl Summary {
    pub fn of(samples: &[f64]) -> Option<Self> {
        if samples.is_empty() {
            return None;
        }
        let mut s = samples.to_vec();
        s.sort_by(f64::total_cmp);
        Some(Self {
            n: s.len(),
            p50: percentile(&s, 50.0),
            p95: percentile(&s, 95.0),
            min: s[0],
            max: s[s.len() - 1],
        })
    }
}

/// §9's budget for `phase`, in ms, on a store of `sessions` parked sessions.
/// Cold start is under 50 ms at today's sizes and under 250 ms at 10,000
/// sessions, read as a line between the two; clean shutdown is under 100 ms;
/// SIGKILL to serving is the cold budget plus 100 ms of tail replay; a binary
/// upgrade is under 200 ms without a protocol answer, at any size. Restore
/// has none yet: §9 asks for the disk's sequential read speed, "to measure".
pub fn budget_ms(phase: &str, sessions: u64) -> Option<f64> {
    let cold = 50.0 + 200.0 * sessions.min(10_000) as f64 / 10_000.0;
    match phase {
        // A start from the config copy is a cold start (theseus-2fo).
        "cold" | "vault" => Some(cold),
        "shutdown" => Some(100.0),
        "kill" => Some(cold + 100.0),
        "swap" => Some(200.0),
        _ => None,
    }
}

/// The noise allowed over each budget, in ms: the spread of the phase's p95
/// over five runs of the gate's own bench (debug binaries, an empty store,
/// ten runs a phase) on this machine, 2026-09-29, rounded up. Measured:
/// cold 23.3–30.1 (6.8), shutdown 47.3–51.3 (4.0), kill 43.2–67.7 (24.5,
/// one run whose redb repair after the SIGKILL took 50 ms instead of 26).
/// The swap, 2026-09-30 (F4b): 68.6–70.6 (1.9).
pub fn margin_ms(phase: &str) -> f64 {
    match phase {
        "cold" | "vault" => 7.0,
        "shutdown" => 4.0,
        "kill" => 25.0,
        "swap" => 2.0,
        _ => 0.0,
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Verdict {
    pub phase: String,
    pub p95: f64,
    pub budget: f64,
    pub margin: f64,
    pub ok: bool,
}

/// A phase misses when its p95 is past its budget plus the noise margin:
/// `margin`, or each phase's measured one (`margin_ms`).
pub fn verdicts(phases: &[(String, Summary)], sessions: u64, margin: Option<f64>) -> Vec<Verdict> {
    phases
        .iter()
        .filter_map(|(name, s)| {
            let budget = budget_ms(name, sessions)?;
            let margin = margin.unwrap_or_else(|| margin_ms(name));
            Some(Verdict {
                phase: name.clone(),
                p95: s.p95,
                budget,
                margin,
                ok: s.p95 <= budget + margin,
            })
        })
        .collect()
}

// ------------------------------------------------------------------ one start

/// One start: the bench's clock, and the daemon's own phases.
#[derive(Debug, Clone, Serialize)]
pub struct Start {
    /// What the start was: `cold`, `restart` (after a clean shutdown, a job
    /// running), or `kill` (after SIGKILL).
    pub after: String,
    /// Spawn to the first `health` answer, by the bench's clock.
    pub ms: f64,
    /// The start path's phases, by the daemon's clock, in ms, in order.
    pub phases: Vec<(String, f64)>,
    /// The kernel's startup steps, in ms.
    pub steps: Vec<(String, f64)>,
    /// When the last start-path phase ended, in ms after the process began.
    pub serving_ms: f64,
    /// The part of `serving_ms` no phase names: between phases, and before
    /// the first. A slow start that shows here has an unnamed cause.
    pub between_ms: f64,
    /// `secrets.state` in that first answer.
    pub secrets: String,
    /// `config.state` in that first answer (theseus-2fo).
    #[serde(default)]
    pub config: String,
    /// A vault start's first answer to the vault's confirmation, when timed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub confirmed_ms: Option<f64>,
    /// When the continuation driver started, in ms after the process began,
    /// by the daemon's clock (theseus-q4v): it waits for no binding, so this
    /// comes before the bot token resolves.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub driver_ms: Option<f64>,
    /// When the Discord binding's token resolved, in ms after the process
    /// began, by the daemon's clock (the end of its `discord.token` phase),
    /// once the binding bound its place at the fake Discord (theseus-l21m).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub token_ms: Option<f64>,
    /// How long the store's open waited for the last process to release it
    /// (the store phase's `lock_wait_ms`, F4b); builds before F4b say
    /// nothing.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lock_wait_ms: Option<f64>,
}

impl Start {
    pub fn from_health(ms: f64, h: &Value) -> Self {
        let (mut phases, mut steps, mut serving_us) = (Vec::new(), Vec::new(), 0u64);
        let mut lock_wait_ms = None;
        for p in h["startup"].as_array().into_iter().flatten() {
            if p["background"].as_bool().unwrap_or(false) {
                continue;
            }
            let (Some(s), Some(e)) = (p["start_us"].as_u64(), p["end_us"].as_u64()) else {
                continue;
            };
            serving_us = serving_us.max(e);
            let name = p["name"].as_str().unwrap_or("?").to_string();
            if name == "store" {
                lock_wait_ms = p["detail"]["lock_wait_ms"].as_f64();
            }
            if name == "kernel" {
                for st in p["detail"]["steps"].as_array().into_iter().flatten() {
                    steps.push((
                        st["name"].as_str().unwrap_or("?").to_string(),
                        st["us"].as_u64().unwrap_or(0) as f64 / 1000.0,
                    ));
                }
            }
            phases.push((name, e.saturating_sub(s) as f64 / 1000.0));
        }
        let named: f64 = phases.iter().map(|(_, ms)| ms).sum();
        let serving_ms = serving_us as f64 / 1000.0;
        Self {
            after: String::new(),
            ms,
            phases,
            steps,
            serving_ms,
            between_ms: (serving_ms - named).max(0.0),
            secrets: h["secrets"]["state"].as_str().unwrap_or("").to_string(),
            config: h["config"]["state"].as_str().unwrap_or("").to_string(),
            confirmed_ms: None,
            driver_ms: driver_ms(h),
            token_ms: None,
            lock_wait_ms,
        }
    }
}

/// The continuation driver's start, in ms after the process began: the end
/// of its phase in health's startup log (theseus-q4v).
fn driver_ms(h: &Value) -> Option<f64> {
    h["startup"]
        .as_array()?
        .iter()
        .find(|p| p["name"] == "driver")?["end_us"]
        .as_u64()
        .map(|us| us as f64 / 1000.0)
}

/// When the Discord binding's token resolved, in ms after the process began:
/// the end of its `discord.token` phase, when it ended with the token.
fn token_ms(h: &Value) -> Option<f64> {
    let p = h["startup"]
        .as_array()?
        .iter()
        .find(|p| p["name"] == "discord.token" && p["detail"]["outcome"] == "ready")?;
    p["end_us"].as_u64().map(|us| us as f64 / 1000.0)
}

/// The bench's config with its Discord REST and gateway on `fake`, the
/// in-process stand-in, instead of a port nothing listens on (theseus-l21m).
pub fn on_fake_discord(
    config: &str,
    fake: &theseus_sim::fake_discord::FakeDiscord,
) -> Result<String> {
    let mut t: toml::Table = config.parse()?;
    let d = t
        .get_mut("discord")
        .and_then(toml::Value::as_table_mut)
        .context("the bench config has a [discord] table")?;
    d.insert("rest_proxy".into(), fake.addr.clone().into());
    let gateway = fake
        .gateway()
        .context("the fake Discord serves a gateway")?
        .url();
    d.insert("gateway_proxy".into(), gateway.into());
    Ok(toml::to_string(&t)?)
}

// ------------------------------------------------------------------ the rig

/// Where the daemon's secrets come from.
pub(crate) enum Vault {
    /// The fake `op` in this directory, first on the daemon's PATH, and a
    /// token that is not one.
    Fake(PathBuf),
    /// The real `op`, under the service-account token in this file.
    Real(PathBuf),
}

/// A spawned `theseusd`, killed and reaped when dropped: a bench that fails
/// anywhere, with `?` or a panic, leaves no daemon running (theseus-hee).
pub(crate) struct Daemon(pub(crate) Child);

impl Daemon {
    /// SIGKILL, and reaped.
    fn kill(&mut self) -> Result<()> {
        self.0.kill()?;
        self.0.wait()?;
        Ok(())
    }
}

impl Drop for Daemon {
    fn drop(&mut self) {
        // A daemon already reaped is not signalled again: std keeps its status.
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

pub(crate) struct Rig {
    pub(crate) theseusd: PathBuf,
    /// A config file, or an `op://` note (the vault phase).
    pub(crate) config: PathBuf,
    pub(crate) state: PathBuf,
    pub(crate) sock: PathBuf,
    pub(crate) vault: Vault,
    pub(crate) log: PathBuf,
}

impl Rig {
    fn spawn(&self) -> Result<(Daemon, Instant)> {
        self.spawn_bin(&self.theseusd)
    }

    /// Start `bin`, another build, on the rig's store (the swap phase).
    fn spawn_bin(&self, bin: &Path) -> Result<(Daemon, Instant)> {
        let mut cmd = self.command(bin, &self.state)?;
        let t0 = Instant::now();
        let child = cmd
            .spawn()
            .with_context(|| format!("starting {}", bin.display()))?;
        Ok((Daemon(child), t0))
    }

    /// `bin` with the rig's config, socket, and vault, on `state`.
    fn command(&self, bin: &Path, state: &Path) -> Result<Command> {
        let log = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.log)?;
        let mut cmd = Command::new(bin);
        cmd.arg("--config")
            .arg(&self.config)
            .arg("--socket")
            .arg(&self.sock)
            .arg("--state-dir")
            .arg(state)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(log);
        match &self.vault {
            Vault::Fake(bin) => {
                let path = format!(
                    "{}:{}",
                    bin.display(),
                    std::env::var("PATH").unwrap_or_default()
                );
                cmd.env("PATH", path)
                    .env("OP_SERVICE_ACCOUNT_TOKEN", "bench-not-a-token")
                    .env_remove("THESEUS_OP_TOKEN_FILE");
            }
            Vault::Real(token_file) => {
                cmd.env_remove("OP_SERVICE_ACCOUNT_TOKEN")
                    .env("THESEUS_OP_TOKEN_FILE", token_file);
            }
        }
        Ok(cmd)
    }

    /// The same rig with its config an `op://` note (the vault phase).
    fn with_config(&self, config: PathBuf) -> Self {
        Self {
            config,
            ..self.clone_rig()
        }
    }

    /// The same rig on another state dir (a restored store).
    fn with_state(&self, state: PathBuf) -> Self {
        Self {
            state,
            ..self.clone_rig()
        }
    }

    fn clone_rig(&self) -> Self {
        Self {
            theseusd: self.theseusd.clone(),
            config: self.config.clone(),
            state: self.state.clone(),
            sock: self.sock.clone(),
            vault: match &self.vault {
                Vault::Fake(b) => Vault::Fake(b.clone()),
                Vault::Real(t) => Vault::Real(t.clone()),
            },
            log: self.log.clone(),
        }
    }

    /// A binary upgrade under load (F4b): the `shutdown` request and its
    /// answer, as `theseus shutdown` waits for it, then `bin` started at once
    /// on the same store, timed from the request to the new process's first
    /// `health` answer. The old process still holds the store for a while
    /// after its answer, and the new one waits for it (`lock_wait_ms`). An
    /// answer counts only from the new process: until the old one's socket is
    /// gone, a connection may still reach it. The old process must exit
    /// cleanly.
    fn swap(&self, old: &mut Daemon, bin: &Path) -> Result<(Daemon, Start)> {
        let s = UnixStream::connect(&self.sock).context("connecting to stop theseusd")?;
        let t0 = Instant::now();
        request(s, "shutdown", Value::Null)?;
        let (mut daemon, _) = self.spawn_bin(bin)?;
        let deadline = t0 + Duration::from_secs(30);
        let pid = daemon.0.id();
        let (ms, h) = loop {
            if let Ok(s) = UnixStream::connect(&self.sock) {
                if peer_pid(&s) == Some(pid) {
                    let h = request(s, "health", Value::Null)?;
                    break (t0.elapsed().as_secs_f64() * 1000.0, h);
                }
            }
            if let Some(status) = daemon.0.try_wait()? {
                bail!(
                    "{} exited ({status}) before answering after a swap; its log ends:\n{}",
                    bin.display(),
                    tail(&self.log)
                );
            }
            if Instant::now() > deadline {
                bail!(
                    "{} did not answer within 30 s of a swap; its log ends:\n{}",
                    bin.display(),
                    tail(&self.log)
                );
            }
            std::thread::sleep(Duration::from_micros(250));
        };
        let status = old.0.wait()?;
        if !status.success() {
            bail!("the stopped theseusd exited with {status} in a swap");
        }
        Ok((daemon, Start::from_health(ms, &h)))
    }

    /// The running job's wrapper, from the spool: its correlation id and pid.
    fn wrapper(&self) -> Result<(String, u32)> {
        let dir = self.state.join("spool").join("pids");
        for e in std::fs::read_dir(&dir).with_context(|| format!("reading {}", dir.display()))? {
            let e = e?;
            let job = e.file_name().to_string_lossy().into_owned();
            let Ok(pid) = std::fs::read_to_string(e.path())?.trim().parse::<u32>() else {
                continue;
            };
            if theseus_kernel::job::wrapper_alive(pid, &job) {
                return Ok((job, pid));
            }
        }
        bail!("no live job wrapper in {}", dir.display())
    }

    /// Ask `health` until the vault's read has confirmed the copy, at most 30 s:
    /// ms from `t0`.
    fn until_confirmed(&self, t0: Instant) -> Result<f64> {
        let deadline = Instant::now() + Duration::from_secs(30);
        loop {
            let h = self.call("health", Value::Null)?;
            match h["config"]["state"].as_str() {
                Some("confirmed") => return Ok(t0.elapsed().as_secs_f64() * 1000.0),
                Some("confirming") => {}
                other => bail!("the config did not confirm: {other:?}: {}", h["config"]),
            }
            if Instant::now() > deadline {
                bail!("the vault did not confirm the config within 30 s");
            }
            std::thread::sleep(Duration::from_millis(2));
        }
    }

    /// Until the first start has kept the config copy, at most 30 s.
    fn until_copy_kept(&self) -> Result<()> {
        let deadline = Instant::now() + Duration::from_secs(30);
        let copy = theseus_core::config_copy::path(Some(&self.state));
        while !copy.exists() {
            if Instant::now() > deadline {
                bail!("no config copy at {} within 30 s", copy.display());
            }
            std::thread::sleep(Duration::from_millis(2));
        }
        Ok(())
    }

    /// Start a daemon and time it to its first `health` answer.
    pub(crate) fn start(&self) -> Result<(Daemon, Start)> {
        let (mut daemon, t0) = self.spawn()?;
        let deadline = t0 + Duration::from_secs(30);
        loop {
            if let Ok(s) = UnixStream::connect(&self.sock) {
                let h = request(s, "health", Value::Null)?;
                let ms = t0.elapsed().as_secs_f64() * 1000.0;
                return Ok((daemon, Start::from_health(ms, &h)));
            }
            if let Some(status) = daemon.0.try_wait()? {
                bail!(
                    "theseusd exited ({status}) before answering; its log ends:\n{}",
                    tail(&self.log)
                );
            }
            if Instant::now() > deadline {
                bail!(
                    "theseusd did not answer within 30 s; its log ends:\n{}",
                    tail(&self.log)
                );
            }
            std::thread::sleep(Duration::from_micros(250));
        }
    }

    /// When the continuation driver started, by the daemon's clock, once
    /// health shows it (it starts once the socket answers).
    fn driver_started(&self) -> Result<Option<f64>> {
        let deadline = Instant::now() + Duration::from_secs(3);
        while Instant::now() < deadline {
            if let Some(ms) = driver_ms(&self.call("health", Value::Null)?) {
                return Ok(Some(ms));
            }
            std::thread::sleep(Duration::from_millis(2));
        }
        Ok(None)
    }

    /// Until the Discord binding has bound its DM at the fake Discord, at
    /// most `resolver_ms` and 30 s: when its token resolved, in ms after the
    /// process began, by the daemon's clock (theseus-l21m). A binding that
    /// never binds fails the bench: the driver check would mean nothing.
    fn binding_bound(&self, resolver_ms: u64) -> Result<f64> {
        let deadline = Instant::now() + Duration::from_millis(resolver_ms + 30_000);
        loop {
            let h = self.call("health", Value::Null)?;
            let b = &h["bindings"][0];
            let bound = b["places"][0]["session_id"]
                .as_str()
                .is_some_and(|s| !s.is_empty());
            if let (true, Some(ms)) = (bound, token_ms(&h)) {
                return Ok(ms);
            }
            if Instant::now() > deadline {
                bail!(
                    "the Discord binding did not bind at the fake Discord within {} s: {}; the \
                     daemon's log ends:\n{}",
                    (resolver_ms + 30_000) / 1000,
                    b,
                    tail(&self.log)
                );
            }
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    /// The `shutdown` request to process exit, in ms.
    pub(crate) fn stop(&self, daemon: &mut Daemon) -> Result<f64> {
        let s = UnixStream::connect(&self.sock).context("connecting to stop theseusd")?;
        let t0 = Instant::now();
        send(&s, "shutdown", Value::Null)?;
        let status = daemon.0.wait()?;
        let ms = t0.elapsed().as_secs_f64() * 1000.0;
        if !status.success() {
            bail!("theseusd exited with {status} on shutdown");
        }
        Ok(ms)
    }

    /// Stop a daemon cleanly if it can be; its guard kills it otherwise.
    fn stop_anyhow(&self, daemon: &mut Daemon) {
        let _ = self.stop(daemon);
    }

    pub(crate) fn call(&self, method: &str, params: Value) -> Result<Value> {
        request(UnixStream::connect(&self.sock)?, method, params)
    }

    /// Executions waiting and a job running: three sessions opened, and a
    /// turn whose model runs `JOB` through `proc.run` and carries on once
    /// `proc_sync_secs` passes. Returns the job's execution.
    fn park_and_start_job(&self) -> Result<String> {
        for i in 0..3 {
            self.call(
                "session.open",
                json!({"label": format!("bench waiting {i}")}),
            )?;
        }
        let r = self.call(
            "turn.submit",
            json!({"input": "Start the background job.", "author": "bench", "attachments": []}),
        )?;
        let exec = r["execution_id"]
            .as_str()
            .context("turn.submit gave no execution id")?
            .to_string();
        if self.running_jobs()? == 0 {
            bail!("the turn left no job running: {r}");
        }
        Ok(exec)
    }

    fn running_jobs(&self) -> Result<u64> {
        let h = self.call("health", Value::Null)?;
        Ok(h["kernel"]["actions_by_state"]["dispatched"]
            .as_u64()
            .unwrap_or(0))
    }
}

fn send(s: &UnixStream, method: &str, params: Value) -> Result<()> {
    let req = theseus_protocol::Request::new(theseus_protocol::Id::Num(1), method, params);
    let mut line = serde_json::to_string(&req)?;
    line.push('\n');
    let mut w = s;
    w.write_all(line.as_bytes())?;
    Ok(())
}

fn request(s: UnixStream, method: &str, params: Value) -> Result<Value> {
    s.set_read_timeout(Some(Duration::from_secs(120)))?;
    send(&s, method, params)?;
    let mut r = BufReader::new(&s);
    let mut line = String::new();
    loop {
        line.clear();
        if r.read_line(&mut line)? == 0 {
            bail!("theseusd closed the connection before answering {method}");
        }
        let v: Value = serde_json::from_str(&line)?;
        if v.get("id") == Some(&json!(1)) {
            if let Some(e) = v.get("error").filter(|e| !e.is_null()) {
                bail!("{method}: {e}");
            }
            return Ok(v["result"].clone());
        }
    }
}

pub(crate) fn tail(log: &Path) -> String {
    let s = std::fs::read_to_string(log).unwrap_or_default();
    let lines: Vec<&str> = s.lines().collect();
    lines[lines.len().saturating_sub(15)..].join("\n")
}

/// The pid of the process listening at the other end (`SO_PEERCRED`): which
/// daemon answered.
fn peer_pid(s: &UnixStream) -> Option<u32> {
    use std::os::fd::AsRawFd;
    let mut cred = libc::ucred {
        pid: 0,
        uid: 0,
        gid: 0,
    };
    let mut len = std::mem::size_of::<libc::ucred>() as libc::socklen_t;
    // SAFETY: a connected socket's fd, and a buffer of the size given.
    let r = unsafe {
        libc::getsockopt(
            s.as_raw_fd(),
            libc::SOL_SOCKET,
            libc::SO_PEERCRED,
            (&mut cred as *mut libc::ucred).cast(),
            &mut len,
        )
    };
    (r == 0 && cred.pid > 0).then_some(cred.pid as u32)
}

/// The WAL segments in `wal`, in order.
fn segments(wal: &Path) -> Result<Vec<PathBuf>> {
    let mut v: Vec<PathBuf> = std::fs::read_dir(wal)
        .with_context(|| format!("reading {}", wal.display()))?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|x| x == "seg"))
        .collect();
    v.sort();
    Ok(v)
}

/// Flush a file and drop its pages from the page cache, so that the next
/// read of it comes from the disk.
fn uncache(path: &Path) -> Result<()> {
    use std::os::fd::AsRawFd;
    let f = std::fs::File::open(path)?;
    f.sync_all()?;
    // SAFETY: an open file's fd; the advice changes no memory of ours.
    let r = unsafe { libc::posix_fadvise(f.as_raw_fd(), 0, 0, libc::POSIX_FADV_DONTNEED) };
    if r != 0 {
        bail!("posix_fadvise on {}: error {r}", path.display());
    }
    Ok(())
}

/// Every segment in `wal` read in order with its pages dropped first: the
/// disk's sequential read of these bytes. Bytes, and ms.
fn read_cold(wal: &Path) -> Result<(u64, f64)> {
    let segs = segments(wal)?;
    for s in &segs {
        uncache(s)?;
    }
    let mut buf = vec![0u8; 1 << 20];
    let mut bytes = 0u64;
    let t0 = Instant::now();
    for s in &segs {
        let mut f = std::fs::File::open(s)?;
        loop {
            let n = f.read(&mut buf)?;
            if n == 0 {
                break;
            }
            bytes += n as u64;
        }
    }
    Ok((bytes, t0.elapsed().as_secs_f64() * 1000.0))
}

// ------------------------------------------------------------------ setup

/// The fake `op`: every reference gets the same value, after `ms`, except
/// the bench's vault note (`VAULT_REF`), which is the file `note`.
pub(crate) fn fake_op(ms: u64, note: &Path) -> String {
    format!(
        "#!/bin/sh\n\
         # The lifecycle bench's stand-in for 1Password's op (theseus-sim):\n\
         # every reference gets one fixed value, after a delay, so a daemon\n\
         # that waited for its secrets or its config note before serving\n\
         # would show it.\n\
         sleep {}\n\
         case \"$1\" in\n\
         \x20 inject) sed -e 's/{{{{ [^}}]* }}}}/bench-secret-value-0000/g' ;;\n\
         \x20 read) if [ \"$3\" = '{VAULT_REF}' ]; then cat '{}'; else printf '%s' bench-secret-value-0000; fi ;;\n\
         \x20 *) echo \"fake op: $1 is not supported\" >&2; exit 1 ;;\n\
         esac\n",
        ms as f64 / 1000.0,
        note.display()
    )
}

/// The bench's bindings file: one DM, so the Discord binding starts and
/// waits for its token, which the fake op gives only after `resolver_ms`.
/// Its ids are invented ones of Discord's length (15 to 21 digits), which the
/// binding takes, and its guild is the fake Discord's (theseus-l21m): with
/// `"1"` and `"2"` the binding failed at load, waited for no token, and the
/// driver check passed whatever the driver did.
pub const BENCH_BINDINGS: &str =
    "guild_id = \"900000000000000001\"\n[[dm]]\nuser = \"100000000000000002\"\nname = \"bench\"\n";

/// A port nothing listens on: the bench's Discord REST and gateway.
const NOWHERE: &str = "127.0.0.1:9";

/// The template, with its endpoints on the fake model, its secrets on the
/// fake `op`, the web UI off, Discord's REST and gateway on a port nothing
/// listens on, and no GitHub token, so nothing leaves the machine.
pub fn bench_config(model: &str, state: &Path, sock: &Path, projects: &Path) -> Result<String> {
    let mut t: toml::Table = theseus_core::Config::EXAMPLE_TOML.parse()?;
    fn table<'a>(t: &'a mut toml::Table, key: &str) -> &'a mut toml::Table {
        t.entry(key)
            .or_insert_with(|| toml::Value::Table(Default::default()))
            .as_table_mut()
            .expect("a table")
    }
    table(&mut t, "model").insert("api_base".into(), model.into());
    for (_, p) in table(&mut t, "providers").iter_mut() {
        if let Some(p) = p.as_table_mut() {
            p.insert("api_base".into(), model.into());
        }
    }
    let secrets = table(&mut t, "secrets");
    let names: Vec<String> = secrets
        .keys()
        .filter(|k| k.as_str() != "github_token")
        .cloned()
        .collect();
    secrets.clear();
    for n in names {
        secrets.insert(n.clone(), format!("op://Bench/{n}/credential").into());
    }
    let server = table(&mut t, "server");
    server.insert("state_dir".into(), state.display().to_string().into());
    server.insert("socket".into(), sock.display().to_string().into());
    // The binding runs, as Eddie's does, and nothing it sends leaves the
    // machine: the continuation driver must not wait for it (theseus-q4v).
    let discord = table(&mut t, "discord");
    discord.insert("enabled".into(), true.into());
    discord.insert("rest_proxy".into(), NOWHERE.into());
    discord.insert("gateway_proxy".into(), format!("ws://{NOWHERE}").into());
    table(&mut t, "web").insert("enabled".into(), false.into());
    // The index tender runs, as Eddie's does (M6 §2.12: the bench runs with
    // it configured, and the start path must not move), on BM25 alone: the
    // model's files are never where it looks, so no bench loads 500 MB.
    table(&mut t, "index").insert(
        "weights_dir".into(),
        projects.join("no-models").display().to_string().into(),
    );
    // Recall runs in shadow (M6 step 30a), as it will on the operator's daemon: no
    // phase may move with it, and the turn bench counts its frames with it.
    table(&mut t, "memory").insert("mode".into(), "shadow".into());
    let tools = table(&mut t, "tools");
    tools.insert("projects_dir".into(), projects.display().to_string().into());
    tools.insert("proc_sync_secs".into(), 1.into());
    table(&mut t, "policy").insert("enforcement".into(), "open".into());
    // An AWS account bound, its endpoint on a port nothing listens on (row
    // 29, C1): every phase meets its budget with an account to check and AWS
    // out of reach, so nothing on the start path waits for AWS (§3.10).
    let account: toml::Table = toml::from_str(&format!(
        "region = \"us-west-2\"\nendpoint = \"http://{NOWHERE}\""
    ))?;
    let mut accounts = toml::Table::new();
    accounts.insert(BENCH_AWS_ACCOUNT.into(), account.into());
    table(&mut t, "aws").insert("accounts".into(), accounts.into());
    Ok(toml::to_string(&t)?)
}

/// The bench's AWS account: AWS's documentation's example id.
pub const BENCH_AWS_ACCOUNT: &str = "111122223333";

pub(crate) fn copy_dir(from: &Path, to: &Path) -> Result<()> {
    std::fs::create_dir_all(to)?;
    for e in std::fs::read_dir(from)? {
        let e = e?;
        let dest = to.join(e.file_name());
        if e.file_type()?.is_dir() {
            copy_dir(&e.path(), &dest)?;
        } else {
            std::fs::copy(e.path(), dest)?;
        }
    }
    Ok(())
}

// ------------------------------------------------------------------ the bench

#[derive(Debug, Serialize)]
pub struct Report {
    pub theseusd: String,
    pub store: String,
    pub sessions: u64,
    pub generated: Option<crate::synth::Generated>,
    pub runs: usize,
    pub resolver_ms: u64,
    pub phases: Vec<(String, Summary)>,
    /// Every start's own phases, by the daemon's clock.
    pub daemon: Vec<(String, Summary)>,
    pub kernel_steps: Vec<(String, Summary)>,
    /// `secrets.state` at each first answer, counted.
    pub secrets_at_first_answer: BTreeMap<String, usize>,
    pub verdicts: Vec<Verdict>,
    /// Whether every start answered before its secrets resolved.
    pub served_before_secrets: bool,
    /// `config.state` at each vault start's first answer, counted.
    pub config_at_first_answer: BTreeMap<String, usize>,
    /// Whether every vault start answered from its copy, before the vault.
    pub served_from_copy: bool,
    /// When each cold start's continuation driver started, by the daemon's
    /// clock (theseus-q4v).
    pub driver: Option<Summary>,
    /// Whether every cold start's driver started before the fake op could
    /// give the Discord binding its token: it waited for no binding.
    pub driver_before_token: bool,
    /// Spawn to the vault's confirmation, when timed (`confirm`).
    pub vault_confirmed: Option<Summary>,
    /// With the operator's note: the first start, which read the vault
    /// before serving because there was no copy yet.
    pub vault_first_ms: Option<f64>,
    /// The swap phase's job (F4b).
    pub swap_job: Option<SwapJob>,
    /// How long each swap's new process waited for the old one's store.
    pub swap_lock_wait: Option<Summary>,
    /// The restore phase (F4b), measured with no budget.
    pub restore: Option<RestoreRow>,
    pub samples: BTreeMap<String, Vec<f64>>,
    /// The first starts, unmeasured: the first creates an empty store, or
    /// reads a copied one into the page cache.
    pub warm_up_ms: Vec<f64>,
    /// Every start, in order, with the daemon's own phases.
    pub starts: Vec<Start>,
    pub wall_ms: f64,
}

impl Report {
    /// The strict verdict: every budget met within its limit, and every other
    /// check of the run held.
    pub fn ok(&self) -> bool {
        self.ok_with(0)
    }

    /// The verdict with the busy allowance `pct` on the timing budgets
    /// (theseus-lew7; `history::allowed`). The run's other checks are never
    /// excused.
    pub fn ok_with(&self, pct: u32) -> bool {
        self.verdicts
            .iter()
            .all(|v| crate::history::allowed(v, pct))
            && self.served_before_secrets
            && self.served_from_copy
            && self.driver_before_token
            && self.swap_job.as_ref().is_none_or(|j| j.kept && j.adopted)
            && self.restore.as_ref().is_none_or(|r| r.serves)
    }
}

/// The job the swap phase keeps running (F4b).
#[derive(Debug, Clone, Serialize)]
pub struct SwapJob {
    /// The builds swapped between, in turn.
    pub builds: [String; 2],
    pub wrapper_pid: u32,
    /// The wrapper was alive after every swap, and each new daemon counted
    /// its job as running.
    pub kept: bool,
    /// The last new daemon's cancel ended the wrapper that an earlier
    /// process started: it had adopted the job.
    pub adopted: bool,
}

/// The restore phase (F4b): `theseusd restore` against a cold sequential
/// read of the same WAL.
#[derive(Debug, Clone, Serialize)]
pub struct RestoreRow {
    pub segments: usize,
    pub wal_bytes: u64,
    /// The segments read in order, their pages dropped first, in ms.
    pub read_ms: f64,
    pub read_mb_s: f64,
    /// The sessions the last restore counted, and those its store served.
    pub sessions_restored: u64,
    pub sessions_served: u64,
    /// The last restored store's first `health` answer, in ms.
    pub served_ms: f64,
    /// The restored store served every session the restore counted.
    pub serves: bool,
    /// Each of the restore's own phases, p50 over the runs, in ms, as
    /// `theseusd restore` reports them (theseus-byu); none from a build
    /// before it.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub phases: Vec<(String, f64)>,
}

/// The phases `theseusd restore` reports: "phases, ms: copy 12.3 · open 210.0 · …".
fn restore_phases(said: &str) -> Vec<(String, f64)> {
    said.lines()
        .find_map(|l| l.strip_prefix("phases, ms: "))
        .map(|rest| {
            rest.split(" · ")
                .filter_map(|p| {
                    let (name, ms) = p.rsplit_once(' ')?;
                    Some((name.to_string(), ms.parse().ok()?))
                })
                .collect()
        })
        .unwrap_or_default()
}

#[expect(clippy::cognitive_complexity, reason = "shape budget: split it")]
#[expect(clippy::too_many_lines, reason = "shape budget: split it")]
pub fn run(o: &Opts) -> Result<Report> {
    let wall = Instant::now();
    let tmp = tempfile::tempdir()?;
    let work = match &o.dir {
        Some(d) => {
            std::fs::create_dir_all(d)?;
            d.clone()
        }
        None => tmp.path().to_path_buf(),
    };
    let state = work.join("state");
    let projects = work.join("projects");
    let fake_bin = work.join("bin");
    for d in [&state, &projects, &fake_bin] {
        std::fs::create_dir_all(d)?;
    }
    let (label, generated) = match (&o.store, o.sessions) {
        (Some(src), _) => {
            copy_dir(src, &state.join("store"))?;
            (format!("a copy of {}", src.display()), None)
        }
        (None, 0) => ("empty".to_string(), None),
        (None, n) => {
            let g = crate::synth::generate(&state.join("store"), n)?;
            (format!("synthetic, {n} parked sessions"), Some(g))
        }
    };
    let sock = work.join("sock");
    // The binding's Discord, REST and gateway, in this process (theseus-l21m):
    // the binding binds its DM there once its token resolves, and nothing
    // leaves the machine. Only the bench's own config uses it.
    let discord = o
        .config
        .is_none()
        .then(theseus_sim::fake_discord::FakeDiscord::start_with_gateway);
    // With the operator's own `op://` note, only the vault phase runs.
    let real_note = o
        .config
        .as_ref()
        .filter(|c| c.to_string_lossy().starts_with("op://"))
        .cloned();
    let (config, vault) = match &o.config {
        Some(c) => {
            let only = if real_note.is_some() { "vault" } else { "cold" };
            if o.phases.iter().any(|p| p != only) {
                bail!(
                    "with this --config only the {only} phase runs: the others need the fake \
                     model, and the vault phase an op:// note"
                );
            }
            let token = o
                .op_token_file
                .clone()
                .context("--config needs --op-token-file: it runs the real op")?;
            (c.clone(), Vault::Real(token))
        }
        None => {
            let model = FakeModel::start(JOB.iter().map(|s| s.to_string()).collect())?;
            let config = work.join("config.toml");
            let text = on_fake_discord(
                &bench_config(&model.base(), &state, &sock, &projects)?,
                discord.as_ref().expect("started with the fake model"),
            )?;
            std::fs::write(&config, &text)?;
            std::fs::write(state.join("bindings.toml"), BENCH_BINDINGS)?;
            // The vault phase starts from the note's copy, as a daemon that
            // has run before does, its digest in the store (theseus-zmgb);
            // the fake op answers the note with the same text after
            // `resolver_ms`.
            theseus_core::config_copy::keep(
                &theseus_core::store::Store::open(&state.join("store"))?,
                &theseus_core::config_copy::path(Some(&state)),
                VAULT_REF,
                &text,
            )?;
            let op = fake_bin.join("op");
            std::fs::write(&op, fake_op(o.resolver_ms, &config))?;
            {
                use std::os::unix::fs::PermissionsExt;
                std::fs::set_permissions(&op, std::fs::Permissions::from_mode(0o755))?;
            }
            (config, Vault::Fake(fake_bin))
        }
    };
    let resolver_ms = match vault {
        Vault::Fake(_) => o.resolver_ms,
        Vault::Real(_) => 0,
    };
    let rig = Rig {
        theseusd: o.theseusd.clone(),
        config,
        state,
        sock,
        vault,
        log: work.join("theseusd.log"),
    };
    let want = |p: &str| o.phases.iter().any(|x| x == p);
    let mut samples: BTreeMap<String, Vec<f64>> = BTreeMap::new();
    let mut starts: Vec<Start> = Vec::new();

    // Two starts first, unmeasured: on an empty directory the first creates
    // the store, and on a copy it reads the files into the page cache; the
    // second pays for the disk still flushing after that (store and kernel
    // at three times their usual, measured). Neither is the cold start of an
    // existing store that §9 budgets. With the operator's note, the vault
    // phase makes its own.
    for _ in 0..if real_note.is_some() { 0 } else { WARM_UPS } {
        let (mut child, mut s) = rig.start()?;
        s.after = "warm-up".into();
        starts.push(s);
        rig.stop(&mut child)?;
    }
    if want("cold") {
        for _ in 0..o.runs {
            let (mut child, mut s) = rig.start()?;
            samples.entry("cold".into()).or_default().push(s.ms);
            s.after = "cold".into();
            if s.driver_ms.is_none() {
                s.driver_ms = rig.driver_started()?;
            }
            if discord.is_some() {
                s.token_ms = Some(rig.binding_bound(resolver_ms)?);
            }
            // The push's seed (theseus-in3): the first `executions.watch`
            // reads every execution and action into the board, after serving.
            // Measured, with no budget yet; `--sessions 10000` is its row.
            if want("seed") {
                let t = Instant::now();
                rig.call("executions.watch", json!({}))?;
                samples
                    .entry("seed".into())
                    .or_default()
                    .push(t.elapsed().as_secs_f64() * 1000.0);
            }
            starts.push(s);
            rig.stop(&mut child)?;
        }
    }
    // From the copy of an `op://` note (theseus-2fo): each first answer must
    // come before the vault's, and say `confirming`. With the operator's
    // note, one start first reads the vault (no copy yet) and keeps the copy,
    // and one more pages it in; neither is measured.
    let mut vault_first_ms = None;
    if want("vault") {
        let vr = rig.with_config(real_note.clone().unwrap_or_else(|| VAULT_REF.into()));
        if real_note.is_some() {
            let _ = std::fs::remove_file(theseus_core::config_copy::path(Some(&vr.state)));
            let (mut child, mut s) = vr.start()?;
            s.after = "first".into();
            vault_first_ms = Some(s.ms);
            starts.push(s);
            vr.until_copy_kept()?;
            vr.stop(&mut child)?;
            let (mut child, mut s) = vr.start()?;
            s.after = "warm-up".into();
            starts.push(s);
            vr.stop(&mut child)?;
        }
        for _ in 0..o.runs {
            let (mut child, mut s) = vr.start()?;
            samples.entry("vault".into()).or_default().push(s.ms);
            s.after = "vault".into();
            if o.confirm {
                let t = Instant::now();
                let waited = vr.until_confirmed(t)?;
                s.confirmed_ms = Some(s.ms + waited);
            }
            starts.push(s);
            vr.stop(&mut child)?;
        }
    }
    let mut sessions = generated.as_ref().map_or(0, |g| g.sessions);
    // The swap's other build: a copy of this one unless given.
    let swap_to = match (&o.swap_to, want("swap")) {
        (Some(p), _) => p.clone(),
        (None, true) => {
            let copy = work.join("swap").join("theseusd");
            std::fs::create_dir_all(work.join("swap"))?;
            std::fs::copy(&o.theseusd, &copy)
                .with_context(|| format!("copying {} for the swap", o.theseusd.display()))?;
            // A daemon runs the index tender beside its own binary (row 51):
            // the copy gets one too, as an install does.
            let tender = o.theseusd.with_file_name("theseus-index");
            if tender.is_file() {
                std::fs::copy(&tender, copy.with_file_name("theseus-index"))
                    .with_context(|| format!("copying {} for the swap", tender.display()))?;
            }
            copy
        }
        (None, false) => o.theseusd.clone(),
    };
    let mut swap_job = None;
    if want("shutdown") || want("kill") || want("swap") {
        let (mut child, mut s) = rig.start()?;
        s.after = "cold".into();
        if s.driver_ms.is_none() {
            s.driver_ms = rig.driver_started()?;
        }
        if discord.is_some() {
            s.token_ms = Some(rig.binding_bound(resolver_ms)?);
        }
        starts.push(s);
        let mut job: Option<String> = None;
        let measured = (|| -> Result<()> {
            job = Some(rig.park_and_start_job()?);
            if want("shutdown") {
                for _ in 0..o.runs {
                    let ms = rig.stop(&mut child)?;
                    samples.entry("shutdown".into()).or_default().push(ms);
                    let (c, mut s) = rig.start()?;
                    child = c;
                    s.after = "restart".into();
                    starts.push(s);
                    if rig.running_jobs()? == 0 {
                        bail!("the job stopped running across a restart");
                    }
                }
            }
            if want("kill") {
                for _ in 0..o.runs {
                    child.kill()?;
                    let (c, mut s) = rig.start()?;
                    child = c;
                    samples.entry("kill".into()).or_default().push(s.ms);
                    s.after = "kill".into();
                    starts.push(s);
                }
            }
            sessions = rig.call("health", Value::Null)?["sessions"]
                .as_u64()
                .unwrap_or(0);
            if want("swap") {
                // Alternately the other build and this one, each started
                // at once on the store the last one stopped.
                let builds = [swap_to.clone(), rig.theseusd.clone()];
                let (corr, pid) = rig.wrapper()?;
                let mut kept = true;
                for i in 0..o.runs {
                    let (c, mut s) = rig.swap(&mut child, &builds[i % 2])?;
                    child = c;
                    samples.entry("swap".into()).or_default().push(s.ms);
                    s.after = "swap".into();
                    starts.push(s);
                    kept &= theseus_kernel::job::wrapper_alive(pid, &corr);
                    kept &= rig.running_jobs()? > 0;
                }
                // Adopted: the daemon serving now ends, with its own cancel,
                // the wrapper an earlier process started.
                if let Some(exec) = job.take() {
                    rig.call("execution.cancel", json!({"execution_id": exec}))?;
                }
                swap_job = Some(SwapJob {
                    builds: builds.map(|b| b.display().to_string()),
                    wrapper_pid: pid,
                    kept,
                    adopted: !theseus_kernel::job::wrapper_alive(pid, &corr),
                });
            }
            Ok(())
        })();
        // Whatever happened: the job cancelled, the daemon stopped.
        if let Some(job) = &job {
            if let Err(e) = rig.call("execution.cancel", json!({"execution_id": job})) {
                eprintln!("could not cancel the bench job's execution: {e:#}");
            }
        }
        rig.stop_anyhow(&mut child);
        measured?;
    }
    if want("inflight") {
        inflight_phase(o, &work, &mut samples)?;
    }
    let restore = if want("restore") {
        Some(restore_phase(&rig, &work, o.runs, &mut samples)?)
    } else {
        None
    };

    let phases: Vec<(String, Summary)> = PHASES
        .iter()
        .filter_map(|p| Some((p.to_string(), Summary::of(samples.get(*p)?)?)))
        .collect();
    let mut by_name: BTreeMap<String, Vec<f64>> = BTreeMap::new();
    let mut order: Vec<String> = Vec::new();
    let mut step_by: BTreeMap<String, Vec<f64>> = BTreeMap::new();
    let mut step_order: Vec<String> = Vec::new();
    let mut secrets: BTreeMap<String, usize> = BTreeMap::new();
    for s in starts
        .iter()
        .filter(|s| s.after != "warm-up" && s.after != "first")
    {
        let totals = [
            ("between".to_string(), s.between_ms),
            ("serving".to_string(), s.serving_ms),
        ];
        for (n, ms) in s.phases.iter().chain(totals.iter()) {
            if !order.contains(n) {
                order.push(n.clone());
            }
            by_name.entry(n.clone()).or_default().push(*ms);
        }
        for (n, ms) in &s.steps {
            if !step_order.contains(n) {
                step_order.push(n.clone());
            }
            step_by.entry(n.clone()).or_default().push(*ms);
        }
        *secrets.entry(s.secrets.clone()).or_default() += 1;
    }
    let summarize = |order: &[String], by: &BTreeMap<String, Vec<f64>>| {
        order
            .iter()
            .filter_map(|n| Some((n.clone(), Summary::of(by.get(n)?)?)))
            .collect::<Vec<_>>()
    };
    // A first answer that came before the resolver could have answered
    // must say `resolving`: `ready` there means the socket waited.
    let served_before_secrets = starts
        .iter()
        .filter(|s| s.after != "first")
        .all(|s| s.secrets == "resolving" || s.ms >= resolver_ms as f64);
    // A vault start that answered from its copy says `confirming`:
    // `confirmed` there means the socket waited for the vault.
    let vault_starts: Vec<&Start> = starts.iter().filter(|s| s.after == "vault").collect();
    let served_from_copy = vault_starts.iter().all(|s| s.config == "confirming");
    let mut config_at_first_answer: BTreeMap<String, usize> = BTreeMap::new();
    for s in &vault_starts {
        *config_at_first_answer.entry(s.config.clone()).or_default() += 1;
    }
    let confirmed: Vec<f64> = vault_starts.iter().filter_map(|s| s.confirmed_ms).collect();
    let warm_up_ms: Vec<f64> = starts
        .iter()
        .filter(|s| s.after == "warm-up")
        .map(|s| s.ms)
        .collect();
    // No driver wait at startup (theseus-q4v): the bench's Discord binding
    // gets its token from the fake op only after `resolver_ms`, so a driver
    // that waited for the binding could not start before then. Each measured
    // cold start's binding bound at the fake Discord, its token resolved no
    // sooner than `resolver_ms` (it waited for it), and the driver started
    // before the token did (theseus-l21m): a binding that failed at load
    // waited for nothing, and the check meant nothing.
    let cold: Vec<&Start> = starts.iter().filter(|s| s.after == "cold").collect();
    let driver: Vec<f64> = cold.iter().filter_map(|s| s.driver_ms).collect();
    let driver_before_token = resolver_ms == 0
        || !want("cold")
        || cold.iter().all(|s| {
            let driver = s.driver_ms.is_some_and(|ms| ms < resolver_ms as f64);
            let token = match (discord.is_some(), s.token_ms) {
                (false, _) => true,
                (true, Some(t)) => t >= resolver_ms as f64 && s.driver_ms.is_some_and(|d| d < t),
                (true, None) => false,
            };
            driver && token
        });
    let verdicts = verdicts(&phases, sessions, o.margin_ms);
    let lock_waits: Vec<f64> = starts
        .iter()
        .filter(|s| s.after == "swap")
        .filter_map(|s| s.lock_wait_ms)
        .collect();
    drop(tmp);
    Ok(Report {
        theseusd: o.theseusd.display().to_string(),
        store: label,
        sessions,
        generated,
        runs: o.runs,
        resolver_ms,
        phases,
        daemon: summarize(&order, &by_name),
        kernel_steps: summarize(&step_order, &step_by),
        secrets_at_first_answer: secrets,
        verdicts,
        served_before_secrets,
        config_at_first_answer,
        served_from_copy,
        driver: Summary::of(&driver),
        driver_before_token,
        vault_confirmed: Summary::of(&confirmed),
        vault_first_ms,
        swap_job,
        swap_lock_wait: Summary::of(&lock_waits),
        restore,
        samples,
        warm_up_ms,
        starts,
        wall_ms: wall.elapsed().as_secs_f64() * 1000.0,
    })
}

/// The reply post the `inflight` phase holds at the fake Discord: its footer,
/// which only the post writes (the stream's text never has it).
const FOOTER: &str = "\n-# ";

/// The `inflight` phase's bindings: one DM, with invented ids the binding
/// takes as Discord's (15 to 21 digits), so its place binds at the fake.
const INFLIGHT_BINDINGS: &str =
    "guild_id = \"100000000000000001\"\n[[dm]]\nuser = \"100000000000000002\"\nname = \"bench\"\n";

/// The `inflight` phase (theseus-ndw): a clean stop with a reply's post in
/// flight. The shutdown phase never has one: its Discord REST is a port
/// nothing answers, so its binding never posts. Here a rig of its own binds a
/// DM to the in-process fake Discord, a turn's reply is posted, and the fake
/// holds the post's answer (`hold_writes_containing`) while the daemon
/// stops, as a slow Discord would: the stop waits for it up to its grace
/// (`[server] stop_grace_ms`, 50 ms), and §9's clean-shutdown row holds the
/// grace inside its 100 ms. The fake op answers at once, so the binding has
/// its token at the start, and the turn's job is `true`, so the reply comes
/// in the same turn. Each start first lets the last stop's held post go out
/// again (it stayed dispatched), so every stop has exactly one in flight.
fn inflight_phase(o: &Opts, work: &Path, samples: &mut BTreeMap<String, Vec<f64>>) -> Result<()> {
    let dir = work.join("inflight");
    let _ = std::fs::remove_dir_all(&dir);
    let (state, projects, bin) = (dir.join("state"), dir.join("projects"), dir.join("bin"));
    for d in [&state, &projects, &bin] {
        std::fs::create_dir_all(d)?;
    }
    let sock = dir.join("sock");
    let model = FakeModel::start(vec!["true".into()])?;
    let fake = theseus_sim::fake_discord::FakeDiscord::start();
    let mut t: toml::Table = bench_config(&model.base(), &state, &sock, &projects)?.parse()?;
    if let Some(d) = t.get_mut("discord").and_then(toml::Value::as_table_mut) {
        d.insert("rest_proxy".into(), fake.addr.clone().into());
    }
    let config = dir.join("config.toml");
    std::fs::write(&config, toml::to_string(&t)?)?;
    std::fs::write(state.join("bindings.toml"), INFLIGHT_BINDINGS)?;
    let op = bin.join("op");
    std::fs::write(&op, fake_op(0, &config))?;
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&op, std::fs::Permissions::from_mode(0o755))?;
    }
    let rig = Rig {
        theseusd: o.theseusd.clone(),
        config,
        state,
        sock,
        vault: Vault::Fake(bin),
        log: dir.join("theseusd.log"),
    };
    let until = |what: &str, f: &mut dyn FnMut() -> Result<bool>| -> Result<()> {
        let deadline = Instant::now() + Duration::from_secs(30);
        while !f()? {
            if Instant::now() > deadline {
                bail!(
                    "inflight: no {what} within 30 s; the daemon's log ends:\n{}",
                    tail(&rig.log)
                );
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        Ok(())
    };
    let held = || fake.seen().iter().filter(|s| s.outcome == "held").count();
    // One run unmeasured: the store is new, and the bind notice goes first.
    for run in 0..=o.runs {
        let (mut child, _) = rig.start()?;
        let mut session = String::new();
        until("bound DM place, its outbox idle", &mut || {
            let h = rig.call("health", Value::Null)?;
            let b = &h["bindings"][0];
            session = b["places"][0]["session_id"]
                .as_str()
                .unwrap_or("")
                .to_string();
            Ok(!session.is_empty()
                && b["outbox"]["pending"] == 0
                && b["outbox"]["sent"].as_u64() >= Some(1))
        })?;
        let before = held();
        fake.hold_writes_containing(Some(FOOTER));
        rig.call(
            "turn.submit",
            json!({"session_id": session, "input": "post it", "author": "bench", "attachments": []}),
        )?;
        until("held post", &mut || Ok(held() > before))?;
        let ms = rig.stop(&mut child)?;
        fake.hold_writes_containing(None);
        if run > 0 {
            samples.entry("inflight".into()).or_default().push(ms);
        }
    }
    Ok(())
}

/// The restore phase (F4b), with the daemon stopped: `runs` restores of a
/// copy of the store's WAL (as a backup holds it), each into a fresh state
/// dir, with the source's pages dropped from the cache first, so each reads
/// it from the disk. Beside them, one cold sequential read of the same
/// segments: §9 asks for a restore at the disk's sequential read speed.
/// Then the last restored store serves, and must hold every session the
/// restore counted.
fn restore_phase(
    rig: &Rig,
    work: &Path,
    runs: usize,
    samples: &mut BTreeMap<String, Vec<f64>>,
) -> Result<RestoreRow> {
    let src = work.join("restore-from");
    let _ = std::fs::remove_dir_all(&src);
    copy_dir(&rig.state.join("store").join("wal"), &src.join("wal"))?;
    let blobs = rig.state.join("store").join("blobs");
    if blobs.is_dir() {
        copy_dir(&blobs, &src.join("blobs"))?;
    }
    let wal = src.join("wal");
    let (wal_bytes, read_ms) = read_cold(&wal)?;
    let nsegs = segments(&wal)?.len();
    let mut last: Option<(PathBuf, String)> = None;
    // Each of the restore's own phases, over the runs (theseus-byu).
    let mut by_phase: Vec<(String, Vec<f64>)> = Vec::new();
    for i in 0..runs {
        let into = work.join(format!("restored-{i}"));
        let _ = std::fs::remove_dir_all(&into);
        for s in segments(&wal)? {
            uncache(&s)?;
        }
        let mut cmd = rig.command(&rig.theseusd, &into)?;
        cmd.arg("restore")
            .arg("--from")
            .arg(&src)
            .stdout(Stdio::piped());
        let t0 = Instant::now();
        let out = cmd.output().context("running theseusd restore")?;
        let ms = t0.elapsed().as_secs_f64() * 1000.0;
        if !out.status.success() {
            bail!(
                "theseusd restore exited with {}; its log ends:\n{}",
                out.status,
                tail(&rig.log)
            );
        }
        samples.entry("restore".into()).or_default().push(ms);
        let said = String::from_utf8_lossy(&out.stdout).into_owned();
        for (name, ms) in restore_phases(&said) {
            match by_phase.iter_mut().find(|(n, _)| *n == name) {
                Some((_, v)) => v.push(ms),
                None => by_phase.push((name, vec![ms])),
            }
        }
        if let Some((prev, _)) = last.replace((into, said)) {
            let _ = std::fs::remove_dir_all(prev);
        }
    }
    let (restored, said) = last.context("no restore ran")?;
    // "… frames …\n5 session(s), 105 node(s), 806 ledger row(s)\n…"
    let sessions_restored = said
        .lines()
        .find(|l| l.contains(" session(s), "))
        .and_then(|l| l.split(' ').next()?.parse().ok())
        .with_context(|| format!("no session count in the restore's report: {said}"))?;
    let served = rig.with_state(restored.clone());
    let (mut daemon, start) = served.start()?;
    let sessions_served = served.call("health", Value::Null)?["sessions"]
        .as_u64()
        .unwrap_or(0);
    served.stop(&mut daemon)?;
    let _ = std::fs::remove_dir_all(&restored);
    let _ = std::fs::remove_dir_all(&src);
    Ok(RestoreRow {
        segments: nsegs,
        wal_bytes,
        read_ms,
        read_mb_s: wal_bytes as f64 / 1e6 / (read_ms / 1000.0).max(1e-9),
        sessions_restored,
        sessions_served,
        served_ms: start.ms,
        serves: sessions_served == sessions_restored,
        phases: by_phase
            .into_iter()
            .filter_map(|(name, v)| Some((name, Summary::of(&v)?.p50)))
            .collect(),
    })
}

const TITLES: [(&str, &str); 8] = [
    ("cold", "cold start to the first health answer"),
    (
        "vault",
        "cold start from the config copy to the first answer",
    ),
    (
        "shutdown",
        "clean shutdown, executions waiting and a job running",
    ),
    (
        "inflight",
        "clean shutdown, a reply's post in flight to Discord",
    ),
    ("kill", "SIGKILL, then restart to the first health answer"),
    (
        "swap",
        "binary swap, stop's request to the new build's answer",
    ),
    ("restore", "theseusd restore from a local WAL, cold"),
    ("seed", "the push's seed: the first executions.watch"),
];

#[expect(clippy::too_many_lines, reason = "shape budget: split it")]
pub fn print(r: &Report) {
    let vault = match r.resolver_ms {
        0 => "the real op".to_string(),
        ms => format!("op answers after {ms} ms"),
    };
    println!(
        "lifecycle bench · {} · store: {} · {} runs per phase · {vault}",
        r.theseusd, r.store, r.runs
    );
    if let Some(g) = &r.generated {
        println!(
            "  synthetic store: {} sessions, {} records in {} frames, {:.1} MB of WAL, written in {:.0} ms",
            g.sessions,
            g.records,
            g.frames,
            g.wal_bytes as f64 / 1e6,
            g.ms
        );
    }
    println!(
        "  the first {} starts, not measured (the first creates an empty store, or pages in a copy): {} ms",
        r.warm_up_ms.len(),
        r.warm_up_ms
            .iter()
            .map(|ms| format!("{ms:.1}"))
            .collect::<Vec<_>>()
            .join(", ")
    );
    for (name, s) in &r.phases {
        let title = TITLES
            .iter()
            .find(|(n, _)| n == name)
            .map_or(name.as_str(), |(_, t)| t);
        let verdict = r.verdicts.iter().find(|v| &v.phase == name);
        println!(
            "  {title:<56} p50 {:>7.1} ms  p95 {:>7.1} ms  (min {:.1}, max {:.1})  {}",
            s.p50,
            s.p95,
            s.min,
            s.max,
            verdict.map_or("measured; no budget yet".to_string(), |v| format!(
                "budget {:.0} ms + {:.0} ms margin: {}",
                v.budget,
                v.margin,
                if v.ok { "ok" } else { "MISSED" }
            ))
        );
    }
    if let Some(j) = &r.swap_job {
        let wait = r.swap_lock_wait.map_or("not reported".to_string(), |w| {
            format!(
                "p50 {:.1} ms, p95 {:.1} ms (min {:.1}, max {:.1})",
                w.p50, w.p95, w.min, w.max
            )
        });
        println!("  swap: each new process waited for the stopped one's store: {wait}");
        println!(
            "  swap: the job's wrapper (pid {}) {}, and {}",
            j.wrapper_pid,
            if j.kept {
                "ran through every swap"
            } else {
                "was LOST in a swap"
            },
            if j.adopted {
                "the last new daemon ended it with its own cancel: adopted"
            } else {
                "the last new daemon's cancel did NOT end it: not adopted"
            }
        );
    }
    if let Some(x) = &r.restore {
        let p50 = r
            .phases
            .iter()
            .find(|(n, _)| n == "restore")
            .map_or(0.0, |(_, s)| s.p50);
        println!(
            "  restore: {} segment(s), {:.2} MB of WAL; a cold sequential read of it took {:.1} ms ({:.0} MB/s), and a restore's p50 is {:.1}x that",
            x.segments,
            x.wal_bytes as f64 / 1e6,
            x.read_ms,
            x.read_mb_s,
            p50 / x.read_ms.max(1e-9)
        );
        println!(
            "  restore: the restored store served in {:.1} ms, with {} of the {} session(s) the restore counted{}",
            x.served_ms,
            x.sessions_served,
            x.sessions_restored,
            if x.serves { "" } else { ": SESSIONS MISSING" }
        );
        if !x.phases.is_empty() {
            let phases: Vec<String> = x
                .phases
                .iter()
                .map(|(name, ms)| format!("{name} {ms:.1}"))
                .collect();
            println!("  restore's own phases, p50 ms: {}", phases.join(" · "));
        }
    }
    let answers: Vec<String> = r
        .secrets_at_first_answer
        .iter()
        .map(|(s, n)| format!("{s} {n}"))
        .collect();
    println!(
        "  secrets at each first answer: {} ({})",
        answers.join(", "),
        if r.served_before_secrets {
            "the socket did not wait for them"
        } else {
            "a start WAITED for its secrets"
        }
    );
    if !r.config_at_first_answer.is_empty() {
        let answers: Vec<String> = r
            .config_at_first_answer
            .iter()
            .map(|(s, n)| format!("{s} {n}"))
            .collect();
        println!(
            "  config at each vault start's first answer: {} ({})",
            answers.join(", "),
            if r.served_from_copy {
                "served from the copy before the vault answered"
            } else {
                "a start WAITED for the vault"
            }
        );
    }
    if let Some(d) = &r.driver {
        println!(
            "  continuation driver started, ms after the process began: p50 {:.1}  p95 {:.1}  (max {:.1}; {})",
            d.p50,
            d.p95,
            d.max,
            if r.driver_before_token {
                "before the Discord binding's token resolved: no wait for the binding"
            } else {
                "a driver WAITED past the binding's token"
            }
        );
    }
    if let Some(ms) = r.vault_first_ms {
        println!("  the first vault start, with no copy yet (it read the vault first): {ms:.1} ms");
    }
    if let Some(c) = &r.vault_confirmed {
        println!(
            "  spawn to the vault's confirmation: p50 {:.1} ms  p95 {:.1} ms  (min {:.1}, max {:.1})",
            c.p50, c.p95, c.min, c.max
        );
    }
    let line = |v: &[(String, Summary)]| {
        v.iter()
            .map(|(n, s)| format!("{n} {:.2}/{:.2}", s.p50, s.p95))
            .collect::<Vec<_>>()
            .join(" · ")
    };
    let n = r.daemon.first().map_or(0, |(_, s)| s.n);
    println!(
        "  the daemon's own clock over {n} starts, p50/p95 ms: {}",
        line(&r.daemon)
    );
    println!(
        "  kernel startup steps, p50/p95 ms: {}",
        line(&r.kernel_steps)
    );
    println!(
        "  {} in {:.1} s",
        if r.ok() {
            "LIFECYCLE OK"
        } else {
            "LIFECYCLE BUDGET MISSED"
        },
        r.wall_ms / 1000.0
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn percentiles_are_nearest_rank() {
        let s: Vec<f64> = (1..=20).map(f64::from).collect();
        assert_eq!(percentile(&s, 50.0), 10.0);
        assert_eq!(percentile(&s, 95.0), 19.0);
        assert_eq!(percentile(&s, 100.0), 20.0);
        assert_eq!(percentile(&[7.0], 95.0), 7.0);
        // Ten runs: p95 is the slowest, so one slow run is never averaged away.
        let ten: Vec<f64> = (1..=10).map(f64::from).collect();
        assert_eq!(percentile(&ten, 95.0), 10.0);
        let s = Summary::of(&[3.0, 1.0, 2.0]).unwrap();
        assert_eq!((s.n, s.p50, s.p95, s.min, s.max), (3, 2.0, 3.0, 1.0, 3.0));
        assert!(Summary::of(&[]).is_none());
    }

    #[test]
    fn budgets_follow_section_nine_and_the_store_size() {
        assert_eq!(budget_ms("cold", 0), Some(50.0));
        assert_eq!(budget_ms("cold", 10_000), Some(250.0));
        assert_eq!(budget_ms("cold", 5_000), Some(150.0));
        assert_eq!(
            budget_ms("cold", 50_000),
            Some(250.0),
            "capped at §9's 10,000"
        );
        assert_eq!(budget_ms("kill", 0), Some(150.0));
        assert_eq!(budget_ms("kill", 10_000), Some(350.0));
        assert_eq!(budget_ms("shutdown", 10_000), Some(100.0));
        // A binary upgrade: under 200 ms without an answer, at any size.
        assert_eq!(budget_ms("swap", 0), Some(200.0));
        assert_eq!(budget_ms("swap", 10_000), Some(200.0));
        // Restore is measured: §9 names no number yet.
        assert_eq!(budget_ms("restore", 0), None);
    }

    #[test]
    fn a_p95_past_its_budget_and_margin_misses() {
        let s = |p95: f64| Summary {
            n: 10,
            p50: p95 - 1.0,
            p95,
            min: 1.0,
            max: p95,
        };
        let phases = vec![
            ("cold".to_string(), s(54.0)),
            ("shutdown".to_string(), s(99.0)),
            ("kill".to_string(), s(160.0)),
            ("unbudgeted".to_string(), s(9999.0)),
        ];
        let v = verdicts(&phases, 0, Some(5.0));
        assert_eq!(v.len(), 3, "a phase without a budget is not judged");
        assert!(v[0].ok, "54 ms is inside 50 ms plus a 5 ms margin");
        assert!(v[1].ok);
        assert!(!v[2].ok, "160 ms is past 150 ms plus 5");
        assert_eq!(v[2].budget, 150.0);
        // Each phase's measured margin, when none is given.
        let v = verdicts(&phases, 0, None);
        assert_eq!(
            v.iter().map(|v| v.margin).collect::<Vec<_>>(),
            [7.0, 4.0, 25.0]
        );
        assert!(
            v[0].ok && v[1].ok && v[2].ok,
            "160 ms is inside 150 plus 25"
        );
        // The throwaway 100 ms sleep in cold start: 23 ms becomes 123 ms.
        let slept = verdicts(&[("cold".to_string(), s(123.0))], 0, None);
        assert!(!slept[0].ok);
        // A swap: 200 ms plus its 2 ms margin, whatever the store's size;
        // a restore is measured and never judged.
        let v = verdicts(
            &[
                ("swap".to_string(), s(203.0)),
                ("restore".to_string(), s(9999.0)),
            ],
            10_000,
            None,
        );
        assert_eq!(v.len(), 1);
        assert!(!v[0].ok);
        assert_eq!((v[0].budget, v[0].margin), (200.0, 2.0));
    }

    #[test]
    fn a_start_is_read_from_health() {
        let h = json!({
            "secrets": {"state": "resolving"},
            "startup": [
                {"name": "config", "background": false, "start_us": 100, "end_us": 400},
                {"name": "kernel", "background": false, "start_us": 1000, "end_us": 36000,
                 "detail": {"steps": [{"name": "store", "us": 7000}, {"name": "load", "us": 6500}]}},
                {"name": "socket", "background": false, "start_us": 37000, "end_us": 37100},
                {"name": "secrets", "background": true, "start_us": 500, "end_us": null},
            ],
        });
        let s = Start::from_health(41.5, &h);
        assert_eq!(s.ms, 41.5);
        assert_eq!(s.secrets, "resolving");
        assert_eq!(
            s.phases,
            vec![
                ("config".to_string(), 0.3),
                ("kernel".to_string(), 35.0),
                ("socket".to_string(), 0.1)
            ]
        );
        assert_eq!(
            s.steps,
            vec![("store".to_string(), 7.0), ("load".to_string(), 6.5)]
        );
        assert_eq!(s.serving_ms, 37.1);
        assert!((s.between_ms - 1.7).abs() < 1e-9, "{}", s.between_ms);
    }

    /// The bench config loads as the daemon loads it, with nothing that
    /// leaves the machine: no GitHub token, Discord and the web UI off,
    /// every secret on the fake vault.
    #[test]
    fn the_bench_config_loads_and_stays_on_the_machine() {
        let d = tempfile::tempdir().unwrap();
        let text = bench_config(
            "http://127.0.0.1:9",
            &d.path().join("state"),
            &d.path().join("sock"),
            d.path(),
        )
        .unwrap();
        let (cfg, _warnings) = theseus_core::Config::parse(&text).unwrap();
        assert!(!cfg.web.enabled);
        // The binding runs, and all it sends goes to a port nothing listens
        // on (theseus-q4v).
        assert!(cfg.discord.enabled);
        assert_eq!(cfg.discord.rest_proxy.as_deref(), Some("127.0.0.1:9"));
        assert_eq!(
            cfg.discord.gateway_proxy.as_deref(),
            Some("ws://127.0.0.1:9")
        );
        assert!(!cfg.secrets.contains_key(&cfg.github.token_secret));
        assert!(cfg.secrets.values().all(|r| r.starts_with("op://Bench/")));
        assert!(cfg
            .all_providers()
            .values()
            .all(|p| p.api_base == "http://127.0.0.1:9"));
        assert_eq!(cfg.tools.proc_sync_secs, 1);
        // An AWS account is bound, and AWS too is a port nothing listens on
        // (row 29, C1): its key's secrets are the fake vault's.
        let a = &cfg.aws.accounts[BENCH_AWS_ACCOUNT];
        assert_eq!(a.endpoint.as_deref(), Some("http://127.0.0.1:9"));
        assert!(cfg.secrets.contains_key(&a.credentials.access_key_id));
        assert!(cfg.secrets.contains_key(&a.credentials.secret_access_key));
        // The index tender runs, and finds no model's files.
        assert!(cfg.index.enabled);
        assert!(!theseus_core::config::expand(&cfg.index.weights_dir).exists());
        // Recall runs in shadow.
        assert!(cfg.memory.on());
    }

    /// The bench's binding is one the binding takes, and its Discord is the
    /// in-process fake, REST and gateway (theseus-l21m): with ids it refused,
    /// it failed at load and the driver check checked nothing.
    #[test]
    fn the_bench_binding_has_discords_ids_and_the_fake_discord() {
        let ids: toml::Table = BENCH_BINDINGS.parse().unwrap();
        let dm = &ids["dm"].as_array().unwrap()[0];
        for id in [&ids["guild_id"], &dm["user"]] {
            let id = id.as_str().unwrap();
            assert!(
                (15..=21).contains(&id.len()) && id.bytes().all(|b| b.is_ascii_digit()),
                "{id:?} is not a Discord id"
            );
        }
        assert_eq!(
            ids["guild_id"].as_str(),
            Some(
                theseus_sim::fake_discord::DEFAULT_GUILD
                    .to_string()
                    .as_str()
            ),
            "the fake gateway's READY names this guild"
        );
        let fake = theseus_sim::fake_discord::FakeDiscord::start_with_gateway();
        let d = tempfile::tempdir().unwrap();
        let text = bench_config(
            "http://127.0.0.1:9",
            &d.path().join("state"),
            &d.path().join("sock"),
            d.path(),
        )
        .and_then(|t| on_fake_discord(&t, &fake))
        .unwrap();
        let (cfg, _warnings) = theseus_core::Config::parse(&text).unwrap();
        assert!(cfg.discord.enabled);
        assert_eq!(cfg.discord.rest_proxy.as_deref(), Some(fake.addr.as_str()));
        assert_eq!(
            cfg.discord.gateway_proxy,
            Some(fake.gateway().unwrap().url())
        );
        assert!(
            fake.addr.starts_with("127.0.0.1:"),
            "nothing leaves the machine"
        );
    }

    #[test]
    fn the_fake_op_answers_every_reference_after_its_delay() {
        let d = tempfile::tempdir().unwrap();
        let op = d.path().join("op");
        let note = d.path().join("note.toml");
        std::fs::write(&note, "[secrets]\n").unwrap();
        std::fs::write(&op, fake_op(10, &note)).unwrap();
        let read = Command::new("sh")
            .arg(&op)
            .args(["read", "--no-newline", VAULT_REF])
            .output()
            .unwrap();
        assert_eq!(String::from_utf8(read.stdout).unwrap(), "[secrets]\n");
        let out = Command::new("sh")
            .arg(&op)
            .arg("inject")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .and_then(|mut c| {
                c.stdin
                    .take()
                    .unwrap()
                    .write_all(b"--b-0\n{{ op://Bench/a/credential }}\n--b-end\n")?;
                c.wait_with_output()
            })
            .unwrap();
        assert_eq!(
            String::from_utf8(out.stdout).unwrap(),
            "--b-0\nbench-secret-value-0000\n--b-end\n"
        );
    }
}
