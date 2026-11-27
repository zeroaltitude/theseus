//! A stop never waits behind a self build (issue 63), with the real `theseusd`: a self build's command started
//! through the gate's own launcher (the debug build's plant, `THESEUS_TEST_SELF_JOB`: `Jail::run`, in L1, waited
//! for on the runtime's blocking pool) that would not end by itself for an hour. The runtime's drop waited for that
//! thread with no bound, so the stop's phases all ran and the process never ended. Now the stop ends the job, every
//! process of it, and goes on: within seconds, with nothing of the job left and nothing else signalled.
//!
//! The restart in place, the same end, is `config_copy.rs`'s
//! `a_restart_in_place_ends_a_self_build_that_would_not_end_by_itself`.
//!
//! Real L1, unprivileged: as root L1 refuses every job (theseus-pv6i), and then there is nothing to stop.

mod common;

use std::os::unix::process::CommandExt;
use std::process::Command;
use std::time::{Duration, Instant};

use common::{alive, descendants, Served};
use serde_json::Value;

/// How long a stop may take with a self build running: the job's grace (2 s), the kill's wait (2 s) and the
/// stop's own phases, with room for a loaded machine. The stop that waited for the job took as long as the job.
const STOPPED_WITHIN: Duration = Duration::from_secs(20);

fn as_root() -> bool {
    if unsafe { libc::geteuid() } == 0 {
        eprintln!(
            "skipped: L1 refuses a root daemon's jobs (theseus-pv6i); run as an ordinary user"
        );
        return true;
    }
    false
}

/// A daemon in a process group of its own, with the plant's command `body`.
fn start(body: &str) -> Served {
    let body = body.to_string();
    Served::start_with(
        |_| {},
        |_| {},
        move |cmd| {
            cmd.env("THESEUS_TEST_SELF_JOB", body).process_group(0);
        },
    )
}

/// The plant's job, once it runs: the pids of every process below the daemon whose `comm` is `comm`.
fn job_running(s: &mut Served, comm: &str) -> Vec<u32> {
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        let found: Vec<u32> = descendants(s.daemon.id())
            .into_iter()
            .filter(|(_, c)| c == comm)
            .map(|(p, _)| p)
            .collect();
        if !found.is_empty() {
            return found;
        }
        if let Some(status) = s.daemon.try_wait() {
            panic!(
                "theseusd exited ({status}) before the job ran:\n{}",
                s.log()
            );
        }
        assert!(
            Instant::now() < deadline,
            "the planted self job never ran:\n{}",
            s.log()
        );
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// The protocol's `shutdown`, then the daemon's exit: how long it took, or a failure that says the stop waited.
fn stop(s: &mut Served) -> Result<Duration, String> {
    let t0 = Instant::now();
    let _ = s.call("shutdown", Value::Null);
    while s.daemon.try_wait().is_none() {
        if t0.elapsed() > STOPPED_WITHIN {
            return Err(format!(
                "the daemon had not ended {STOPPED_WITHIN:?} after the stop: it waits for the job:\n{}",
                common::tail(&s.log(), 40)
            ));
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    Ok(t0.elapsed())
}

/// KEY: a self build that would run for an hour is ended at a stop, every process of it, and the stop goes on.
/// What the cut job returns is never read: the task that waited for it is gone first.
#[test]
fn a_stop_ends_a_self_build_that_would_not_end_by_itself() {
    if as_root() {
        return;
    }
    let mut s = start("exec sleep 3600");
    let job = job_running(&mut s, "sleep");
    let took = stop(&mut s).unwrap_or_else(|e| panic!("{e}"));
    for p in &job {
        assert!(!alive(*p), "the job's sleep {p} outlived the stop");
    }
    let log = s.log();
    assert!(
        !log.contains("planted self job: its result came back"),
        "the cut job's end was read as its result:\n{log}"
    );
    eprintln!("stopped in {took:?}");
}

/// A job that ignores SIGTERM is killed when the grace ends, and the stop still ends within its time.
#[test]
fn a_self_build_that_ignores_sigterm_is_killed_at_the_stop() {
    if as_root() {
        return;
    }
    let mut s = start("trap '' TERM; while :; do sleep 1; done");
    job_running(&mut s, "sleep");
    let shells: Vec<u32> = descendants(s.daemon.id())
        .into_iter()
        .filter(|(_, c)| c == "sh")
        .map(|(p, _)| p)
        .collect();
    assert!(!shells.is_empty(), "the job's shell");
    let took = stop(&mut s).unwrap_or_else(|e| panic!("{e}"));
    for p in &shells {
        assert!(
            !alive(*p),
            "the job's shell {p}, which ignores SIGTERM, outlived the stop"
        );
    }
    eprintln!("stopped in {took:?}");
}

/// The job's init is in the daemon's process group; the stop ends the job through its own namespace and never
/// signals the group: a process beside the daemon in it is still running after the stop.
#[test]
fn a_stop_never_signals_the_daemons_process_group() {
    if as_root() {
        return;
    }
    let mut s = start("exec sleep 3600");
    job_running(&mut s, "sleep");
    let pgid = s.daemon.id() as i32;
    let mut sibling = Command::new("sleep")
        .arg("120")
        .process_group(pgid)
        .spawn()
        .expect("a sibling in the daemon's group");
    let stopped = stop(&mut s);
    let still = sibling.try_wait().expect("the sibling's status").is_none();
    let _ = sibling.kill();
    let _ = sibling.wait();
    stopped.unwrap_or_else(|e| panic!("{e}"));
    assert!(
        still,
        "a process in the daemon's group was ended by the stop"
    );
}

/// With no self build running, the stop is as quick as it was, and says it waits for nothing.
#[test]
fn a_stop_with_no_self_build_is_quick() {
    let mut s = Served::start(|_| {}, |_| {});
    let took = stop(&mut s).unwrap_or_else(|e| panic!("{e}"));
    assert!(
        took < Duration::from_secs(5),
        "a stop with nothing running took {took:?}"
    );
    let log = s.log();
    assert!(
        !log.contains("stop: still waiting"),
        "a stop with nothing running waited:\n{log}"
    );
}

/// A stop whose terminal close does not return (the debug build's plant holds it 20 s, as a program stuck in the
/// kernel holds its reap) goes on past it within the close's bound (10 s), and says so; the stop's later phases
/// still run.
#[test]
fn a_terminal_close_that_does_not_return_does_not_hold_the_stop() {
    const HOLD: Duration = Duration::from_secs(20);
    let mut s = Served::start_with(
        |_| {},
        |_| {},
        |cmd| {
            cmd.env(
                "THESEUS_TEST_HOLD_TERMINAL_CLOSE_MS",
                HOLD.as_millis().to_string(),
            );
        },
    );
    let t0 = Instant::now();
    let _ = s.call("shutdown", Value::Null);
    while !s.log().contains("posts settled") {
        assert!(
            t0.elapsed() < Duration::from_secs(60) && s.daemon.try_wait().is_none(),
            "the stop never reached its posts:\n{}",
            common::tail(&s.log(), 40)
        );
        std::thread::sleep(Duration::from_millis(10));
    }
    let reached = t0.elapsed();
    assert!(
        reached < HOLD - Duration::from_secs(3),
        "the stop waited {reached:?} for the terminals' close:\n{}",
        common::tail(&s.log(), 40)
    );
    while s.daemon.try_wait().is_none() {
        assert!(
            t0.elapsed() < Duration::from_secs(90),
            "the daemon never ended"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
    let log = s.log();
    assert!(
        log.contains("the terminals' close did not return within its bound"),
        "no warning:\n{}",
        common::tail(&log, 40)
    );
    assert!(
        log.contains("last checkpoint"),
        "a later phase was skipped:\n{}",
        common::tail(&log, 40)
    );
}

/// A daemon whose serving fails at its start (another listens on its socket) while a self build runs on its
/// blocking pool ends within the bound, and still reports its error.
#[test]
fn a_daemon_whose_serving_fails_ends_its_self_build_and_reports_the_error() {
    if as_root() {
        return;
    }
    let mut listener = None;
    let mut s = Served::spawn_with(
        |d| listener = Some(std::os::unix::net::UnixListener::bind(d.join("sock")).unwrap()),
        |_| {},
        |cmd| {
            cmd.env("THESEUS_TEST_SELF_JOB", "exec sleep 3600")
                .process_group(0);
        },
    );
    let t0 = Instant::now();
    let status = loop {
        if let Some(status) = s.daemon.try_wait() {
            break status;
        }
        assert!(
            t0.elapsed() < Duration::from_secs(30) + STOPPED_WITHIN,
            "the daemon whose serving failed never ended: it waits for the job:\n{}",
            common::tail(&s.log(), 40)
        );
        std::thread::sleep(Duration::from_millis(20));
    };
    drop(listener);
    let log = s.log();
    assert!(!status.success(), "it ended well:\n{log}");
    assert!(
        log.contains("another theseusd is already listening"),
        "its error was not reported:\n{}",
        common::tail(&log, 40)
    );
    assert!(
        log.contains("planted self job: started"),
        "the self build never ran:\n{}",
        common::tail(&log, 40)
    );
}
