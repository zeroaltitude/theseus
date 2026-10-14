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

use serde_json::json;

use crate::job::{Reap, WrapperArgs, L1};
use crate::job_l1;
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
    let mut env = vec![(
        "PATH".to_string(),
        "/usr/local/bin:/usr/bin:/bin".to_string(),
    )];
    env.extend(std::env::var("HOME").ok().map(|h| ("HOME".to_string(), h)));
    let args = WrapperArgs {
        spool_dir: PathBuf::new(),
        correlation_id: "watch".into(),
        deadline_ms,
        notify_socket: None,
        argv: argv.to_vec(),
        cwd: std::env::var("HOME")
            .ok()
            .map(PathBuf::from)
            .or(Some("/".into())),
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
    let (outcome, note) = job_l1::run(&args, l1, Reap::Command, out.1, err.1, &mut detail);
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
