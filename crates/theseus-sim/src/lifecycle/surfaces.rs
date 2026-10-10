//! The `surfaces` phase (theseus-7bee): what the operator waits for at a
//! prompt on a store with a great many sessions. `theseus watch
//! --interactive` from its spawn to the line that says it reads input, and
//! the terminal UI's first screen, which is its startup reads: `executions.watch`
//! (the board) and `confirm.list` (the questions), on one fresh connection.
//! Each run's sample is the slower of the two, so the budget holds both; the
//! rows print apart. It runs on `--sessions N` (`--phases surfaces`), where
//! the CLI's asking the daemon for every session showed: 543 ms at 21,779
//! sessions (the scale report's row 2). §9's 50 ms holds at any size.

use std::collections::BTreeMap;
use std::io::{BufRead, BufReader};
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use anyhow::{bail, Context, Result};
use serde_json::{json, Value};

use super::{percentile, tail, Rig};

/// What `watch --interactive` prints to stderr once it has its session and
/// reads input.
const PROMPT: &str = "a line you type is sent to the session";

/// §FAST: the slower surface's budget at any size, in ms.
pub const BUDGET_MS: f64 = 50.0;

/// `theseus watch --interactive` from spawn to its prompt, in ms; the
/// process is killed after it.
fn watch_prompt(rig: &Rig, theseus: &Path) -> Result<f64> {
    let t = Instant::now();
    let mut child = Command::new(theseus)
        .arg("--socket")
        .arg(&rig.sock)
        .args(["watch", "--interactive"])
        .env_remove("THESEUS_SOCKET")
        .env_remove("THESEUS_SESSION")
        .env_remove("HERDR_ENV")
        .env_remove("HERDR_PANE_ID")
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .with_context(|| format!("running {}", theseus.display()))?;
    let mut err = BufReader::new(child.stderr.take().context("no stderr")?);
    let mut line = String::new();
    let found = loop {
        line.clear();
        if err.read_line(&mut line)? == 0 {
            break false;
        }
        if line.contains(PROMPT) {
            break true;
        }
    };
    let ms = t.elapsed().as_secs_f64() * 1000.0;
    let _ = child.kill();
    let _ = child.wait();
    if !found {
        bail!(
            "theseus watch --interactive ended before its prompt; the daemon's log ends:\n{}",
            tail(&rig.log)
        );
    }
    Ok(ms)
}

/// The terminal UI's startup reads, in ms: the board's snapshot and the
/// questions. (A pseudo-terminal would time its drawing too, and adds noise
/// that is not the daemon's.)
fn tui_ready(rig: &Rig) -> Result<f64> {
    let t = Instant::now();
    rig.call("executions.watch", json!({}))?;
    rig.call("confirm.list", Value::Null)?;
    Ok(t.elapsed().as_secs_f64() * 1000.0)
}

/// Until the daemon's newest-session read is quick: the index's shape is built
/// after serving, and while it is not, that read is every session's.
fn until_shaped(rig: &Rig) -> Result<()> {
    let deadline = Instant::now() + Duration::from_secs(120);
    let mut quick = 0;
    while quick < 3 {
        let t = Instant::now();
        rig.call("session.list", json!({"n": 1}))?;
        if t.elapsed().as_secs_f64() * 1000.0 < BUDGET_MS / 2.0 {
            quick += 1;
        } else {
            quick = 0;
            std::thread::sleep(Duration::from_millis(50));
        }
        if Instant::now() > deadline {
            bail!("surfaces: the newest session was not read quickly within 120 s");
        }
    }
    Ok(())
}

/// Runs the phase on a daemon of its own, and puts each run's slower surface
/// under `surfaces`.
pub(super) fn phase(
    rig: &Rig,
    theseus: &Path,
    runs: usize,
    samples: &mut BTreeMap<String, Vec<f64>>,
) -> Result<()> {
    if !theseus.is_file() {
        bail!(
            "surfaces: no {} beside the daemon; build it, or name a build with --theseusd",
            theseus.display()
        );
    }
    let (mut daemon, _) = rig.start()?;
    let measured = (|| -> Result<()> {
        until_shaped(rig)?;
        // One of each unmeasured: the first watch seeds the board.
        watch_prompt(rig, theseus)?;
        tui_ready(rig)?;
        let (mut watch, mut tui) = (Vec::new(), Vec::new());
        for _ in 0..runs {
            let (w, u) = (watch_prompt(rig, theseus)?, tui_ready(rig)?);
            samples.entry("surfaces".into()).or_default().push(w.max(u));
            watch.push(w);
            tui.push(u);
        }
        for (name, mut v) in [("watch --interactive", watch), ("tui startup reads", tui)] {
            v.sort_by(f64::total_cmp);
            eprintln!(
                "  surfaces: {name}: p50 {:.1} ms, p95 {:.1} ms",
                percentile(&v, 50.0),
                percentile(&v, 95.0)
            );
        }
        Ok(())
    })();
    rig.stop_anyhow(&mut daemon);
    measured
}
