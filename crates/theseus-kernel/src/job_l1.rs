//! The job wrapper's L1 path (M4 17b; design §2.2): the command runs below
//! `theseus_sandbox`'s init, in its own namespaces, over a view built from an
//! empty root, instead of as the wrapper's own child. Nothing falls back: a
//! job that cannot start in L1 fails with the stage and the error, and no
//! part of it runs at L0.
//!
//! The wrapper is the init's parent, so it reaps the init alone: the job's
//! own processes live in the job's pid namespace, and the kernel kills them
//! all when the init exits, so none is ever reparented to the wrapper.

use std::os::fd::OwnedFd;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use serde_json::{json, Value};
use theseus_sandbox::cgroup::JobCgroup;
use theseus_sandbox::{Exit, Init, SandboxChild, Spec, Stdio};

use crate::job::{Reap, WrapperArgs, L1};
use crate::types::Outcome;

/// The first words of a failed start's note.
const NOT_STARTED: &str = "the job could not start in L1";

/// The probe (17b; design §2.2, "Cost and the probe"): `/bin/true` in L1,
/// over `l1`'s view, started as a job starts. Whether L1 runs here, and if
/// not why (`stage`, `error`); the start's microseconds (`start_us`, the
/// spawn's own, `setup_us`, the init's part); what the start found (`sys`,
/// a fresh sysfs; `lo`); and the `ro_paths` skipped. Run by `theseusd
/// sandbox-probe`, a process of its own, so the clone is never the daemon's.
pub fn probe(l1: &L1) -> Value {
    let mut env = vec![("PATH".to_string(), "/usr/bin:/bin".to_string())];
    env.extend(std::env::var("HOME").ok().map(|h| ("HOME".to_string(), h)));
    let args = WrapperArgs {
        spool_dir: PathBuf::new(),
        correlation_id: "probe".into(),
        deadline_ms: 10_000,
        notify_socket: None,
        argv: vec!["/bin/true".into()],
        cwd: Some("/".into()),
        env: env.clone(),
        umask: None,
        redact: vec![],
        output_max_bytes: 0,
        sandbox: None,
    };
    let (spec, skipped) = spec(&args, l1, env);
    let null = || -> std::io::Result<OwnedFd> {
        Ok(std::fs::OpenOptions::new()
            .write(true)
            .open("/dev/null")?
            .into())
    };
    let mut out = json!({"skipped": skipped});
    let (stdout, stderr) = match (null(), null()) {
        (Ok(a), Ok(b)) => (a, b),
        (Err(e), _) | (_, Err(e)) => {
            out["ok"] = json!(false);
            out["stage"] = json!("opening /dev/null");
            out["error"] = json!(e.to_string());
            return out;
        }
    };
    let t0 = Instant::now();
    let spawned = theseus_sandbox::spawn(
        &spec,
        &Init::default(),
        Stdio {
            stdin: None,
            stdout,
            stderr,
        },
    );
    out["start_us"] = json!(t0.elapsed().as_micros() as u64);
    match spawned {
        Err(e) => {
            out["ok"] = json!(false);
            out["stage"] = json!(e.stage);
            out["error"] = json!(e.error);
        }
        Ok(mut child) => {
            let s = child.started().clone();
            out["setup_us"] = json!(s.setup_us);
            out["sys"] = json!(s.sys);
            out["lo"] = json!(s.lo);
            match child.wait() {
                Ok(e) if e.success() => out["ok"] = json!(true),
                Ok(e) => {
                    out["ok"] = json!(false);
                    out["stage"] = json!("running /bin/true");
                    out["error"] = json!(format!("it ended with {:?}", e.code.or(e.signal)));
                }
                Err(e) => {
                    out["ok"] = json!(false);
                    out["stage"] = json!("waiting for the job");
                    out["error"] = json!(e.to_string());
                }
            }
        }
    }
    out
}

/// Runs `args`' command in L1, with its output on `stdout` and `stderr`,
/// until it exits or its deadline kills the whole job. Its outcome and the
/// completion's note; `detail` gets `sandbox` (how it started, its cgroup,
/// its limits hit), and `scratch`, what it wrote there.
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
    let mut sandbox = json!({"class": "l1"});
    if !skipped.is_empty() {
        sandbox["skipped"] = json!(skipped);
    }
    let cgroup = match l1.cgroup.as_deref() {
        Some(jobs) => match JobCgroup::create(
            jobs,
            &args.correlation_id,
            Some(l1.memory_mb),
            Some(l1.limits.pids),
        ) {
            Ok(c) => {
                spec.cgroup = Some(c.path().to_path_buf());
                sandbox["cgroup"] = json!(c.path());
                Some(c)
            }
            // The namespaces and seccomp still hold, and RLIMIT_NPROC caps
            // its processes: the job runs, and says it had no cgroup.
            Err(e) => {
                sandbox["cgroup_error"] = json!(e.to_string());
                None
            }
        },
        None => None,
    };
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
            if let Some(c) = cgroup {
                let _ = c.remove();
            }
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
    let (exit, timed_out) = wait(
        &mut child,
        reap,
        Duration::from_millis(args.deadline_ms),
        t0,
    );
    if let Some(c) = cgroup {
        limits_hit(&c, &mut sandbox);
        // Empty once the init is reaped: its pid namespace went with it.
        let _ = c.remove();
    }
    let ended = match exit {
        Ok(e) => ended(&e, timed_out, args.deadline_ms, &mut sandbox, detail),
        Err(e) => (Outcome::Unknown, format!("wait: {e}")),
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

/// Waits for the job's end; past its deadline, kills the whole job (SIGKILL
/// to the init: the kernel then kills its pid namespace) and waits for that.
/// Whether the deadline killed it.
fn wait(
    child: &mut SandboxChild,
    reap: Reap,
    deadline: Duration,
    t0: Instant,
) -> (std::io::Result<Exit>, bool) {
    let mut timed_out = false;
    loop {
        let r = match reap {
            Reap::Command => child.try_wait(),
            Reap::Descendants => reap_init(child),
        };
        match r {
            Ok(Some(exit)) => return (Ok(exit), timed_out),
            Ok(None) => {
                if !timed_out && t0.elapsed() >= deadline {
                    let _ = child.kill();
                    timed_out = true;
                }
                std::thread::sleep(Duration::from_millis(20));
            }
            Err(e) => return (Err(e), timed_out),
        }
    }
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

/// What the job's cgroup counted: forks `pids.max` refused, and kills by
/// `memory.max`.
fn limits_hit(c: &JobCgroup, sandbox: &mut Value) {
    if let Ok(n @ 1..) = c.pids_refused() {
        sandbox["pids_refused"] = json!(n);
    }
    let oom = std::fs::read_to_string(c.path().join("memory.events"))
        .ok()
        .and_then(|s| {
            s.lines()
                .find_map(|l| l.strip_prefix("oom_kill "))
                .and_then(|n| n.trim().parse::<u64>().ok())
        })
        .unwrap_or(0);
    if oom > 0 {
        sandbox["oom_kills"] = json!(oom);
    }
}

/// How the job ended, into `detail` as an L0 job's end goes, with its
/// scratch summary: its outcome and the note.
fn ended(
    e: &Exit,
    timed_out: bool,
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
    if timed_out {
        detail["timed_out"] = json!(true);
        return (
            Outcome::Failed,
            format!("deadline {deadline_ms} ms exceeded; the whole job was killed"),
        );
    }
    match (e.code, e.signal.or(e.init_signal)) {
        (Some(0), _) => (Outcome::Succeeded, "exit status: 0".into()),
        (Some(c), _) => (Outcome::Failed, format!("exit status: {c}")),
        (None, Some(s)) => (Outcome::Failed, format!("signal: {s}")),
        (None, None) => (Outcome::Failed, "it ended with no status".into()),
    }
}
