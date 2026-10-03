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
use theseus_sandbox::cgroup::JobCgroup;
use theseus_sandbox::{Exit, Init, SandboxChild, Spec, Stdio};

use crate::job::{Reap, WrapperArgs, L1};
use crate::job_egress as egress;
use crate::tree::{self, Proc};
use crate::types::{Outcome, Verdict, VerifiedBy};

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
    let allow = egress::prepare(&mut spec, l1);
    let mut sandbox = json!({"class": "l1"});
    if !skipped.is_empty() {
        sandbox["skipped"] = json!(skipped);
    }
    let cgroup = cgroup(args, l1, &mut spec, &mut sandbox);
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
    let proxy = egress::start(&mut child, l1, &allow, detail);
    let (exit, end) = wait(
        &mut child,
        cgroup.as_ref(),
        reap,
        Duration::from_millis(args.deadline_ms),
        t0,
    );
    // The job has ended, its whole tree with it (a stop included).
    egress::stop(proxy, &allow, detail);
    if let Some(c) = cgroup {
        limits_hit(&c, &mut sandbox);
        // Empty once the init is reaped: its pid namespace went with it.
        let _ = c.remove();
    }
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

/// The job's own cgroup, with its limits, where the daemon's is delegated.
/// One that cannot be made leaves the namespaces and seccomp in place, and
/// RLIMIT_NPROC still caps its processes: the job runs, and says it had no
/// cgroup.
fn cgroup(args: &WrapperArgs, l1: &L1, spec: &mut Spec, sandbox: &mut Value) -> Option<JobCgroup> {
    let jobs = l1.cgroup.as_deref()?;
    let pids = Some(l1.limits.pids);
    match JobCgroup::create(jobs, &args.correlation_id, Some(l1.memory_mb), pids) {
        Ok(c) => {
            spec.cgroup = Some(c.path().to_path_buf());
            sandbox["cgroup"] = json!(c.path());
            Some(c)
        }
        Err(e) => {
            sandbox["cgroup_error"] = json!(e.to_string());
            None
        }
    }
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
    spec.binds = l1.binds.clone();
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
    cgroup: Option<&JobCgroup>,
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
            let (exit, v) = stop(child, cgroup, reap, asked.grace());
            return (exit, End::Stopped(v));
        }
        if t0.elapsed() >= deadline {
            // A test's thread kills at once, as it did.
            let grace = match reap {
                Reap::Descendants => crate::job::STOP_GRACE,
                Reap::Command => Duration::ZERO,
            };
            let (exit, v) = stop(child, cgroup, reap, grace);
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
/// the kill. With the job's cgroup, `cgroup.kill`, verified by
/// `cgroup.events` at `populated 0`; without, SIGKILL to the init, whose reap
/// means the kernel has killed its pid namespace: a namespace's init does not
/// finish exiting until every other process of it has. The verdict counts
/// the job's processes, the init and each one below it, found before the
/// stop and as it went, and checks each one gone. The init's end, unless it
/// could not be reaped within the kill's wait.
fn stop(
    child: &mut SandboxChild,
    cgroup: Option<&JobCgroup>,
    reap: Reap,
    grace: Duration,
) -> (std::io::Result<Exit>, Verdict) {
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
        // The cgroup's own list, the kernel's accounting of the same set.
        for pid in cgroup.and_then(|c| c.procs().ok()).unwrap_or_default() {
            if let Some(s) = tree::stat(pid) {
                met.insert(Proc {
                    pid,
                    start: s.start,
                });
            }
        }
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
    // The kill: the cgroup's, or the init's.
    let by = match cgroup {
        Some(c) if exit.is_some() || c.kill().is_ok() => VerifiedBy::Cgroup,
        _ => {
            if exit.is_none() {
                look(&mut met);
                let _ = child.kill();
            }
            VerifiedBy::Pidns
        }
    };
    let until = Instant::now() + tree::KILL_WAIT;
    let gone = || match (by, cgroup) {
        (VerifiedBy::Cgroup, Some(c)) => !c.populated().unwrap_or(true),
        _ => true,
    };
    loop {
        if exit.is_none() {
            match try_reap(child, reap) {
                Ok(Some(e)) => exit = Some(Ok(e)),
                Err(e) => exit = Some(Err(e)),
                Ok(None) => {}
            }
        }
        if (exit.is_some() && gone()) || Instant::now() >= until {
            break;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    let survivors = match (by, cgroup) {
        (VerifiedBy::Cgroup, Some(c)) if !gone() => c.procs().map_or(1, |p| p.len().max(1)),
        _ => met.iter().filter(|p| tree::alive(**p)).count(),
    } as u32;
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
        verified_by: by,
        killed: Some(met.len() as u32),
        survivors: Some(survivors),
        scope: Some(
            if by == VerifiedBy::Cgroup {
                "cgroup"
            } else {
                "namespace"
            }
            .into(),
        ),
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
