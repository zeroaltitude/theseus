//! What a running daemon costs, read from `/proc` (theseus-goa8): its memory,
//! the CPU time it has used, and how often its threads were woken.
//!
//! - **Memory** is `VmRSS` (resident now) and `VmHWM` (its peak) from
//!   `/proc/<pid>/status`.
//! - **CPU** is the sum of every thread's run time in `/proc/<pid>/task/*/
//!   schedstat`, to the nanosecond; where a kernel lacks it, the process's
//!   `utime` and `stime` in clock ticks, to 10 ms.
//! - **Wakeups** are the voluntary context switches of every thread, summed
//!   from `/proc/<pid>/task/*/status`: each time a thread blocked on a timer,
//!   a socket, or a futex and was woken to run. `/proc/<pid>/status` alone
//!   counts the main thread only, so it would miss a tokio worker's.
//!
//! The parsers take text, so tests read them on samples.

use std::path::Path;

use anyhow::{bail, Context, Result};
use serde::Serialize;

/// A daemon's cost at one moment.
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize)]
pub struct Sample {
    /// Resident memory now, in kB.
    pub rss_kb: u64,
    /// Its peak, in kB.
    pub hwm_kb: u64,
    pub threads: u64,
    /// CPU time used since the process began, in ns.
    pub cpu_ns: u64,
    /// Voluntary context switches since each thread began, summed.
    pub wakeups: u64,
}

impl Sample {
    pub fn rss_mb(&self) -> f64 {
        self.rss_kb as f64 / 1024.0
    }

    pub fn hwm_mb(&self) -> f64 {
        self.hwm_kb as f64 / 1024.0
    }
}

/// The counts that one `/proc/<pid>/status` holds.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Status {
    pub rss_kb: u64,
    pub hwm_kb: u64,
    pub threads: u64,
    pub voluntary: u64,
}

/// A `/proc/<pid>/status` (or a thread's), by its `Name: value` lines. A line
/// that is absent reads as 0: a kernel thread has no `VmRSS`.
pub fn parse_status(text: &str) -> Status {
    let mut s = Status::default();
    for line in text.lines() {
        let Some((key, rest)) = line.split_once(':') else {
            continue;
        };
        let n = rest
            .split_whitespace()
            .next()
            .and_then(|v| v.parse::<u64>().ok())
            .unwrap_or(0);
        match key {
            "VmRSS" => s.rss_kb = n,
            "VmHWM" => s.hwm_kb = n,
            "Threads" => s.threads = n,
            "voluntary_ctxt_switches" => s.voluntary = n,
            _ => {}
        }
    }
    s
}

/// The `utime` and `stime` of a `/proc/<pid>/stat`, in clock ticks. The
/// command's name sits in parentheses and may hold spaces and parentheses, so
/// the fields are counted from the last `)`.
pub fn parse_stat_ticks(text: &str) -> Option<(u64, u64)> {
    let after = &text[text.rfind(')')? + 1..];
    // After the name: state is field 3, so utime (14) and stime (15) are the
    // 12th and 13th words from here.
    let mut words = after.split_whitespace();
    let utime = words.nth(11)?.parse().ok()?;
    let stime = words.next()?.parse().ok()?;
    Some((utime, stime))
}

/// The run time of one task, in ns: the first word of its `schedstat`.
pub fn parse_schedstat_ns(text: &str) -> Option<u64> {
    text.split_whitespace().next()?.parse().ok()
}

/// Clock ticks a second, for `stat`'s times.
fn clock_ticks() -> u64 {
    // SAFETY: sysconf reads a constant and touches no memory of ours.
    let t = unsafe { libc::sysconf(libc::_SC_CLK_TCK) };
    if t > 0 {
        t as u64
    } else {
        100
    }
}

/// Read what process `pid` costs now. An error when the process is gone.
pub fn sample(pid: u32) -> Result<Sample> {
    let root = Path::new("/proc").join(pid.to_string());
    let status = std::fs::read_to_string(root.join("status"))
        .with_context(|| format!("reading /proc/{pid}/status: is the process alive?"))?;
    let head = parse_status(&status);
    let mut out = Sample {
        rss_kb: head.rss_kb,
        hwm_kb: head.hwm_kb,
        threads: head.threads,
        ..Sample::default()
    };
    let (mut have_sched, mut cpu_ns) = (false, 0u64);
    let tasks = std::fs::read_dir(root.join("task"))
        .with_context(|| format!("reading /proc/{pid}/task"))?;
    for t in tasks.flatten() {
        // A thread that ends between the listing and the read is skipped:
        // what it did is lost to the sum, by at most one tick of its work.
        if let Ok(text) = std::fs::read_to_string(t.path().join("status")) {
            out.wakeups += parse_status(&text).voluntary;
        }
        if let Some(ns) = std::fs::read_to_string(t.path().join("schedstat"))
            .ok()
            .and_then(|s| parse_schedstat_ns(&s))
        {
            have_sched = true;
            cpu_ns += ns;
        }
    }
    if have_sched {
        out.cpu_ns = cpu_ns;
    } else {
        let stat = std::fs::read_to_string(root.join("stat"))?;
        let Some((u, s)) = parse_stat_ticks(&stat) else {
            bail!("/proc/{pid}/stat does not read as a stat line");
        };
        out.cpu_ns = (u + s) * (1_000_000_000 / clock_ticks());
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    const STATUS: &str = "Name:\ttheseusd\nUmask:\t0022\nState:\tS (sleeping)\n\
        VmPeak:\t  412344 kB\nVmHWM:\t   45120 kB\nVmRSS:\t   41984 kB\n\
        Threads:\t17\nvoluntary_ctxt_switches:\t4021\nnonvoluntary_ctxt_switches:\t9\n";

    #[test]
    fn a_status_gives_its_memory_threads_and_wakeups() {
        let s = parse_status(STATUS);
        assert_eq!(
            s,
            Status {
                rss_kb: 41984,
                hwm_kb: 45120,
                threads: 17,
                voluntary: 4021,
            }
        );
        assert_eq!(parse_status("Name:\tkthreadd\n"), Status::default());
    }

    #[test]
    fn a_stat_line_is_counted_from_its_last_parenthesis() {
        // The name holds a space and a parenthesis, as a thread's can.
        let line =
            "4242 (tokio-rt (wk 1) S 1 4242 4242 0 -1 4194560 1200 0 0 0 305 112 0 0 20 0 17 0 99 \
            1000000 500 18446744073709551615 0 0 0 0 0 0 0 0 0 0 0 0 17 3 0 0 0 0 0";
        assert_eq!(parse_stat_ticks(line), Some((305, 112)));
        assert_eq!(parse_stat_ticks("no parenthesis"), None);
        assert_eq!(parse_stat_ticks("1 (x) S 1 2"), None, "a short line");
    }

    #[test]
    fn a_schedstat_gives_the_run_time_in_ns() {
        assert_eq!(parse_schedstat_ns("123456789 5000 42\n"), Some(123_456_789));
        assert_eq!(parse_schedstat_ns(""), None);
    }

    #[test]
    fn this_process_reads() {
        // Enough work to show even in clock ticks, where schedstat is absent.
        let t = std::time::Instant::now();
        while t.elapsed() < std::time::Duration::from_millis(30) {
            std::hint::spin_loop();
        }
        let s = sample(std::process::id()).unwrap();
        assert!(s.rss_kb > 0 && s.hwm_kb >= s.rss_kb && s.threads >= 1);
        assert!(s.cpu_ns > 0, "a test that ran has used CPU");
        assert!(sample(u32::MAX).is_err(), "no such process");
    }
}
