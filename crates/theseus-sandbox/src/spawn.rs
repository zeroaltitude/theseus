//! The wrapper's side of L1 (design §2.2): clone the init into its
//! namespaces, write user namespace 1's maps, release it, hand it the spec,
//! and wait for its first word.
//!
//! The wrapper may have threads (18b's proxy), so between the clone and the
//! init's exec the child makes raw system calls only, on memory prepared
//! before the clone. The init execs at once and builds everything else.

use std::ffi::CString;
use std::fs::File;
use std::io::{self, BufRead, BufReader, Read, Write};
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd, RawFd};
use std::time::Duration;

use crate::report::{Exit, Report, Started};
use crate::spec::{Init, Spec};
use crate::sys;

/// The init's descriptors: the spec it reads, the report it writes, and
/// (18b) the socket that carries the egress listener back out.
pub(crate) const SPEC_FD: RawFd = 3;
pub(crate) const REPORT_FD: RawFd = 4;
pub(crate) const EGRESS_FD: RawFd = 5;

/// How long the init may take to build the view and start the command.
const SETUP_TIMEOUT: Duration = Duration::from_secs(10);

/// The namespaces an L1 job gets: user, pid, mount, network, uts, ipc, and
/// cgroup.
const NAMESPACES: libc::c_int = libc::CLONE_NEWUSER
    | libc::CLONE_NEWPID
    | libc::CLONE_NEWNS
    | libc::CLONE_NEWNET
    | libc::CLONE_NEWUTS
    | libc::CLONE_NEWIPC
    | libc::CLONE_NEWCGROUP;

/// The job's standard streams. Without stdin it reads `/dev/null`.
pub struct Stdio {
    pub stdin: Option<OwnedFd>,
    pub stdout: OwnedFd,
    pub stderr: OwnedFd,
}

/// Why a job could not start in L1: the stage, and the error. Nothing of
/// the job ran.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpawnError {
    pub stage: String,
    pub error: String,
}

impl std::fmt::Display for SpawnError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.stage, self.error)
    }
}

impl std::error::Error for SpawnError {}

fn fail(stage: impl Into<String>, error: impl std::fmt::Display) -> SpawnError {
    SpawnError {
        stage: stage.into(),
        error: error.to_string(),
    }
}

/// Why L1 refuses every job of an operator who is root (theseus-pv6i).
pub const ROOT_REFUSED: &str = "the daemon runs as root, and Linux exempts root from \
     RLIMIT_NPROC, so an L1 job would have no process limit: run theseusd as an ordinary user";

/// Why a job of operator `uid` may not start in L1, or None. User namespace
/// 1 maps the job back to the operator (`write_maps`), and `copy_process`
/// never applies `RLIMIT_NPROC`, the job's one process limit, to the initial
/// namespace's root.
pub fn refusal(uid: libc::uid_t) -> Option<&'static str> {
    (uid == 0).then_some(ROOT_REFUSED)
}

/// `refusal` for this process, the operator of any job it starts.
pub fn refused_here() -> Option<&'static str> {
    refusal(unsafe { libc::geteuid() })
}

/// Starts `spec` in L1, with `init` as its pid 1. Returns once the command
/// has been exec'd, or with the reason it could not be.
#[expect(clippy::too_many_lines, reason = "shape budget: split it")]
pub fn spawn(spec: &Spec, init: &Init, stdio: Stdio) -> Result<SandboxChild, SpawnError> {
    if let Some(why) = spec.invalid() {
        return Err(fail("checking the job", why));
    }
    // Before anything is made: every L1 start comes through here.
    if let Some(why) = refused_here() {
        return Err(fail("checking the job's process limit", why));
    }
    let wire = serde_json::to_vec(spec).map_err(|e| fail("encoding the job", e))?;
    // Everything the child reads is built now.
    let exe = sys::cpath(&init.exe).map_err(|e| fail("the init's path", e))?;
    let mut args: Vec<CString> = vec![exe.clone()];
    for a in &init.args {
        args.push(sys::cbytes(a.as_bytes()).map_err(|e| fail("the init's arguments", e))?);
    }
    let argv: Vec<*const libc::c_char> = args
        .iter()
        .map(|a| a.as_ptr())
        .chain(std::iter::once(std::ptr::null()))
        .collect();
    let envp: [*const libc::c_char; 1] = [std::ptr::null()];

    let pipes =
        || -> io::Result<_> { Ok((sys::pipe()?, sys::pipe()?, sys::pipe()?, sys::pipe()?)) };
    let ((sync_r, sync_w), (spec_r, spec_w), (report_r, report_w), (err_r, err_w)) =
        pipes().map_err(|e| fail("making the init's pipes", e))?;
    let (egress_ours, egress_init) = match spec.egress_port {
        Some(_) => {
            let (a, b) = sys::socketpair().map_err(|e| fail("making the egress socket pair", e))?;
            (Some(a), Some(b))
        }
        None => (None, None),
    };
    let stdin = match stdio.stdin {
        Some(fd) => fd,
        None => File::open("/dev/null")
            .map_err(|e| fail("opening /dev/null", e))?
            .into(),
    };
    // Each moved to 10 or above, so that placing them at 0 to 5 overwrites
    // none of the others.
    let mut placed = vec![stdin, stdio.stdout, stdio.stderr, spec_r, report_w];
    placed.extend(egress_init);
    let high = placed
        .iter()
        .map(|fd| sys::dup_above(fd, 10))
        .collect::<io::Result<Vec<OwnedFd>>>()
        .map_err(|e| fail("placing the init's descriptors", e))?;
    drop(placed);
    let raw: Vec<RawFd> = high.iter().map(AsRawFd::as_raw_fd).collect();

    let mut pidfd: libc::c_int = -1;
    let flags = (NAMESPACES | libc::CLONE_PIDFD | libc::SIGCHLD) as libc::c_ulong;
    // Every signal is blocked across the clone, so no handler of this
    // process ever runs in the child; the init sets its own mask, and a
    // signal that arrived meanwhile reaches it then.
    let (mut all, mut old): (libc::sigset_t, libc::sigset_t) =
        unsafe { (std::mem::zeroed(), std::mem::zeroed()) };
    unsafe {
        libc::sigfillset(&mut all);
        libc::pthread_sigmask(libc::SIG_SETMASK, &all, &mut old);
    }
    // clone(2) with no new stack: the child runs on a copy of this one, as
    // after fork. The pidfd is written to `pidfd` in this process alone.
    let pid = unsafe {
        libc::syscall(
            libc::SYS_clone,
            flags,
            0usize,
            &mut pidfd as *mut libc::c_int,
            0usize,
            0usize,
        )
    };
    if pid != 0 {
        unsafe { libc::pthread_sigmask(libc::SIG_SETMASK, &old, std::ptr::null_mut()) };
    }
    if pid == 0 {
        unsafe {
            child(
                sync_r.as_raw_fd(),
                sync_w.as_raw_fd(),
                err_w.as_raw_fd(),
                &raw,
                exe.as_ptr(),
                argv.as_ptr(),
                envp.as_ptr(),
            )
        }
    }
    if pid == -1 {
        return Err(fail(
            "cloning the init into its namespaces",
            io::Error::last_os_error(),
        ));
    }
    let pid = pid as libc::pid_t;
    let mut guard = Guard {
        pid,
        pidfd: Some(unsafe { OwnedFd::from_raw_fd(pidfd) }),
    };
    // The child's ends.
    drop((sync_r, err_w, high));

    write_maps(pid).map_err(|e| fail("writing user namespace 1's maps", e))?;
    File::from(sync_w)
        .write_all(b"r")
        .map_err(|e| fail("releasing the init", e))?;
    // The error pipe closes at the init's exec, or carries its errno.
    let mut errno = [0u8; 4];
    let n = read_full(File::from(err_r), &mut errno).map_err(|e| fail("starting the init", e))?;
    if n == errno.len() {
        return Err(fail(
            format!("executing the init, {}", init.exe.display()),
            io::Error::from_raw_os_error(i32::from_ne_bytes(errno)),
        ));
    }
    // A failed write means the init has gone; its report says why.
    let _ = File::from(spec_w).write_all(&wire);
    let mut report = BufReader::new(File::from(report_r));
    let started = first_word(&mut report).map_err(|mut e| {
        if e.error == DIED {
            e.error = format!("{DIED} ({})", guard.reap());
        }
        e
    })?;
    let egress = match egress_ours {
        Some(sock) => Some(
            sys::recv_fd(sock.as_raw_fd()).map_err(|e| fail("receiving the egress listener", e))?,
        ),
        None => None,
    };
    Ok(SandboxChild {
        pid,
        pidfd: guard.disarm(),
        report,
        started,
        egress,
        exit: None,
    })
}

/// The child, between the clone and the init's exec. Raw system calls only:
/// another thread of the parent may hold a lock that it never releases here.
unsafe fn child(
    sync_r: RawFd,
    sync_w: RawFd,
    err_w: RawFd,
    fds: &[RawFd],
    exe: *const libc::c_char,
    argv: *const *const libc::c_char,
    envp: *const *const libc::c_char,
) -> ! {
    libc::close(sync_w);
    // The init dies with its wrapper, and so does the whole job.
    libc::prctl(
        libc::PR_SET_PDEATHSIG,
        libc::SIGKILL as libc::c_ulong,
        0,
        0,
        0,
    );
    // Wait to be released, once user namespace 1's maps are written.
    let mut byte = 0u8;
    loop {
        let n = libc::read(sync_r, (&mut byte as *mut u8).cast(), 1);
        if n == 1 {
            break;
        }
        if n == -1 && sys::errno() == libc::EINTR {
            continue;
        }
        // The wrapper gave up before the release.
        libc::_exit(126);
    }
    for (i, &fd) in fds.iter().enumerate() {
        if libc::dup2(fd, i as libc::c_int) == -1 {
            exec_failed(err_w);
        }
    }
    libc::execve(exe, argv, envp);
    exec_failed(err_w)
}

unsafe fn exec_failed(err_w: RawFd) -> ! {
    let e = sys::errno().to_ne_bytes();
    libc::write(err_w, e.as_ptr().cast(), e.len());
    libc::_exit(127)
}

/// User namespace 1: the init is root in it, and that root is the
/// operator's uid outside. `setgroups` is denied first, as an unprivileged
/// map requires.
fn write_maps(pid: libc::pid_t) -> io::Result<()> {
    let (uid, gid) = unsafe { (libc::geteuid(), libc::getegid()) };
    std::fs::write(format!("/proc/{pid}/setgroups"), "deny")?;
    std::fs::write(format!("/proc/{pid}/uid_map"), format!("0 {uid} 1\n"))?;
    std::fs::write(format!("/proc/{pid}/gid_map"), format!("0 {gid} 1\n"))?;
    Ok(())
}

fn read_full(mut f: File, buf: &mut [u8]) -> io::Result<usize> {
    let mut at = 0;
    while at < buf.len() {
        match f.read(&mut buf[at..]) {
            Ok(0) => break,
            Ok(n) => at += n,
            Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
            Err(e) => return Err(e),
        }
    }
    Ok(at)
}

/// The init's first word: started, or failed with its stage.
fn first_word(report: &mut BufReader<File>) -> Result<Started, SpawnError> {
    if report.buffer().is_empty() {
        let mut pfd = libc::pollfd {
            fd: report.get_ref().as_raw_fd(),
            events: libc::POLLIN,
            revents: 0,
        };
        let ms = SETUP_TIMEOUT.as_millis() as libc::c_int;
        loop {
            let r = unsafe { libc::poll(&mut pfd, 1, ms) };
            if r == -1 && sys::errno() == libc::EINTR {
                continue;
            }
            if r == 0 {
                return Err(fail(
                    "waiting for the init",
                    format!("no word within {} s", SETUP_TIMEOUT.as_secs()),
                ));
            }
            break;
        }
    }
    let mut line = String::new();
    report
        .read_line(&mut line)
        .map_err(|e| fail("reading the init's report", e))?;
    if line.is_empty() {
        return Err(fail("waiting for the init", DIED));
    }
    match serde_json::from_str::<Report>(&line) {
        Ok(Report::Started(s)) => Ok(s),
        Ok(Report::Failed { stage, error }) => Err(SpawnError { stage, error }),
        _ => Err(fail(
            "reading the init's report",
            format!("an unexpected line: {}", line.trim_end()),
        )),
    }
}

/// The init's report closed before its first word.
const DIED: &str = "it exited before it started the command";

/// Kills and reaps the init if the spawn fails after the clone.
struct Guard {
    pid: libc::pid_t,
    pidfd: Option<OwnedFd>,
}

impl Guard {
    fn disarm(mut self) -> OwnedFd {
        self.pidfd.take().expect("armed")
    }

    /// Reaps an init that has died: how it ended.
    fn reap(&mut self) -> String {
        let mut status = 0;
        if unsafe { libc::waitpid(self.pid, &mut status, 0) } == -1 {
            return format!("its status is unknown: {}", io::Error::last_os_error());
        }
        self.pidfd = None;
        if libc::WIFSIGNALED(status) {
            format!("killed by signal {}", libc::WTERMSIG(status))
        } else {
            format!("exit status {}", libc::WEXITSTATUS(status))
        }
    }
}

impl Drop for Guard {
    fn drop(&mut self) {
        if let Some(fd) = &self.pidfd {
            let _ = sys::pidfd_send_signal(fd.as_raw_fd(), libc::SIGKILL);
            let mut status = 0;
            unsafe { libc::waitpid(self.pid, &mut status, 0) };
        }
    }
}

/// A job running in L1: its init, as the wrapper sees it.
///
/// Dropped before it was waited for, it kills the job: no job outlives its
/// handle.
pub struct SandboxChild {
    pid: libc::pid_t,
    pidfd: OwnedFd,
    report: BufReader<File>,
    started: Started,
    egress: Option<OwnedFd>,
    exit: Option<Exit>,
}

impl SandboxChild {
    /// The init's pid, in the wrapper's pid namespace.
    pub fn id(&self) -> u32 {
        self.pid as u32
    }

    pub fn started(&self) -> &Started {
        &self.started
    }

    /// 18b: the listening socket the init opened on 127.0.0.1 inside the
    /// job's network namespace, for the wrapper's proxy to serve.
    pub fn take_egress_listener(&mut self) -> Option<OwnedFd> {
        self.egress.take()
    }

    /// A cancel: SIGKILL to the init. The kernel then kills everything in
    /// the job's pid namespace, so the whole tree goes with it.
    pub fn kill(&self) -> io::Result<()> {
        self.signal(libc::SIGKILL)
    }

    /// SIGTERM to the init, which forwards it to the command.
    pub fn terminate(&self) -> io::Result<()> {
        self.signal(libc::SIGTERM)
    }

    pub fn signal(&self, sig: libc::c_int) -> io::Result<()> {
        if self.exit.is_some() {
            return Ok(());
        }
        sys::pidfd_send_signal(self.pidfd.as_raw_fd(), sig)
    }

    /// Waits for the job to end.
    pub fn wait(&mut self) -> io::Result<Exit> {
        if let Some(exit) = &self.exit {
            return Ok(exit.clone());
        }
        let mut status = 0;
        loop {
            if unsafe { libc::waitpid(self.pid, &mut status, 0) } != -1 {
                break;
            }
            let e = io::Error::last_os_error();
            if e.kind() != io::ErrorKind::Interrupted {
                return Err(e);
            }
        }
        Ok(self.reaped(status))
    }

    /// The job's end, if it has ended.
    pub fn try_wait(&mut self) -> io::Result<Option<Exit>> {
        if let Some(exit) = &self.exit {
            return Ok(Some(exit.clone()));
        }
        let mut status = 0;
        match unsafe { libc::waitpid(self.pid, &mut status, libc::WNOHANG) } {
            0 => Ok(None),
            -1 => Err(io::Error::last_os_error()),
            _ => Ok(Some(self.reaped(status))),
        }
    }

    /// The job's end, from the init's raw wait status, for a wrapper that
    /// reaps every child itself (`waitpid(-1)`, as a subreaper does). Once
    /// the init has been reaped, every process of its namespace is gone, so
    /// its report is complete.
    pub fn reaped(&mut self, status: libc::c_int) -> Exit {
        let mut exited = None;
        let mut line = String::new();
        while matches!(self.report.read_line(&mut line), Ok(n) if n > 0) {
            if let Ok(Report::Exited(e)) = serde_json::from_str::<Report>(&line) {
                exited = Some(e);
            }
            line.clear();
        }
        let init_signal = libc::WIFSIGNALED(status).then(|| libc::WTERMSIG(status));
        let exit = match exited {
            Some(e) => Exit {
                code: e.code,
                signal: e.signal,
                init_signal,
                output_capped: e.signal == Some(libc::SIGXFSZ),
                scratch: Some(e.scratch),
            },
            None => Exit {
                code: None,
                signal: None,
                init_signal,
                output_capped: false,
                scratch: None,
            },
        };
        self.exit = Some(exit.clone());
        exit
    }
}

impl Drop for SandboxChild {
    fn drop(&mut self) {
        if self.exit.is_none() {
            let _ = self.kill();
            let _ = self.wait();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Root's job would have no process limit, so L1 refuses it; every
    /// other operator's runs under `RLIMIT_NPROC` (theseus-pv6i).
    #[test]
    fn root_is_refused_and_no_one_else_is() {
        assert_eq!(refusal(0), Some(ROOT_REFUSED));
        for uid in [1, 1000, 65534] {
            assert_eq!(refusal(uid), None, "uid {uid}");
        }
        assert!(ROOT_REFUSED.contains("RLIMIT_NPROC"));
    }
}
