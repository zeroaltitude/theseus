//! What a `job_wait` that returns with its job still running says it saw
//! (theseus-53ujg.6): how long the job has run and this wait lasted, its CPU
//! time where its cgroup gives one, and its output, through the one tail
//! reader (`peek.rs`). Facts only: nothing here says what to do.
//!
//! A job is silent for a wait while its output file has not grown since the
//! wait began or since the last growth a look saw (`Silence`); `[tools]
//! job_wait_beat_secs` of that returns the wait early, and the wait's other
//! still-running result carries the same facts (`text`).

use std::path::PathBuf;
use std::time::Duration;

use tokio::time::Instant;

use serde_json::json;
use theseus_kernel::Action;
use theseus_tools::ToolOutput;

use super::{Found, KEY};
use crate::narrative;
use crate::toolrun::peek::{peek, Peek};
use crate::Core;

/// The lines of its output a wait's facts show.
const LAST_LINES: u64 = 10;
/// Output that stopped longer ago than this in a wait is "no new output", not
/// "it printed during this wait".
const LIVE: Duration = Duration::from_secs(60);

/// How long a job's output file has stood still, as a wait's looks saw it.
pub(super) struct Silence {
    path: Option<PathBuf>,
    size: u64,
    since: Instant,
    /// The output grew at some look of this wait.
    grew: bool,
}

impl Silence {
    /// The file's size now: the wait began. A stat: call it where a wait for
    /// the disk may block.
    pub(super) fn start(path: Option<PathBuf>) -> Self {
        let size = size_of(path.as_deref());
        Self {
            path,
            size,
            since: Instant::now(),
            grew: false,
        }
    }

    /// One look: growth since the last restarts the window.
    pub(super) fn look(&mut self) {
        let size = size_of(self.path.as_deref());
        if size > self.size {
            self.grew = true;
            self.since = Instant::now();
        }
        self.size = size;
    }

    /// How long it has stood still.
    pub(super) fn quiet(&self) -> Duration {
        self.since.elapsed()
    }

    /// The time until it has stood still for `beat`.
    pub(super) fn until(&self, beat: Duration) -> Duration {
        beat.saturating_sub(self.quiet())
    }

    pub(super) fn grew(&self) -> bool {
        self.grew
    }

    pub(super) fn path(&self) -> Option<&std::path::Path> {
        self.path.as_deref()
    }
}

fn size_of(path: Option<&std::path::Path>) -> u64 {
    path.and_then(|p| std::fs::metadata(p).ok())
        .map_or(0, |m| m.len())
}

/// What a wait saw of its job.
pub(super) struct Facts {
    /// `j7 (cargo test --workspace)`.
    pub called: String,
    pub ran_ms: u64,
    pub waited_ms: u64,
    /// The seconds the call asked for; set when the wait returned at the beat.
    pub beat_of: Option<u64>,
    /// The job's CPU time so far, in µs, where it has a cgroup.
    pub cpu_usec: Option<u64>,
    pub peek: Peek,
    /// Seconds with no new output, none when it printed during the wait.
    pub quiet_secs: Option<u64>,
}

/// The facts, in lines: the run, the CPU, the output, and at the beat what
/// the wait was.
pub(super) fn text(f: &Facts) -> String {
    let ran = narrative::duration(f.ran_ms);
    let waited = narrative::duration(f.waited_ms);
    let mut out = match f.beat_of {
        Some(_) => format!(
            "Job {} still running after {ran}; this wait lasted {waited}.",
            f.called
        ),
        None => format!("Job {} has run {ran}; this wait lasted {waited}.", f.called),
    };
    let ran_s = f.ran_ms / 1000;
    if let (Some(cpu), true) = (f.cpu_usec, ran_s > 0) {
        let cpu_s = cpu / 1_000_000;
        out.push_str(&format!(
            "\nCPU {} s in {} s, about {:.1} CPU.",
            narrative::thousands(cpu_s),
            narrative::thousands(ran_s),
            cpu as f64 / 1e6 / ran_s as f64
        ));
    }
    out.push('\n');
    out.push_str(&output(f));
    if let Some(asked) = f.beat_of {
        out.push_str(&format!(
            "\n[the wait returned at {} s of {asked} s asked: the job printed nothing in that \
             time]",
            f.waited_ms / 1000
        ));
    }
    out
}

fn output(f: &Facts) -> String {
    let p = &f.peek;
    if p.bytes == 0 {
        return "No output since it started.".into();
    }
    let so_far = match p.lines {
        Some(n) => format!("{} so far", narrative::count(n, "line", "lines")),
        None => format!("{} so far", narrative::bytes(p.bytes)),
    };
    let last = match (p.shown, p.lines) {
        (n, Some(all)) if n == all => ":".to_string(),
        (n, _) => format!("; the last {}:", narrative::count(n, "line", "lines")),
    };
    let said = match f.quiet_secs {
        Some(q) => format!("No new output for {q} s; {so_far}{last}"),
        None => format!("It printed during this wait; {so_far}{last}"),
    };
    let mut s = format!("{said}\n{}", p.text);
    if p.held {
        s.push_str(
            "\n[its output has passed what its file keeps from the start: its newest lines wait \
             in its wrapper until it ends]",
        );
    }
    s
}

/// When a wait began, and how long its job had run by then.
pub(super) struct Clock {
    began: Instant,
    ran0_ms: u64,
}

impl Clock {
    pub(super) fn start(job: &Action) -> Self {
        let since = job.dispatched_at_ms.unwrap_or(job.planned_at_ms);
        Self {
            began: Instant::now(),
            ran0_ms: theseus_protocol::now_unix_ms().saturating_sub(since),
        }
    }

    /// How long the job has run, as of now.
    pub(super) fn ran_ms(&self) -> u64 {
        self.ran0_ms + self.began.elapsed().as_millis() as u64
    }

    /// The still-running result's facts. Reads the disk.
    pub(super) fn said(&self, core: &Core, f: &Found, silence: &Silence) -> String {
        text(&self.read(core, f, silence, None))
    }

    /// The result of a wait that returned at the beat; none when the job's
    /// output is held in its wrapper, which is not silence. Reads the disk.
    pub(super) fn beat(
        &self,
        core: &Core,
        f: &Found,
        silence: &Silence,
        asked: u64,
        reads: u32,
    ) -> Option<ToolOutput> {
        let seen = self.read(core, f, silence, Some(asked));
        let meta = json!({KEY: f.name, "correlation_id": f.corr, "state": "running",
            "beat": true, "silent_secs": silence.quiet().as_secs(), "warm_reads": reads});
        (!seen.peek.held).then(|| ToolOutput {
            text: text(&seen),
            meta,
        })
    }

    fn read(&self, core: &Core, f: &Found, silence: &Silence, beat_of: Option<u64>) -> Facts {
        let waited = self.began.elapsed();
        read(core, f, silence, self.ran_ms(), waited, beat_of)
    }
}

/// What the wait saw of `f`: its output file, and its cgroup's CPU time.
fn read(
    core: &Core,
    f: &Found,
    silence: &Silence,
    ran_ms: u64,
    waited: Duration,
    beat_of: Option<u64>,
) -> Facts {
    let peek = silence.path().map_or_else(Peek::default, |p| {
        peek(p, LAST_LINES, core.tools.output_max_bytes)
    });
    let quiet = silence.quiet();
    let quiet_secs = match silence.grew() && quiet < LIVE {
        true => None,
        false => {
            // A file quiet since before the wait began has been so longer.
            let stood = silence
                .path()
                .and_then(|p| std::fs::metadata(p).ok()?.modified().ok()?.elapsed().ok())
                .unwrap_or_default();
            Some(quiet.max(stood).as_secs())
        }
    };
    Facts {
        called: f.called(),
        ran_ms,
        waited_ms: waited.as_millis() as u64,
        beat_of,
        cpu_usec: theseus_kernel::cgroup::job_cpu_usec(&f.corr),
        peek,
        quiet_secs,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn facts(cpu: Option<u64>, bytes: u64, quiet: Option<u64>, beat: Option<u64>) -> String {
        text(&Facts {
            called: "j1 (bash -c ./attack)".into(),
            ran_ms: 600_000,
            waited_ms: 600_000,
            beat_of: beat,
            cpu_usec: cpu,
            peek: Peek {
                text: "round 3".into(),
                shown: 1,
                lines: Some(3),
                bytes,
                held: false,
            },
            quiet_secs: quiet,
        })
    }

    #[test]
    fn a_cpu_line_needs_a_reading() {
        let with = facts(Some(598_000_000), 20, Some(120), None);
        assert!(
            with.contains("CPU 598 s in 600 s, about 1.0 CPU."),
            "{with}"
        );
        let without = facts(None, 20, Some(120), None);
        assert!(!without.contains("CPU"), "{without}");
    }

    #[test]
    fn the_output_line_says_what_was_seen() {
        let none = facts(None, 0, Some(600), Some(1500));
        assert!(none.contains("No output since it started."), "{none}");
        assert!(
            none.contains("[the wait returned at 600 s of 1500 s asked"),
            "{none}"
        );
        let quiet = facts(None, 20, Some(120), None);
        assert!(
            quiet.contains("No new output for 120 s; 3 lines so far; the last 1 line:\nround 3"),
            "{quiet}"
        );
        let live = facts(None, 20, None, None);
        assert!(
            live.contains("It printed during this wait; 3 lines so far"),
            "{live}"
        );
    }
}
