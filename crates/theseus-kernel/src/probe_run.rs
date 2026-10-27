//! A watch's command (theseus-f2r2b): argv run in L1, over the view a job
//! gets, from the calling thread, with no job record and no frame: the
//! daemon's blocking pool runs it, and only news is written afterwards
//! (`watch.rs`). It is `job_l1::self_test`'s path with a real command and its
//! output kept (capped), so it starts as a job starts and ends as one ends:
//! the whole job stopped at the deadline, nothing left behind. L1 refuses a
//! root daemon's job that has no cgroup (theseus-pv6i), and says so here as
//! it does for a job. Nothing falls back to L0.

use std::io::Read;
use std::os::fd::OwnedFd;
use std::path::PathBuf;
use std::time::Instant;

use serde_json::json;

use crate::job::{Reap, WrapperArgs, L1};
use crate::job_l1;
use crate::patience::Patience;
use crate::types::Outcome;

/// What a probe's command did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProbeRun {
    /// Its exit status; none when a signal ended it or it did not start.
    pub code: Option<i32>,
    pub stdout: String,
    pub stderr: String,
    /// It could not start, or its deadline stopped it: the reason, in words.
    pub failed: Option<String>,
}

/// Run `argv` in L1 for at most `deadline_ms`, keeping at most `max_bytes` of
/// each stream (the head). The environment is a bare `PATH` and the daemon's
/// `HOME`: nothing the daemon holds reaches the command.
pub fn run(l1: &L1, argv: &[String], deadline_ms: u64, max_bytes: usize) -> ProbeRun {
    let cwd = std::env::var("HOME")
        .ok()
        .map(PathBuf::from)
        .unwrap_or_else(|| "/".into());
    run_in(l1, argv, (&cwd, &[]), "watch", deadline_ms, max_bytes)
}

/// `run` in a directory of the caller's, with variables of its own beside
/// `PATH` and `HOME` (a whole-turn replay's program, theseus-pw1q.20): `id`
/// names the job in L1's own notes.
pub fn run_in(
    l1: &L1,
    argv: &[String],
    (cwd, extra): (&std::path::Path, &[(String, String)]),
    id: &str,
    deadline_ms: u64,
    max_bytes: usize,
) -> ProbeRun {
    let mut env = vec![(
        "PATH".to_string(),
        "/usr/local/bin:/usr/bin:/bin".to_string(),
    )];
    env.extend(std::env::var("HOME").ok().map(|h| ("HOME".to_string(), h)));
    env.extend(extra.iter().cloned());
    let args = WrapperArgs {
        spool_dir: PathBuf::new(),
        correlation_id: id.into(),
        deadline_ms,
        notify_socket: None,
        argv: argv.to_vec(),
        cwd: Some(cwd.to_path_buf()),
        env,
        umask: None,
        redact: vec![],
        output_max_bytes: 0,
        sandbox: None,
        cgroup: None,
    };
    let (out, err) = match (pipe(), pipe()) {
        (Ok(o), Ok(e)) => (o, e),
        (Err(e), _) | (_, Err(e)) => {
            return ProbeRun {
                code: None,
                stdout: String::new(),
                stderr: String::new(),
                failed: Some(format!("could not open the command's output: {e}")),
            }
        }
    };
    // Each stream is read as it is written and only its head kept, so a
    // command that prints without end costs a deadline and no memory.
    let readers = [out.0, err.0].map(|r| std::thread::spawn(move || head(r, max_bytes)));
    let mut detail = json!({});
    let mut patience = Patience::clock(deadline_ms, Instant::now());
    let (outcome, note) = job_l1::run(
        &args,
        l1,
        Reap::Command,
        (out.1, err.1),
        &mut patience,
        &mut detail,
    );
    let [stdout, stderr] = readers.map(|h| h.join().unwrap_or_default());
    let code = detail["exit_code"].as_i64().map(|c| c as i32);
    let failed = match (&outcome, code) {
        // A non-zero exit is the command's answer, not a failure to run it.
        (Outcome::Succeeded, _) | (_, Some(_)) if detail.pointer("/sandbox/error").is_none() => {
            None
        }
        _ => Some(note),
    };
    ProbeRun {
        code,
        stdout,
        stderr,
        failed,
    }
}

/// A pipe: the read end as a file, the write end for the child.
fn pipe() -> std::io::Result<(std::fs::File, OwnedFd)> {
    use std::os::fd::FromRawFd;
    let mut fds = [0; 2];
    if unsafe { libc::pipe2(fds.as_mut_ptr(), libc::O_CLOEXEC) } != 0 {
        return Err(std::io::Error::last_os_error());
    }
    // SAFETY: both descriptors are fresh and owned by nothing else.
    Ok(unsafe {
        (
            std::fs::File::from_raw_fd(fds[0]),
            OwnedFd::from_raw_fd(fds[1]),
        )
    })
}

/// The first `max` bytes a stream carries, as text; the rest is read and
/// dropped until the writer closes.
fn head(mut f: std::fs::File, max: usize) -> String {
    let mut buf = Vec::new();
    let mut chunk = [0u8; 8192];
    loop {
        match f.read(&mut chunk) {
            Ok(0) | Err(_) => break,
            Ok(n) => {
                let room = max.saturating_sub(buf.len());
                buf.extend_from_slice(&chunk[..n.min(room)]);
            }
        }
    }
    String::from_utf8_lossy(&buf).into_owned()
}

/// What a command run to a log did (`run_logged`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Logged {
    /// Its exit status; none when a signal ended it or it did not start.
    pub code: Option<i32>,
    /// Its deadline stopped it, with the whole job.
    pub timed_out: bool,
    /// It could not start in L1, or could not be waited for: the reason.
    pub failed: Option<String>,
    /// Wall time at which it passed its limit still printing, and was let
    /// run on (theseus-x1zz8).
    pub ran_past_limit_ms: Option<u64>,
    /// How long it had printed nothing when its limit stopped it; none for a
    /// job stopped at the ceiling, still printing.
    pub silent_ms: Option<u64>,
}

/// Run `argv` in L1 with `env` as its whole environment, its output on
/// `log`, for at most `deadline_ms` (a self build's gate, theseus-pw1q.21.1:
/// hours long, so its output is a file, not a head kept in memory). The same
/// path as `run_in`: from the calling thread, no job record, and the whole
/// job stopped at the deadline. Nothing falls back to L0.
pub fn run_logged(
    l1: &L1,
    argv: &[String],
    cwd: &std::path::Path,
    env: Vec<(String, String)>,
    log: std::fs::File,
    deadline_ms: u64,
) -> Logged {
    let args = WrapperArgs {
        spool_dir: PathBuf::new(),
        correlation_id: "self-build".into(),
        deadline_ms,
        notify_socket: None,
        argv: argv.to_vec(),
        cwd: Some(cwd.to_path_buf()),
        env,
        umask: None,
        redact: vec![],
        output_max_bytes: 0,
        sandbox: None,
        cgroup: None,
    };
    let err = match log.try_clone() {
        Ok(f) => f,
        Err(e) => {
            return Logged {
                code: None,
                timed_out: false,
                failed: Some(format!("could not open the command's output: {e}")),
                ran_past_limit_ms: None,
                silent_ms: None,
            }
        }
    };
    let mut detail = json!({});
    // The log's length is the job's sign of life: stopped when it has grown
    // by nothing for the limit, with the clock's ceiling past it
    // (theseus-x1zz8).
    let sized = match log.try_clone() {
        Ok(f) => f,
        Err(e) => {
            return Logged {
                code: None,
                timed_out: false,
                failed: Some(format!("could not open the command's output: {e}")),
                ran_past_limit_ms: None,
                silent_ms: None,
            }
        }
    };
    let mut patience = Patience::silence(deadline_ms, Instant::now(), move || {
        sized.metadata().map_or(0, |m| m.len())
    });
    let (outcome, note) = job_l1::run(
        &args,
        l1,
        Reap::Command,
        (log.into(), err.into()),
        &mut patience,
        &mut detail,
    );
    patience.note(&mut detail);
    let code = detail["exit_code"].as_i64().map(|c| c as i32);
    let timed_out = detail["timed_out"].as_bool().unwrap_or(false);
    let failed = match (&outcome, code) {
        (Outcome::Succeeded, _) => None,
        (_, Some(_)) if !timed_out && detail.pointer("/sandbox/error").is_none() => None,
        _ => Some(note),
    };
    Logged {
        code,
        timed_out,
        failed,
        ran_past_limit_ms: detail
            .pointer("/ran_past_limit_ms/noticed_at_ms")
            .and_then(|v| v.as_u64()),
        silent_ms: detail["silent_ms"].as_u64(),
    }
}
