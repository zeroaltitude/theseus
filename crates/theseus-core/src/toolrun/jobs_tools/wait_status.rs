//! The line a person gets in their place at each beat of a silent `job_wait`
//! (theseus-5d204): the job, how long it has run and printed nothing, its CPU
//! where its cgroup gives a reading, and its last line. Written by the
//! daemon from facts the beat already holds, with no model call; the model's
//! result is `wait_facts`'s and never sees this line.

use std::time::Duration;

use serde_json::json;

use super::wait_facts::{Clock, Silence};
use super::Found;
use crate::narrative;
use crate::toolrun::peek::last_line;
use crate::Core;

/// The longest the job's name and the last line run in the line.
const CALLED_MAX: usize = 80;
const LAST_MAX: usize = 160;

/// Post one status line to the session's place, when `[tools]
/// job_wait_status` is on and the session has a place. The wait's result
/// never depends on it: a failed post is logged and dropped. Reads the disk
/// and writes a frame.
pub(super) fn post(core: &Core, f: &Found, clock: &Clock, silence: &Silence) {
    if !core.cfg.tools.job_wait_status {
        return;
    }
    let last = silence.path().and_then(last_line);
    let line = line(
        &f.called(),
        clock.ran_ms(),
        silence.quiet(),
        theseus_kernel::cgroup::job_cpu_usec(&f.corr),
        last.as_deref(),
    );
    // A job's last line can carry a secret: nothing leaves unscrubbed.
    let (text, _) = core.tools.scrubber.scrub(&line);
    let a = &f.action;
    let body = json!({"kind": "notice", "text": text});
    if let Err(e) = core.outbox.post_for(&a.session_id, &a.execution_id, body) {
        tracing::warn!(job = %f.name, "job_wait: the status line was not posted: {e:#}");
    }
}

/// The line, on one line: `Job j3 (cargo test) is still running: 10 min 0 s
/// so far, no new output for 10 min 0 s, about 3.9 CPU. Last line: "ok"`.
fn line(
    called: &str,
    ran_ms: u64,
    quiet: Duration,
    cpu_usec: Option<u64>,
    last: Option<&str>,
) -> String {
    let last = last.map(one_line).filter(|l| !l.is_empty());
    let mut facts = vec![format!("{} so far", narrative::duration(ran_ms))];
    facts.push(match last {
        Some(_) => format!(
            "no new output for {}",
            narrative::duration(quiet.as_millis() as u64)
        ),
        None => "no output since it started".into(),
    });
    let ran_s = ran_ms / 1000;
    if let (Some(cpu), true) = (cpu_usec, ran_s > 0) {
        facts.push(format!("about {:.1} CPU", cpu as f64 / 1e6 / ran_s as f64));
    }
    let mut out = format!(
        "Job {} is still running: {}.",
        cut(&one_line(called), CALLED_MAX),
        facts.join(", ")
    );
    if let Some(l) = last {
        out.push_str(&format!(" Last line: \"{}\"", cut(&l, LAST_MAX)));
    }
    out
}

/// Whitespace runs and control characters as single spaces.
fn one_line(s: &str) -> String {
    s.split(|c: char| c.is_whitespace() || c.is_control())
        .filter(|w| !w.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

/// At most `max` characters, the last an ellipsis when it cut.
fn cut(s: &str, max: usize) -> String {
    match s.chars().count() > max {
        true => format!("{}…", s.chars().take(max - 1).collect::<String>()),
        false => s.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TEN_MIN: Duration = Duration::from_secs(600);

    #[test]
    fn the_line_says_what_the_job_did() {
        let l = line(
            "j3 (cargo test)",
            600_000,
            TEN_MIN,
            Some(2_340_000_000),
            Some("ok"),
        );
        assert_eq!(
            l,
            "Job j3 (cargo test) is still running: 10 min 0 s so far, no new output for \
             10 min 0 s, about 3.9 CPU. Last line: \"ok\""
        );
    }

    #[test]
    fn cpu_needs_a_reading_and_a_run_time() {
        let with = line("j1 (x)", 600_000, TEN_MIN, Some(598_000_000), Some("a"));
        assert!(with.contains("about 1.0 CPU"), "{with}");
        let none = line("j1 (x)", 600_000, TEN_MIN, None, Some("a"));
        assert!(!none.contains("CPU"), "{none}");
        let instant = line("j1 (x)", 400, TEN_MIN, Some(5_000_000), Some("a"));
        assert!(!instant.contains("CPU"), "{instant}");
    }

    #[test]
    fn no_output_says_so() {
        let l = line("j1 (bash -c ./search.sh)", 1_500_000, TEN_MIN, None, None);
        assert_eq!(
            l,
            "Job j1 (bash -c ./search.sh) is still running: 25 min 0 s so far, no output since \
             it started."
        );
        let blank = line("j1 (x)", 600_000, TEN_MIN, None, Some("  \t "));
        assert!(blank.contains("no output since it started"), "{blank}");
    }

    #[test]
    fn long_names_and_lines_are_cut_with_an_ellipsis() {
        let called = format!("j1 ({})", "a".repeat(200));
        let last = "b".repeat(500);
        let l = line(&called, 60_000, TEN_MIN, None, Some(&last));
        let (head, tail) = l.split_once(" is still running").unwrap();
        assert_eq!(head.trim_start_matches("Job ").chars().count(), CALLED_MAX);
        assert!(head.ends_with('…'), "{head}");
        let quoted = tail.split('"').nth(1).unwrap();
        assert_eq!(quoted.chars().count(), LAST_MAX);
        assert!(quoted.ends_with('…'), "{quoted}");
    }

    #[test]
    fn the_line_holds_no_newline() {
        let l = line(
            "j1 (bash -c 'a\nb')",
            60_000,
            TEN_MIN,
            None,
            Some("x\ny\r\nz"),
        );
        assert!(!l.contains(['\n', '\r']), "{l:?}");
        // A progress bar's rewrites come from the one tail reader as the last.
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("out");
        std::fs::write(&path, "10%\r50%\r100%\n").unwrap();
        let l = line("j1 (x)", 60_000, TEN_MIN, None, last_line(&path).as_deref());
        assert!(l.ends_with("Last line: \"100%\""), "{l}");
    }
}
