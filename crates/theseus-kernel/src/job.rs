//! The job wrapper (§3.16, §7): detached, durable, cancellable.
//!
//! `spawn_detached` starts *this binary* in wrapper mode as its own session
//! (setsid), so its lifetime does not depend on the harness. The wrapper
//! (`run_wrapper_process`) runs the real command, enforces its own deadline, captures
//! output to the spool's `results/`, writes the `Completion` to the spool
//! (tmp+rename), then pokes the harness over a Unix socket if one is given.
//! It is a child subreaper, so a descendant that a double fork orphans stays
//! under it, and it lingers until the last one has exited (theseus-6qy).
//! The daemon reaps each wrapper it spawned once it exits (`children`,
//! theseus-z4b), so a wrapper's pid can be another process's afterwards: a
//! wrapper is alive only while its pid's command line still names its job.
//! Cancellation kills the wrapper's process group by correlation id and
//! verifies that no live process is left in it (`Stopping`, several jobs at
//! once); `WrapperEvidence` is what the reconciler asks.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use anyhow::{Context, Result};

use crate::kernel::{Evidence, Probe};
use crate::spool::Spool;
use crate::types::{Action, Completion, Outcome, Verdict, VerifiedBy};

/// Arguments the wrapper mode receives.
#[derive(Clone)]
pub struct WrapperArgs {
    pub spool_dir: PathBuf,
    pub correlation_id: String,
    pub deadline_ms: u64,
    pub notify_socket: Option<PathBuf>,
    pub argv: Vec<String>,
    /// Working directory for the command.
    pub cwd: Option<PathBuf>,
    /// The complete environment of the wrapper and its command. The spawner
    /// clears everything else, so nothing the harness holds (the 1Password
    /// service-account token above all) leaks into a job. It may hold a
    /// secret the broker granted (theseus-dcy): it goes to the wrapper as its
    /// environment, never as an argument, and its Debug names only.
    pub env: Vec<(String, String)>,
    /// The operator's umask, for the command (theseus-wz2): the daemon and
    /// its wrapper run under 077, so the spool stays private, and the job's
    /// files are made as the operator's shell would make them. `None`: the
    /// command keeps the wrapper's.
    pub umask: Option<u32>,
    /// The broker's grants (theseus-l0d): each variable that holds a granted
    /// secret, and the secret's name. Names only: the values reach the
    /// wrapper as its environment, and it withholds each from the job's raw
    /// output (`crate::redact`).
    pub redact: Vec<(String, String)>,
    /// The most bytes of the job's output its raw output file keeps
    /// (theseus-102, `[tools] job_output_max_bytes`). Past it the wrapper
    /// reads on and counts what it drops; the command is never stopped for
    /// printing.
    pub output_max_bytes: u64,
    /// An L1 job's view and limits (M4 17b): the command runs below the
    /// sandbox's init, never at L0. `None`: an L0 job.
    pub sandbox: Option<L1>,
}

/// An L1 job's view and limits (M4 17b; design §2.2): what the wrapper
/// hands `theseus_sandbox::spawn`. Paths and numbers only, so it rides on
/// the wrapper's command line (`--sandbox <json>`): the job's environment is
/// the wrapper's own, and a granted secret is never in it (18d brings them).
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct L1 {
    /// The workspace roots: read-only, under overlays whose writes go to
    /// scratch and are discarded.
    pub workspace: Vec<PathBuf>,
    /// `[sandbox] ro_paths`: more read-only binds. One that does not exist is
    /// skipped, and the completion says so.
    pub ro_paths: Vec<PathBuf>,
    /// Covered in the view whatever binds them: Theseus's floor (its store,
    /// spool, bindings, the 1Password token and credentials) and its socket.
    pub hidden: Vec<PathBuf>,
    pub limits: theseus_sandbox::Limits,
    /// `[sandbox] memory_mb`: the job's cgroup's `memory.max`.
    pub memory_mb: u64,
    /// The delegated cgroup's `jobs` directory: the wrapper makes the job's
    /// own cgroup there, with its limits, and removes it after. `None`: the
    /// daemon's cgroup is not delegated, so no memory limit.
    pub cgroup: Option<PathBuf>,
}

/// A job's output file keeps this much unless the config says otherwise
/// (theseus-102): 64 MiB, sixteen times what the runtime reads of it.
pub const DEFAULT_OUTPUT_MAX_BYTES: u64 = 64 * 1024 * 1024;

impl std::fmt::Debug for WrapperArgs {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let names: Vec<&str> = self.env.iter().map(|(k, _)| k.as_str()).collect();
        f.debug_struct("WrapperArgs")
            .field("spool_dir", &self.spool_dir)
            .field("correlation_id", &self.correlation_id)
            .field("deadline_ms", &self.deadline_ms)
            .field("notify_socket", &self.notify_socket)
            .field("argv", &self.argv)
            .field("cwd", &self.cwd)
            .field("env", &names)
            .field("umask", &self.umask.map(crate::umask::format))
            .field("redact", &self.redact)
            .field("output_max_bytes", &self.output_max_bytes)
            .field("sandbox", &self.sandbox)
            .finish()
    }
}

/// Start `self_exe` in wrapper mode, detached. Returns the wrapper's pid,
/// which is also written to the spool so a restarted harness can find it.
/// `self_exe_args` is the prefix that puts the binary into wrapper mode
/// (e.g. `["job-wrapper"]`); the wrapper flags follow.
pub fn spawn_detached(
    self_exe: &Path,
    self_exe_args: &[&str],
    spool: &Spool,
    args: &WrapperArgs,
) -> Result<u32> {
    let mut cmd = Command::new(self_exe);
    cmd.args(self_exe_args)
        .arg("--spool")
        .arg(&args.spool_dir)
        .arg("--correlation-id")
        .arg(&args.correlation_id)
        .arg("--deadline-ms")
        .arg(args.deadline_ms.to_string());
    if let Some(s) = &args.notify_socket {
        cmd.arg("--notify").arg(s);
    }
    if let Some(c) = &args.cwd {
        cmd.arg("--cwd").arg(c);
    }
    if let Some(u) = args.umask {
        cmd.arg("--umask").arg(crate::umask::format(u));
    }
    for (var, secret) in &args.redact {
        cmd.arg("--redact").arg(format!("{var}={secret}"));
    }
    cmd.arg("--output-max-bytes")
        .arg(args.output_max_bytes.to_string());
    if let Some(l1) = &args.sandbox {
        cmd.arg("--sandbox")
            .arg(serde_json::to_string(l1).context("encoding the L1 view")?);
    }
    cmd.arg("--").args(&args.argv);
    cmd.env_clear();
    cmd.envs(args.env.iter().map(|(k, v)| (k.as_str(), v.as_str())));
    cmd.stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        // Spawned as /proc/self/exe, it still shows as theseusd in ps.
        cmd.arg0("theseusd");
        // Own session and process group: the harness dying does not take us
        // with it, and `kill(-pgid)` reaches the whole tree on cancel.
        unsafe {
            cmd.pre_exec(|| {
                if libc::setsid() == -1 {
                    return Err(std::io::Error::last_os_error());
                }
                Ok(())
            });
        }
    }
    // Registered as this job's wrapper as it is spawned, so the daemon's
    // sweep reaps it by its pid once it exits (theseus-z4b).
    let child = crate::children::spawn(
        crate::children::Kind::Wrapper(&args.correlation_id),
        || cmd.spawn(),
        |c| Some(c.id()),
    )
    .context("spawning job wrapper")?;
    let pid = child.id();
    spool.write_pid(&args.correlation_id, pid)?;
    // Nothing waits here: it is detached by design, and the sweep reaps it.
    std::mem::forget(child);
    Ok(pid)
}

/// The mode word that puts a binary into wrapper mode. A wrapper's command
/// line is `<binary> job-wrapper --spool … --correlation-id <id> … -- <argv>`.
pub const WRAPPER_MODE: &str = "job-wrapper";

/// The role word of the L1 probe's process (17b): `theseusd sandbox-probe`
/// reads an `L1` on stdin, and prints `probe`'s answer.
pub const PROBE_MODE: &str = "sandbox-probe";

pub use crate::job_l1::probe;
/// An L1 job's limits, as `L1` carries them.
pub use theseus_sandbox::Limits as SandboxLimits;

/// The wrapper's body as its own process (`theseusd job-wrapper`).
///
/// It is a child subreaper (theseus-6qy): a descendant orphaned by a double
/// fork (`setsid nohup … &`) is reparented to the wrapper, not to init, so it
/// stays a descendant of its job, which is what the core's check of who
/// answers an approval looks for. While the command runs, the wrapper reaps
/// every child that exits, so orphans never pile up as zombies. It reports
/// the command's result as `run_wrapper` does, then lingers until no
/// descendant remains, and only then exits. It kills nothing.
pub fn run_wrapper_process(args: &WrapperArgs) -> Result<()> {
    // First, so the daemon reads this wrapper as one that stops its tree
    // (`catches_sigterm`) from as early as it can (M4 18a).
    let caught = catch_sigterm();
    let subreaper = crate::children::set_subreaper();
    let errors: Vec<String> = [subreaper.err(), caught.err()]
        .into_iter()
        .flatten()
        .collect();
    let (spool, mut copier) = run(
        args,
        Reap::Descendants,
        (!errors.is_empty()).then(|| errors.join("; ")),
    )?;
    linger(&spool, &args.correlation_id);
    // What a descendant printed after the report goes through the copy too,
    // to the pipe's end, which comes once the last descendant has gone.
    copier.wait(crate::redact::DRAIN);
    // A SIGTERM that was not a cancel ends the wrapper by it, as it did
    // before 18a, once its tree is stopped: the daemon still reads a wrapper
    // killed before it reported (theseus-6uo).
    if stop_asked() == Some(StopAsked::Signal) {
        // SAFETY: the default action back, then the signal to this thread.
        unsafe {
            libc::signal(libc::SIGTERM, libc::SIG_DFL);
            libc::raise(libc::SIGTERM);
        }
    }
    Ok(())
}

/// What asked the wrapper to stop its job (M4 18a), as its SIGTERM handler
/// noted it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum StopAsked {
    /// The daemon's cancel (`ask_to_stop`): the grace rides in the signal's
    /// value. The wrapper stops its tree, writes its verdict to the spool,
    /// and exits, with no completion.
    Cancel { grace: Duration },
    /// A SIGTERM from anything else: the job itself, a `kill` by hand, a
    /// daemon from before 18a stopping the group. The tree is stopped, then
    /// the wrapper ends by the signal.
    Signal,
}

impl StopAsked {
    pub(crate) fn grace(self) -> Duration {
        match self {
            StopAsked::Cancel { grace } => grace,
            StopAsked::Signal => STOP_GRACE,
        }
    }
}

/// The stop's signal, as the handler noted it: 0, none; `u64::MAX`, a plain
/// SIGTERM; otherwise a cancel's grace in milliseconds, plus one.
static STOP_ASKED: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// The wrapper's SIGTERM handler: it only notes what came, with an atomic
/// store, which is async-signal-safe; the wrapper's loop acts on it.
extern "C" fn on_sigterm(_: libc::c_int, info: *mut libc::siginfo_t, _: *mut libc::c_void) {
    // SAFETY: the kernel's siginfo, read in the handler it was given to.
    let asked = unsafe {
        match info.as_ref() {
            Some(i) if i.si_code == libc::SI_QUEUE => {
                (i.si_value().sival_ptr as usize as u64).clamp(0, u64::MAX - 2) + 1
            }
            _ => u64::MAX,
        }
    };
    let _ = STOP_ASKED.compare_exchange(
        0,
        asked,
        std::sync::atomic::Ordering::SeqCst,
        std::sync::atomic::Ordering::SeqCst,
    );
}

/// Catch SIGTERM (M4 18a). Without `SA_RESTART`, so the linger's blocking
/// wait returns to look. `/proc/<pid>/status` then shows it caught, which is
/// how the daemon tells this wrapper from one from before 18a.
fn catch_sigterm() -> std::result::Result<(), String> {
    // SAFETY: a handler that only stores to an atomic, installed for SIGTERM.
    unsafe {
        let mut sa: libc::sigaction = std::mem::zeroed();
        sa.sa_sigaction = on_sigterm as *const () as usize;
        sa.sa_flags = libc::SA_SIGINFO;
        libc::sigemptyset(&mut sa.sa_mask);
        if libc::sigaction(libc::SIGTERM, &sa, std::ptr::null_mut()) == -1 {
            return Err(format!(
                "catching SIGTERM: {}",
                std::io::Error::last_os_error()
            ));
        }
    }
    Ok(())
}

/// What asked this wrapper to stop, if anything has.
pub(crate) fn stop_asked() -> Option<StopAsked> {
    match STOP_ASKED.load(std::sync::atomic::Ordering::SeqCst) {
        0 => None,
        u64::MAX => Some(StopAsked::Signal),
        ms => Some(StopAsked::Cancel {
            grace: Duration::from_millis(ms - 1),
        }),
    }
}

/// Ask job `pid`'s wrapper to stop its job (M4 18a): SIGTERM to the wrapper
/// alone, by `sigqueue`, the grace in its value. Whether it was sent.
pub fn ask_to_stop(pid: u32, grace: Duration) -> bool {
    let value = libc::sigval {
        sival_ptr: grace.as_millis().min(u32::MAX as u128) as usize as *mut libc::c_void,
    };
    // SAFETY: sigqueue sends a signal and a value; no memory is shared.
    unsafe { libc::sigqueue(pid as libc::pid_t, libc::SIGTERM, value) == 0 }
}

/// Whether `pid` catches SIGTERM: its `/proc/<pid>/status` lists it among
/// the signals it catches (`SigCgt`). A wrapper from 18a on does from its
/// first moments; one from before dies at it.
pub fn catches_sigterm(pid: u32) -> bool {
    std::fs::read_to_string(format!("/proc/{pid}/status"))
        .ok()
        .and_then(|s| {
            let hex = s.lines().find_map(|l| l.strip_prefix("SigCgt:"))?;
            u64::from_str_radix(hex.trim(), 16).ok()
        })
        .is_some_and(|mask| mask & (1 << (libc::SIGTERM - 1)) != 0)
}

/// The wrapper's body on a thread of this process (the tests' in-process
/// launcher). It waits for its own command only: a thread that waited for
/// any child, or made its process a subreaper, would take other threads'
/// children.
pub fn run_wrapper(args: &WrapperArgs) -> Result<()> {
    run(args, Reap::Command, None).map(|_| ())
}

/// The values the broker granted this job, each with its secret's name
/// (theseus-l0d): in process, from the environment the job was given; in a
/// wrapper process, from the wrapper's own, which is the job's.
fn granted(args: &WrapperArgs, reap: Reap) -> Vec<(String, Vec<u8>)> {
    use std::os::unix::ffi::OsStringExt;
    args.redact
        .iter()
        .filter_map(|(var, secret)| {
            let value = match reap {
                Reap::Command => args
                    .env
                    .iter()
                    .find(|(k, _)| k == var)?
                    .1
                    .clone()
                    .into_bytes(),
                Reap::Descendants => std::env::var_os(var)?.into_vec(),
            };
            Some((secret.clone(), value))
        })
        .collect()
}

/// What a wrapper waits for while its command runs.
#[derive(Clone, Copy)]
pub(crate) enum Reap {
    /// The command alone.
    Command,
    /// Every child: the command, and each descendant reparented here.
    Descendants,
}

/// The command, its deadline, and the report. `subreaper_error` is why the
/// wrapper could not become a subreaper, recorded in the completion. Also
/// the copy of the command's output, which may outlive the report while a
/// descendant holds the output open.
fn run(
    args: &WrapperArgs,
    reap: Reap,
    subreaper_error: Option<String>,
) -> Result<(Spool, crate::redact::Copier)> {
    let spool = Spool::open(&args.spool_dir)?;
    let started = now_ms();
    let t0 = Instant::now();
    let out_path = spool.result_path(&args.correlation_id);
    // The output before the scrubber sees it, which can hold a secret a
    // program printed: the operator's alone, whatever the umask, and
    // whatever a file already there had (theseus-wz2). The core deletes it
    // once the result is absorbed.
    let out_file = {
        use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
        let f = std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(&out_path)?;
        f.set_permissions(std::fs::Permissions::from_mode(0o600))?;
        f
    };
    // The command writes into a pipe, and a copy writes what it reads into
    // the file: with each granted value withheld (theseus-l0d), and no more
    // than the cap (theseus-102). Past the cap the copy reads on and counts,
    // so a job that prints 20 GB runs as it would, and its file stays at the
    // cap. Before the cap, a job without a grant wrote its file itself.
    let redactor = crate::redact::Redactor::new(granted(args, reap));
    let (read, write) = std::io::pipe()?;
    let err = write.try_clone()?;
    let mut copier = crate::redact::Copier::spawn(read, out_file, redactor, args.output_max_bytes)?;
    let mut detail = serde_json::json!({});
    if let Some(e) = subreaper_error {
        detail["subreaper_error"] = serde_json::Value::String(e);
    }
    // An L1 job runs below the sandbox's init, and never at L0 instead (17b).
    let (outcome, note) = match &args.sandbox {
        Some(l1) => crate::job_l1::run(args, l1, reap, write.into(), err.into(), &mut detail),
        None => run_l0(args, reap, (write, err), t0, &mut detail),
    };
    // A stop ended the job (M4 18a). Its verdict goes to the spool for the
    // cancel that asked, and no completion, as a cancelled job wrote none
    // before; once the copy has ended, since the stop left no writer.
    if detail.get("stopped").is_some() {
        let _ = copier.wait(crate::redact::DRAIN);
        if let (Some(StopAsked::Cancel { .. }), Ok(v)) = (
            stop_asked(),
            serde_json::from_value::<Verdict>(detail["stop"].clone()),
        ) {
            spool.write_stop(&args.correlation_id, &v)?;
        }
        spool.remove_pid(&args.correlation_id);
        return Ok((spool, copier));
    }
    // The copy ends once the command and every descendant sharing its output
    // have closed it (theseus-l0d). One that keeps it open does not hold the
    // report: the copy goes on after it, and the report says what was
    // dropped so far, and what of the output's end the ring still holds
    // (`held`, theseus-gsn9), which the file takes only at the pipe's end.
    let (dropped, held) = match copier.wait(crate::redact::DRAIN) {
        Some(Ok(c)) => {
            if c.withheld > 0 {
                detail["withheld"] = serde_json::json!(c.withheld);
            }
            if let Some(e) = c.write_error {
                detail["output_error"] = serde_json::Value::String(e);
            }
            // The output's two ends, when bytes between them were dropped.
            if c.tail > 0 {
                detail["head"] = serde_json::json!(c.head);
                detail["tail"] = serde_json::json!(c.tail);
            }
            (c.dropped, 0)
        }
        Some(Err(e)) => {
            detail["output_error"] = serde_json::Value::String(e);
            (copier.dropped_so_far(), copier.held_so_far())
        }
        None => {
            detail["output_open"] = serde_json::Value::Bool(true);
            let (dropped, held) = (copier.dropped_so_far(), copier.held_so_far());
            // Past the head, with bytes dropped, the report names the two ends
            // as they stand, as a finished copy's does: the head is full (the
            // ring takes bytes only then), and the end so far is what the ring
            // holds. Without them the result read the head-only file as the
            // whole of what was kept (theseus-z3de).
            if dropped > 0 && held > 0 {
                detail["head"] = serde_json::json!(crate::redact::split(args.output_max_bytes).0);
                detail["tail"] = serde_json::json!(held);
            }
            (dropped, held)
        }
    };
    if dropped > 0 {
        detail["truncated"] = serde_json::Value::Bool(true);
        detail["dropped"] = serde_json::json!(dropped);
        detail["output_max_bytes"] = serde_json::json!(args.output_max_bytes);
    }
    if held > 0 {
        detail["held"] = serde_json::json!(held);
    }
    detail["duration_ms"] = serde_json::json!(t0.elapsed().as_millis() as u64);
    detail["bytes"] = serde_json::json!(std::fs::metadata(&out_path).map(|m| m.len()).unwrap_or(0));
    detail["note"] = serde_json::Value::String(note);
    let c = Completion {
        correlation_id: args.correlation_id.clone(),
        outcome,
        result_ref: Some(out_path.to_string_lossy().into_owned()),
        external_op_id: None,
        started_at_ms: started,
        finished_at_ms: now_ms(),
        producer: format!("wrapper:{}", std::process::id()),
        signature: None,
        cost_micros: None,
        detail: Some(detail),
    };
    spool.write(&c)?; // durable before any delivery attempt
    spool.remove_pid(&args.correlation_id);
    if let Some(sock) = &args.notify_socket {
        notify(sock, &args.correlation_id);
    }
    Ok((spool, copier))
}

/// An L0 job: the command as the wrapper's own child, its output into the
/// copy's pipe, until it exits or its deadline kills it. Its outcome, and
/// the completion's note.
fn run_l0(
    args: &WrapperArgs,
    reap: Reap,
    (write, err): (std::io::PipeWriter, std::io::PipeWriter),
    t0: Instant,
    detail: &mut serde_json::Value,
) -> (Outcome, String) {
    let mut command = Command::new(&args.argv[0]);
    command
        .args(&args.argv[1..])
        .stdin(Stdio::null())
        .stdout(Stdio::from(write))
        .stderr(Stdio::from(err));
    if let Some(c) = &args.cwd {
        command.current_dir(c);
    }
    if let Some(u) = args.umask {
        use std::os::unix::process::CommandExt;
        // SAFETY: umask is async-signal-safe and touches only the child.
        unsafe {
            command.pre_exec(move || {
                libc::umask(u);
                Ok(())
            });
        }
    }
    // A wrapper process has the job's environment as its own, as
    // `spawn_detached` set it. In process, the command gets it here.
    if matches!(reap, Reap::Command) {
        command
            .env_clear()
            .envs(args.env.iter().map(|(k, v)| (k.as_str(), v.as_str())));
    }
    let child = command.spawn();
    drop(command);
    let mut child = match child {
        Err(e) => {
            detail["spawn_error"] = serde_json::Value::String(e.to_string());
            return (Outcome::Failed, format!("spawn: {e}"));
        }
        Ok(child) => child,
    };
    let deadline = Duration::from_millis(args.deadline_ms);
    loop {
        let exited = match reap {
            Reap::Command => child.try_wait(),
            Reap::Descendants => reap_children(child.id()),
        };
        match exited {
            Ok(Some(status)) => {
                detail["exit_code"] = serde_json::json!(status.code());
                #[cfg(unix)]
                {
                    use std::os::unix::process::ExitStatusExt;
                    detail["signal"] = serde_json::json!(status.signal());
                }
                return if status.success() {
                    (Outcome::Succeeded, format!("exit {status}"))
                } else {
                    (Outcome::Failed, format!("exit {status}"))
                };
            }
            Ok(None) => {
                // A stop asked of a wrapper process ends its whole tree (18a).
                if let (Reap::Descendants, Some(asked)) = (reap, stop_asked()) {
                    let v = stop_tree(asked.grace(), detail);
                    detail["stopped"] = serde_json::Value::Bool(true);
                    return (Outcome::Failed, format!("stopped: {}", v.words()));
                }
                if t0.elapsed() >= deadline {
                    detail["timed_out"] = serde_json::Value::Bool(true);
                    // The deadline's stop is the cancel's: the whole tree,
                    // not only the command (theseus-hcc). A thread in a test
                    // process kills its command alone.
                    let how = match reap {
                        Reap::Descendants => stop_tree(STOP_GRACE, detail).words(),
                        Reap::Command => {
                            let _ = child.kill();
                            let _ = child.wait();
                            "killed".into()
                        }
                    };
                    return (
                        Outcome::Failed,
                        format!("deadline {} ms exceeded; {how}", args.deadline_ms),
                    );
                }
                std::thread::sleep(Duration::from_millis(20));
            }
            Err(e) => return (Outcome::Unknown, format!("wait: {e}")),
        }
    }
}

/// Stop this wrapper's whole tree (M4 18a; `tree::stop`) and put its verdict
/// in `detail.stop`: verified by the tree when nothing is left, which is
/// everything below the wrapper (`scope: descendants`).
fn stop_tree(grace: Duration, detail: &mut serde_json::Value) -> Verdict {
    let s = crate::tree::stop(std::process::id(), grace, &mut reap_all);
    let v = Verdict {
        verified_by: VerifiedBy::Tree,
        killed: Some(s.killed),
        survivors: Some(s.survivors.len() as u32),
        scope: Some("descendants".into()),
        ms: s.ms,
        why: s.why(),
    };
    detail["stop"] = serde_json::to_value(&v).unwrap_or_default();
    v
}

/// Reap every child that has exited, and nothing more: what a stop waits on
/// is its tree.
fn reap_all() {
    loop {
        let mut status = 0;
        match unsafe { libc::waitpid(-1, &mut status, libc::WNOHANG) } {
            -1 if std::io::Error::last_os_error().raw_os_error() == Some(libc::EINTR) => {}
            0 | -1 => return,
            _ => {}
        }
    }
}

/// Reap every child that has exited, orphans reparented here among them.
/// The command's status, when it was one of them.
fn reap_children(command: u32) -> std::io::Result<Option<std::process::ExitStatus>> {
    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt;
        let mut found = None;
        loop {
            let mut status = 0;
            match unsafe { libc::waitpid(-1, &mut status, libc::WNOHANG) } {
                0 => return Ok(found),
                -1 => {
                    let e = std::io::Error::last_os_error();
                    match e.raw_os_error() {
                        Some(libc::EINTR) => continue,
                        Some(libc::ECHILD) if found.is_some() => return Ok(found),
                        _ => return Err(e),
                    }
                }
                pid if pid as u32 == command => {
                    found = Some(std::process::ExitStatus::from_raw(status));
                }
                _ => {}
            }
        }
    }
    #[cfg(not(unix))]
    {
        let _ = command;
        Err(std::io::Error::other("reaping needs Unix"))
    }
}

/// After the report: wait for every descendant that outlived the command,
/// each reparented here as its parent exited (theseus-6qy). While any
/// remains, the spool's `lingering/<id>` names this wrapper, which health
/// counts. The wrapper signals nothing unless it is asked to stop (18a):
/// then it stops what is left of its tree, and a cancel gets its verdict.
fn linger(spool: &Spool, correlation_id: &str) {
    #[cfg(unix)]
    {
        let mut marked = false;
        let mut stopped = false;
        loop {
            if let Some(asked) = stop_asked().filter(|_| !stopped) {
                stopped = true;
                let mut detail = serde_json::json!({});
                let v = stop_tree(asked.grace(), &mut detail);
                // A stop that ended the command has written its verdict.
                if matches!(asked, StopAsked::Cancel { .. })
                    && spool.read_stop(correlation_id).is_none()
                {
                    let _ = spool.write_stop(correlation_id, &v);
                }
            }
            let mut status = 0;
            let flags = if marked { 0 } else { libc::WNOHANG };
            match unsafe { libc::waitpid(-1, &mut status, flags) } {
                0 => {
                    // Descendants remain, and none has exited: say so, then block.
                    let _ = spool.write_lingering(correlation_id, std::process::id());
                    marked = true;
                }
                -1 if std::io::Error::last_os_error().raw_os_error() == Some(libc::EINTR) => {}
                // ECHILD: no descendant remains.
                -1 => break,
                _ => {}
            }
        }
        if marked {
            spool.remove_lingering(correlation_id);
        }
    }
    #[cfg(not(unix))]
    {
        let _ = (spool, correlation_id);
    }
}

/// The job a live wrapper runs, from its command line (theseus-6qy): a
/// process whose first argument is the mode word is a wrapper, and its
/// `--correlation-id`, before the `--` that starts the job's own argv, names
/// the job ("?" if it has none). A zombie's command line is empty, so a
/// wrapper that has exited is none, and so is one still in its exec, whose
/// job cannot be read yet (see `holder`).
pub fn wrapper_job(pid: u32) -> Option<String> {
    let cmdline = std::fs::read(format!("/proc/{pid}/cmdline")).ok()?;
    job_in_cmdline(&cmdline)
}

/// `wrapper_job` over a NUL-separated command line, as `/proc` gives it.
pub fn job_in_cmdline(cmdline: &[u8]) -> Option<String> {
    let mut args = cmdline.split(|&b| b == 0);
    args.next()?;
    if args.next()? != WRAPPER_MODE.as_bytes() {
        return None;
    }
    let own: Vec<&[u8]> = args.take_while(|a| *a != b"--").collect();
    let id = own
        .windows(2)
        .find(|w| w[0] == b"--correlation-id")
        .map(|w| String::from_utf8_lossy(w[1]).into_owned());
    Some(id.unwrap_or_else(|| "?".into()))
}

/// `theseusd`'s options that take a value (theseus-6uo): what follows one is
/// its value, never a subcommand. A test in `theseusd` keeps this in step with
/// its command line.
pub const DAEMON_VALUE_FLAGS: [&str; 4] =
    ["--config", "--op-token-file", "--socket", "--state-dir"];

/// Is this command line a serving `theseusd` (theseus-6uo)? Its program's file
/// name is `theseusd`, and no subcommand follows: the socket daemon or
/// `--stdio`, not a job wrapper, `check`, or `config`. Any daemon's descendant
/// is refused an answer, as a job's is, since no process that answers an
/// approval runs under one.
pub fn daemon_in_cmdline(cmdline: &[u8]) -> bool {
    let mut args = cmdline.split(|&b| b == 0);
    let Some(first) = args.next() else {
        return false;
    };
    let name = first.rsplit(|&b| b == b'/').next().unwrap_or(first);
    if name != b"theseusd" {
        return false;
    }
    while let Some(a) = args.next() {
        if a.is_empty() {
            continue;
        }
        if DAEMON_VALUE_FLAGS.iter().any(|f| f.as_bytes() == a) {
            args.next();
        } else if !a.starts_with(b"-") {
            return false;
        }
    }
    true
}

/// `daemon_in_cmdline` for a live process.
pub fn serving_daemon(pid: u32) -> bool {
    std::fs::read(format!("/proc/{pid}/cmdline")).is_ok_and(|c| daemon_in_cmdline(&c))
}

/// Best-effort poke: one line on a Unix stream socket. Failure is fine; the
/// spool is the truth and the reconciler will find it.
#[cfg(unix)]
pub fn notify(sock: &Path, correlation_id: &str) {
    use std::io::Write;
    if let Ok(mut s) = std::os::unix::net::UnixStream::connect(sock) {
        let _ = s.set_write_timeout(Some(Duration::from_millis(500)));
        let _ = writeln!(s, "{correlation_id}");
    }
}
#[cfg(not(unix))]
pub fn notify(_: &Path, _: &str) {}

/// Is a pid alive? It exists (`kill(pid, 0)`) and is not a zombie. A wrapper
/// that has exited stays a zombie until its parent reaps it. Before the daemon
/// reaped its wrappers (theseus-z4b), a cancel took a wrapper it had killed
/// for alive, and settled `OutcomeUncertain` after its whole grace
/// (theseus-6qy).
pub fn pid_alive(pid: u32) -> bool {
    #[cfg(unix)]
    {
        let exists = unsafe { libc::kill(pid as i32, 0) == 0 };
        exists && !exited(pid)
    }
    #[cfg(not(unix))]
    {
        false
    }
}

/// A zombie, or a process being reaped (`/proc/<pid>/stat` state Z or X).
fn exited(pid: u32) -> bool {
    proc_stat(pid).is_some_and(|s| matches!(s.state, 'Z' | 'X'))
}

/// What holds a job's wrapper pid now (theseus-mi6a).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Holder {
    /// The job's wrapper: its command line names the job.
    Wrapper,
    /// A live process whose command line is still empty: in its exec, or
    /// ending. Whose it is shows at the next look.
    Starting,
    /// Another process, which took the pid once the wrapper was reaped, or a
    /// kernel thread.
    Other,
    /// No live process: none at all, or a zombie.
    Gone,
}

/// The flag of a kernel thread in `/proc/<pid>/stat` (`PF_KTHREAD`), whose
/// command line is empty for good.
const PF_KTHREAD: u64 = 0x0020_0000;

/// What holds `pid` now, for `job` (theseus-mi6a). A process in its exec has
/// an empty command line, as a zombie has: the exec has switched to the new
/// image, and has not set its arguments yet. A spawn returns inside that
/// window, so a wrapper spawned a moment ago may read so. Measured
/// 2026-10-01: a quarter of reads made at once came back empty at load 12,
/// for about 0.1 ms, and the window lasted up to 84 ms for a child starved
/// at nice 19, in state R, S, or D. Its state tells it from a zombie.
pub fn holder(pid: u32, job: &str) -> Holder {
    let cmdline = std::fs::read(format!("/proc/{pid}/cmdline")).ok();
    let stat = match &cmdline {
        Some(c) if c.is_empty() => proc_stat(pid).map(|s| (s.state, s.flags)),
        _ => None,
    };
    decide(cmdline.as_deref(), stat, job)
}

/// `holder`'s rule, over what `/proc` gave: the command line (`None`: no such
/// process) and, for an empty one, the state letter and the flags.
fn decide(cmdline: Option<&[u8]>, stat: Option<(char, u64)>, job: &str) -> Holder {
    match (cmdline, stat) {
        (None, _) => Holder::Gone,
        (Some(c), _) if !c.is_empty() => {
            if job_in_cmdline(c).as_deref() == Some(job) {
                Holder::Wrapper
            } else {
                Holder::Other
            }
        }
        (Some(_), None | Some(('Z' | 'X', _))) => Holder::Gone,
        (Some(_), Some((_, flags))) if flags & PF_KTHREAD != 0 => Holder::Other,
        (Some(_), Some(_)) => Holder::Starting,
    }
}

/// Is `pid` the live wrapper of `job`, as far as can be told? Its command
/// line names the job, which a zombie's (empty) does not, and no other
/// process's can: the daemon reaps its wrappers (theseus-z4b), so a
/// wrapper's pid may be reused once it has gone. A live process still in its
/// exec counts as alive too (theseus-mi6a): it is the wrapper a moment after
/// its spawn, and a reader that keeps or waits on a live wrapper decides
/// again at its next look. A stop, which signals, waits for the exec instead
/// (`Stopping`).
pub fn wrapper_alive(pid: u32, job: &str) -> bool {
    matches!(holder(pid, job), Holder::Wrapper | Holder::Starting)
}

/// How long `execution.cancel` and `/stop` give the jobs they stop, after
/// SIGTERM, before SIGKILL.
pub const STOP_GRACE: Duration = Duration::from_secs(2);

/// How long a SIGKILLed job has to be gone before a stop says it may still
/// run.
const KILL_WAIT: Duration = Duration::from_millis(500);

/// How long past the grace a wrapper asked to stop its tree has to answer
/// (M4 18a): its freeze and its kill's wait (`tree::KILL_WAIT`) take up to a
/// second, and its verdict and exit the rest. Then the daemon kills its
/// group, and the cancel is uncertain.
pub const ANSWER_WAIT: Duration = Duration::from_millis(2500);

/// The longest a stop waits between two looks at its jobs.
const STOP_POLL_MAX: Duration = Duration::from_millis(50);

/// Jobs stopped together (theseus-bzq): each is asked at once, they share one
/// grace, and the stragglers are killed together, so N jobs that ignore
/// SIGTERM cost one grace, not N. It never sleeps: its owner waits between
/// polls, a task on the runtime's timer (the daemon's cancel and stop) or a
/// thread (`terminate`), so no runtime worker is held while a job takes its
/// time.
///
/// **How a job is stopped (M4 18a).** A wrapper from 18a on catches SIGTERM
/// (`catches_sigterm`): it gets SIGTERM alone, by `ask_to_stop`, stops its
/// whole tree itself (`tree::stop`, or the L1 job's init or cgroup), writes
/// its verdict to the spool, and exits. Its job is gone once it has, and its
/// verdict says how that is known. One that has not answered within the
/// grace and `ANSWER_WAIT`, or that exits with no verdict and no completion,
/// has its process group killed, and its verdict is uncertain, with why.
///
/// A wrapper from before 18a dies at the first SIGTERM, so it is stopped as
/// every job was before: its process group gets SIGTERM, then SIGKILL past the
/// grace, and it is gone when no live process is left in the group (a zombie
/// is not live), its verdict `group`. A descendant that left the group
/// (`setsid`) is out of that stop's reach.
///
/// A group is signalled only while `pid` is still this job's wrapper, or no
/// live process at all: while any process of the group lives, its id is not
/// handed out again, but once the wrapper has been reaped and the group is
/// empty, `pid` may be another process's, which is left alone. A live process
/// whose command line is still empty is in its exec (theseus-mi6a), and may
/// be either: it is asked once it reads as the wrapper, at a later look, and
/// never if it reads as another process.
pub struct Stopping {
    spool: Spool,
    jobs: Vec<Stopped>,
    grace: Duration,
    started: Instant,
    polls: u32,
}

/// One job a stop reaches: its wrapper's pid, which is its process group.
struct Stopped {
    pid: u32,
    job: String,
    /// How it was asked: `None` until its pid read as its wrapper, or as no
    /// live process.
    asked: Option<Asked>,
    /// When its group got SIGKILL.
    killed: Option<Instant>,
    /// Its group's live members when it was asked (an older wrapper's).
    members: u32,
    /// Its verdict, once it is settled.
    verdict: Option<Verdict>,
}

/// How a job was asked to stop.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Asked {
    /// Its wrapper stops its tree and answers with a verdict.
    Tree,
    /// Its process group, as before 18a: a wrapper that dies at SIGTERM, or
    /// one already gone.
    Group,
}

impl Stopping {
    /// Ask every job to stop, now; a job whose wrapper is still in its exec,
    /// at the first look that reads it as the job's. `spool` is where a
    /// wrapper's verdict is.
    pub fn start(
        spool: &Spool,
        jobs: impl IntoIterator<Item = (u32, String)>,
        grace: Duration,
    ) -> Self {
        let mut s = Self {
            spool: spool.clone(),
            jobs: jobs
                .into_iter()
                .map(|(pid, job)| Stopped {
                    pid,
                    job,
                    asked: None,
                    killed: None,
                    members: 0,
                    verdict: None,
                })
                .collect(),
            grace,
            started: Instant::now(),
            polls: 0,
        };
        s.ask_known();
        s
    }

    /// Ask each job not yet asked whose pid now reads as its wrapper or as
    /// no live process. A pid another process holds means the job's wrapper
    /// is long gone: its verdict is whatever it left. One still in its exec
    /// waits for the next look.
    fn ask_known(&mut self) {
        let late = self.started.elapsed() >= self.grace;
        for i in 0..self.jobs.len() {
            let j = &self.jobs[i];
            if j.asked.is_some() || j.verdict.is_some() {
                continue;
            }
            let (pid, grace) = (j.pid, self.grace);
            match holder(pid, &j.job) {
                Holder::Starting => {}
                Holder::Other => {
                    let v = self.left_behind(i, false);
                    self.jobs[i].verdict = Some(v);
                }
                // A wrapper that answered an earlier ask (a stop at its launch, then the cancel's).
                Holder::Gone if self.spool.read_stop(&self.jobs[i].job).is_some() => {
                    let v = self.left_behind(i, true);
                    self.jobs[i].verdict = Some(v);
                }
                Holder::Wrapper if catches_sigterm(pid) => {
                    ask_to_stop(pid, grace.saturating_sub(self.started.elapsed()));
                    self.jobs[i].asked = Some(Asked::Tree);
                }
                Holder::Wrapper | Holder::Gone => {
                    let j = &mut self.jobs[i];
                    j.members = group_members(pid);
                    j.asked = Some(Asked::Group);
                    if late {
                        signal_group(pid, libc::SIGKILL);
                        j.killed = Some(Instant::now());
                    } else {
                        signal_group(pid, libc::SIGTERM);
                    }
                }
            }
        }
    }

    /// The verdict of job `i`, whose wrapper was asked to stop its tree and
    /// is gone: what it wrote; or, with no verdict but a completion, a job
    /// that ended before the stop reached it, whose wrapper left only once
    /// its tree was empty; or neither, and its group is killed, uncertain.
    /// `asked`: whether it was asked at all (its pid read as another
    /// process's at the first look).
    fn left_behind(&self, i: usize, asked: bool) -> Verdict {
        let j = &self.jobs[i];
        let ms = self.started.elapsed().as_millis() as u64;
        if let Some(v) = self.spool.read_stop(&j.job) {
            self.spool.remove_stop(&j.job);
            return v;
        }
        if self.spool.has_completion(&j.job) {
            return Verdict {
                verified_by: VerifiedBy::Tree,
                killed: Some(0),
                survivors: Some(0),
                scope: Some("descendants".into()),
                ms,
                why: None,
            };
        }
        // Its group: what is left of it, killed, when the pid is free (a
        // pid another process holds left an empty group).
        if holder(j.pid, &j.job) == Holder::Gone {
            signal_group(j.pid, libc::SIGKILL);
        }
        let why = if asked {
            "its wrapper ended without a verdict or a report"
        } else {
            "its wrapper had ended before the stop, with no report"
        };
        Verdict {
            ms,
            ..Verdict::uncertain(VerifiedBy::Tree, why)
        }
    }

    /// Look at every job not yet settled; kill the groups whose time is up.
    /// How long to wait before the next look, or `None` once every job has
    /// its verdict.
    pub fn poll(&mut self) -> Option<Duration> {
        self.ask_known();
        let now = Instant::now();
        let ms = self.started.elapsed().as_millis() as u64;
        let grace_over = now >= self.started + self.grace;
        let answer_over = now >= self.started + self.grace + ANSWER_WAIT;
        let groups: Vec<u32> = self
            .jobs
            .iter()
            .filter(|j| j.verdict.is_none() && j.asked == Some(Asked::Group))
            .map(|j| j.pid)
            .collect();
        let live = live_groups(&groups);
        for i in 0..self.jobs.len() {
            let j = &self.jobs[i];
            if j.verdict.is_some() {
                continue;
            }
            let (pid, alive, in_group) =
                (j.pid, wrapper_alive(j.pid, &j.job), live.contains(&j.pid));
            let killed_over = j.killed.is_some_and(|k| now >= k + KILL_WAIT);
            let verdict = match j.asked {
                // Never read as its wrapper: once the kill's time is past too.
                None => (now >= self.started + self.grace + ANSWER_WAIT + KILL_WAIT).then(|| Verdict {
                    ms,
                    ..Verdict::uncertain(VerifiedBy::None, "its wrapper never read as the job's")
                }),
                Some(Asked::Group) if !alive && !in_group => Some(Verdict {
                    verified_by: VerifiedBy::Group,
                    killed: Some(j.members),
                    survivors: Some(0),
                    scope: Some("group".into()),
                    ms,
                    why: None,
                }),
                Some(Asked::Group) if killed_over => {
                    let left = group_members(pid);
                    Some(Verdict {
                        killed: Some(j.members),
                        survivors: Some(left),
                        ms,
                        ..Verdict::uncertain(
                            VerifiedBy::Group,
                            format!("{left} of its process group outlived the kill"),
                        )
                    })
                }
                Some(Asked::Group) => {
                    if j.killed.is_none() && grace_over {
                        signal_group(pid, libc::SIGKILL);
                        self.jobs[i].killed = Some(now);
                    }
                    None
                }
                Some(Asked::Tree) if !alive && j.killed.is_none() => Some(self.left_behind(i, true)),
                Some(Asked::Tree) if killed_over || (!alive && j.killed.is_some()) => Some(Verdict {
                    survivors: Some(group_members(pid)),
                    ms,
                    ..Verdict::uncertain(
                        VerifiedBy::Tree,
                        format!(
                            "its wrapper did not answer within {} ms, so its process group was killed",
                            (self.grace + ANSWER_WAIT).as_millis()
                        ),
                    )
                }),
                Some(Asked::Tree) => {
                    if j.killed.is_none() && answer_over {
                        signal_group(pid, libc::SIGKILL);
                        self.jobs[i].killed = Some(now);
                    }
                    None
                }
            };
            if verdict.is_some() {
                self.jobs[i].verdict = verdict;
            }
        }
        if self.all_gone() {
            return None;
        }
        // The next deadline: the grace's end, the answer's, or a kill's wait.
        let next = [
            Some(self.started + self.grace),
            Some(self.started + self.grace + ANSWER_WAIT),
            Some(self.started + self.grace + ANSWER_WAIT + KILL_WAIT),
        ]
        .into_iter()
        .chain(self.jobs.iter().map(|j| j.killed.map(|k| k + KILL_WAIT)))
        .flatten()
        .filter(|t| *t > now)
        .min()
        .map_or(STOP_POLL_MAX, |t| t - now);
        // Soon at first, since most jobs end at once; then less often.
        self.polls += 1;
        let step = Duration::from_millis(5 << self.polls.min(4)).min(STOP_POLL_MAX);
        Some(step.min(next).max(Duration::from_millis(1)))
    }

    /// Each job, and its verdict (uncertain while it has none).
    pub fn verdicts(&self) -> impl Iterator<Item = (&str, Verdict)> {
        self.jobs.iter().map(|j| {
            (
                j.job.as_str(),
                j.verdict.clone().unwrap_or_else(|| {
                    Verdict::uncertain(VerifiedBy::None, "the stop ended before its verdict")
                }),
            )
        })
    }

    /// Every job has its verdict.
    pub fn all_gone(&self) -> bool {
        self.jobs.iter().all(|j| j.verdict.is_some())
    }
}

/// Stop one job and wait for it on this thread, as `Stopping` stops several.
/// Its verdict.
pub fn terminate(spool: &Spool, pid: u32, job: &str, grace: Duration) -> Verdict {
    let mut s = Stopping::start(spool, [(pid, job.to_string())], grace);
    while let Some(wait) = s.poll() {
        std::thread::sleep(wait);
    }
    let v = s.verdicts().next().map(|(_, v)| v);
    v.expect("one job")
}

/// Signal the process group `pgid`. One with no member left is a no-op.
fn signal_group(pgid: u32, signal: libc::c_int) {
    // SAFETY: kill(2) on a negated pid signals that group; no memory is shared.
    unsafe {
        libc::kill(-(pgid as i32), signal);
    }
}

/// The live members of process group `pgid`: running, sleeping, or stopped,
/// never a zombie. A group with no member costs no scan.
fn group_members(pgid: u32) -> u32 {
    // SAFETY: signal 0 only checks that the group exists.
    if unsafe { libc::kill(-(pgid as i32), 0) } != 0 {
        return 0;
    }
    let Ok(dir) = std::fs::read_dir("/proc") else {
        return 1;
    };
    dir.flatten()
        .filter_map(|e| e.file_name().to_str().and_then(|s| s.parse::<u32>().ok()))
        .filter_map(proc_stat)
        .filter(|s| s.pgrp == pgid && !matches!(s.state, 'Z' | 'X'))
        .count() as u32
}

/// The process groups among `pgids` with a live member: running, sleeping,
/// or stopped, never a zombie. A group with no member at all fails
/// `kill(-pgid, 0)` and costs no scan; the rest are found in one pass over
/// `/proc`. When `/proc` cannot be read, every group that has a member is
/// taken for live.
fn live_groups(pgids: &[u32]) -> Vec<u32> {
    // SAFETY: signal 0 only checks that the group exists and may be signalled.
    let members: Vec<u32> = pgids
        .iter()
        .copied()
        .filter(|&g| unsafe { libc::kill(-(g as i32), 0) } == 0)
        .collect();
    if members.is_empty() {
        return members;
    }
    let Ok(dir) = std::fs::read_dir("/proc") else {
        return members;
    };
    let mut live = Vec::new();
    for e in dir.flatten() {
        let Some(pid) = e.file_name().to_str().and_then(|s| s.parse::<u32>().ok()) else {
            continue;
        };
        if let Some(s) = proc_stat(pid) {
            if members.contains(&s.pgrp) && !matches!(s.state, 'Z' | 'X') && !live.contains(&s.pgrp)
            {
                live.push(s.pgrp);
            }
        }
    }
    live
}

/// What `/proc/<pid>/stat` says of a process.
struct ProcStat {
    state: char,
    pgrp: u32,
    flags: u64,
}

/// A process's state letter, process group, and flags, from
/// `/proc/<pid>/stat`.
fn proc_stat(pid: u32) -> Option<ProcStat> {
    let s = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    let mut f = s[s.rfind(')')? + 1..].split_whitespace();
    let state = f.next()?.chars().next()?;
    f.next()?; // the parent's pid
    let pgrp = f.next()?.parse().ok()?;
    // After the session, the terminal, and its process group.
    let flags = f.nth(3)?.parse().ok()?;
    Some(ProcStat { state, pgrp, flags })
}

/// Evidence from the spool and the wrapper pids.
pub struct WrapperEvidence {
    pub spool: Spool,
}

impl Evidence for WrapperEvidence {
    fn probe(&self, action: &Action) -> Probe {
        if let Ok(Some(c)) = self.spool.read_completion(&action.correlation_id) {
            return Probe::Completed(c);
        }
        match self.spool.read_pid(&action.correlation_id) {
            Some(pid) if wrapper_alive(pid, &action.correlation_id) => Probe::StillRunning,
            _ => Probe::Gone,
        }
    }
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// Parse wrapper-mode argv (everything after the mode word). Kept here so
/// every binary that offers wrapper mode parses it identically.
pub fn parse_wrapper_args<I: IntoIterator<Item = String>>(args: I) -> Result<WrapperArgs> {
    let mut it = args.into_iter();
    let mut spool_dir = None;
    let mut correlation_id = None;
    let mut deadline_ms = 600_000u64;
    let mut notify_socket = None;
    let mut cwd = None;
    let mut umask = None;
    let mut redact = Vec::new();
    let mut output_max_bytes = DEFAULT_OUTPUT_MAX_BYTES;
    let mut sandbox = None;
    let mut argv = Vec::new();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--sandbox" => {
                let v = it.next().unwrap_or_default();
                sandbox =
                    Some(serde_json::from_str(&v).with_context(|| format!("bad --sandbox {v:?}"))?);
            }
            "--output-max-bytes" => {
                let v = it.next().unwrap_or_default();
                output_max_bytes = v
                    .parse()
                    .with_context(|| format!("bad --output-max-bytes {v:?}"))?;
            }
            "--redact" => {
                let v = it.next().unwrap_or_default();
                let (var, secret) = v
                    .split_once('=')
                    .with_context(|| format!("bad --redact {v:?}: VARIABLE=secret"))?;
                redact.push((var.to_string(), secret.to_string()));
            }
            "--umask" => {
                let v = it.next().unwrap_or_default();
                umask =
                    Some(crate::umask::parse(&v).with_context(|| format!("bad --umask {v:?}"))?);
            }
            "--spool" => spool_dir = it.next().map(PathBuf::from),
            "--correlation-id" => correlation_id = it.next(),
            "--deadline-ms" => {
                deadline_ms = it
                    .next()
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(deadline_ms)
            }
            "--notify" => notify_socket = it.next().map(PathBuf::from),
            "--cwd" => cwd = it.next().map(PathBuf::from),
            "--" => {
                argv = it.collect();
                break;
            }
            other => anyhow::bail!("unknown wrapper argument {other:?}"),
        }
    }
    if argv.is_empty() {
        anyhow::bail!("wrapper: no command after --");
    }
    Ok(WrapperArgs {
        spool_dir: spool_dir.context("--spool required")?,
        correlation_id: correlation_id.context("--correlation-id required")?,
        deadline_ms,
        notify_socket,
        argv,
        cwd,
        env: Vec::new(),
        umask,
        redact,
        output_max_bytes,
        sandbox,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cmdline(args: &[&str]) -> Vec<u8> {
        let mut v = args.join("\0").into_bytes();
        v.push(0);
        v
    }

    /// A job's raw output is 0600 whatever the process's umask (theseus-wz2):
    /// here the test's own, not the daemon's 077, and over a file a retried
    /// job left open to others. `--umask` round-trips through the command
    /// line.
    #[test]
    fn a_jobs_raw_output_is_0600_whatever_the_umask() {
        use std::os::unix::fs::PermissionsExt;
        let d = tempfile::tempdir().unwrap();
        let spool = Spool::open(d.path()).unwrap();
        let out = spool.result_path("act_mode");
        std::fs::write(&out, "stale").unwrap();
        std::fs::set_permissions(&out, std::fs::Permissions::from_mode(0o644)).unwrap();
        let args = WrapperArgs {
            spool_dir: d.path().to_path_buf(),
            correlation_id: "act_mode".into(),
            deadline_ms: 10_000,
            notify_socket: None,
            argv: vec!["sh".into(), "-c".into(), "echo out".into()],
            cwd: None,
            env: vec![("PATH".into(), std::env::var("PATH").unwrap_or_default())],
            umask: None,
            redact: vec![],
            output_max_bytes: DEFAULT_OUTPUT_MAX_BYTES,
            sandbox: None,
        };
        run_wrapper(&args).unwrap();
        assert_eq!(std::fs::read_to_string(&out).unwrap(), "out\n");
        let mode = std::fs::metadata(&out).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
        let parsed = parse_wrapper_args(
            [
                "--spool",
                "/s",
                "--correlation-id",
                "act_x",
                "--umask",
                "0027",
                "--",
                "true",
            ]
            .map(String::from),
        )
        .unwrap();
        assert_eq!(parsed.umask, Some(0o027));
        assert!(parse_wrapper_args(
            [
                "--spool",
                "/s",
                "--correlation-id",
                "act_x",
                "--umask",
                "9",
                "--",
                "true"
            ]
            .map(String::from)
        )
        .is_err());
    }

    /// A job that prints its own granted value (theseus-l0d), split across
    /// two writes, among other output on stdout and stderr: its raw output
    /// holds everything else, byte for byte, and the value withheld, and the
    /// completion counts it. `--redact` round-trips through the command line,
    /// names only.
    #[test]
    fn a_granted_value_a_job_prints_never_reaches_its_raw_output() {
        let d = tempfile::tempdir().unwrap();
        let spool = Spool::open(d.path()).unwrap();
        // 26 bytes: two halves of 13.
        let value = "tv-invented_grant-5c1e9b7a";
        let args = WrapperArgs {
            spool_dir: d.path().to_path_buf(),
            correlation_id: "act_grant".into(),
            deadline_ms: 10_000,
            notify_socket: None,
            argv: vec![
                "sh".into(),
                "-c".into(),
                "printf 'one\\n%s' \"${INVENTED_GRANT%?????????????}\"; sleep 0.2; \
                 printf '%s two\\n' \"${INVENTED_GRANT#?????????????}\"; echo three >&2"
                    .into(),
            ],
            cwd: None,
            env: vec![
                ("PATH".into(), std::env::var("PATH").unwrap_or_default()),
                ("INVENTED_GRANT".into(), value.into()),
            ],
            umask: None,
            redact: vec![("INVENTED_GRANT".into(), "invented_grant".into())],
            output_max_bytes: DEFAULT_OUTPUT_MAX_BYTES,
            sandbox: None,
        };
        run_wrapper(&args).unwrap();
        let out = std::fs::read(spool.result_path("act_grant")).unwrap();
        assert_eq!(
            String::from_utf8_lossy(&out),
            "one\n[redacted:invented_grant] two\nthree\n"
        );
        let c = spool.read_completion("act_grant").unwrap().unwrap();
        let detail = c.detail.unwrap();
        assert_eq!(detail["withheld"], 1, "{detail}");
        assert_eq!(detail["bytes"], out.len() as u64, "{detail}");
        let parsed = parse_wrapper_args(
            [
                "--spool",
                "/s",
                "--correlation-id",
                "act_x",
                "--redact",
                "GH_TOKEN=github_token",
                "--",
                "gh",
            ]
            .map(String::from),
        )
        .unwrap();
        assert_eq!(
            parsed.redact,
            vec![("GH_TOKEN".to_string(), "github_token".to_string())]
        );
        assert_eq!(parsed.output_max_bytes, DEFAULT_OUTPUT_MAX_BYTES);
        assert!(!format!("{args:?}").contains(value), "Debug names only");
    }

    /// Review 2's R3 (theseus-102), with a grant: the file keeps the cap, its
    /// value withheld. Since theseus-gsn9 the cap keeps both ends: the head,
    /// a marker that counts the bytes dropped between, and the last bytes,
    /// the value there withheld too, since it is kept. The command runs to
    /// its end. `--output-max-bytes` round-trips.
    #[test]
    fn a_granted_job_that_prints_past_the_cap_keeps_both_ends_without_its_value() {
        let d = tempfile::tempdir().unwrap();
        let spool = Spool::open(d.path()).unwrap();
        let value = "tv-invented_grant-5c1e9b7a";
        let after = d.path().join("after");
        let args = WrapperArgs {
            spool_dir: d.path().to_path_buf(),
            correlation_id: "act_capped".into(),
            deadline_ms: 30_000,
            notify_socket: None,
            argv: vec![
                "sh".into(),
                "-c".into(),
                "echo \"$INVENTED_GRANT\"; head -c 1000000 /dev/zero; echo \"$INVENTED_GRANT\"; \
                 : > \"$0\""
                    .into(),
                after.display().to_string(),
            ],
            cwd: None,
            env: vec![
                ("PATH".into(), std::env::var("PATH").unwrap_or_default()),
                ("INVENTED_GRANT".into(), value.into()),
            ],
            umask: None,
            redact: vec![("INVENTED_GRANT".into(), "invented_grant".into())],
            output_max_bytes: 4096,
            sandbox: None,
        };
        run_wrapper(&args).unwrap();
        assert!(after.exists(), "the job ran to its end");
        let out = std::fs::read(spool.result_path("act_capped")).unwrap();
        // Printed: the value's line (27 bytes), a million zeros, the value's
        // line again; withheld, 26 + 1,000,000 + 26. The cap of 4,096 keeps
        // a head of 1,920 and an end of 2,048, with the marker between.
        let (head, tail) = crate::redact::split(4096);
        assert_eq!((head, tail), (1_920, 2_048));
        let dropped = 1_000_052 - head - tail;
        let marker = crate::redact::marker(dropped, tail);
        assert!(out.len() <= 4096, "{}", out.len());
        assert_eq!(out.len() as u64, head + marker.len() as u64 + tail);
        let mark = b"[redacted:invented_grant]\n";
        assert_eq!(&out[..mark.len()], mark);
        assert!(out[mark.len()..head as usize].iter().all(|&b| b == 0));
        let (between, end) = out[head as usize..].split_at(marker.len());
        assert_eq!(between, marker.as_bytes());
        assert_eq!(&end[end.len() - mark.len()..], mark, "the end, withheld");
        assert!(end[..end.len() - mark.len()].iter().all(|&b| b == 0));
        let c = spool.read_completion("act_capped").unwrap().unwrap();
        assert_eq!(c.outcome, Outcome::Succeeded);
        let detail = c.detail.unwrap();
        assert_eq!(detail["dropped"], dropped, "{detail}");
        assert_eq!(detail["truncated"], true, "{detail}");
        assert_eq!(detail["output_max_bytes"], 4096, "{detail}");
        assert_eq!(
            (&detail["head"], &detail["tail"]),
            (&serde_json::json!(head), &serde_json::json!(tail)),
            "{detail}"
        );
        assert_eq!(detail["withheld"], 2, "{detail}");
        assert_eq!(detail["bytes"], out.len(), "{detail}");
        let parsed = parse_wrapper_args(
            [
                "--spool",
                "/s",
                "--correlation-id",
                "act_x",
                "--output-max-bytes",
                "4096",
                "--",
                "true",
            ]
            .map(String::from),
        )
        .unwrap();
        assert_eq!(parsed.output_max_bytes, 4096);
    }

    /// A wrapper is known by its command line, as `spawn_detached` makes it,
    /// and names its job; the job's own argv, after `--`, names nothing.
    #[test]
    fn a_wrapper_is_known_by_its_command_line() {
        let wrapper = |id_args: &[&str], argv: &[&str]| {
            let mut a = vec!["theseusd", WRAPPER_MODE, "--spool", "/s/spool"];
            a.extend_from_slice(id_args);
            a.extend_from_slice(&["--deadline-ms", "600000", "--"]);
            a.extend_from_slice(argv);
            job_in_cmdline(&cmdline(&a))
        };
        assert_eq!(
            wrapper(&["--correlation-id", "act_1"], &["sh", "-c", "true"]).as_deref(),
            Some("act_1")
        );
        // The job's argv cannot name another job, and a wrapper with no id is
        // still a wrapper.
        assert_eq!(
            wrapper(&[], &["x", "--correlation-id", "act_2"]).as_deref(),
            Some("?")
        );
        for other in [
            cmdline(&["theseusd", "--config", "x"]),
            cmdline(&["sh", "-c", "theseusd job-wrapper --correlation-id act_3"]),
            cmdline(&["theseusd"]),
            vec![],
        ] {
            assert_eq!(job_in_cmdline(&other), None, "{other:?}");
        }
    }

    /// A serving daemon is known by its command line (theseus-6uo): `theseusd`
    /// with options only, the socket daemon or `--stdio`. A wrapper, a
    /// subcommand, and another program are not, and an option's value is not
    /// taken for a subcommand.
    #[test]
    fn a_serving_daemon_is_known_by_its_command_line() {
        for serving in [
            &["theseusd"][..],
            &[
                "/home/x/.local/bin/theseusd",
                "--config",
                "check",
                "--socket",
                "/s",
            ],
            &["theseusd", "--stdio", "--state-dir", "/tmp/x"],
            &[
                "target/debug/theseusd",
                "--config=/c.toml",
                "--op-token-file",
                "config",
            ],
        ] {
            assert!(daemon_in_cmdline(&cmdline(serving)), "{serving:?}");
        }
        for other in [
            &["theseusd", WRAPPER_MODE, "--spool", "/s", "--", "sh"][..],
            &["theseusd", "--config", "/c.toml", "check"],
            &["theseusd", "config"],
            &["theseus", "--socket", "/s", "confirm"],
            &["theseusd-old"],
            &["sh", "-c", "theseusd"],
        ] {
            assert!(!daemon_in_cmdline(&cmdline(other)), "{other:?}");
        }
        assert!(!daemon_in_cmdline(&[]));
    }

    /// A zombie is not alive: it has exited, and waits only to be reaped.
    #[test]
    fn a_zombie_is_not_alive() {
        let mut child = std::process::Command::new("true").spawn().unwrap();
        let pid = child.id();
        let t0 = Instant::now();
        while !exited(pid) {
            assert!(t0.elapsed() < Duration::from_secs(10), "true never exited");
            std::thread::sleep(Duration::from_millis(5));
        }
        assert!(!pid_alive(pid), "unreaped, it still answers kill(pid, 0)");
        assert_eq!(holder(pid, "act_1"), Holder::Gone);
        assert!(!wrapper_alive(pid, "act_1"));
        child.wait().unwrap();
        assert!(pid_alive(std::process::id()));
    }

    /// What holds a wrapper's pid (theseus-mi6a), from its command line and,
    /// when that is empty, its state. A process in its exec and a zombie both
    /// have an empty command line; the state tells them apart, and a kernel
    /// thread's flag tells it from a process starting. Only the wrapper and
    /// a process still in its exec count as alive.
    #[test]
    fn the_holder_of_a_wrappers_pid_is_told_by_its_command_line_then_its_state() {
        let job = "act_1";
        let wrapper = cmdline(&[
            "theseusd",
            WRAPPER_MODE,
            "--correlation-id",
            job,
            "--",
            "sh",
        ]);
        let others = [
            cmdline(&[
                "theseusd",
                WRAPPER_MODE,
                "--correlation-id",
                "act_2",
                "--",
                "sh",
            ]),
            cmdline(&["sleep", "30"]),
            cmdline(&["sh", "-c", "theseusd job-wrapper --correlation-id act_1"]),
        ];
        // Running, sleeping, and waiting on the disk, as a starved exec was
        // seen to be; also stopped, traced, and idle.
        let live = ['R', 'S', 'D', 'T', 't', 'I'];
        for state in live {
            assert_eq!(decide(Some(&wrapper), None, job), Holder::Wrapper);
            assert_eq!(
                decide(Some(&[]), Some((state, 0)), job),
                Holder::Starting,
                "{state}"
            );
            assert_eq!(
                decide(Some(&[]), Some((state, 0x40_0140)), job),
                Holder::Starting,
                "{state}, other flags"
            );
            assert_eq!(
                decide(Some(&[]), Some((state, PF_KTHREAD | 0x8040)), job),
                Holder::Other,
                "a kernel thread, {state}"
            );
        }
        for other in &others {
            assert_eq!(decide(Some(other), None, job), Holder::Other, "{other:?}");
        }
        for zombie in ['Z', 'X'] {
            assert_eq!(decide(Some(&[]), Some((zombie, 0)), job), Holder::Gone);
        }
        // No such process, or none left by the time its state was read.
        assert_eq!(decide(None, None, job), Holder::Gone);
        assert_eq!(decide(Some(&[]), None, job), Holder::Gone);
    }

    /// `holder` over live processes: this one is another process, and a
    /// stand-in wrapper, once its exec is done, is the wrapper of its job and
    /// of no other.
    #[test]
    fn a_live_wrapper_and_another_process_are_told_apart() {
        let me = std::process::id();
        assert_eq!(holder(me, "act_1"), Holder::Other);
        assert!(!wrapper_alive(me, "act_1"));
        let d = tempfile::tempdir().unwrap();
        std::fs::write(
            d.path().join(WRAPPER_MODE),
            "while [ -d \"$PWD\" ]; do sleep 0.05; done\n",
        )
        .unwrap();
        let mut w = Command::new("sh")
            .args([WRAPPER_MODE, "--correlation-id", "act_1"])
            .current_dir(d.path())
            .spawn()
            .unwrap();
        let pid = w.id();
        let t0 = Instant::now();
        while holder(pid, "act_1") == Holder::Starting {
            assert!(
                t0.elapsed() < Duration::from_secs(10),
                "its exec never ended"
            );
            std::thread::sleep(Duration::from_millis(1));
        }
        assert_eq!(holder(pid, "act_1"), Holder::Wrapper);
        assert!(wrapper_alive(pid, "act_1"));
        assert_eq!(holder(pid, "act_2"), Holder::Other);
        assert!(!wrapper_alive(pid, "act_2"));
        let _ = w.kill();
        let _ = w.wait();
    }
}
