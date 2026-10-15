//! `bench agenda` (theseus-nuhsz; horizon HZ8): what `work.agenda` costs. A
//! scratch daemon holds reminders (half of them daily series) and routines,
//! enough that a week's window fills the 200 lines the answer is capped at,
//! and the call is timed over many runs, the connection included. Outside
//! lifecycle's gated phases; `--check` holds p90 to 5 ms, which is meant for
//! the owner's machine.

use std::path::PathBuf;
use std::time::Instant;

use anyhow::{bail, Context, Result};
use clap::Args;
use serde::Serialize;
use serde_json::json;

use super::{emit, ms_since, scratch, theseusd_or_beside, Output};
use crate::lifecycle::{percentile, Summary};

/// §FAST: `work.agenda` at 200 lines in at most this many ms at p90.
const P90_LIMIT_MS: f64 = 5.0;

/// The lines the answer is capped at (`AGENDA_MAX`).
const LINES: usize = 200;

#[derive(Args)]
pub struct AgendaArgs {
    /// The daemon to serve (default: the `theseusd` beside this binary).
    #[arg(long)]
    theseusd: Option<PathBuf>,
    /// Reminders to set (the daemon's cap is 100; every other one repeats daily).
    #[arg(long, default_value_t = 90)]
    reminders: u32,
    /// Routines to set (each runs every hour).
    #[arg(long, default_value_t = 3)]
    routines: u32,
    /// How many calls.
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
struct AgendaReport {
    reminders: u32,
    routines: u32,
    lines: usize,
    runs: usize,
    p50_ms: f64,
    p90_ms: f64,
    min_ms: f64,
    max_ms: f64,
    limit_ms: f64,
    ok: bool,
}

pub fn agenda_cmd(a: AgendaArgs) -> Result<()> {
    let theseusd = theseusd_or_beside(a.theseusd)?;
    let s = scratch(&theseusd, a.dir.as_deref())?;
    let (mut daemon, _) = s.rig.start()?;
    for n in 0..a.reminders {
        let mut p = json!({"when": format!("in {}h", 1 + n % 20), "note": format!("Reminder {n}")});
        if n % 2 == 0 {
            p["every"] = json!("1d");
        }
        s.rig.call("wake.create", p).context("setting a reminder")?;
    }
    for n in 0..a.routines {
        s.rig
            .call(
                "routine.create",
                json!({"title": format!("Routine {n}"), "schedule": "every 1h",
                       "brief": "Say when the next high tide is."}),
            )
            .context("setting a routine")?;
    }
    let now = theseus_protocol::now_unix_ms();
    let window = json!({"from_ms": now, "to_ms": now + 7 * 86_400_000});
    let mut lines = 0;
    let mut run = || -> Result<f64> {
        let t = Instant::now();
        let r = s.rig.call("work.agenda", window.clone())?;
        let took = ms_since(t);
        lines = r["lines"].as_array().map_or(0, Vec::len);
        Ok(took)
    };
    // The first call seeds the daemon's board; it is warm-up, not a sample.
    run()?;
    let mut ms = Vec::with_capacity(a.runs);
    for _ in 0..a.runs.max(1) {
        ms.push(run()?);
    }
    // The floor: a call that does nothing, connection and parse included.
    let mut floor = Vec::with_capacity(a.runs);
    for _ in 0..a.runs.max(1) {
        let t = Instant::now();
        s.rig.call("status.short", serde_json::Value::Null)?;
        floor.push(ms_since(t));
    }
    floor.sort_by(f64::total_cmp);
    if lines < LINES {
        bail!("the window held {lines} lines, not the {LINES} the bench is for");
    }
    s.rig.stop(&mut daemon)?;
    ms.sort_by(f64::total_cmp);
    let p90 = percentile(&ms, 90.0);
    let report = AgendaReport {
        reminders: a.reminders,
        routines: a.routines,
        lines,
        runs: ms.len(),
        p50_ms: percentile(&ms, 50.0),
        p90_ms: p90,
        min_ms: ms[0],
        max_ms: ms[ms.len() - 1],
        limit_ms: P90_LIMIT_MS,
        ok: p90 <= P90_LIMIT_MS,
    };
    println!(
        "the floor, status.short: p50 {:.2} ms, p90 {:.2} ms",
        percentile(&floor, 50.0),
        percentile(&floor, 90.0)
    );
    println!(
        "work.agenda at {} lines ({} reminders, {} routines), {} runs: p50 {:.2} ms, p90 {:.2} ms \
         (limit {:.0}), min {:.2}, max {:.2}: {}",
        report.lines,
        report.reminders,
        report.routines,
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
        "agenda",
        &out,
        &report,
        &[("work_agenda".to_string(), column)],
        &[],
        report.ok,
    )?;
    if a.check && !report.ok {
        std::process::exit(1);
    }
    Ok(())
}
