//! `theseus-sim bench turn`, `bench idle`, and `bench size` (theseus-goa8;
//! review 2's S4 and consideration 8): what a running daemon costs, beside the
//! lifecycle bench's start and stop.
//!
//! - **`turn`**: a plain turn and a tool-call turn on the stand-in model, each
//!   run N times on one warm session: wall time by this clock and by the
//!   daemon's, and **frames per turn**, counted from the WAL (`walcount`). A
//!   frame is one `fdatasync`, so frames are §9's per-turn overhead in a unit
//!   that does not depend on the disk: the plain turn's count is the gated
//!   budget (5 today, with a floor of 2, review 2's S5), the way the cold
//!   start is. The rest is measured with no budget: the disk's own `fdatasync`
//!   (so the harness's share of a turn can be read off), and the daemon's
//!   resident memory after the start and after a burst of turns.
//! - **`idle`**: a daemon with nothing to do, over a window (30 s by
//!   default): the CPU time it used, how often its threads woke, and the
//!   frames it wrote (QUIET BY CONSTRUCTION says none), on an empty store or
//!   a synthetic one of parked sessions (`--sessions 10000`).
//! - **`size`**: the shipped binaries' sizes, against §9's 60 MB.
//!
//! Each appends a row to the bench history (`--record`), the file the
//! lifecycle bench keeps (`history`). The scratch daemon has Discord and the
//! web UI off, so what runs is the core and nothing else.

use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use anyhow::{bail, Context, Result};
use clap::Args;
use serde::Serialize;
use serde_json::{json, Value};
use theseus_core::tender::START_AFTER;

use crate::fake_model::{FakeModel, TOOL_MARK};
use crate::history;
use crate::lifecycle::{self, Rig, Summary, Vault, Verdict};
use crate::procfs::{self, Sample};
use crate::walcount::{Frame, Tail};

/// §9's per-turn overhead, restated as frames (review 2, consideration 8): a
/// plain one-loop turn writes at most this many. 5 since theseus-l6y; the
/// floor is 2 (review 2's S5: everything up to the dispatch is one frame, and
/// the completion with the turn's end is the other). A step that writes fewer
/// lowers this number in the same commit, so it only goes down.
pub const PLAIN_TURN_FRAMES: f64 = 5.0;

/// §9's binary size, in MB (10^6 bytes): under 60.
pub const BINARY_MB: f64 = 60.0;

/// How long the WAL must be still, after a turn answers, before the turn's
/// frames are counted: a write that follows the answer is the turn's too.
const QUIET: Duration = Duration::from_millis(50);

/// A single reading, as the history's p50 and p95.
fn single(x: f64) -> Summary {
    Summary {
        n: 1,
        p50: x,
        p95: x,
        min: x,
        max: x,
    }
}

fn ms_since(t: Instant) -> f64 {
    t.elapsed().as_secs_f64() * 1000.0
}

// ------------------------------------------------------------------ the rig

/// A scratch daemon for a bench: its own state directory, socket, and config;
/// the stand-in model; the fake `op`. It lives as long as this does.
struct Scratch {
    rig: Rig,
    _model: FakeModel,
    _tmp: Option<tempfile::TempDir>,
    work: PathBuf,
}

impl Scratch {
    /// The WAL's directory, `<state>/store/wal`.
    fn wal(&self) -> PathBuf {
        self.rig.state.join("store").join("wal")
    }
}

/// The lifecycle bench's config with Discord off as well as the web UI: a
/// bench of the core's own cost has no binding trying a port nothing listens
/// on, which would be its own wakeups.
fn quiet_config(model: &str, state: &Path, sock: &Path, projects: &Path) -> Result<String> {
    let mut t: toml::Table = lifecycle::bench_config(model, state, sock, projects)?.parse()?;
    t.entry("discord")
        .or_insert_with(|| toml::Value::Table(Default::default()))
        .as_table_mut()
        .context("[discord] is a table")?
        .insert("enabled".into(), false.into());
    Ok(toml::to_string(&t)?)
}

fn scratch(theseusd: &Path, dir: Option<&Path>) -> Result<Scratch> {
    let (tmp, work) = match dir {
        Some(d) => {
            std::fs::create_dir_all(d)?;
            (None, d.to_path_buf())
        }
        None => {
            let t = tempfile::tempdir()?;
            let w = t.path().to_path_buf();
            (Some(t), w)
        }
    };
    let (state, projects, bin) = (work.join("state"), work.join("projects"), work.join("bin"));
    for d in [&state, &projects, &bin] {
        std::fs::create_dir_all(d)?;
    }
    let sock = work.join("sock");
    // A tool call that finishes at once: `true`, started through the product's
    // own path for a job.
    let model = FakeModel::start_mixed(vec!["true".to_string()])?;
    let config = work.join("config.toml");
    std::fs::write(
        &config,
        quiet_config(&model.base(), &state, &sock, &projects)?,
    )?;
    let op = bin.join("op");
    // The fake `op` answers at once: a bench of turns waits for no secret.
    std::fs::write(&op, lifecycle::fake_op(0, &config))?;
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&op, std::fs::Permissions::from_mode(0o755))?;
    }
    let rig = Rig {
        theseusd: theseusd.to_path_buf(),
        config,
        state,
        sock,
        vault: Vault::Fake(bin),
        log: work.join("theseusd.log"),
    };
    Ok(Scratch {
        rig,
        _model: model,
        _tmp: tmp,
        work,
    })
}

/// The WAL read until it has been still for [`QUIET`]: every frame written
/// since the last read, whatever wrote it. An error when it never settles,
/// since a bench of a quiet daemon then measures a busy one.
fn until_quiet(tail: &mut Tail) -> Result<Vec<Frame>> {
    let mut seen = Vec::new();
    let mut still_since = Instant::now();
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let more = tail.read()?;
        if more.is_empty() {
            if still_since.elapsed() >= QUIET {
                return Ok(seen);
            }
        } else {
            seen.extend(more);
            still_since = Instant::now();
        }
        if Instant::now() > deadline {
            bail!("the WAL was still growing after 10 s: the daemon is not idle");
        }
        std::thread::sleep(Duration::from_millis(5));
    }
}

/// The start's own work after serving, read off the WAL until it is done:
/// the index tender starts [`START_AFTER`] after serving and records its
/// start (`ledger:index.tender`) in a frame of its own, which a measured
/// turn would otherwise count (theseus-u55z; the join gate's six-frame plain
/// turn at 11:08 on 2026-10-02). A daemon whose tender is off writes no such
/// row, so the wait ends at its deadline; either way the WAL is then quiet.
fn after_serving(tail: &mut Tail) -> Result<()> {
    let deadline = Instant::now() + START_AFTER + Duration::from_secs(8);
    while Instant::now() < deadline {
        let frames = tail.read()?;
        if frames
            .iter()
            .any(|f| f.records.iter().any(|r| r == "ledger:index.tender"))
        {
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    until_quiet(tail).map(drop)
}

fn labels(frames: &[Frame]) -> Vec<String> {
    frames.iter().map(Frame::label).collect()
}

// ------------------------------------------------------------------ the turn bench

pub struct TurnOpts {
    pub theseusd: PathBuf,
    /// Measured turns of each kind.
    pub runs: usize,
    /// Turns in the burst whose memory is read afterwards.
    pub burst: usize,
    /// Work here and keep it; otherwise in a temporary directory.
    pub dir: Option<PathBuf>,
}

/// One kind of turn, measured.
#[derive(Debug, Serialize)]
pub struct Kind {
    pub name: String,
    /// `turn.submit` sent to its answer, by this clock, in ms.
    pub wall_ms: Summary,
    /// The same turn by the daemon's clock (`elapsed_ms`).
    pub daemon_ms: Summary,
    /// Frames written from before the turn to a quiet stretch after it.
    pub frames: Summary,
    /// Each measured turn's frames, in order.
    pub frames_each: Vec<u64>,
    /// The last turn's frames, each by what it holds.
    pub last_frames: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct Burst {
    pub turns: usize,
    pub sessions: usize,
    pub tool_turns: usize,
    pub wall_ms: f64,
    pub per_turn_ms: f64,
    pub frames: u64,
}

#[derive(Debug, Serialize)]
pub struct TurnReport {
    pub theseusd: String,
    pub runs: usize,
    /// Spawn to the first `health` answer, in ms.
    pub start_ms: f64,
    pub plain: Kind,
    pub tool: Kind,
    /// One `fdatasync` of a 4 KiB append on this disk, in ms: the quieter of
    /// two probes, one before the daemon starts and one after it stops, since
    /// a disk another process is using changes in seconds.
    pub fsync_ms: Summary,
    /// Each probe's p50 (before, after), in ms.
    pub fsync_probes_ms: [f64; 2],
    pub rss_start: Sample,
    pub rss_burst: Sample,
    pub burst: Burst,
    pub verdicts: Vec<Verdict>,
    pub wall_ms: f64,
}

impl TurnReport {
    pub fn ok(&self) -> bool {
        self.verdicts.iter().all(|v| v.ok)
    }

    /// What the history keeps of the run.
    pub fn columns(&self) -> Vec<(String, Summary)> {
        vec![
            ("turn_plain".to_string(), self.plain.wall_ms),
            ("frames_plain".to_string(), self.plain.frames),
            ("turn_tool".to_string(), self.tool.wall_ms),
            ("frames_tool".to_string(), self.tool.frames),
            ("rss_start".to_string(), single(self.rss_start.rss_mb())),
            ("rss_burst".to_string(), single(self.rss_burst.rss_mb())),
        ]
    }
}

/// The verdicts of a turn bench: the plain turn's frames against its budget.
/// A count is exact, so the margin is none.
pub fn turn_verdicts(plain_frames: &Summary) -> Vec<Verdict> {
    vec![Verdict {
        phase: "frames_plain".to_string(),
        p95: plain_frames.p95,
        budget: PLAIN_TURN_FRAMES,
        margin: 0.0,
        ok: plain_frames.p95 <= PLAIN_TURN_FRAMES,
    }]
}

/// One `fdatasync` of an append of 4 KiB, as the WAL pays for each frame: `n`
/// of them, after two that are not counted, in `dir` (the bench's store is on
/// the same disk).
fn fsync_probe(dir: &Path, n: usize) -> Result<Summary> {
    let path = dir.join("fsync-probe");
    let mut f = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)?;
    let block = vec![0x5au8; 4096];
    let mut ms = Vec::with_capacity(n);
    for i in 0..n + 2 {
        let t = Instant::now();
        f.write_all(&block)?;
        f.sync_data()?;
        if i >= 2 {
            ms.push(ms_since(t));
        }
    }
    drop(f);
    let _ = std::fs::remove_file(&path);
    Summary::of(&ms).context("no fsync samples")
}

/// A bench's driver of one session's turns: the rig, and the WAL's tail.
struct Driver<'a> {
    rig: &'a Rig,
    tail: Tail,
}

impl Driver<'_> {
    fn open_session(&self, label: &str) -> Result<String> {
        let r = self.rig.call("session.open", json!({ "label": label }))?;
        Ok(r["session_id"]
            .as_str()
            .context("session.open gave no session id")?
            .to_string())
    }

    fn submit(&self, session: &str, input: &str) -> Result<Value> {
        self.rig.call(
            "turn.submit",
            json!({"session_id": session, "input": input, "author": "bench", "attachments": []}),
        )
    }

    /// One measured turn: what it answered, its wall time here, and the
    /// frames from just before it to a quiet stretch after it. Whatever the
    /// WAL held before it, a straggler of the last turn's among it, is not
    /// this turn's.
    fn measured(&mut self, session: &str, input: &str) -> Result<(Value, f64, Vec<Frame>)> {
        until_quiet(&mut self.tail)?;
        let t = Instant::now();
        let r = self.submit(session, input)?;
        let wall = ms_since(t);
        let frames = until_quiet(&mut self.tail)?;
        Ok((r, wall, frames))
    }

    /// `runs` measured turns of `input` (`loops` and `tool_calls` as the
    /// stand-in makes them: a bench that measured another shape would be
    /// measuring something else).
    fn kind(
        &mut self,
        name: &str,
        session: &str,
        input: &str,
        runs: usize,
        shape: (u64, u64),
    ) -> Result<Kind> {
        let (mut wall, mut daemon, mut frames_each) = (Vec::new(), Vec::new(), Vec::new());
        let mut last = Vec::new();
        for i in 0..runs {
            let (r, w, frames) = self.measured(session, &format!("{input} {i}"))?;
            let got = (
                r["loops"].as_u64().unwrap_or(0),
                r["tool_calls"].as_u64().unwrap_or(0),
            );
            if got != shape {
                bail!(
                    "a {name} turn ran {} loop(s) and {} tool call(s), not {} and {}: the stand-in \
                     did not make the turn this bench means to measure; it answered {:?}",
                    got.0,
                    got.1,
                    shape.0,
                    shape.1,
                    r["output"]
                );
            }
            wall.push(w);
            daemon.push(r["elapsed_ms"].as_f64().unwrap_or(0.0));
            frames_each.push(frames.len() as u64);
            last = labels(&frames);
        }
        let frames: Vec<f64> = frames_each.iter().map(|n| *n as f64).collect();
        Ok(Kind {
            name: name.to_string(),
            wall_ms: Summary::of(&wall).context("no runs")?,
            daemon_ms: Summary::of(&daemon).context("no runs")?,
            frames: Summary::of(&frames).context("no runs")?,
            frames_each,
            last_frames: last,
        })
    }
}

pub fn run_turn(o: &TurnOpts) -> Result<TurnReport> {
    let wall = Instant::now();
    let s = scratch(&o.theseusd, o.dir.as_deref())?;
    let fsync_before = fsync_probe(&s.work, 20)?;
    let (mut daemon, start) = s.rig.start()?;
    let pid = daemon.0.id();
    // The start's own background work is done before a turn is timed.
    let mut tail = Tail::at_start(&s.wal());
    after_serving(&mut tail)?;
    let rss_start = procfs::sample(pid)?;
    let mut d = Driver { rig: &s.rig, tail };
    let session = d.open_session("bench turns")?;
    // Warm-ups, unmeasured: the first turn of a session builds its context,
    // and the first tool call starts the job path.
    for input in ["warm up", "warm up again", "warm up with bench-tool"] {
        d.submit(&session, input)?;
    }
    let plain = d.kind("plain", &session, "a plain turn", o.runs, (1, 0))?;
    let tool = d.kind(
        "tool-call",
        &session,
        &format!("a turn with a call, {TOOL_MARK}"),
        o.runs,
        (2, 1),
    )?;
    let burst = burst(&mut d, o.burst)?;
    std::thread::sleep(Duration::from_millis(300));
    let rss_burst = procfs::sample(pid)?;
    s.rig.stop(&mut daemon)?;
    let fsync_after = fsync_probe(&s.work, 20)?;
    let fsync_probes_ms = [fsync_before.p50, fsync_after.p50];
    let fsync_ms = if fsync_after.p50 < fsync_before.p50 {
        fsync_after
    } else {
        fsync_before
    };
    let verdicts = turn_verdicts(&plain.frames);
    Ok(TurnReport {
        theseusd: o.theseusd.display().to_string(),
        runs: o.runs,
        start_ms: start.ms,
        plain,
        tool,
        fsync_ms,
        fsync_probes_ms,
        rss_start,
        rss_burst,
        burst,
        verdicts,
        wall_ms: ms_since(wall),
    })
}

/// `turns` turns back to back across six sessions, every fifth with a tool
/// call, with the WAL's frames counted over the lot: what a busy hour would
/// leave resident. No turns, no burst.
fn burst(d: &mut Driver<'_>, turns: usize) -> Result<Burst> {
    const SESSIONS: usize = 6;
    if turns == 0 {
        return Ok(Burst {
            turns: 0,
            sessions: 0,
            tool_turns: 0,
            wall_ms: 0.0,
            per_turn_ms: 0.0,
            frames: 0,
        });
    }
    let ids: Vec<String> = (0..SESSIONS)
        .map(|i| d.open_session(&format!("bench burst {i}")))
        .collect::<Result<_>>()?;
    until_quiet(&mut d.tail)?;
    let mut tool_turns = 0;
    let t = Instant::now();
    for i in 0..turns {
        let tool = i % 5 == 4;
        tool_turns += usize::from(tool);
        let input = if tool {
            format!("burst turn {i}, {TOOL_MARK}")
        } else {
            format!("burst turn {i}")
        };
        d.submit(&ids[i % SESSIONS], &input)?;
    }
    let wall_ms = ms_since(t);
    let frames = until_quiet(&mut d.tail)?.len() as u64;
    Ok(Burst {
        turns,
        sessions: SESSIONS,
        tool_turns,
        wall_ms,
        per_turn_ms: wall_ms / turns.max(1) as f64,
        frames,
    })
}

pub fn print_turn(r: &TurnReport) {
    println!(
        "bench turn · {} · {} runs of each kind, on the stand-in model, Discord and the web UI off",
        r.theseusd, r.runs
    );
    println!(
        "  {:<10} {:>7} {:>9} {:>9} {:>9} {:>11}",
        "turn", "frames", "wall p50", "wall p95", "max", "daemon p50"
    );
    for k in [&r.plain, &r.tool] {
        let budget = if k.name == "plain" {
            format!(" (budget {})", PLAIN_TURN_FRAMES)
        } else {
            String::new()
        };
        println!(
            "  {:<10} {:>7} {:>6.1} ms {:>6.1} ms {:>6.1} ms {:>8.1} ms{budget}",
            k.name,
            frames_text(&k.frames_each),
            k.wall_ms.p50,
            k.wall_ms.p95,
            k.wall_ms.max,
            k.daemon_ms.p50
        );
    }
    let fsync = r.fsync_ms.p50;
    let plain_frames = r.plain.frames.p50;
    println!(
        "  this disk's fdatasync: p50 {fsync:.1} ms, the quieter of two probes ({:.1} before the turns, {:.1} after). \
         A plain turn's {plain_frames} frames are about {:.1} ms of its {:.1} ms, so the harness's own share is at \
         most {:.1} ms",
        r.fsync_probes_ms[0],
        r.fsync_probes_ms[1],
        plain_frames * fsync,
        r.plain.wall_ms.p50,
        (r.plain.wall_ms.p50 - plain_frames * fsync).max(0.0)
    );
    if r.burst.turns == 0 {
        println!(
            "  resident memory: {:.1} MB after the start; {:.1} MB (peak {:.1}) after the measured turns",
            r.rss_start.rss_mb(),
            r.rss_burst.rss_mb(),
            r.rss_burst.hwm_mb()
        );
    } else {
        println!(
            "  resident memory: {:.1} MB after the start; {:.1} MB (peak {:.1}) after a burst of {} turns \
             ({} with a tool call, over {} sessions) in {:.0} ms, {:.1} ms a turn, {} frames",
            r.rss_start.rss_mb(),
            r.rss_burst.rss_mb(),
            r.rss_burst.hwm_mb(),
            r.burst.turns,
            r.burst.tool_turns,
            r.burst.sessions,
            r.burst.wall_ms,
            r.burst.per_turn_ms,
            r.burst.frames
        );
    }
    println!(
        "  a plain turn's frames: {}",
        r.plain.last_frames.join("  ")
    );
    println!(
        "  a tool-call turn's frames: {}",
        r.tool.last_frames.join("  ")
    );
    for v in &r.verdicts {
        println!(
            "  {}: {} frame(s) at the p95, budget {}: {}",
            v.phase,
            v.p95,
            v.budget,
            if v.ok { "ok" } else { "MISSED" }
        );
    }
    println!("  the bench took {:.1} s", r.wall_ms / 1000.0);
}

/// The frames of each run, `5` when every run wrote the same count and
/// `5..6` when they differed.
fn frames_text(each: &[u64]) -> String {
    let (lo, hi) = (
        each.iter().min().copied().unwrap_or(0),
        each.iter().max().copied().unwrap_or(0),
    );
    if lo == hi {
        lo.to_string()
    } else {
        format!("{lo}..{hi}")
    }
}

// ------------------------------------------------------------------ the idle bench

pub struct IdleOpts {
    pub theseusd: PathBuf,
    /// The window, in seconds.
    pub seconds: u64,
    /// How long the daemon may take to go quiet after its first answer before
    /// the window begins anyway, in seconds.
    pub settle_secs: u64,
    /// A synthetic store of this many parked sessions (0: empty).
    pub sessions: u64,
    /// Or a copy of this store directory.
    pub store: Option<PathBuf>,
    pub dir: Option<PathBuf>,
}

#[derive(Debug, Serialize)]
pub struct IdleReport {
    pub theseusd: String,
    pub store: String,
    pub sessions: u64,
    pub window_s: f64,
    /// Seconds from the first answer until the daemon's CPU was quiet; the
    /// window began then.
    pub settled_s: f64,
    /// Whether it settled within the limit; if not, the window began anyway.
    pub settled: bool,
    /// The CPU used in each ten seconds of the wait, as a share of one core
    /// (%), so a daemon that does not go quiet shows whether it is slowing.
    pub settle_trace: Vec<f64>,
    pub cpu_ms: f64,
    pub cpu_percent: f64,
    pub wakeups: u64,
    pub wakeups_per_s: f64,
    /// Frames written in the window: a quiet daemon writes none.
    pub frames: u64,
    pub frame_shapes: Vec<String>,
    pub threads: u64,
    pub rss: Sample,
    pub wall_ms: f64,
}

impl IdleReport {
    pub fn columns(&self) -> Vec<(String, Summary)> {
        vec![
            ("idle_cpu".to_string(), single(self.cpu_ms)),
            ("idle_wakeups".to_string(), single(self.wakeups_per_s)),
            ("idle_frames".to_string(), single(self.frames as f64)),
            ("rss_idle".to_string(), single(self.rss.rss_mb())),
        ]
    }
}

/// How long the daemon may take to go quiet after its start, in seconds, by
/// default: a 10,000-session store checks its history after serving.
const SETTLE_MAX_S: u64 = 60;

/// CPU in a half second at or under this is quiet, in ns (2 ms).
const QUIET_CPU_NS: u64 = 2_000_000;

pub fn run_idle(o: &IdleOpts) -> Result<IdleReport> {
    let wall = Instant::now();
    let s = scratch(&o.theseusd, o.dir.as_deref())?;
    let (label, sessions) = match (&o.store, o.sessions) {
        (Some(src), _) => {
            lifecycle::copy_dir(src, &s.rig.state.join("store"))?;
            (format!("a copy of {}", src.display()), 0)
        }
        (None, 0) => ("empty".to_string(), 0),
        (None, n) => {
            let g = crate::synth::generate(&s.rig.state.join("store"), n)?;
            (format!("synthetic, {n} parked sessions"), g.sessions)
        }
    };
    let (mut daemon, _) = s.rig.start()?;
    let pid = daemon.0.id();
    let first_answer = Instant::now();
    // Quiet: four half-second stretches in a row that used under 2 ms of CPU.
    // Every tenth second the CPU used since the last one is kept, as a
    // percentage of one core, so a daemon that never goes quiet shows whether
    // it is slowing down.
    let (mut settled, mut quiet_run) = (false, 0);
    let mut last = procfs::sample(pid)?;
    let (mut trace, mut mark) = (Vec::new(), (Instant::now(), last.cpu_ns));
    while first_answer.elapsed() < Duration::from_secs(o.settle_secs) {
        std::thread::sleep(Duration::from_millis(500));
        let now = procfs::sample(pid)?;
        if now.cpu_ns.saturating_sub(last.cpu_ns) <= QUIET_CPU_NS {
            quiet_run += 1;
        } else {
            quiet_run = 0;
        }
        last = now;
        if mark.0.elapsed() >= Duration::from_secs(10) {
            let secs = mark.0.elapsed().as_secs_f64();
            trace.push(now.cpu_ns.saturating_sub(mark.1) as f64 / 1e9 / secs * 100.0);
            mark = (Instant::now(), now.cpu_ns);
        }
        if quiet_run >= 4 {
            settled = true;
            break;
        }
    }
    let settled_s = first_answer.elapsed().as_secs_f64();
    let mut tail = Tail::at_end(&s.wal())?;
    let before = procfs::sample(pid)?;
    let t = Instant::now();
    std::thread::sleep(Duration::from_secs(o.seconds));
    let window_s = t.elapsed().as_secs_f64();
    let after = procfs::sample(pid)?;
    let frames = tail.read()?;
    s.rig.stop(&mut daemon)?;
    let cpu_ms = after.cpu_ns.saturating_sub(before.cpu_ns) as f64 / 1e6;
    let wakeups = after.wakeups.saturating_sub(before.wakeups);
    Ok(IdleReport {
        theseusd: o.theseusd.display().to_string(),
        store: label,
        sessions,
        window_s,
        settled_s,
        settled,
        settle_trace: trace,
        cpu_ms,
        cpu_percent: cpu_ms / (window_s * 1000.0) * 100.0,
        wakeups,
        wakeups_per_s: wakeups as f64 / window_s,
        frames: frames.len() as u64,
        frame_shapes: labels(&frames),
        threads: after.threads,
        rss: after,
        wall_ms: ms_since(wall),
    })
}

pub fn print_idle(r: &IdleReport) {
    println!(
        "bench idle · {} · {} store · a {:.0} s window, begun {:.1} s after the first answer{}",
        r.theseusd,
        r.store,
        r.window_s,
        r.settled_s,
        if r.settled {
            ""
        } else {
            " (the daemon never went quiet: measured anyway)"
        }
    );
    if !r.settle_trace.is_empty() {
        println!(
            "  CPU while it settled, % of one core by ten seconds: {}",
            r.settle_trace
                .iter()
                .map(|p| format!("{p:.1}"))
                .collect::<Vec<_>>()
                .join(", ")
        );
    }
    println!(
        "  CPU {:.1} ms ({:.3} % of one core) · {} wakeups ({:.1} a second) over {} threads · {} frame(s) written",
        r.cpu_ms, r.cpu_percent, r.wakeups, r.wakeups_per_s, r.threads, r.frames
    );
    if !r.frame_shapes.is_empty() {
        println!("  frames written while idle: {}", r.frame_shapes.join("  "));
    }
    println!(
        "  resident memory {:.1} MB (peak {:.1} MB)",
        r.rss.rss_mb(),
        r.rss.hwm_mb()
    );
    println!("  the bench took {:.1} s", r.wall_ms / 1000.0);
}

// ------------------------------------------------------------------ the size bench

/// The binaries a build ships, and whether §9's size budget holds each. The
/// simulator is a tool beside them, measured without one.
const BINARIES: [(&str, Option<&str>); 4] = [
    ("theseusd", Some("size_theseusd")),
    ("theseus", Some("size_theseus")),
    ("theseus-tui", Some("size_theseus_tui")),
    ("theseus-sim", None),
];

#[derive(Debug, Serialize)]
pub struct SizeRow {
    pub binary: String,
    pub bytes: u64,
    pub mb: f64,
}

#[derive(Debug, Serialize)]
pub struct SizeReport {
    pub dir: String,
    pub rows: Vec<SizeRow>,
    /// Binaries the directory lacks.
    pub missing: Vec<String>,
    pub verdicts: Vec<Verdict>,
}

impl SizeReport {
    pub fn ok(&self) -> bool {
        self.verdicts.iter().all(|v| v.ok)
    }

    pub fn columns(&self) -> Vec<(String, Summary)> {
        BINARIES
            .iter()
            .filter_map(|(bin, col)| {
                let row = self.rows.iter().find(|r| r.binary == *bin)?;
                Some((col.as_ref()?.to_string(), single(round3(row.mb))))
            })
            .collect()
    }
}

fn round3(x: f64) -> f64 {
    (x * 1000.0).round() / 1000.0
}

/// The sizes of the binaries in `dir`, each against §9's 60 MB. A directory
/// with none of them is an error: there is nothing to measure.
pub fn run_size(dir: &Path) -> Result<SizeReport> {
    let (mut rows, mut missing, mut verdicts) = (Vec::new(), Vec::new(), Vec::new());
    for (bin, col) in BINARIES {
        match std::fs::metadata(dir.join(bin)) {
            Ok(m) if m.is_file() => {
                let mb = m.len() as f64 / 1e6;
                if let Some(col) = col {
                    verdicts.push(Verdict {
                        phase: col.to_string(),
                        p95: round3(mb),
                        budget: BINARY_MB,
                        margin: 0.0,
                        ok: mb <= BINARY_MB,
                    });
                }
                rows.push(SizeRow {
                    binary: bin.to_string(),
                    bytes: m.len(),
                    mb,
                });
            }
            _ => missing.push(bin.to_string()),
        }
    }
    if rows.is_empty() {
        bail!(
            "none of {:?} is in {}: build a release first (`cargo build --release`)",
            BINARIES.map(|(b, _)| b),
            dir.display()
        );
    }
    Ok(SizeReport {
        dir: dir.display().to_string(),
        rows,
        missing,
        verdicts,
    })
}

pub fn print_size(r: &SizeReport) {
    println!("bench size · {} · §9: under {BINARY_MB} MB", r.dir);
    for row in &r.rows {
        println!(
            "  {:<12} {:>12} bytes  {:>7.2} MB",
            row.binary, row.bytes, row.mb
        );
    }
    for b in &r.missing {
        println!("  {b:<12} not built");
    }
    for v in &r.verdicts {
        if !v.ok {
            println!(
                "  {}: {:.2} MB is over {} MB: MISSED",
                v.phase, v.p95, v.budget
            );
        }
    }
}

// ------------------------------------------------------------------ the commands

/// `--theseusd`, or the one beside this binary.
fn theseusd_or_beside(given: Option<PathBuf>) -> Result<PathBuf> {
    match given {
        Some(p) => Ok(p),
        None => Ok(std::env::current_exe()?
            .parent()
            .context("locating this binary's directory")?
            .join("theseusd")),
    }
}

/// Where a bench's report goes besides the terminal: JSON where `--json`
/// says, and a row in the history where `--record` says, labelled `--label`.
struct Output<'a> {
    json: Option<&'a Path>,
    record: Option<&'a Path>,
    label: Option<&'a str>,
}

fn emit<T: Serialize>(
    bench: &str,
    out: &Output<'_>,
    report: &T,
    columns: &[(String, Summary)],
    verdicts: &[Verdict],
    passed: bool,
) -> Result<()> {
    if let Some(path) = out.json {
        std::fs::write(path, serde_json::to_vec_pretty(report)?)?;
    }
    if let Some(path) = out.record {
        history::record(
            path,
            bench,
            out.label.unwrap_or_default(),
            columns,
            verdicts,
            passed,
        );
    }
    // Drift shows before it fails: a warning, never a failure.
    if passed {
        for line in history::near_limits_of(bench, verdicts) {
            println!("{line}");
        }
    }
    Ok(())
}

#[derive(Args)]
pub struct TurnArgs {
    /// The daemon to measure (default: the `theseusd` beside this binary).
    #[arg(long)]
    theseusd: Option<PathBuf>,
    /// Measured turns of each kind, after three that are not.
    #[arg(long, default_value_t = 10)]
    runs: usize,
    /// Turns in the burst whose memory is read afterwards (0: none, and the
    /// memory is read after the measured turns).
    #[arg(long, default_value_t = 30)]
    burst: usize,
    /// Exit 1 when the plain turn writes more frames than §9's budget.
    #[arg(long)]
    check: bool,
    /// Also write the report as JSON here.
    #[arg(long)]
    json: Option<PathBuf>,
    /// Work in this directory and keep it (default: a temporary one).
    #[arg(long)]
    dir: Option<PathBuf>,
    /// Append the run's row to this history, the CSV `bench history` reads.
    #[arg(long)]
    record: Option<PathBuf>,
    /// The run's label in the history (the gate's: the branch and the commit).
    #[arg(long, requires = "record")]
    label: Option<String>,
}

pub fn turn_cmd(a: TurnArgs) -> Result<()> {
    let report = run_turn(&TurnOpts {
        theseusd: theseusd_or_beside(a.theseusd)?,
        runs: a.runs.max(1),
        burst: a.burst,
        dir: a.dir,
    })?;
    print_turn(&report);
    let out = Output {
        json: a.json.as_deref(),
        record: a.record.as_deref(),
        label: a.label.as_deref(),
    };
    emit(
        "turn",
        &out,
        &report,
        &report.columns(),
        &report.verdicts,
        report.ok(),
    )?;
    if a.check && !report.ok() {
        std::process::exit(1);
    }
    Ok(())
}

#[derive(Args)]
pub struct IdleArgs {
    /// The daemon to measure (default: the `theseusd` beside this binary).
    #[arg(long)]
    theseusd: Option<PathBuf>,
    /// The window, in seconds.
    #[arg(long, default_value_t = 30)]
    seconds: u64,
    /// How long to wait for the daemon to go quiet before the window begins
    /// anyway, in seconds (a long wait shows whether it ever does).
    #[arg(long, default_value_t = SETTLE_MAX_S)]
    settle: u64,
    /// A synthetic store of this many parked sessions (0: empty).
    #[arg(long, default_value_t = 0)]
    sessions: u64,
    /// Or a copy of this store directory (e.g. a copy of ~/.theseus/store).
    #[arg(long, conflicts_with = "sessions")]
    store: Option<PathBuf>,
    /// Also write the report as JSON here.
    #[arg(long)]
    json: Option<PathBuf>,
    /// Work in this directory and keep it (default: a temporary one).
    #[arg(long)]
    dir: Option<PathBuf>,
    /// Append the run's row to this history, the CSV `bench history` reads.
    #[arg(long)]
    record: Option<PathBuf>,
    /// The run's label in the history (the gate's: the branch and the commit).
    #[arg(long, requires = "record")]
    label: Option<String>,
}

pub fn idle_cmd(a: IdleArgs) -> Result<()> {
    let report = run_idle(&IdleOpts {
        theseusd: theseusd_or_beside(a.theseusd)?,
        seconds: a.seconds.max(1),
        settle_secs: a.settle,
        sessions: a.sessions,
        store: a.store,
        dir: a.dir,
    })?;
    print_idle(&report);
    let out = Output {
        json: a.json.as_deref(),
        record: a.record.as_deref(),
        label: a.label.as_deref(),
    };
    emit("idle", &out, &report, &report.columns(), &[], true)
}

#[derive(Args)]
pub struct SizeArgs {
    /// The directory of the built binaries (default: the one this binary is in).
    #[arg(long)]
    dir: Option<PathBuf>,
    /// Exit 1 when a binary is over §9's 60 MB.
    #[arg(long)]
    check: bool,
    /// Also write the report as JSON here.
    #[arg(long)]
    json: Option<PathBuf>,
    /// Append the run's row to this history, the CSV `bench history` reads.
    #[arg(long)]
    record: Option<PathBuf>,
    /// The run's label in the history (the gate's: the branch and the commit).
    #[arg(long, requires = "record")]
    label: Option<String>,
}

pub fn size_cmd(a: SizeArgs) -> Result<()> {
    let dir = match a.dir {
        Some(d) => d,
        None => std::env::current_exe()?
            .parent()
            .context("locating this binary's directory")?
            .to_path_buf(),
    };
    let report = run_size(&dir)?;
    print_size(&report);
    let out = Output {
        json: a.json.as_deref(),
        record: a.record.as_deref(),
        label: a.label.as_deref(),
    };
    emit(
        "size",
        &out,
        &report,
        &report.columns(),
        &report.verdicts,
        report.ok(),
    )?;
    if a.check && !report.ok() {
        std::process::exit(1);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frames(each: &[f64]) -> Summary {
        Summary::of(each).unwrap()
    }

    #[test]
    fn the_plain_turns_frames_are_held_to_their_budget_exactly() {
        let ok = turn_verdicts(&frames(&[5.0; 10]));
        assert!(ok[0].ok && ok[0].budget == 5.0 && ok[0].margin == 0.0);
        // One run of ten that wrote a sixth frame is the p95: a miss.
        let mut some = vec![5.0; 9];
        some.push(6.0);
        assert!(!turn_verdicts(&frames(&some))[0].ok);
        // Fewer frames than the budget passes: a step that gets to 2 is a gain.
        assert!(turn_verdicts(&frames(&[2.0; 10]))[0].ok);
    }

    #[test]
    fn a_run_of_frames_reads_as_one_count_or_a_range() {
        assert_eq!(frames_text(&[5, 5, 5]), "5");
        assert_eq!(frames_text(&[5, 6, 5]), "5..6");
        assert_eq!(frames_text(&[]), "0");
    }

    #[test]
    fn a_single_reading_is_its_own_p50_and_p95() {
        let s = single(41.5);
        assert_eq!(
            (s.n, s.p50, s.p95, s.min, s.max),
            (1, 41.5, 41.5, 41.5, 41.5)
        );
    }

    #[test]
    fn sizes_are_judged_against_sixty_megabytes() {
        let d = tempfile::tempdir().unwrap();
        std::fs::write(d.path().join("theseusd"), vec![0u8; 21_700_000]).unwrap();
        std::fs::write(d.path().join("theseus"), vec![0u8; 61_000_000]).unwrap();
        std::fs::write(d.path().join("theseus-sim"), vec![0u8; 3_000_000]).unwrap();
        let r = run_size(d.path()).unwrap();
        assert_eq!(r.rows.len(), 3);
        assert_eq!(r.missing, ["theseus-tui"]);
        assert_eq!(r.verdicts.len(), 2, "the simulator has no budget");
        assert!(!r.ok(), "61 MB is over 60");
        let cols = r.columns();
        assert_eq!(
            cols.iter().map(|(n, _)| n.as_str()).collect::<Vec<_>>(),
            ["size_theseusd", "size_theseus"]
        );
        assert_eq!(cols[0].1.p50, 21.7);
        assert!(run_size(&d.path().join("nowhere")).is_err());
    }

    #[test]
    fn every_column_a_bench_records_has_a_history_column() {
        let columns: Vec<&str> = crate::history::columns().collect();
        let report = IdleReport {
            theseusd: String::new(),
            store: String::new(),
            sessions: 0,
            window_s: 30.0,
            settled_s: 1.0,
            settled: true,
            settle_trace: Vec::new(),
            cpu_ms: 3.0,
            cpu_percent: 0.01,
            wakeups: 40,
            wakeups_per_s: 1.3,
            frames: 0,
            frame_shapes: Vec::new(),
            threads: 12,
            rss: Sample::default(),
            wall_ms: 0.0,
        };
        for (name, _) in report.columns() {
            assert!(columns.contains(&name.as_str()), "{name} has no column");
        }
        let d = tempfile::tempdir().unwrap();
        std::fs::write(d.path().join("theseusd"), [0u8; 10]).unwrap();
        for (name, _) in run_size(d.path()).unwrap().columns() {
            assert!(columns.contains(&name.as_str()), "{name} has no column");
        }
        let kind = |name: &str| Kind {
            name: name.to_string(),
            wall_ms: single(1.0),
            daemon_ms: single(1.0),
            frames: single(5.0),
            frames_each: vec![5],
            last_frames: Vec::new(),
        };
        let turn = TurnReport {
            theseusd: String::new(),
            runs: 1,
            start_ms: 1.0,
            plain: kind("plain"),
            tool: kind("tool-call"),
            fsync_ms: single(7.0),
            fsync_probes_ms: [7.0, 8.0],
            rss_start: Sample::default(),
            rss_burst: Sample::default(),
            burst: Burst {
                turns: 0,
                sessions: 0,
                tool_turns: 0,
                wall_ms: 0.0,
                per_turn_ms: 0.0,
                frames: 0,
            },
            verdicts: turn_verdicts(&single(5.0)),
            wall_ms: 0.0,
        };
        assert!(turn.ok());
        for (name, _) in turn.columns() {
            assert!(columns.contains(&name.as_str()), "{name} has no column");
        }
    }
}
