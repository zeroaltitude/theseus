//! The on-demand sync's real spawn, deadline and kill (issue 6): the path that had only ever
//! run behind a swapped runner. Each test starts a real child through `run_command`, the code
//! `run` itself calls, with a command and a deadline of its own.

use std::time::{Duration, Instant};

use tokio::process::Command;

use super::sync_run::{run, run_command, Ran};

fn sh(script: &str) -> Command {
    let mut c = Command::new("/bin/sh");
    c.args(["-c", script])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .kill_on_drop(true);
    c
}

/// Whether a process with this pid still exists (a reaped child does not).
fn alive(pid: u32) -> bool {
    // SAFETY: signal 0 only asks whether the pid exists.
    unsafe { libc::kill(pid as i32, 0) == 0 }
}

/// A child that exits 0 inside its deadline is a run that worked, with its pid.
#[tokio::test]
async fn a_child_that_exits_clean_is_a_run_that_worked() {
    let ran = run_command(&mut sh("exit 0"), Duration::from_secs(30)).await;
    assert!(ran.ok(), "{ran:?}");
    assert!(ran.pid > 0, "it really started: {ran:?}");
    assert_eq!(ran.why(), None);
}

/// A child that exits non-zero says so, in the words the thread is told.
#[tokio::test]
async fn a_child_that_exits_non_zero_says_its_code() {
    let ran = run_command(&mut sh("exit 3"), Duration::from_secs(30)).await;
    assert_eq!((ran.code, ran.timed_out), (Some(3), false), "{ran:?}");
    assert_eq!(ran.why().as_deref(), Some("it exited 3"));
}

/// The pids whose command line holds `marker`.
fn with_marker(marker: &str) -> Vec<u32> {
    std::fs::read_dir("/proc")
        .unwrap()
        .flatten()
        .filter_map(|e| e.file_name().to_str()?.parse::<u32>().ok())
        .filter(|p| {
            std::fs::read(format!("/proc/{p}/cmdline"))
                .is_ok_and(|c| String::from_utf8_lossy(&c).contains(marker))
        })
        .collect()
}

/// KEY: a child that outlives its deadline is killed and reaped, and the run says it ran past.
/// Without the kill the child would stand for thirty seconds.
#[tokio::test]
async fn a_child_past_its_deadline_is_killed_and_reaped() {
    // `exec`: a `/bin/sh` that forks for a lone `-c` command (dash does) would leave the sleep as a
    // grandchild the kill never reaches, standing on with this test's stderr. The marker finds it.
    let marker = format!("30.{}", std::process::id());
    let started = Instant::now();
    let ran = run_command(
        &mut sh(&format!("exec sleep {marker}")),
        Duration::from_millis(300),
    )
    .await;
    assert!(ran.timed_out, "{ran:?}");
    assert!(!ran.ok());
    assert!(
        ran.why().is_some_and(|w| w.contains("was stopped")),
        "{:?}",
        ran.why()
    );
    assert!(
        started.elapsed() < Duration::from_secs(20),
        "it did not wait for the child: {:?}",
        started.elapsed()
    );
    assert!(ran.pid > 0 && !alive(ran.pid), "the child is gone: {ran:?}");
    let left = with_marker(&format!("sleep\0{marker}"));
    assert!(left.is_empty(), "nothing it started is left: {left:?}");
}

/// A command that cannot start is an error in words, not a panic, and has no pid.
#[tokio::test]
async fn a_command_that_cannot_start_is_said() {
    let mut cmd = Command::new("/nonexistent/theseus-sync");
    let ran = run_command(&mut cmd, Duration::from_secs(30)).await;
    assert_eq!(ran.pid, 0);
    assert!(
        ran.error
            .as_deref()
            .is_some_and(|e| e.contains("did not start")),
        "{ran:?}"
    );
}

/// The production entry: the one fixed command, whose binary is `theseus` beside this process's
/// own image. A test binary has none beside it, so `run` reports that it did not start, through
/// the same spawn and the real environment rebuild: the path is reached, not swapped.
#[tokio::test]
async fn the_production_entry_reaches_the_real_spawn() {
    let ran: Ran = run().await;
    assert!(!ran.ok(), "{ran:?}");
    assert!(
        ran.error
            .as_deref()
            .is_some_and(|e| e.contains("did not start")),
        "{ran:?}"
    );
}
