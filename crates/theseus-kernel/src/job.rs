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
    /// Working directory for the command.
    pub cwd: Option<PathBuf>,
    /// The complete environment of the wrapper and its command. The spawner
    /// clears everything else, so nothing the harness holds (the 1Password
    /// service-account token above all) leaks into a job.
    pub env: Vec<(String, String)>,
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
    let subreaper = crate::children::set_subreaper();
    let spool = run(args, Reap::Descendants, subreaper.err())?;
    linger(&spool, &args.correlation_id);
    Ok(())
}

/// The wrapper's body on a thread of this process (the tests' in-process
/// launcher). It waits for its own command only: a thread that waited for
/// any child, or made its process a subreaper, would take other threads'
/// children.
pub fn run_wrapper(args: &WrapperArgs) -> Result<()> {
    run(args, Reap::Command, None).map(|_| ())
}

/// What a wrapper waits for while its command runs.
#[derive(Clone, Copy)]
enum Reap {
    /// The command alone.
    Command,
    /// Every child: the command, and each descendant reparented here.
    Descendants,
}

/// The command, its deadline, and the report. `subreaper_error` is why the
/// wrapper could not become a subreaper, recorded in the completion.
fn run(args: &WrapperArgs, reap: Reap, subreaper_error: Option<String>) -> Result<Spool> {
    let spool = Spool::open(&args.spool_dir)?;
    let started = now_ms();
    let t0 = Instant::now();
    let out_path = spool.result_path(&args.correlation_id);
    let out_file = std::fs::File::create(&out_path)?;
    let err_file = out_file.try_clone()?;
    let mut command = Command::new(&args.argv[0]);
    command
        .args(&args.argv[1..])
        .stdin(Stdio::null())
        .stdout(Stdio::from(out_file))
        .stderr(Stdio::from(err_file));
    if let Some(c) = &args.cwd {
        command.current_dir(c);
    }
    let mut child = command.spawn();
    drop(command);
    let mut detail = serde_json::json!({});
    if let Some(e) = subreaper_error {
        detail["subreaper_error"] = serde_json::Value::String(e);
    }
    let (outcome, note) = match &mut child {
        Err(e) => {
            detail["spawn_error"] = serde_json::Value::String(e.to_string());
            (Outcome::Failed, format!("spawn: {e}"))
        }
        Ok(child) => {
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
                            detail["timed_out"] = serde_json::Value::Bool(true);
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
    Ok(spool)
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
/// counts. The wrapper signals nothing; it only waits.
fn linger(spool: &Spool, correlation_id: &str) {
    #[cfg(unix)]
    {
        let mut marked = false;
        loop {
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
/// wrapper that has exited is none.
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
    std::fs::read_to_string(format!("/proc/{pid}/stat"))
        .ok()
        .and_then(|s| {
            s.rfind(')')
                .map(|i| s[i + 1..].trim_start().starts_with(['Z', 'X']))
        })
        .unwrap_or(false)
}

/// Is `pid` the live wrapper of `job`? Its command line names the job, which
/// a zombie's (empty) does not, and no other process's can: the daemon reaps
/// its wrappers (theseus-z4b), so a wrapper's pid may be reused once it has
/// gone.
pub fn wrapper_alive(pid: u32, job: &str) -> bool {
    wrapper_job(pid).as_deref() == Some(job)
}

/// Terminate a job's wrapper and its whole process group: SIGTERM, wait up
/// to `grace`, then SIGKILL. Returns true if the wrapper is gone afterwards.
/// A group is signalled only while `pid` is still this job's wrapper, or no
/// live process at all: while any process of the group lives, its id is not
/// handed out again, but once the wrapper has been reaped and the group is
/// empty, `pid` may be another process's, which is left alone.
pub fn terminate(pid: u32, job: &str, grace: Duration) -> bool {
    #[cfg(unix)]
    {
        if pid_alive(pid) && !wrapper_alive(pid, job) {
            return true;
        }
        unsafe {
            libc::kill(-(pid as i32), libc::SIGTERM);
        }
        let t0 = Instant::now();
        while t0.elapsed() < grace {
            if !wrapper_alive(pid, job) {
                return true;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        unsafe {
            libc::kill(-(pid as i32), libc::SIGKILL);
        }
        let t0 = Instant::now();
        while t0.elapsed() < Duration::from_millis(500) {
            if !wrapper_alive(pid, job) {
                return true;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        !wrapper_alive(pid, job)
    }
    #[cfg(not(unix))]
    {
        let _ = (pid, job, grace);
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
        child.wait().unwrap();
        assert!(pid_alive(std::process::id()));
    }
}
