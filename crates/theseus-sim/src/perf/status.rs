//! `bench status` (theseus-lweh): what `theseus status --short` costs a
//! prompt. A scratch daemon serves a synthetic store of parked sessions, and
//! the CLI runs against it many times: the whole process, from spawn to exit,
//! timed. Outside lifecycle's gated phases; `--check` holds p90 to 5 ms, which
//! is meant for the owner's machine.

use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::Instant;

use anyhow::{bail, Context, Result};
use clap::Args;
use serde::Serialize;

use super::{emit, ms_since, scratch, theseusd_or_beside, Output};
use crate::lifecycle::{percentile, Summary};

/// §FAST: a prompt's segment in at most this many ms at p90.
const P90_LIMIT_MS: f64 = 5.0;

#[derive(Args)]
pub struct StatusArgs {
    /// The daemon to serve (default: the `theseusd` beside this binary).
    #[arg(long)]
    theseusd: Option<PathBuf>,
    /// The CLI to time (default: the `theseus` beside this binary).
    #[arg(long)]
    theseus: Option<PathBuf>,
    /// A synthetic store of this many parked sessions.
    #[arg(long, default_value_t = 1000)]
    sessions: u64,
    /// How many runs.
    #[arg(long, default_value_t = 300)]
    runs: usize,
    /// Exit 1 when p90 is over 5 ms.
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
    /// The run's label in the history.
    #[arg(long, requires = "record")]
    label: Option<String>,
}

#[derive(Serialize)]
struct StatusReport {
    sessions: u64,
    runs: usize,
    p50_ms: f64,
    p90_ms: f64,
    min_ms: f64,
    max_ms: f64,
    limit_ms: f64,
    ok: bool,
}

pub fn status_cmd(a: StatusArgs) -> Result<()> {
    let theseusd = theseusd_or_beside(a.theseusd)?;
    let theseus = match a.theseus {
        Some(p) => p,
        None => theseusd.with_file_name("theseus"),
    };
    let s = scratch(&theseusd, a.dir.as_deref())?;
    let g = crate::synth::generate(&s.rig.state.join("store"), a.sessions)?;
    let (mut daemon, _) = s.rig.start()?;
    let mut ms = Vec::with_capacity(a.runs);
    let run = || -> Result<f64> {
        let t = Instant::now();
        let out = Command::new(&theseus)
            .arg("--socket")
            .arg(&s.rig.sock)
            .args(["status", "--short"])
            .env_remove("THESEUS_SOCKET")
            .stdin(Stdio::null())
            .output()
            .with_context(|| format!("running {}", theseus.display()))?;
        let took = ms_since(t);
        if !out.status.success() {
            bail!("theseus status --short exited {:?}", out.status.code());
        }
        Ok(took)
    };
    // The first run seeds the daemon's board; it is warm-up, not a sample.
    run()?;
    for _ in 0..a.runs.max(1) {
        ms.push(run()?);
    }
    s.rig.stop(&mut daemon)?;
    ms.sort_by(f64::total_cmp);
    let p90 = percentile(&ms, 90.0);
    let report = StatusReport {
        sessions: g.sessions,
        runs: ms.len(),
        p50_ms: percentile(&ms, 50.0),
        p90_ms: p90,
        min_ms: ms[0],
        max_ms: ms[ms.len() - 1],
        limit_ms: P90_LIMIT_MS,
        ok: p90 <= P90_LIMIT_MS,
    };
    println!(
        "theseus status --short against {} parked sessions, {} runs: p50 {:.2} ms, p90 {:.2} ms \
         (limit {:.0}), min {:.2}, max {:.2}: {}",
        report.sessions,
        report.runs,
        report.p50_ms,
        report.p90_ms,
        P90_LIMIT_MS,
        report.min_ms,
        report.max_ms,
        if report.ok { "ok" } else { "over" }
    );
    let out = Output {
        json: a.json.as_deref(),
        record: a.record.as_deref(),
        label: a.label.as_deref(),
    };
    let column = Summary::of(&ms).context("no runs")?;
    emit(
        "status",
        &out,
        &report,
        &[("status_short".to_string(), column)],
        &[],
        report.ok,
    )?;
    if a.check && !report.ok {
        std::process::exit(1);
    }
    Ok(())
}
