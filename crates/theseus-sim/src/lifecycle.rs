//! `theseus-sim bench lifecycle`: the lifecycle budgets of §9, measured on a
//! real `theseusd` over its real socket (FAST, §2; P5b; theseus-qa0).
//!
//! Three phases, each run `runs` times, with p50 and p95 per phase, and per
//! start-path phase and kernel step by the daemon's own clock (`health`):
//! - `cold`: process start to the first `health` answer;
//! - `shutdown`: the `shutdown` request to process exit, with executions
//!   waiting and a job running (a real `proc.run`, started by a real turn
//!   against a stand-in for the Messages API, `fake_model`);
//! - `kill`: SIGKILL, then a new process to its first `health` answer.
//!
//! The daemon's secrets come from a fake `op` that answers only after
//! `resolver_ms`, so a start that waited for them would show: at each first
//! answer, health must still say `resolving`. Binary swap and restore, P5b's
//! other two phases, are left to theseus-qa0's step F4.

use std::collections::BTreeMap;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use anyhow::{bail, Context, Result};
use serde::Serialize;
use serde_json::{json, Value};

use crate::fake_model::FakeModel;

pub const PHASES: [&str; 3] = ["cold", "shutdown", "kill"];

/// The job the shutdown and kill phases keep running.
const JOB: [&str; 2] = ["sleep", "300"];

/// Unmeasured starts before the first measured one.
const WARM_UPS: usize = 2;

pub struct Opts {
    pub theseusd: PathBuf,
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
/// SIGKILL to serving is the cold budget plus 100 ms of tail replay.
pub fn budget_ms(phase: &str, sessions: u64) -> Option<f64> {
    let cold = 50.0 + 200.0 * sessions.min(10_000) as f64 / 10_000.0;
    match phase {
        "cold" => Some(cold),
        "shutdown" => Some(100.0),
        "kill" => Some(cold + 100.0),
        _ => None,
    }
}

/// The noise allowed over each budget, in ms: the spread of the phase's p95
/// over five runs of the gate's own bench (debug binaries, an empty store,
/// ten runs a phase) on this machine, 2026-09-29, rounded up. Measured:
/// cold 23.3–30.1 (6.8), shutdown 47.3–51.3 (4.0), kill 43.2–67.7 (24.5,
/// one run whose redb repair after the SIGKILL took 50 ms instead of 26).
pub fn margin_ms(phase: &str) -> f64 {
    match phase {
        "cold" => 7.0,
        "shutdown" => 4.0,
        "kill" => 25.0,
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
    /// `secrets.state` in that first answer.
    pub secrets: String,
}

impl Start {
    pub fn from_health(ms: f64, h: &Value) -> Self {
        let (mut phases, mut steps, mut serving_us) = (Vec::new(), Vec::new(), 0u64);
        for p in h["startup"].as_array().into_iter().flatten() {
            if p["background"].as_bool().unwrap_or(false) {
                continue;
            }
            let (Some(s), Some(e)) = (p["start_us"].as_u64(), p["end_us"].as_u64()) else {
                continue;
            };
            serving_us = serving_us.max(e);
            let name = p["name"].as_str().unwrap_or("?").to_string();
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
        Self {
            after: String::new(),
            ms,
            phases,
            steps,
            serving_ms: serving_us as f64 / 1000.0,
            secrets: h["secrets"]["state"].as_str().unwrap_or("").to_string(),
        }
    }
}

// ------------------------------------------------------------------ the rig

struct Rig {
    theseusd: PathBuf,
    config: PathBuf,
    state: PathBuf,
    sock: PathBuf,
    fake_bin: PathBuf,
    log: PathBuf,
}

impl Rig {
    fn spawn(&self) -> Result<(Child, Instant)> {
        let log = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.log)?;
        let path = format!(
            "{}:{}",
            self.fake_bin.display(),
            std::env::var("PATH").unwrap_or_default()
        );
        let t0 = Instant::now();
        let child = Command::new(&self.theseusd)
            .arg("--config")
            .arg(&self.config)
            .arg("--socket")
            .arg(&self.sock)
            .arg("--state-dir")
            .arg(&self.state)
            .env("PATH", path)
            .env("OP_SERVICE_ACCOUNT_TOKEN", "bench-not-a-token")
            .env_remove("THESEUS_OP_TOKEN_FILE")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(log)
            .spawn()
            .with_context(|| format!("starting {}", self.theseusd.display()))?;
        Ok((child, t0))
    }

    /// Start a daemon and time it to its first `health` answer.
    fn start(&self) -> Result<(Child, Start)> {
        let (mut child, t0) = self.spawn()?;
        let deadline = t0 + Duration::from_secs(30);
        loop {
            if let Ok(s) = UnixStream::connect(&self.sock) {
                let h = request(s, "health", Value::Null)?;
                let ms = t0.elapsed().as_secs_f64() * 1000.0;
                return Ok((child, Start::from_health(ms, &h)));
            }
            if let Some(status) = child.try_wait()? {
                bail!(
                    "theseusd exited ({status}) before answering; its log ends:\n{}",
                    tail(&self.log)
                );
            }
            if Instant::now() > deadline {
                let _ = child.kill();
                bail!(
                    "theseusd did not answer within 30 s; its log ends:\n{}",
                    tail(&self.log)
                );
            }
            std::thread::sleep(Duration::from_micros(250));
        }
    }

    /// The `shutdown` request to process exit, in ms.
    fn stop(&self, child: &mut Child) -> Result<f64> {
        let s = UnixStream::connect(&self.sock).context("connecting to stop theseusd")?;
        let t0 = Instant::now();
        send(&s, "shutdown", Value::Null)?;
        let status = child.wait()?;
        let ms = t0.elapsed().as_secs_f64() * 1000.0;
        if !status.success() {
            bail!("theseusd exited with {status} on shutdown");
        }
        Ok(ms)
    }

    /// Stop a daemon however it can be stopped: a failed bench leaves none.
    fn stop_anyhow(&self, child: &mut Child) {
        if self.stop(child).is_err() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }

    fn call(&self, method: &str, params: Value) -> Result<Value> {
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

fn tail(log: &Path) -> String {
    let s = std::fs::read_to_string(log).unwrap_or_default();
    let lines: Vec<&str> = s.lines().collect();
    lines[lines.len().saturating_sub(15)..].join("\n")
}

// ------------------------------------------------------------------ setup

/// The fake `op`: every reference gets the same value, after `ms`.
fn fake_op(ms: u64) -> String {
    format!(
        "#!/bin/sh\n\
         # The lifecycle bench's stand-in for 1Password's op (theseus-sim):\n\
         # every reference gets one fixed value, after a delay, so a daemon\n\
         # that waited for its secrets before serving would show it.\n\
         sleep {}\n\
         case \"$1\" in\n\
         \x20 inject) sed -e 's/{{{{ [^}}]* }}}}/bench-secret-value-0000/g' ;;\n\
         \x20 read) printf '%s' bench-secret-value-0000 ;;\n\
         \x20 *) echo \"fake op: $1 is not supported\" >&2; exit 1 ;;\n\
         esac\n",
        ms as f64 / 1000.0
    )
}

/// The template, with its endpoints on the fake model, its secrets on the
/// fake `op`, Discord and the web UI off, and no GitHub token, so nothing
/// leaves the machine.
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
    table(&mut t, "discord").insert("enabled".into(), false.into());
    table(&mut t, "web").insert("enabled".into(), false.into());
    let tools = table(&mut t, "tools");
    tools.insert("projects_dir".into(), projects.display().to_string().into());
    tools.insert("proc_sync_secs".into(), 1.into());
    table(&mut t, "policy").insert("enforcement".into(), "open".into());
    Ok(toml::to_string(&t)?)
}

fn copy_dir(from: &Path, to: &Path) -> Result<()> {
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
    pub samples: BTreeMap<String, Vec<f64>>,
    /// The first starts, unmeasured: the first creates an empty store, or
    /// reads a copied one into the page cache.
    pub warm_up_ms: Vec<f64>,
    /// Every start, in order, with the daemon's own phases.
    pub starts: Vec<Start>,
    pub wall_ms: f64,
}

impl Report {
    pub fn ok(&self) -> bool {
        self.verdicts.iter().all(|v| v.ok) && self.served_before_secrets
    }
}

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
    let op = fake_bin.join("op");
    std::fs::write(&op, fake_op(o.resolver_ms))?;
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&op, std::fs::Permissions::from_mode(0o755))?;
    }
    let model = FakeModel::start(JOB.iter().map(|s| s.to_string()).collect())?;
    let sock = work.join("sock");
    let config = work.join("config.toml");
    std::fs::write(
        &config,
        bench_config(&model.base(), &state, &sock, &projects)?,
    )?;
    let rig = Rig {
        theseusd: o.theseusd.clone(),
        config,
        state,
        sock,
        fake_bin,
        log: work.join("theseusd.log"),
    };
    let want = |p: &str| o.phases.iter().any(|x| x == p);
    let mut samples: BTreeMap<String, Vec<f64>> = BTreeMap::new();
    let mut starts: Vec<Start> = Vec::new();

    // Two starts first, unmeasured: on an empty directory the first creates
    // the store, and on a copy it reads the files into the page cache; the
    // second pays for the disk still flushing after that (store and kernel
    // at three times their usual, measured). Neither is the cold start of an
    // existing store that §9 budgets.
    for _ in 0..WARM_UPS {
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
            starts.push(s);
            rig.stop(&mut child)?;
        }
    }
    let mut sessions = generated.as_ref().map_or(0, |g| g.sessions);
    if want("shutdown") || want("kill") {
        let (mut child, mut s) = rig.start()?;
        s.after = "cold".into();
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
                    child.wait()?;
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

    let phases: Vec<(String, Summary)> = PHASES
        .iter()
        .filter_map(|p| Some((p.to_string(), Summary::of(samples.get(*p)?)?)))
        .collect();
    let mut by_name: BTreeMap<String, Vec<f64>> = BTreeMap::new();
    let mut order: Vec<String> = Vec::new();
    let mut step_by: BTreeMap<String, Vec<f64>> = BTreeMap::new();
    let mut step_order: Vec<String> = Vec::new();
    let mut secrets: BTreeMap<String, usize> = BTreeMap::new();
    for s in starts.iter().filter(|s| s.after != "warm-up") {
        for (n, ms) in s
            .phases
            .iter()
            .chain([&("serving".to_string(), s.serving_ms)])
        {
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
        .all(|s| s.secrets == "resolving" || s.ms >= o.resolver_ms as f64);
    let warm_up_ms: Vec<f64> = starts
        .iter()
        .filter(|s| s.after == "warm-up")
        .map(|s| s.ms)
        .collect();
    let verdicts = verdicts(&phases, sessions, o.margin_ms);
    drop(tmp);
    Ok(Report {
        theseusd: o.theseusd.display().to_string(),
        store: label,
        sessions,
        generated,
        runs: o.runs,
        resolver_ms: o.resolver_ms,
        phases,
        daemon: summarize(&order, &by_name),
        kernel_steps: summarize(&step_order, &step_by),
        secrets_at_first_answer: secrets,
        verdicts,
        served_before_secrets,
        samples,
        warm_up_ms,
        starts,
        wall_ms: wall.elapsed().as_secs_f64() * 1000.0,
    })
}

const TITLES: [(&str, &str); 3] = [
    ("cold", "cold start to the first health answer"),
    (
        "shutdown",
        "clean shutdown, executions waiting and a job running",
    ),
    ("kill", "SIGKILL, then restart to the first health answer"),
];

pub fn print(r: &Report) {
    println!(
        "lifecycle bench · {} · store: {} · {} runs per phase · op answers after {} ms",
        r.theseusd, r.store, r.runs, r.resolver_ms
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
            verdict.map_or(String::new(), |v| format!(
                "budget {:.0} ms + {:.0} ms margin: {}",
                v.budget,
                v.margin,
                if v.ok { "ok" } else { "MISSED" }
            ))
        );
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
        assert_eq!(budget_ms("swap", 0), None);
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
        assert!(!cfg.discord.enabled && !cfg.web.enabled);
        assert!(!cfg.secrets.contains_key(&cfg.github.token_secret));
        assert!(cfg.secrets.values().all(|r| r.starts_with("op://Bench/")));
        assert!(cfg
            .all_providers()
            .values()
            .all(|p| p.api_base == "http://127.0.0.1:9"));
        assert_eq!(cfg.tools.proc_sync_secs, 1);
    }

    #[test]
    fn the_fake_op_answers_every_reference_after_its_delay() {
        let d = tempfile::tempdir().unwrap();
        let op = d.path().join("op");
        std::fs::write(&op, fake_op(10)).unwrap();
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
