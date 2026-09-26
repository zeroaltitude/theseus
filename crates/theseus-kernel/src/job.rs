//! The job wrapper (§3.16, §7): detached, durable, cancellable.
//!
//! `spawn_detached` starts *this binary* in wrapper mode as its own session
//! (setsid), so its lifetime does not depend on the harness. The wrapper
//! (`run_wrapper`) runs the real command, enforces its own deadline, captures
//! output to the spool's `results/`, writes the `Completion` to the spool
//! (tmp+rename), then pokes the harness over a Unix socket if one is given.
//! Cancellation kills the wrapper's process group by correlation id and
//! verifies the pid is gone; `WrapperEvidence` is what the reconciler asks.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use anyhow::{Context, Result};

use crate::kernel::{Evidence, Probe};
use crate::spool::Spool;
use crate::types::{Action, Completion, Outcome};

/// Arguments the wrapper mode receives.
#[derive(Debug, Clone)]
pub struct WrapperArgs {
    pub spool_dir: PathBuf,
    pub correlation_id: String,
    pub deadline_ms: u64,
    pub notify_socket: Option<PathBuf>,
    pub argv: Vec<String>,
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
    cmd.arg("--").args(&args.argv);
    cmd.stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
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
    let child = cmd.spawn().context("spawning job wrapper")?;
    let pid = child.id();
    spool.write_pid(&args.correlation_id, pid)?;
    // Do not wait on it; it is detached by design. Reap nothing here.
    std::mem::forget(child);
    Ok(pid)
}

/// The wrapper's body. Runs in the detached process.
pub fn run_wrapper(args: WrapperArgs) -> Result<()> {
    let spool = Spool::open(&args.spool_dir)?;
    let started = now_ms();
    let t0 = Instant::now();
    let out_path = spool.result_path(&args.correlation_id);
    let out_file = std::fs::File::create(&out_path)?;
    let err_file = out_file.try_clone()?;
    let mut child = Command::new(&args.argv[0])
        .args(&args.argv[1..])
        .stdin(Stdio::null())
        .stdout(Stdio::from(out_file))
        .stderr(Stdio::from(err_file))
        .spawn();
    let (outcome, note) = match &mut child {
        Err(e) => (Outcome::Failed, format!("spawn: {e}")),
        Ok(child) => {
            let deadline = Duration::from_millis(args.deadline_ms);
            loop {
                match child.try_wait() {
                    Ok(Some(status)) => {
                        break if status.success() {
                            (Outcome::Succeeded, format!("exit {status}"))
                        } else {
                            (Outcome::Failed, format!("exit {status}"))
                        };
                    }
                    Ok(None) => {
                        if t0.elapsed() >= deadline {
                            let _ = child.kill();
                            let _ = child.wait();
                            break (
                                Outcome::Failed,
                                format!("deadline {} ms exceeded; killed", args.deadline_ms),
                            );
                        }
                        std::thread::sleep(Duration::from_millis(20));
                    }
                    Err(e) => break (Outcome::Unknown, format!("wait: {e}")),
                }
            }
        }
    };
    let c = Completion {
        correlation_id: args.correlation_id.clone(),
        outcome,
        result_ref: Some(out_path.to_string_lossy().into_owned()),
        external_op_id: Some(note.to_string()),
        started_at_ms: started,
        finished_at_ms: now_ms(),
        producer: format!("wrapper:{}", std::process::id()),
        signature: None,
        usage_units: None,
    };
    spool.write(&c)?; // durable before any delivery attempt
    spool.remove_pid(&args.correlation_id);
    if let Some(sock) = &args.notify_socket {
        notify(sock, &args.correlation_id);
    }
    Ok(())
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

/// Is a pid alive? (`kill(pid, 0)`).
pub fn pid_alive(pid: u32) -> bool {
    #[cfg(unix)]
    {
        unsafe { libc::kill(pid as i32, 0) == 0 }
    }
    #[cfg(not(unix))]
    {
        false
    }
}

/// Terminate a wrapper's whole process group: SIGTERM, wait up to `grace`,
/// then SIGKILL. Returns true if the pid is gone afterwards.
pub fn terminate(pid: u32, grace: Duration) -> bool {
    #[cfg(unix)]
    {
        unsafe {
            libc::kill(-(pid as i32), libc::SIGTERM);
        }
        let t0 = Instant::now();
        while t0.elapsed() < grace {
            if !pid_alive(pid) {
                return true;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        unsafe {
            libc::kill(-(pid as i32), libc::SIGKILL);
        }
        let t0 = Instant::now();
        while t0.elapsed() < Duration::from_millis(500) {
            if !pid_alive(pid) {
                return true;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        !pid_alive(pid)
    }
    #[cfg(not(unix))]
    {
        let _ = (pid, grace);
        false
    }
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
            Some(pid) if pid_alive(pid) => Probe::StillRunning,
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
    let mut argv = Vec::new();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--spool" => spool_dir = it.next().map(PathBuf::from),
            "--correlation-id" => correlation_id = it.next(),
            "--deadline-ms" => {
                deadline_ms = it
                    .next()
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(deadline_ms)
            }
            "--notify" => notify_socket = it.next().map(PathBuf::from),
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
    })
}
