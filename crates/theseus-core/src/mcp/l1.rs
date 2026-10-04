//! A stdio server in L1 (M7 43a): the command the board spawns for it, the
//! daemon's own binary in its `mcp-sandbox` role (`theseus_kernel::mcp_l1`),
//! which holds the server's init. To the board it is an ordinary stdio
//! child: its stdin and stdout are the server's own pipes, its stderr the
//! server's log, and a stop's SIGTERM to its group stops the server's whole
//! namespace.
//!
//! The server gets the view a job gets (`Sandbox::job_view`: the workspace
//! read-only under overlays, `ro_paths`, the floor hidden), its own egress
//! list, and nothing else; an extension's frozen copy is bound read-only at
//! its own path and is its working directory. It never falls back to L0: a
//! daemon that cannot start L1 (one that runs as root) fails the start, with
//! the reason.

use std::path::PathBuf;
use std::sync::Arc;

use theseus_kernel::mcp_l1::{Server, ROLE, SPEC_ENV};

use crate::config::McpServerConfig;

/// What the board needs to start a server in L1.
#[derive(Clone)]
pub struct L1Spawn {
    /// The daemon's own binary: `/proc/self/exe`, as a job wrapper's.
    pub exe: PathBuf,
    /// L1's state: the view a job gets, and whether this daemon refuses L1.
    pub sandbox: Arc<crate::sandbox::Sandbox>,
    /// The operator's umask.
    pub umask: Option<u32>,
}

/// The role's command for `cfg`'s server, run with `env` as its whole
/// environment, in `cwd` (an extension's frozen copy, when it has one);
/// or why it cannot start in L1.
pub fn command(
    l1: &L1Spawn,
    cfg: &McpServerConfig,
    env: Vec<(String, String)>,
    cwd: PathBuf,
) -> Result<tokio::process::Command, String> {
    if let Some(why) = theseus_sandbox::refused_here() {
        return Err(format!("it cannot start in L1: {why}"));
    }
    let mut view = l1.sandbox.job_view();
    view.egress.clone_from(&cfg.egress);
    let server = Server {
        argv: cfg.command.clone(),
        env,
        cwd,
        binds: cfg.frozen.iter().cloned().collect(),
        l1: view,
        umask: l1.umask,
    };
    let spec = serde_json::to_string(&server).map_err(|e| format!("its L1 spec: {e}"))?;
    let mut cmd = tokio::process::Command::new(&l1.exe);
    cmd.arg(ROLE)
        .env_clear()
        .env(SPEC_ENV, spec)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .process_group(0);
    Ok(cmd)
}
