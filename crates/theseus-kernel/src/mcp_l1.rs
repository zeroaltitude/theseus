//! An MCP server in L1 (M7 step 43a; design §2.1, §2.7): the `mcp-sandbox`
//! role of the daemon's own binary, which the MCP board spawns as an
//! ordinary stdio child (`children::spawn`, its own process group) and
//! which holds the server's init.
//!
//! ```text
//! theseusd                 the board: the role's stdin and stdout are its pipes
//! └─ theseusd mcp-sandbox  this role: hands those pipes to the init, then
//!    │                     waits; its stderr is the server's log
//!    └─ init               `theseusd job-sandbox`, pid 1 of the server
//!       └─ the server      over the view a job gets: the workspace's
//! ```
//!
//! The server's stdin and stdout are the board's pipes themselves, so its
//! words never pass through this process, and once the init has them this
//! process keeps no end of either: a daemon that closes them ends the server.
//! The rest is a job's L1, as `job_l1` builds it: no network unless its list
//! asks (`job_egress`, the same proxy on this process's threads), the view a
//! job gets (the workspace read-only under overlays whose writes are
//! scratch, `ro_paths`, the hidden floor), and the same limits.
//!
//! It ends when:
//! - the server ends (its init's pidfd), with the server's status;
//! - it is asked to stop (SIGTERM, SIGINT, or SIGHUP; the board's stop
//!   sends SIGTERM to the group): SIGTERM to the init, which forwards it to
//!   the server, up to `STOP_GRACE`, then SIGKILL to the init, which ends its
//!   whole pid namespace;
//! - the daemon goes, a `kill -9` included (its pidfd): SIGKILL to the init
//!   at once.
//!
//! And should this process itself be killed, the init's parent-death
//! signal kills the init, and with it the server's namespace.

use std::os::fd::{AsRawFd, BorrowedFd, FromRawFd, OwnedFd};
use std::path::PathBuf;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use serde_json::json;
use theseus_sandbox::{Exit, Init, SandboxChild, Spec, Stdio};

use crate::job::L1;
use crate::job_egress as egress;

/// The role word: `theseusd mcp-sandbox`.
pub const ROLE: &str = "mcp-sandbox";
/// The variable that carries the server's [`Server`], as JSON. It is the
/// role's alone: the server's environment is `Server::env`, nothing else.
pub const SPEC_ENV: &str = "THESEUS_MCP_L1";
/// How long a stop waits for the server to end on SIGTERM.
pub const STOP_GRACE: Duration = Duration::from_secs(2);
/// The first words of the line a failed start writes to the server's log.
pub const NOT_STARTED: &str = "the MCP server could not start in L1";

/// One MCP server to run in L1.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Server {
    /// The server's command, by its argv (no shell). `argv[0]` without a
    /// slash is found on its own `PATH` inside the view.
    pub argv: Vec<String>,
    /// Its whole environment.
    pub env: Vec<(String, String)>,
    /// Its working directory, inside the view.
    pub cwd: PathBuf,
    /// More read-only binds beside the view's `ro_paths`: an extension's
    /// frozen copy, which the view shows at its own path.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub binds: Vec<PathBuf>,
    /// The view a job gets, its limits, and the server's egress list.
    pub l1: L1,
    /// The operator's umask, which the server's files take.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub umask: Option<u32>,
}

impl std::fmt::Debug for Server {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // The environment's names alone: a value may be a secret.
        let names: Vec<&str> = self.env.iter().map(|(k, _)| k.as_str()).collect();
        f.debug_struct("Server")
            .field("argv", &self.argv)
            .field("env", &names)
            .field("cwd", &self.cwd)
            .field("binds", &self.binds)
            .field("l1", &self.l1)
            .finish()
    }
}

/// The sandbox's spec for `s`: the view a job gets, with `s`'s binds, and
/// the paths that do not exist left out (a missing one would fail every
/// start).
pub fn spec(s: &Server) -> Spec {
    let mut spec = Spec::new(s.argv.clone(), s.cwd.clone());
    spec.home = s
        .env
        .iter()
        .find(|(k, _)| k == "HOME")
        .map(|(_, v)| PathBuf::from(v))
        .filter(|h| h.is_absolute() && h.parent().is_some());
    spec.env = s.env.clone();
    spec.ro_paths =
        s.l1.ro_paths
            .iter()
            .chain(&s.binds)
            .filter(|p| p.exists())
            .cloned()
            .collect();
    spec.workspace =
        s.l1.workspace
            .iter()
            .filter(|p| p.is_dir())
            .cloned()
            .collect();
    spec.hidden = s.l1.hidden.clone();
    spec.limits = s.l1.limits;
    spec
}

/// The role's `main`: reads its [`Server`] from [`SPEC_ENV`], runs it in
/// L1, and returns the exit status the process should end with. A server
/// that cannot start says why on stderr (the server's log) and returns 125.
pub fn role_main() -> i32 {
    let s = match std::env::var(SPEC_ENV)
        .map_err(|e| e.to_string())
        .and_then(|raw| serde_json::from_str::<Server>(&raw).map_err(|e| e.to_string()))
    {
        Ok(s) => s,
        Err(e) => {
            eprintln!("{NOT_STARTED}: its spec ({SPEC_ENV}) did not read: {e}");
            return 125;
        }
    };
    match run(&s) {
        Ok(code) => code,
        Err(e) => {
            eprintln!("{NOT_STARTED}: {e}. It did not run, in L1 or at L0");
            125
        }
    }
}

/// Why the role stopped waiting.
enum Woke {
    Exited,
    Asked,
    DaemonGone,
}

/// Runs `s` until it ends, is asked to stop, or the daemon goes. Its exit
/// status; or why it never started.
fn run(s: &Server) -> Result<i32, String> {
    // The daemon, watched by its pidfd: a parent gone before this read
    // leaves a new parent, which is not watched.
    let parent_pid = unsafe { libc::getppid() };
    let parent = crate::job_wait::pidfd(parent_pid as u32)
        .ok_or_else(|| "this kernel has no pidfd for the daemon".to_string())?;
    if unsafe { libc::getppid() } != parent_pid {
        return Err("the daemon had already gone".into());
    }
    let signals = stop_signals().map_err(|e| format!("catching the stop signals: {e}"))?;
    let mut spec = spec(s);
    let allow = egress::prepare(&mut spec, &s.l1);
    let dup = |fd: i32| -> std::io::Result<OwnedFd> {
        // SAFETY: a borrow of a standard descriptor this process holds.
        unsafe { BorrowedFd::borrow_raw(fd) }.try_clone_to_owned()
    };
    let stdio = Stdio {
        stdin: Some(dup(0).map_err(|e| format!("the server's stdin: {e}"))?),
        stdout: dup(1).map_err(|e| format!("the server's stdout: {e}"))?,
        stderr: dup(2).map_err(|e| format!("the server's stderr: {e}"))?,
    };
    let old = s.umask.map(|u| unsafe { libc::umask(u) });
    let spawned = theseus_sandbox::spawn(&spec, &Init::default(), stdio);
    if let Some(u) = old {
        unsafe { libc::umask(u) };
    }
    let mut child = spawned.map_err(|e| e.to_string())?;
    // The pipes are the server's alone now: when the daemon closes its ends,
    // the server reads the end of its input.
    drop_std_pipes();
    let mut detail = json!({});
    let proxy = egress::start(&mut child, &s.l1, &allow, &mut detail);
    let exit = match wait(&child, &parent, &signals) {
        Woke::Exited => child.wait(),
        Woke::Asked => stop(&mut child, STOP_GRACE),
        Woke::DaemonGone => stop(&mut child, Duration::ZERO),
    };
    egress::stop(proxy, &allow, &mut detail);
    if let Some(e) = detail.get("egress") {
        eprintln!("theseusd mcp-sandbox: egress {e}");
    }
    Ok(match exit {
        Ok(e) => status(&e),
        Err(e) => {
            eprintln!("theseusd mcp-sandbox: the server's end is unknown: {e}");
            125
        }
    })
}

/// SIGTERM, SIGINT, and SIGHUP, blocked and read from a signalfd, so a stop
/// asked at any moment is seen by the next wait.
fn stop_signals() -> std::io::Result<OwnedFd> {
    // SAFETY: a signal set built and passed by pointer, for this one thread.
    unsafe {
        let mut set: libc::sigset_t = std::mem::zeroed();
        libc::sigemptyset(&mut set);
        for sig in [libc::SIGTERM, libc::SIGINT, libc::SIGHUP] {
            libc::sigaddset(&mut set, sig);
        }
        if libc::pthread_sigmask(libc::SIG_BLOCK, &set, std::ptr::null_mut()) != 0 {
            return Err(std::io::Error::last_os_error());
        }
        let fd = libc::signalfd(-1, &set, libc::SFD_CLOEXEC);
        if fd == -1 {
            return Err(std::io::Error::last_os_error());
        }
        Ok(OwnedFd::from_raw_fd(fd))
    }
}

/// This process's stdin and stdout become `/dev/null`: it keeps no end of
/// the server's pipes.
fn drop_std_pipes() {
    // SAFETY: open, dup2 onto 0 and 1, and close, on descriptors this
    // process owns.
    unsafe {
        let null = libc::open(c"/dev/null".as_ptr(), libc::O_RDWR | libc::O_CLOEXEC);
        if null >= 0 {
            libc::dup2(null, 0);
            libc::dup2(null, 1);
            libc::close(null);
        }
    }
}

/// Asleep until the server's init exits, a stop signal comes, or the daemon
/// goes.
fn wait(child: &SandboxChild, parent: &OwnedFd, signals: &OwnedFd) -> Woke {
    loop {
        let mut fds = [
            child.pidfd().as_raw_fd(),
            parent.as_raw_fd(),
            signals.as_raw_fd(),
        ]
        .map(|fd| libc::pollfd {
            fd,
            events: libc::POLLIN,
            revents: 0,
        });
        // SAFETY: poll over the three descriptors above, alive for the call.
        let n = unsafe { libc::poll(fds.as_mut_ptr(), fds.len() as libc::nfds_t, -1) };
        if n == -1 {
            if std::io::Error::last_os_error().kind() == std::io::ErrorKind::Interrupted {
                continue;
            }
            // Nothing can be watched: stop the server rather than leave it.
            return Woke::DaemonGone;
        }
        if fds[0].revents != 0 {
            return Woke::Exited;
        }
        if fds[1].revents != 0 {
            return Woke::DaemonGone;
        }
        if fds[2].revents != 0 {
            return Woke::Asked;
        }
    }
}

/// Stop the server: SIGTERM to its init, which forwards it, and up to
/// `grace` for it to end; then SIGKILL to the init, whose end is its whole
/// namespace's.
fn stop(child: &mut SandboxChild, grace: Duration) -> std::io::Result<Exit> {
    if !grace.is_zero() {
        let _ = child.terminate();
        let until = Instant::now() + grace;
        loop {
            if let Some(e) = child.try_wait()? {
                return Ok(e);
            }
            let left = until.saturating_duration_since(Instant::now());
            if left.is_zero() {
                break;
            }
            let mut pfd = libc::pollfd {
                fd: child.pidfd().as_raw_fd(),
                events: libc::POLLIN,
                revents: 0,
            };
            let ms = left.as_millis().clamp(1, i32::MAX as u128) as libc::c_int;
            // SAFETY: poll over one descriptor, alive for the call.
            unsafe { libc::poll(&mut pfd, 1, ms) };
        }
    }
    let _ = child.kill();
    child.wait()
}

/// A process's exit status for the server's end: its code, or 128 and its
/// signal, as a shell says it.
fn status(e: &Exit) -> i32 {
    match (e.code, e.signal.or(e.init_signal)) {
        (Some(c), _) => c,
        (None, Some(s)) => 128 + s,
        (None, None) => 125,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn server() -> Server {
        Server {
            argv: vec!["wc-server".into(), "--stdio".into()],
            env: vec![
                ("HOME".into(), "/home/someone".into()),
                ("TOKEN".into(), "planted-value-0042".into()),
            ],
            cwd: "/srv/frozen".into(),
            binds: vec!["/srv/frozen".into(), "/nonexistent/frozen".into()],
            l1: L1 {
                workspace: vec!["/nonexistent/projects".into(), "/".into()],
                ro_paths: vec!["/usr".into()],
                hidden: vec!["/srv/state/store".into()],
                ..Default::default()
            },
            umask: Some(0o022),
        }
    }

    /// The spec carries the server's argv, cwd, environment, and HOME; the
    /// binds join the view's `ro_paths`, and a path that does not exist is
    /// left out; the hidden paths and the limits are the view's.
    #[test]
    fn the_spec_is_the_views_with_the_servers_binds() {
        let s = server();
        let spec = spec(&s);
        assert_eq!(spec.argv, s.argv);
        assert_eq!(spec.cwd, PathBuf::from("/srv/frozen"));
        assert_eq!(spec.home, Some(PathBuf::from("/home/someone")));
        assert_eq!(spec.env, s.env);
        // `/srv/frozen` does not exist on the test's host either.
        assert_eq!(spec.ro_paths, [PathBuf::from("/usr")]);
        assert_eq!(spec.workspace, [PathBuf::from("/")]);
        assert_eq!(spec.hidden, s.l1.hidden);
        assert_eq!(spec.limits, s.l1.limits);
        assert_eq!(spec.egress_port, None, "no list, no listener");
    }

    /// It rides in one variable as JSON and reads back whole; its debug
    /// form names the environment's variables, never their values.
    #[test]
    fn it_round_trips_and_its_debug_form_keeps_no_value() {
        let s = server();
        let back: Server = serde_json::from_str(&serde_json::to_string(&s).unwrap()).unwrap();
        assert_eq!(back, s);
        let d = format!("{s:?}");
        assert!(
            d.contains("TOKEN") && !d.contains("planted-value-0042"),
            "{d}"
        );
    }

    #[test]
    fn its_status_is_the_servers() {
        let e = |code, signal| Exit {
            code,
            signal,
            init_signal: None,
            output_capped: false,
            scratch: None,
        };
        assert_eq!(status(&e(Some(3), None)), 3);
        assert_eq!(status(&e(None, Some(15))), 143);
        assert_eq!(status(&e(None, None)), 125);
    }
}
