//! The owner's toolchain, from a login shell (theseus-v4cj).
//!
//! A systemd user service never sources the shell's rc files, and `proc.run`
//! runs a line as `bash -c`, so `VIRTUAL_ENV`, `NVM_DIR` and `JAVA_HOME` reach
//! no job from the daemon's own environment. After the socket answers, the
//! daemon runs `$SHELL -lc env` once, under a deadline, and keeps what it
//! prints. The names go through `resolve_proc_env`, the same allowlist and the
//! same secret rule as the configured ones; a refused name is warned about by
//! name and never by value. The shell is a child of the serving daemon, so it
//! goes through `theseus_kernel::children::spawn` before anything waits for
//! it (`run_shell`); unregistered, the daemon's SIGCHLD sweep could reap it as
//! an orphan and the capture's own wait would fail `ECHILD`, which, because
//! the capture runs once, would cost that daemon the toolchain for its whole
//! life. The result joins only the environment `proc.run`
//! builds (`ToolRuntime::job_base_env`), and the daemon's own value of a name
//! wins over the shell's — **except `PATH`, which is merged rather than
//! chosen between** (`super::job_path`, at parity with OpenClaw): the
//! operator's `[tools] path_prepend` first, then the daemon's entries, then
//! the shell's that neither holds. The daemon's own environment is never
//! changed by any of it.

use std::process::Stdio;
use std::sync::OnceLock;
use std::time::Duration;

use super::{forbidden_env, resolve_proc_env};

/// How long the shell has: a login shell can wait on a prompt or a mount.
pub(crate) const DEADLINE: Duration = Duration::from_secs(2);

/// What the capture found, once it has run.
#[derive(Default)]
pub struct LoginEnv {
    /// `[tools] proc_env`: the names a captured variable must be listed in.
    names: Vec<String>,
    got: OnceLock<Vec<(String, String)>>,
}

impl LoginEnv {
    pub fn new(names: Vec<String>) -> Self {
        Self {
            names,
            got: OnceLock::new(),
        }
    }

    /// The kept variables; empty until the capture has run, and after one
    /// that failed or timed out.
    pub fn kept(&self) -> &[(String, String)] {
        self.got.get().map_or(&[], Vec::as_slice)
    }

    /// Whether the capture has run (it runs once).
    pub fn done(&self) -> bool {
        self.got.get().is_some()
    }

    /// Run `shell -lc env` under `deadline` and keep its allowlisted,
    /// secret-free names. Once: a second call changes nothing. Returns the
    /// names kept. Nothing here logs a value.
    pub async fn capture(&self, shell: Option<&str>, deadline: Duration) -> Vec<String> {
        if self.done() {
            return self.kept().iter().map(|(k, _)| k.clone()).collect();
        }
        let out = match pick_shell(shell, is_executable) {
            Some(sh) => run_shell(&sh, deadline).await,
            None => Err("no executable login shell".to_string()),
        };
        let kept = match out {
            Ok(text) => {
                let captured = parse_env(&text);
                let (kept, _) = resolve_proc_env(&self.names, |k| {
                    captured
                        .iter()
                        .find(|(n, _)| n == k)
                        .map(|(_, v)| v.clone())
                });
                // Every name the shell printed that the secret rule refuses
                // is said, listed or not: names only, never a value.
                let refused: Vec<&str> = captured
                    .iter()
                    .map(|(n, _)| n.as_str())
                    .filter(|n| forbidden_env(n))
                    .collect();
                if !refused.is_empty() {
                    tracing::warn!(
                        names = %refused.join(", "),
                        "the login shell's environment holds these, and no job sees them: their names look like secrets"
                    );
                }
                kept
            }
            Err(why) => {
                tracing::warn!(%why, "login-shell environment not captured; jobs keep the daemon's own");
                Vec::new()
            }
        };
        let names = kept.iter().map(|(k, _)| k.clone()).collect();
        let _ = self.got.set(kept);
        names
    }
}

/// The shell to run: `$SHELL` when it is an executable file, else bash, else sh.
pub(crate) fn pick_shell(shell: Option<&str>, is_exec: impl Fn(&str) -> bool) -> Option<String> {
    shell
        .into_iter()
        .chain(["/bin/bash", "/bin/sh"])
        .find(|p| !p.is_empty() && is_exec(p))
        .map(str::to_string)
}

fn is_executable(path: &str) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(path)
        .map(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
        .unwrap_or(false)
}

async fn run_shell(shell: &str, deadline: Duration) -> Result<String, String> {
    use theseus_kernel::children::{self, Kind};
    let mut cmd = tokio::process::Command::new(shell);
    cmd.args(["-l", "-c", "env"])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true);
    // Registered before anything waits for it, as `op` is
    // (`Secrets::start`): this runs inside the serving daemon, whose SIGCHLD
    // sweep reaps any zombie child it does not know as an orphan
    // (`theseus_kernel::children::sweep`), and the shell's own exit is what
    // raises that SIGCHLD. Unregistered, the sweep and tokio's process
    // driver race for the same exit status at every capture, and the loser's
    // `wait()` is ECHILD. So: register, then wait.
    let child = match children::spawn(Kind::Owned, || cmd.spawn(), tokio::process::Child::id) {
        Ok(c) => c,
        Err(e) => return Err(format!("the login shell did not start: {e}")),
    };
    // Dropping the timed-out future drops the child, and `kill_on_drop`
    // kills it; the wait is inside the timeout, so the registration is not.
    match tokio::time::timeout(deadline, child.wait_with_output()).await {
        Err(_) => Err(format!("the login shell took over {deadline:?}")),
        Ok(Err(e)) => Err(format!("the login shell could not be waited for: {e}")),
        Ok(Ok(o)) if !o.status.success() => Err(format!("the login shell exited {}", o.status)),
        Ok(Ok(o)) => Ok(String::from_utf8_lossy(&o.stdout).into_owned()),
    }
}

/// `NAME=value` lines; anything else (an rc file's greeting, a continuation
/// of a multi-line value) is skipped. A later line wins.
pub(crate) fn parse_env(text: &str) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = Vec::new();
    for line in text.lines() {
        let Some((k, v)) = line.split_once('=') else {
            continue;
        };
        let ok = !k.is_empty()
            && !k.starts_with(|c: char| c.is_ascii_digit())
            && k.chars().all(|c| c.is_ascii_alphanumeric() || c == '_');
        if !ok {
            continue;
        }
        out.retain(|(n, _)| n != k);
        out.push((k.to_string(), v.to_string()));
    }
    out
}

impl crate::Core {
    /// After serving: the login shell's environment, captured once in the
    /// background. Never on the start path; a job started before it ends has
    /// the daemon's own environment alone.
    pub fn login_env_after_serving(self: &std::sync::Arc<Self>) {
        let core = self.clone();
        tokio::spawn(async move {
            let shell = std::env::var("SHELL").ok();
            let kept = core
                .tools
                .login_env
                .capture(shell.as_deref(), DEADLINE)
                .await;
            tracing::info!(names = %kept.join(", "), "login-shell environment captured");
        });
    }
}
