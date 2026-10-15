//! `bench turn --soak-turns N` (theseus-0l3g): one session grown by `N` turns
//! on the stand-in model, back to back, every fifth with a tool call, and the
//! daemon's CPU and resident memory read every `SAMPLE_EVERY` turns.
//!
//! What it shows is whether a turn's cost grows with the session: the CPU a
//! turn took in each window of turns (the process's `utime` and `stime`, which
//! keep the work of threads that ended, unlike the per-thread sum), and the
//! resident memory at its end. Two numbers judge it, against the turn-60
//! window (turns 50 to 100), the first after the warm-up:
//! - `cpu_ratio`: the last window's CPU a turn over the turn-60 window's;
//! - `rss_kb_per_turn`: the resident memory's growth a turn from the end of
//!   the turn-60 window to the last sample.
//!
//! `--check` fails a ratio over `CPU_RATIO_MAX` or a growth over
//! `RSS_KB_PER_TURN_MAX`.

use super::*;

/// Turns between samples.
pub const SAMPLE_EVERY: usize = 50;
/// The last window's CPU a turn, at most this times the turn-60 window's.
pub const CPU_RATIO_MAX: f64 = 1.5;
/// Resident growth a turn after the warm-up, at most this many kB.
pub const RSS_KB_PER_TURN_MAX: f64 = 10.0;

pub struct SoakOpts {
    pub theseusd: PathBuf,
    pub turns: usize,
    pub dir: Option<PathBuf>,
}

/// One window of turns.
#[derive(Debug, Clone, Serialize)]
pub struct Window {
    /// The turns done at its end.
    pub turns: usize,
    /// The daemon's CPU a turn in the window, in ms.
    pub cpu_ms_per_turn: f64,
    /// The bench's wall a turn in the window, in ms.
    pub wall_ms_per_turn: f64,
    /// Resident memory at its end, in kB.
    pub rss_kb: u64,
}

#[derive(Debug, Serialize)]
pub struct SoakReport {
    pub theseusd: String,
    pub turns: usize,
    pub windows: Vec<Window>,
    pub cpu_ratio: f64,
    pub rss_kb_per_turn: f64,
    pub bench_ms: f64,
}

impl SoakReport {
    pub fn ok(&self) -> bool {
        self.cpu_ratio <= CPU_RATIO_MAX && self.rss_kb_per_turn <= RSS_KB_PER_TURN_MAX
    }
}

/// The CPU ratio and the memory's growth a turn, from the windows: against
/// the window that ends at turn 100 (or the first, in a shorter run).
pub fn judge(windows: &[Window]) -> (f64, f64) {
    let (Some(first), Some(last)) = (
        windows
            .iter()
            .find(|w| w.turns >= 2 * SAMPLE_EVERY)
            .or(windows.first()),
        windows.last(),
    ) else {
        return (0.0, 0.0);
    };
    let ratio = last.cpu_ms_per_turn / first.cpu_ms_per_turn.max(f64::MIN_POSITIVE);
    let turns = last.turns.saturating_sub(first.turns).max(1);
    let grew = last.rss_kb as f64 - first.rss_kb as f64;
    (ratio, (grew / turns as f64).max(0.0))
}

pub fn run_soak(o: &SoakOpts) -> Result<SoakReport> {
    let t0 = Instant::now();
    let s = scratch(&o.theseusd, o.dir.as_deref())?;
    let (mut daemon, _start) = s.rig.start()?;
    let pid = daemon.0.id();
    let mut tail = Tail::at_start(&s.wal());
    after_serving(&mut tail)?;
    let d = Driver { rig: &s.rig, tail };
    let session = d.open_session("bench soak")?;
    let mut windows = Vec::new();
    let (mut before, mut since) = (procfs::sample(pid)?, Instant::now());
    for i in 0..o.turns {
        let input = match i % 5 {
            4 => format!("soak turn {i}, {TOOL_MARK}"),
            _ => format!("soak turn {i}"),
        };
        let r = d.submit(&session, &input)?;
        if r["output"].is_null() && r["error"].is_object() {
            bail!("soak turn {i} failed: {r}");
        }
        if (i + 1) % SAMPLE_EVERY == 0 {
            if let Some(dir) = &o.dir {
                std::fs::write(dir.join(format!("soak-turn-{}.json", i + 1)), r.to_string())?;
                let h = s.rig.call("health", json!({}))?;
                std::fs::write(
                    dir.join(format!("soak-health-{}.json", i + 1)),
                    h.to_string(),
                )?;
            }
            let now = procfs::sample(pid)?;
            let n = SAMPLE_EVERY as f64;
            let w = Window {
                turns: i + 1,
                cpu_ms_per_turn: now.cpu_ns.saturating_sub(before.cpu_ns) as f64 / 1e6 / n,
                wall_ms_per_turn: ms_since(since) / n,
                rss_kb: now.rss_kb,
            };
            println!(
                "  turns {:>5}: cpu {:>7.1} ms a turn, wall {:>7.1} ms a turn, rss {:>8.1} MB",
                w.turns,
                w.cpu_ms_per_turn,
                w.wall_ms_per_turn,
                w.rss_kb as f64 / 1024.0
            );
            windows.push(w);
            (before, since) = (now, Instant::now());
        }
    }
    // The daemon trims its heap after a quiet spell (`resident`): what it
    // still holds then is kept, not freed and held.
    std::thread::sleep(Duration::from_secs(12));
    let quiet = procfs::sample(pid)?;
    println!(
        "  after 12 s quiet: rss {:.1} MB",
        quiet.rss_kb as f64 / 1024.0
    );
    s.rig.stop(&mut daemon)?;
    let (cpu_ratio, rss_kb_per_turn) = judge(&windows);
    Ok(SoakReport {
        theseusd: o.theseusd.display().to_string(),
        turns: o.turns,
        windows,
        cpu_ratio,
        rss_kb_per_turn,
        bench_ms: ms_since(t0),
    })
}

pub fn print_soak(r: &SoakReport) {
    println!(
        "bench turn --soak-turns {} · {}: cpu a turn {:.2}x the turn-60 window's (at most {CPU_RATIO_MAX}), \
         resident growth {:.2} kB a turn after it (at most {RSS_KB_PER_TURN_MAX}); the bench took {:.1} s",
        r.turns,
        r.theseusd,
        r.cpu_ratio,
        r.rss_kb_per_turn,
        r.bench_ms / 1000.0
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    fn w(turns: usize, cpu: f64, rss_kb: u64) -> Window {
        Window {
            turns,
            cpu_ms_per_turn: cpu,
            wall_ms_per_turn: 0.0,
            rss_kb,
        }
    }

    /// The judge reads against the window ending at turn 100.
    #[test]
    fn the_soak_is_judged_against_the_turn_sixty_window() {
        let windows = [w(50, 40.0, 1000), w(100, 20.0, 2000), w(300, 30.0, 4000)];
        let (ratio, kb) = judge(&windows);
        assert!((ratio - 1.5).abs() < 1e-9, "{ratio}");
        assert!((kb - 10.0).abs() < 1e-9, "{kb}");
        assert_eq!(judge(&[]), (0.0, 0.0));
    }
}
