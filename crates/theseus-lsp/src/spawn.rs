//! A plain spawn, for tests and the probe only. The daemon never uses it: L2
//! starts each server through `theseus_kernel::children::spawn`, so the
//! daemon's registry knows the child and its sweep never reaps it as an
//! orphan, and builds the [`Server`] itself.
//!
//! The server runs in a process group of its own, so a kill reaches what it
//! starts (pyright's and typescript-language-server's node children). The
//! kill does nothing once the server has been reaped, so a reused pid is
//! never signalled.

use std::path::Path;
use std::process::{ExitStatus, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use tokio::sync::watch;

use crate::client::Server;

/// A spawned server's process, beside the [`Server`] the client takes.
pub struct Process {
    pub pid: u32,
    /// Its exit status, once it has exited.
    pub status: watch::Receiver<Option<ExitStatus>>,
}

/// Start `argv` in `cwd` with its stdin and stdout piped, its stderr to
/// `stderr` (a log file, or null), and `env` added to this process's
/// environment.
pub fn spawn(
    argv: &[String],
    cwd: &Path,
    env: &[(String, String)],
    stderr: Stdio,
) -> std::io::Result<(Server, Process)> {
    let (program, args) = argv
        .split_first()
        .ok_or_else(|| std::io::Error::other("an empty command"))?;
    let mut cmd = tokio::process::Command::new(program);
    cmd.args(args)
        .current_dir(cwd)
        .envs(env.iter().map(|(k, v)| (k, v)))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(stderr)
        .process_group(0);
    let mut child = cmd.spawn()?;
    let pid = child
        .id()
        .ok_or_else(|| std::io::Error::other("the server exited at once"))?;
    let (Some(stdin), Some(stdout)) = (child.stdin.take(), child.stdout.take()) else {
        return Err(std::io::Error::other("the server's pipes are missing"));
    };
    let exited = Arc::new(AtomicBool::new(false));
    let (tx, status) = watch::channel(None);
    let flag = exited.clone();
    tokio::spawn(async move {
        let s = child.wait().await.ok();
        flag.store(true, Ordering::SeqCst);
        let _ = tx.send(s);
    });
    let kill = move || {
        if exited.load(Ordering::SeqCst) {
            return;
        }
        let Ok(pgid) = i32::try_from(pid) else { return };
        // SAFETY: kill(2) only sends a signal; it touches no memory of ours.
        // The negative pid names the group `process_group(0)` made, whose
        // leader has not been reaped (checked above), so it is still ours.
        unsafe {
            libc::kill(-pgid, libc::SIGKILL);
        }
    };
    let server = Server {
        reader: Box::new(stdout),
        writer: Box::new(stdin),
        kill: Some(Box::new(kill)),
        pid: Some(pid),
    };
    Ok((server, Process { pid, status }))
}

/// The resident memory of a process group, in KiB: every process whose
/// group is `pgid`, from `/proc`. For the probe's memory column.
pub fn group_rss_kib(pgid: u32) -> u64 {
    let Ok(dir) = std::fs::read_dir("/proc") else {
        return 0;
    };
    dir.filter_map(Result::ok)
        .filter_map(|e| e.file_name().to_str()?.parse::<u32>().ok())
        .filter_map(|pid| {
            let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
            // The fields after the command's closing paren: state, ppid, pgrp.
            let rest = &stat[stat.rfind(')')? + 2..];
            let pgrp: u32 = rest.split_whitespace().nth(2)?.parse().ok()?;
            if pgrp != pgid {
                return None;
            }
            let status = std::fs::read_to_string(format!("/proc/{pid}/status")).ok()?;
            status
                .lines()
                .find_map(|l| l.strip_prefix("VmRSS:"))
                .and_then(|v| v.split_whitespace().next()?.parse::<u64>().ok())
        })
        .sum()
}
