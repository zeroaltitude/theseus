//! A measure, never run by the gate (theseus-ehkp): a burst of turns on a
//! scratch daemon with the judge on at the fake Jev, every pack as wired
//! (the `packs` arm of `bench turn --judge`), three sessions of turns back
//! to back for a while; then a `shutdown` with whatever the sink holds.
//! It prints each 15 s window's turn wall p50 and p95, the judge's frames
//! and blobs over the burst, and the stop's line (its `ms`, `frames`, and
//! `blob_syncs`). The judge's sink writes only between turns, so a burst
//! holds a backlog that drains in the gaps; what the burst measures is what
//! that costs the turns beside it.
//!
//! Run it on a frozen build, A B B A against another:
//!
//! ```text
//! THESEUS_BURST_THESEUSD=/tmp/b/theseusd THESEUS_BURST_SECS=180 \
//!   cargo test -p theseus-sim --bin theseus-sim -- --ignored --nocapture burst
//! ```
//!
//! (`cargo test`, not nextest, whose two minutes kill a longer run.)

use std::path::PathBuf;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use theseus_judge::fake::FakeJev;

use super::judge::{is_judges, judged, scripted, Arm};
use super::{after_serving, scratch_with, Driver};
use crate::walcount::Tail;

/// The sessions whose turns run back to back.
const SESSIONS: usize = 3;
/// A window of the turns' walls.
const WINDOW: Duration = Duration::from_secs(15);

/// The p50 and p95 of `ms`, sorted here.
fn p50_p95(ms: &mut [f64]) -> (f64, f64) {
    ms.sort_by(f64::total_cmp);
    let at = |q: f64| ms[((ms.len() - 1) as f64 * q).round() as usize];
    (at(0.5), at(0.95))
}

/// The burst, on `theseusd`, for `secs`, in `dir` (kept, with the daemon's
/// log).
fn burst(theseusd: PathBuf, secs: u64, dir: PathBuf) -> Result<()> {
    let jev = FakeJev::start()?;
    scripted(&jev);
    let s = scratch_with(&theseusd, Some(&dir), |t| {
        judged(t, &jev.base(), Arm::Packs)
    })?;
    let (mut daemon, _) = s.rig.start()?;
    let mut tail = Tail::at_start(&s.wal());
    after_serving(&mut tail)?;
    let d = Driver { rig: &s.rig, tail };
    let sessions: Vec<String> = (0..SESSIONS)
        .map(|i| d.open_session(&format!("burst {i}")))
        .collect::<Result<_>>()?;
    let t0 = Instant::now();
    let until = t0 + Duration::from_secs(secs);
    // Each turn: when it began (from t0) and its wall, in ms.
    let walls: Vec<(f64, f64)> = std::thread::scope(|sc| {
        let each: Vec<_> = sessions
            .iter()
            .map(|sid| {
                let d = &d;
                sc.spawn(move || -> Result<Vec<(f64, f64)>> {
                    let mut out = Vec::new();
                    let mut i = 0;
                    while Instant::now() < until {
                        let at = t0.elapsed().as_secs_f64() * 1000.0;
                        let t = Instant::now();
                        d.submit(sid, &format!("a plain turn {i}"))?;
                        out.push((at, t.elapsed().as_secs_f64() * 1000.0));
                        i += 1;
                    }
                    Ok(out)
                })
            })
            .collect();
        each.into_iter()
            .map(|h| h.join().expect("a session's thread"))
            .collect::<Result<Vec<_>>>()
            .map(|v| v.into_iter().flatten().collect())
    })?;
    let mut d = d;
    let frames = d.tail.read()?;
    let judge_frames = frames.iter().filter(|f| is_judges(f)).count();
    let blobs = std::fs::read_dir(s.rig.state.join("store").join("blobs"))
        .map(|r| r.count())
        .unwrap_or(0);
    let stop_ms = s.rig.stop(&mut daemon)?;
    println!(
        "burst · {} · {SESSIONS} sessions of back-to-back turns for {secs} s, the judge on at the fake Jev, every pack as wired",
        theseusd.display()
    );
    let windows = (secs * 1000).div_ceil(WINDOW.as_millis() as u64) as usize;
    for w in 0..windows {
        let (from, to) = (
            (w as u128 * WINDOW.as_millis()) as f64,
            ((w as u128 + 1) * WINDOW.as_millis()) as f64,
        );
        let mut ms: Vec<f64> = walls
            .iter()
            .filter(|(at, _)| *at >= from && *at < to)
            .map(|(_, ms)| *ms)
            .collect();
        if ms.is_empty() {
            continue;
        }
        let n = ms.len();
        let (p50, p95) = p50_p95(&mut ms);
        println!(
            "  {:>4}-{:<4} s  {n:>4} turns  wall p50 {p50:7.1} ms  p95 {p95:7.1} ms",
            from / 1000.0,
            to / 1000.0
        );
    }
    let mut all: Vec<f64> = walls.iter().map(|(_, ms)| *ms).collect();
    let (p50, p95) = p50_p95(&mut all);
    println!(
        "  all          {:>4} turns  wall p50 {p50:7.1} ms  p95 {p95:7.1} ms",
        all.len()
    );
    println!(
        "  over the burst: {judge_frames} judge frames of {} frames; {blobs} blobs; Jev called {} times",
        frames.len(),
        jev.connections()
    );
    let log = std::fs::read_to_string(&s.rig.log).context("the daemon's log")?;
    let line = log
        .lines()
        .find(|l| l.contains("the judge's settled judgments are written"))
        .unwrap_or("(no judgments were left for the stop)");
    println!("  shutdown: {stop_ms:.1} ms to exit; {line}");
    Ok(())
}

/// See the module's doc: `THESEUS_BURST_THESEUSD` (required),
/// `THESEUS_BURST_SECS` (180), `THESEUS_BURST_DIR` (a temporary one).
#[test]
#[ignore = "a measure: run it alone, on a frozen build"]
fn burst_of_three_sessions_with_the_judge_on() -> Result<()> {
    let theseusd: PathBuf = std::env::var_os("THESEUS_BURST_THESEUSD")
        .context("THESEUS_BURST_THESEUSD names the theseusd to measure")?
        .into();
    let secs = std::env::var("THESEUS_BURST_SECS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(180);
    let keep = tempfile::tempdir()?;
    let dir = std::env::var_os("THESEUS_BURST_DIR")
        .map_or_else(|| keep.path().to_path_buf(), PathBuf::from);
    burst(theseusd, secs, dir)
}
