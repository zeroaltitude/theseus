//! The job wrapper's L1 path (M4 17b; design §2.2): the command runs below
//! `theseus_sandbox`'s init, in its own namespaces, over a view built from an
//! empty root, instead of as the wrapper's own child. Nothing falls back: a
//! job that cannot start in L1 fails with the stage and the error, and no
//! part of it runs at L0.
//!
//! The wrapper is the init's parent, so it reaps the init alone: the job's
//! own processes live in the job's pid namespace, and the kernel kills them
//! all when the init exits, so none is ever reparented to the wrapper.

use std::collections::BTreeSet;
use std::os::fd::OwnedFd;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use serde_json::{json, Value};
use theseus_sandbox::{Exit, Init, SandboxChild, Spec, Stdio};

use crate::job::{Reap, WrapperArgs, L1};
use crate::job_egress as egress;
use crate::tree::{self, Proc};
use crate::types::{Outcome, Verdict, VerifiedBy};

/// The first words of a failed start's note.
const NOT_STARTED: &str = "the job could not start in L1";

/// The self-test that `theseusd check` runs on demand (theseus-gyin; design
/// §2.2): `/bin/true` in L1 over `l1`'s view, started as a job starts, from
/// the calling thread, which lives until it ends. The completion's `detail`
/// a job would have: its `sandbox` says how the launch went, as a real
/// job's does, and a `/bin/true` that did not succeed is an `error` there.
pub fn self_test(l1: &L1) -> Value {
    let mut env = vec![("PATH".to_string(), "/usr/bin:/bin".to_string())];
    env.extend(std::env::var("HOME").ok().map(|h| ("HOME".to_string(), h)));
    let args = WrapperArgs {
        spool_dir: PathBuf::new(),
        correlation_id: "self-test".into(),
        deadline_ms: 10_000,
        notify_socket: None,
        argv: vec!["/bin/true".into()],
        cwd: Some("/".into()),
        env,
        umask: None,
        redact: vec![],
        output_max_bytes: 0,
        sandbox: None,
    };
    let null = || -> std::io::Result<OwnedFd> {
        Ok(std::fs::OpenOptions::new()
            .write(true)
            .open("/dev/null")?
            .into())
    };
    let mut detail = json!({});
    match (null(), null()) {
        (Ok(out), Ok(err)) => {
            let (outcome, note) = run(&args, l1, Reap::Command, out, err, &mut detail);
            if outcome != Outcome::Succeeded && detail.pointer("/sandbox/error").is_none() {
                detail["sandbox"]["error"] = json!({"stage": "running /bin/true", "error": note});
            }
        }
        (Err(e), _) | (_, Err(e)) => {
            detail["sandbox"] = json!({"class": "l1",
                "error": {"stage": "opening /dev/null", "error": e.to_string()}});
        }
    }
    detail
}

/// Runs `args`' command in L1, with its output on `stdout` and `stderr`,
/// until it exits or its deadline kills the whole job. Its outcome and the
/// completion's note; `detail` gets `sandbox` (how it started), and
/// `scratch`, what it wrote there.
pub(crate) fn run(
    args: &WrapperArgs,
    l1: &L1,
    reap: Reap,
    stdout: OwnedFd,
    stderr: OwnedFd,
    detail: &mut Value,
) -> (Outcome, String) {
    let t0 = Instant::now();
    // A wrapper process has the job's environment as its own, as
    // `spawn_detached` set it; in process, it is the arguments'.
    let env = match reap {
        Reap::Command => args.env.clone(),
        Reap::Descendants => std::env::vars().collect(),
    };
    let (mut spec, skipped) = spec(args, l1, env);
    let allow = egress::prepare(&mut spec, l1);
    let mut sandbox = json!({"class": "l1"});
    // The variables its grants gave it, names only, for its result's head.
    if !args.redact.is_empty() {
        let vars: Vec<&str> = args.redact.iter().map(|(var, _)| var.as_str()).collect();
        sandbox["granted"] = json!(vars);
    }
    if !skipped.is_empty() {
        sandbox["skipped"] = json!(skipped);
    }
    // The command is made with the operator's umask, as at L0: the init
    // takes the wrapper's, and the command the init's. Only a wrapper
    // process changes its own; a test's thread leaves its process's alone.
    let old = match (reap, args.umask) {
        (Reap::Descendants, Some(u)) => Some(unsafe { libc::umask(u) }),
        _ => None,
    };
    let spawned = theseus_sandbox::spawn(
        &spec,
        &Init::default(),
        Stdio {
            stdin: None,
            stdout,
            stderr,
        },
    );
    if let Some(u) = old {
        unsafe { libc::umask(u) };
    }
    sandbox["start_us"] = json!(t0.elapsed().as_micros() as u64);
    let mut child = match spawned {
        Ok(c) => c,
        Err(e) => {
            sandbox["error"] = json!({"stage": e.stage, "error": e.error});
            detail["sandbox"] = sandbox;
            return (
                Outcome::Failed,
                format!("{NOT_STARTED}: {e}. It did not run, in L1 or at L0"),
            );
        }
    };
    let started = child.started();
    sandbox["setup_us"] = json!(started.setup_us);
    sandbox["sys"] = json!(started.sys);
    sandbox["lo"] = json!(started.lo);
    let proxy = egress::start(&mut child, l1, &allow, detail);
    let (exit, end) = wait(
        &mut child,
        reap,
        Duration::from_millis(args.deadline_ms),
        t0,
    );
    // The job has ended, its whole tree with it (a stop included).
    egress::stop(proxy, &allow, detail);
    // An init a stop could not reap (a process of its namespace in `D`):
    // its handle would wait for it, and the verdict would never be written.
    // It is reparented to the daemon, which reaps it once it ends.
    if exit.is_err() {
        std::mem::forget(child);
    }
    if let End::Stopped(v) | End::TimedOut(v) = &end {
        detail["stop"] = serde_json::to_value(v).unwrap_or_default();
    }
    let ended = match (exit, end) {
        (_, End::Stopped(v)) => {
            detail["stopped"] = json!(true);
            (Outcome::Failed, format!("stopped: {}", v.words()))
        }
        (Ok(e), end) => ended(&e, end, args.deadline_ms, &mut sandbox, detail),
        (Err(e), End::TimedOut(v)) => (
            Outcome::Failed,
            format!(
                "deadline {} ms exceeded; {}: {e}",
                args.deadline_ms,
                v.words()
            ),
        ),
        (Err(e), End::Finished) => (Outcome::Unknown, format!("wait: {e}")),
    };
    detail["sandbox"] = sandbox;
    ended
}

/// The spec for `args`' command, and the `ro_paths` skipped because they do
/// not exist (a missing one would fail every job).
fn spec(args: &WrapperArgs, l1: &L1, env: Vec<(String, String)>) -> (Spec, Vec<String>) {
    let cwd = args.cwd.clone().unwrap_or_else(|| PathBuf::from("/"));
    let mut spec = Spec::new(args.argv.clone(), cwd);
    spec.home = env
        .iter()
        .find(|(k, _)| k == "HOME")
        .map(|(_, v)| PathBuf::from(v))
        .filter(|h| h.is_absolute() && h.parent().is_some());
    spec.env = env;
    let (present, missing): (Vec<&PathBuf>, Vec<&PathBuf>) =
        l1.ro_paths.iter().partition(|p| p.exists());
    spec.ro_paths = present.into_iter().cloned().collect();
    spec.workspace = l1
        .workspace
        .iter()
        .filter(|p| p.is_dir())
        .cloned()
        .collect();
    spec.hidden = l1.hidden.clone();
    spec.limits = l1.limits;
    let skipped = missing.iter().map(|p| p.display().to_string()).collect();
    (spec, skipped)
}

/// How an L1 job ended: by itself, or stopped, at its deadline or when asked
/// (M4 18a), with the stop's verdict.
enum End {
    Finished,
    TimedOut(Verdict),
    Stopped(Verdict),
}

/// Waits for the job's end. Past its deadline, or asked to stop, it stops
/// the whole job (`stop`).
fn wait(
    child: &mut SandboxChild,
    reap: Reap,
    deadline: Duration,
    t0: Instant,
) -> (std::io::Result<Exit>, End) {
    loop {
        match try_reap(child, reap) {
            Ok(Some(exit)) => return (Ok(exit), End::Finished),
            Err(e) => return (Err(e), End::Finished),
            Ok(None) => {}
        }
        if let (Reap::Descendants, Some(asked)) = (reap, crate::job::stop_asked()) {
            let (exit, v) = stop(child, reap, asked.grace());
            return (exit, End::Stopped(v));
        }
        if t0.elapsed() >= deadline {
            // A test's thread kills at once, as it did.
            let grace = match reap {
                Reap::Descendants => crate::job::STOP_GRACE,
                Reap::Command => Duration::ZERO,
            };
            let (exit, v) = stop(child, reap, grace);
            return (exit, End::TimedOut(v));
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// The init's end, if it has ended: a wrapper process reaps every child, a
/// test's thread its own.
fn try_reap(child: &mut SandboxChild, reap: Reap) -> std::io::Result<Option<Exit>> {
    match reap {
        Reap::Command => child.try_wait(),
        Reap::Descendants => reap_init(child),
    }
}

/// Stop the whole job (M4 18a; design §2.3): SIGTERM to its init, which
/// forwards it to the command, and up to `grace` for the job to end; then
/// SIGKILL to the init, whose reap means the kernel has killed its pid
/// namespace: a namespace's init does not finish exiting until every other
/// process of it has. The verdict counts the job's processes, the init and
/// each one below it, found before the stop and as it went, and checks each
/// one gone. The init's end, unless it could not be reaped within the kill's
/// wait.
fn stop(child: &mut SandboxChild, reap: Reap, grace: Duration) -> (std::io::Result<Exit>, Verdict) {
    let t0 = Instant::now();
    let init = child.id();
    let mut met: BTreeSet<Proc> = BTreeSet::new();
    let look = |met: &mut BTreeSet<Proc>| {
        if let Some(s) = tree::stat(init).filter(|s| !matches!(s.state, 'Z' | 'X')) {
            met.insert(Proc {
                pid: init,
                start: s.start,
            });
        }
        met.extend(tree::descendants(init));
    };
    look(&mut met);
    let _ = child.terminate();
    let mut exit = None;
    let until = t0 + grace;
    loop {
        match try_reap(child, reap) {
            Ok(Some(e)) => exit = Some(Ok(e)),
            Err(e) => exit = Some(Err(e)),
            Ok(None) => {}
        }
        if exit.is_some() || Instant::now() >= until {
            break;
        }
        look(&mut met);
        std::thread::sleep(Duration::from_millis(10));
    }
    // The kill: the init's, and with it its pid namespace's.
    if exit.is_none() {
        look(&mut met);
        let _ = child.kill();
    }
    let until = Instant::now() + tree::KILL_WAIT;
    while exit.is_none() && Instant::now() < until {
        match try_reap(child, reap) {
            Ok(Some(e)) => exit = Some(Ok(e)),
            Err(e) => exit = Some(Err(e)),
            Ok(None) => std::thread::sleep(Duration::from_millis(5)),
        }
    }
    let survivors = met.iter().filter(|p| tree::alive(**p)).count() as u32;
    let why = match (survivors, &exit) {
        (0, Some(Ok(_))) => None,
        (0, _) => Some("its init was not reaped".to_string()),
        (n, _) => Some(format!(
            "{} of the job outlived the kill",
            if n == 1 {
                "1 process".to_string()
            } else {
                format!("{n} processes")
            }
        )),
    };
    let v = Verdict {
        verified_by: VerifiedBy::Pidns,
        killed: Some(met.len() as u32),
        survivors: Some(survivors),
        scope: Some("namespace".into()),
        ms: t0.elapsed().as_millis() as u64,
        why,
    };
    let exit = exit.unwrap_or_else(|| Err(std::io::Error::other("its init was not reaped")));
    (exit, v)
}

/// A subreaper's wait: every child that has exited is reaped, and the
/// init's status read when it is one of them.
fn reap_init(child: &mut SandboxChild) -> std::io::Result<Option<Exit>> {
    let init = child.id() as libc::pid_t;
    let mut found = None;
    loop {
        let mut status = 0;
        match unsafe { libc::waitpid(-1, &mut status, libc::WNOHANG) } {
            0 => return Ok(found),
            -1 => {
                let e = std::io::Error::last_os_error();
                match e.raw_os_error() {
                    Some(libc::EINTR) => {}
                    Some(libc::ECHILD) if found.is_some() => return Ok(found),
                    _ => return Err(e),
                }
            }
            pid if pid == init => found = Some(child.reaped(status)),
            _ => {}
        }
    }
}

/// How the job ended, into `detail` as an L0 job's end goes, with its
/// scratch summary: its outcome and the note.
fn ended(
    e: &Exit,
    end: End,
    deadline_ms: u64,
    sandbox: &mut Value,
    detail: &mut Value,
) -> (Outcome, String) {
    detail["exit_code"] = json!(e.code);
    detail["signal"] = json!(e.signal.or(e.init_signal));
    if e.output_capped {
        sandbox["output_capped"] = json!(true);
    }
    if let Some(s) = &e.scratch {
        detail["scratch"] = json!({"files": s.files, "bytes": s.bytes, "removed": s.removed,
            "paths": s.paths, "summary": s.summary()});
    }
    if let End::TimedOut(v) = &end {
        detail["timed_out"] = json!(true);
        return (
            Outcome::Failed,
            format!(
                "deadline {deadline_ms} ms exceeded; the whole job was stopped ({})",
                v.words()
            ),
        );
    }
    match (e.code, e.signal.or(e.init_signal)) {
        (Some(0), _) => (Outcome::Succeeded, "exit status: 0".into()),
        (Some(c), _) => (Outcome::Failed, format!("exit status: {c}")),
        (None, Some(s)) => (Outcome::Failed, format!("signal: {s}")),
        (None, None) => (Outcome::Failed, "it ended with no status".into()),
    }
}
