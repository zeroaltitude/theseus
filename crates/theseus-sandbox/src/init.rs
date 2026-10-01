//! The init (design §2.2): pid 1 of the job, this binary in its
//! `job-sandbox` role.
//!
//! It reads the spec, builds the view, names the host, and brings `lo` up;
//! with egress (18b) it opens the listener and hands it to the wrapper. It
//! then forks the command, which enters user namespace 2 (the operator's uid
//! mapped back, so the job runs as the same uid as at L0), drops the
//! bounding set, takes its limits, `no_new_privs`, and the seccomp filter,
//! and execs. The init then drops its own capabilities and takes the same
//! filter. It reaps, forwards SIGTERM, SIGINT, SIGHUP, and SIGQUIT to the
//! command, and when the command exits it kills what is left, reports, and
//! exits with the command's status. When it exits, the kernel kills
//! everything still in the pid namespace, so the tree dies with the job by
//! construction.

use std::ffi::CString;
use std::fs::{self, File};
use std::io::{self, Read};
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd, RawFd};
use std::os::unix::fs::{FileTypeExt, MetadataExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::time::Instant;

use crate::report::{Exited, Report, Scratch, Started};
use crate::seccomp;
use crate::spawn::{EGRESS_FD, REPORT_FD, SPEC_FD};
use crate::spec::Spec;
use crate::sys::{self, cvt, Failure, Stage};
use crate::view;

/// The `job-sandbox` role: the init of one L1 job. It never returns.
pub fn init_main() -> ! {
    if unsafe { libc::getpid() } != 1 {
        eprintln!("job-sandbox: this role runs only as an L1 job's init, started by its wrapper");
        std::process::exit(2);
    }
    let t0 = Instant::now();
    let mut report = unsafe { File::from_raw_fd(REPORT_FD) };
    let code = match run(&mut report, t0) {
        Ok(code) => code,
        Err(f) => {
            let _ = Report::Failed {
                stage: f.stage,
                error: f.error.to_string(),
            }
            .write_to(&mut report);
            125
        }
    };
    std::process::exit(code)
}

/// The signals the init takes with `sigwaitinfo`: SIGCHLD, and those it
/// forwards to the command.
const FORWARDED: [libc::c_int; 4] = [libc::SIGTERM, libc::SIGINT, libc::SIGHUP, libc::SIGQUIT];

fn run(report: &mut File, t0: Instant) -> Result<i32, Failure> {
    // Dies with the wrapper (also set before the exec, in the clone).
    unsafe {
        libc::prctl(
            libc::PR_SET_PDEATHSIG,
            libc::SIGKILL as libc::c_ulong,
            0,
            0,
            0,
        )
    };
    // The wrapper cloned it with every signal blocked: from here, only those
    // the init takes with sigwaitinfo are.
    let set = signal_set();
    cvt(unsafe { libc::sigprocmask(libc::SIG_SETMASK, &set, std::ptr::null_mut()) })
        .stage("setting the signal mask")?;
    let spec = read_spec().stage("reading the job")?;
    // Descriptors 0 to 4 are the job's, and 5 with egress; anything else
    // inherited goes.
    let first_stray = if spec.egress_port.is_some() { 6 } else { 5 };
    sys::close_range(first_stray, u32::MAX, false).stage("closing inherited descriptors")?;
    let (uid, gid) = outer_ids().stage("reading user namespace 1's maps")?;
    let last_cap = last_cap();

    let view = view::build(&spec)?;
    sys::cvt(unsafe { libc::sethostname(spec.hostname.as_ptr().cast(), spec.hostname.len()) })
        .stage("setting the hostname")?;
    let lo = loopback_up().is_ok();
    if let Some(port) = spec.egress_port {
        egress_listener(port).stage(format!("opening the egress listener on 127.0.0.1:{port}"))?;
    }

    let filter = seccomp::program();
    let command = Command::prepare(&spec, uid, gid, last_cap, &filter)?;
    let (err_r, err_w) = sys::pipe().stage("making the command's pipe")?;
    let pid = match unsafe { libc::fork() } {
        -1 => return Err(io::Error::last_os_error()).stage("forking the command"),
        0 => {
            drop(err_r);
            command.exec(err_w.as_raw_fd())
        }
        pid => pid,
    };
    drop(err_w);
    drop_own_privileges(last_cap, &filter).stage("dropping the init's privileges")?;
    // The pipe closes at the command's exec, or carries the step that failed.
    if let Some((step, errno)) = read_step(err_r) {
        let mut status = 0;
        unsafe { libc::waitpid(pid, &mut status, 0) };
        return Err(io::Error::from_raw_os_error(errno)).stage(command.step_name(step));
    }
    Report::Started(Started {
        pid,
        setup_us: t0.elapsed().as_micros() as u64,
        sys: view.sys,
        lo,
    })
    .write_to(report)
    .stage("reporting the start")?;

    let status = supervise(pid, &set);
    // What is left of the job goes now, before scratch is read.
    unsafe { libc::kill(-1, libc::SIGKILL) };
    reap_all();
    let scratch = match &view.scratch {
        Some(fd) => summarize(fd, &spec.workspace),
        None => Scratch::default(),
    };
    let (code, signal) = if libc::WIFSIGNALED(status) {
        (None, Some(libc::WTERMSIG(status)))
    } else {
        (Some(libc::WEXITSTATUS(status)), None)
    };
    let _ = Report::Exited(Exited {
        code,
        signal,
        scratch,
    })
    .write_to(report);
    Ok(code.unwrap_or_else(|| 128 + signal.unwrap_or(0)))
}

fn read_spec() -> io::Result<Spec> {
    let mut f = unsafe { File::from_raw_fd(SPEC_FD) };
    let mut buf = Vec::new();
    f.read_to_end(&mut buf)?;
    serde_json::from_slice(&buf).map_err(io::Error::other)
}

/// The operator's uid and gid: what user namespace 1's root maps to.
fn outer_ids() -> io::Result<(u32, u32)> {
    let outer = |file: &str| -> io::Result<u32> {
        let map = fs::read_to_string(file)?;
        let mut f = map.split_whitespace();
        match (f.next(), f.next()) {
            (Some("0"), Some(id)) => id.parse().map_err(io::Error::other),
            _ => Err(io::Error::other(format!("{file}: {}", map.trim()))),
        }
    };
    Ok((outer("/proc/self/uid_map")?, outer("/proc/self/gid_map")?))
}

fn last_cap() -> u32 {
    fs::read_to_string("/proc/sys/kernel/cap_last_cap")
        .ok()
        .and_then(|s| s.trim().parse().ok())
        .unwrap_or(63)
}

fn signal_set() -> libc::sigset_t {
    let mut set: libc::sigset_t = unsafe { std::mem::zeroed() };
    unsafe {
        libc::sigemptyset(&mut set);
        libc::sigaddset(&mut set, libc::SIGCHLD);
        for s in FORWARDED {
            libc::sigaddset(&mut set, s);
        }
    }
    set
}

/// Brings up `lo`, alone in the job's network namespace.
fn loopback_up() -> io::Result<()> {
    let fd = cvt(unsafe { libc::socket(libc::AF_INET, libc::SOCK_DGRAM | libc::SOCK_CLOEXEC, 0) })?;
    let fd = unsafe { OwnedFd::from_raw_fd(fd) };
    let mut ifr: libc::ifreq = unsafe { std::mem::zeroed() };
    for (i, b) in b"lo".iter().enumerate() {
        ifr.ifr_name[i] = *b as libc::c_char;
    }
    cvt(unsafe { libc::ioctl(fd.as_raw_fd(), libc::SIOCGIFFLAGS, &mut ifr) })?;
    unsafe { ifr.ifr_ifru.ifru_flags |= libc::IFF_UP as libc::c_short };
    cvt(unsafe { libc::ioctl(fd.as_raw_fd(), libc::SIOCSIFFLAGS, &ifr) })?;
    Ok(())
}

/// 18b: listens on 127.0.0.1:`port` inside the job's network namespace, and
/// hands the listener to the wrapper over `SCM_RIGHTS`, which serves it from
/// the host's.
fn egress_listener(port: u16) -> io::Result<()> {
    let listener = std::net::TcpListener::bind(("127.0.0.1", port))?;
    sys::send_fd(EGRESS_FD, listener.as_raw_fd())?;
    drop(listener);
    unsafe { libc::close(EGRESS_FD) };
    Ok(())
}

/// Everything the command's child needs, prepared before the fork.
struct Command {
    path: CString,
    argv: Vec<CString>,
    envp: Vec<CString>,
    cwd: CString,
    uid_map: String,
    gid_map: String,
    last_cap: u32,
    output_bytes: u64,
    pids: u64,
    filter: Vec<libc::sock_filter>,
    shown: String,
}

/// The command's steps, numbered for the pipe that reports a failure.
const STEPS: [&str; 12] = [
    "",
    "entering user namespace 2",
    "denying setgroups in user namespace 2",
    "writing user namespace 2's uid map",
    "writing user namespace 2's gid map",
    "dropping the bounding set",
    "clearing the ambient capabilities",
    "setting the limits",
    "setting no_new_privs",
    "installing the seccomp filter",
    "entering the working directory",
    "executing the command",
];

impl Command {
    fn prepare(
        spec: &Spec,
        uid: u32,
        gid: u32,
        last_cap: u32,
        filter: &[libc::sock_filter],
    ) -> Result<Self, Failure> {
        let cs = |s: &str| sys::cbytes(s.as_bytes());
        let path = find(&spec.argv[0], spec).stage(format!("finding {}", spec.argv[0]))?;
        Ok(Self {
            path: sys::cpath(&path).stage("the command's path")?,
            argv: spec
                .argv
                .iter()
                .map(|a| cs(a))
                .collect::<io::Result<_>>()
                .stage("the command's arguments")?,
            envp: spec
                .env
                .iter()
                .map(|(k, v)| cs(&format!("{k}={v}")))
                .collect::<io::Result<_>>()
                .stage("the command's environment")?,
            cwd: sys::cpath(&spec.cwd).stage("the working directory")?,
            uid_map: format!("{uid} 0 1\n"),
            gid_map: format!("{gid} 0 1\n"),
            last_cap,
            output_bytes: spec.limits.output_mb.saturating_mul(1 << 20),
            pids: spec.limits.pids,
            filter: filter.to_vec(),
            shown: path.display().to_string(),
        })
    }

    fn step_name(&self, step: u32) -> String {
        let name = STEPS
            .get(step as usize)
            .copied()
            .unwrap_or("starting the command");
        match step {
            10 => format!("{name}, {}", self.cwd.to_string_lossy()),
            11 => format!("{name}, {}", self.shown),
            _ => name.to_string(),
        }
    }

    /// The command's side of the fork: the init is single-threaded, so this
    /// child may allocate. A step that fails is written to `err_w`.
    fn exec(&self, err_w: RawFd) -> ! {
        let (step, errno) = match self.setup() {
            Ok(()) => {
                let argv = ptrs(&self.argv);
                let envp = ptrs(&self.envp);
                unsafe { libc::execve(self.path.as_ptr(), argv.as_ptr(), envp.as_ptr()) };
                (11u32, sys::errno())
            }
            Err((step, e)) => (step, e.raw_os_error().unwrap_or(libc::EIO)),
        };
        let mut msg = [0u8; 8];
        msg[..4].copy_from_slice(&step.to_ne_bytes());
        msg[4..].copy_from_slice(&errno.to_ne_bytes());
        unsafe {
            libc::write(err_w, msg.as_ptr().cast(), msg.len());
            libc::_exit(127)
        }
    }

    fn setup(&self) -> Result<(), (u32, io::Error)> {
        let at = |step: u32| move |e: io::Error| (step, e);
        // The command starts as a fresh process would: every signal at its
        // default (the init's runtime ignores SIGPIPE, say), none blocked.
        let empty = {
            let mut s: libc::sigset_t = unsafe { std::mem::zeroed() };
            unsafe { libc::sigemptyset(&mut s) };
            s
        };
        reset_signals();
        unsafe { libc::sigprocmask(libc::SIG_SETMASK, &empty, std::ptr::null_mut()) };
        cvt(unsafe { libc::unshare(libc::CLONE_NEWUSER) }).map_err(at(1))?;
        fs::write("/proc/self/setgroups", "deny").map_err(at(2))?;
        fs::write("/proc/self/uid_map", &self.uid_map).map_err(at(3))?;
        fs::write("/proc/self/gid_map", &self.gid_map).map_err(at(4))?;
        drop_bounding_set(self.last_cap).map_err(at(5))?;
        clear_ambient().map_err(at(6))?;
        let limit = |resource, value: u64| {
            let r = libc::rlimit {
                rlim_cur: value as libc::rlim_t,
                rlim_max: value as libc::rlim_t,
            };
            cvt(unsafe { libc::setrlimit(resource, &r) })
        };
        limit(libc::RLIMIT_FSIZE, self.output_bytes).map_err(at(7))?;
        limit(libc::RLIMIT_NPROC, self.pids).map_err(at(7))?;
        limit(libc::RLIMIT_CORE, 0).map_err(at(7))?;
        no_new_privs().map_err(at(8))?;
        seccomp::install(&self.filter).map_err(at(9))?;
        cvt(unsafe { libc::chdir(self.cwd.as_ptr()) }).map_err(at(10))?;
        // Only the standard streams reach the command; the report pipe and
        // the step pipe close at its exec.
        sys::close_range(3, u32::MAX, true).map_err(at(11))?;
        Ok(())
    }
}

/// Every signal to its default action, by the raw system call: the C
/// library refuses to touch the two it reserves (32 and 33), and an ignored
/// disposition would otherwise pass through every exec to the command.
fn reset_signals() {
    // The kernel's struct sigaction; all zero is SIG_DFL with no flags.
    #[repr(C)]
    struct KernelSigaction {
        handler: usize,
        flags: libc::c_ulong,
        restorer: usize,
        mask: u64,
    }
    let default = KernelSigaction {
        handler: 0,
        flags: 0,
        restorer: 0,
        mask: 0,
    };
    for sig in 1..=64 {
        if sig != libc::SIGKILL && sig != libc::SIGSTOP {
            unsafe {
                libc::syscall(
                    libc::SYS_rt_sigaction,
                    sig,
                    &default as *const KernelSigaction,
                    std::ptr::null_mut::<KernelSigaction>(),
                    8usize,
                )
            };
        }
    }
}

fn ptrs(v: &[CString]) -> Vec<*const libc::c_char> {
    v.iter()
        .map(|c| c.as_ptr())
        .chain(std::iter::once(std::ptr::null()))
        .collect()
}

/// `argv0` as `execvp` finds it: as given with a slash, else on the job's
/// own PATH, in the view.
fn find(argv0: &str, spec: &Spec) -> io::Result<PathBuf> {
    if argv0.contains('/') {
        return Ok(argv0.into());
    }
    let path = spec
        .env
        .iter()
        .find(|(k, _)| k == "PATH")
        .map_or("/usr/local/bin:/usr/bin:/bin", |(_, v)| v.as_str());
    for dir in path.split(':') {
        let dir = spec.cwd.join(if dir.is_empty() { "." } else { dir });
        let p = dir.join(argv0);
        let runnable = fs::metadata(&p)
            .map(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
            .unwrap_or(false);
        if runnable {
            return Ok(p);
        }
    }
    Err(io::Error::from_raw_os_error(libc::ENOENT))
}

fn read_step(r: OwnedFd) -> Option<(u32, i32)> {
    let mut msg = [0u8; 8];
    let mut f = File::from(r);
    let mut at = 0;
    while at < msg.len() {
        match f.read(&mut msg[at..]) {
            Ok(0) => break,
            Ok(n) => at += n,
            Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
            Err(_) => break,
        }
    }
    (at == msg.len()).then(|| {
        (
            u32::from_ne_bytes(msg[..4].try_into().unwrap()),
            i32::from_ne_bytes(msg[4..].try_into().unwrap()),
        )
    })
}

fn no_new_privs() -> io::Result<()> {
    cvt(unsafe { libc::prctl(libc::PR_SET_NO_NEW_PRIVS, 1, 0, 0, 0) }).map(drop)
}

/// Every capability out of the bounding set, so nothing an exec meets can
/// bring one back.
fn drop_bounding_set(last_cap: u32) -> io::Result<()> {
    for cap in 0..=last_cap {
        let r = unsafe { libc::prctl(libc::PR_CAPBSET_DROP, cap as libc::c_ulong, 0, 0, 0) };
        if r == -1 && sys::errno() != libc::EINVAL {
            return Err(io::Error::last_os_error());
        }
    }
    Ok(())
}

fn clear_ambient() -> io::Result<()> {
    cvt(unsafe {
        libc::prctl(
            libc::PR_CAP_AMBIENT,
            libc::PR_CAP_AMBIENT_CLEAR_ALL as libc::c_ulong,
            0,
            0,
            0,
        )
    })
    .map(drop)
}

/// The init, once the command is forked: no capabilities left, and the
/// command's filter, so pid 1's capabilities in user namespace 1 cannot be
/// used through any call the filter denies.
fn drop_own_privileges(last_cap: u32, filter: &[libc::sock_filter]) -> io::Result<()> {
    #[repr(C)]
    struct Header {
        version: u32,
        pid: i32,
    }
    #[repr(C)]
    #[derive(Clone, Copy)]
    struct Data {
        effective: u32,
        permitted: u32,
        inheritable: u32,
    }
    const VERSION_3: u32 = 0x2008_0522;
    no_new_privs()?;
    drop_bounding_set(last_cap)?;
    clear_ambient()?;
    let header = Header {
        version: VERSION_3,
        pid: 0,
    };
    let none = [Data {
        effective: 0,
        permitted: 0,
        inheritable: 0,
    }; 2];
    sys::cvt_long(unsafe {
        libc::syscall(libc::SYS_capset, &header as *const Header, none.as_ptr())
    })?;
    seccomp::install(filter)
}

/// Waits for the command, reaping every orphan the namespace hands the init
/// and forwarding the stop signals. The command's wait status.
fn supervise(command: libc::pid_t, set: &libc::sigset_t) -> libc::c_int {
    loop {
        let mut info: libc::siginfo_t = unsafe { std::mem::zeroed() };
        let sig = unsafe { libc::sigwaitinfo(set, &mut info) };
        if sig == libc::SIGCHLD {
            if let Some(status) = reap(command) {
                return status;
            }
        } else if FORWARDED.contains(&sig) {
            unsafe { libc::kill(command, sig) };
        }
    }
}

/// Reaps every child that has exited. The command's status, if it was one.
fn reap(command: libc::pid_t) -> Option<libc::c_int> {
    let mut found = None;
    loop {
        let mut status = 0;
        match unsafe { libc::waitpid(-1, &mut status, libc::WNOHANG) } {
            0 | -1 => return found,
            pid if pid == command => found = Some(status),
            _ => {}
        }
    }
}

fn reap_all() {
    loop {
        let mut status = 0;
        if unsafe { libc::waitpid(-1, &mut status, 0) } == -1 && sys::errno() != libc::EINTR {
            return;
        }
    }
}

/// What the job wrote to scratch: each overlay's upper directory, walked
/// through the descriptor the view kept, shown at the workspace's paths.
fn summarize(scratch: &OwnedFd, roots: &[PathBuf]) -> Scratch {
    let base = PathBuf::from(format!("/proc/self/fd/{}", scratch.as_raw_fd()));
    let mut s = Scratch::default();
    let mut budget = 100_000u32;
    for (i, root) in roots.iter().enumerate() {
        walk(
            &base.join(i.to_string()).join("upper"),
            root,
            &mut s,
            &mut budget,
        );
    }
    s
}

fn walk(dir: &Path, shown: &Path, s: &mut Scratch, budget: &mut u32) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for e in entries.flatten() {
        if *budget == 0 {
            return;
        }
        *budget -= 1;
        let Ok(m) = e.metadata() else { continue };
        let shown_path = shown.join(e.file_name());
        let ft = m.file_type();
        if ft.is_dir() {
            walk(&e.path(), &shown_path, s, budget);
        } else if ft.is_char_device() && m.rdev() == 0 {
            // An overlay's whiteout: a path the job removed.
            s.removed += 1;
        } else {
            s.files += 1;
            s.bytes += m.len();
            if s.paths.len() < 20 {
                s.paths.push(shown_path.display().to_string());
            }
        }
    }
}
