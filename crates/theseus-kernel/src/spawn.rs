//! A job's command, started by its wrapper with nothing of the wrapper copied
//! (theseus-ypqg), and born in the job's cgroup when it has one
//! (theseus-a5nv): `clone3` with `CLONE_VM | CLONE_VFORK`, as glibc's
//! posix_spawn clones, plus `CLONE_INTO_CGROUP`. std's `Command` can set
//! neither the operator's umask nor a cgroup without `pre_exec`, which makes
//! it fork; and a move into a cgroup by `cgroup.procs` waits for an RCU grace
//! period, measured at 8 to 40 ms after an idle spell, where the clone costs
//! what a posix_spawn does.

use std::ffi::{CString, OsStr};
use std::io::{self, Read};
use std::os::fd::{AsRawFd, BorrowedFd, RawFd};
use std::os::unix::ffi::OsStrExt;
use std::path::Path;

/// linux/sched.h's, as a u64: libc gives it the wrong width on some targets.
const CLONE_INTO_CGROUP: u64 = 0x2_0000_0000;

unsafe extern "C" {
    static environ: *const *const libc::c_char;
}

/// What the child runs, made before the clone: the child allocates nothing.
pub(crate) struct Exec {
    /// Each file `argv[0]` may be, in `PATH`'s order, as execvp looks.
    files: Vec<CString>,
    argv: Vec<CString>,
    cwd: Option<CString>,
}

fn c(b: &[u8]) -> io::Result<CString> {
    CString::new(b).map_err(|_| io::Error::other("an argument holds a NUL byte"))
}

impl Exec {
    /// `argv`, run in `cwd`, found on this process's `PATH`, which is the
    /// job's: a wrapper process has the job's environment as its own.
    pub(crate) fn new(argv: &[String], cwd: Option<&Path>) -> io::Result<Exec> {
        let prog = argv
            .first()
            .ok_or_else(|| io::Error::other("no program to run"))?;
        let files = if prog.contains('/') {
            vec![c(prog.as_bytes())?]
        } else {
            let path = std::env::var_os("PATH").unwrap_or_else(|| "/bin:/usr/bin".into());
            path.as_bytes()
                .split(|&b| b == b':')
                .map(|d| c(&[if d.is_empty() { b"." } else { d }, b"/", prog.as_bytes()].concat()))
                .collect::<io::Result<_>>()?
        };
        Ok(Exec {
            files,
            argv: argv
                .iter()
                .map(|a| c(a.as_bytes()))
                .collect::<io::Result<_>>()?,
            cwd: cwd.map(|d| c(OsStr::as_bytes(d.as_os_str()))).transpose()?,
        })
    }
}

/// What the child needs, made before the clone: it only reads it.
struct Child<'a> {
    exec: &'a Exec,
    argv: *const *const libc::c_char,
    envp: *const *const libc::c_char,
    stdio: [RawFd; 3],
    umask: Option<u32>,
    err: RawFd,
}

/// Start `e` with `stdio` as its descriptors 0, 1, and 2, under `umask`, and
/// in `cgroup` when there is one; its pid. Each of `stdio` is 3 or above: a
/// wrapper's own 0 to 2 are /dev/null. An exec that fails is this call's
/// error, as with std's spawn.
pub(crate) fn spawn(
    e: &Exec,
    stdio: [RawFd; 3],
    umask: Option<u32>,
    cgroup: Option<BorrowedFd<'_>>,
) -> io::Result<u32> {
    let argv: Vec<*const libc::c_char> = e
        .argv
        .iter()
        .map(|a| a.as_ptr())
        .chain([std::ptr::null()])
        .collect();
    // The child's errno comes back through a pipe that its exec closes.
    let (mut err_r, err_w) = std::io::pipe()?;
    // SAFETY: zeroed is clone_args' "no field set"; the flags follow.
    let mut args: libc::clone_args = unsafe { std::mem::zeroed() };
    args.flags = (libc::CLONE_VM | libc::CLONE_VFORK) as u64;
    args.exit_signal = libc::SIGCHLD as u64;
    if let Some(cg) = cgroup {
        args.flags |= CLONE_INTO_CGROUP;
        args.cgroup = cg.as_raw_fd() as u64;
    }
    let c = Child {
        exec: e,
        argv: argv.as_ptr(),
        // SAFETY: the process's environment, the job's in a wrapper.
        envp: unsafe { environ },
        stdio,
        umask,
        err: err_w.as_raw_fd(),
    };
    // Every signal is blocked across the clone, so no handler of the wrapper
    // runs in the child, which shares its memory until the exec.
    // SAFETY: sigsets filled by libc, and the mask put back below.
    let (mut all, mut old): (libc::sigset_t, libc::sigset_t) =
        unsafe { (std::mem::zeroed(), std::mem::zeroed()) };
    unsafe {
        libc::sigfillset(&mut all);
        libc::pthread_sigmask(libc::SIG_SETMASK, &all, &mut old);
    }
    let pid: i64;
    // SAFETY: clone3 as this function's own instruction. CLONE_VFORK stops
    // this thread until the child has execed or exited, and the child runs
    // `child` alone, on this stack below this frame, which it only reads. A
    // call to libc's syscall() would return in the child too, and the child's
    // next call would push over the return address its parent takes on waking.
    unsafe {
        #[cfg(target_arch = "x86_64")]
        std::arch::asm!(
            "syscall",
            inlateout("rax") libc::SYS_clone3 => pid,
            in("rdi") &mut args as *mut libc::clone_args,
            in("rsi") std::mem::size_of::<libc::clone_args>(),
            lateout("rcx") _,
            lateout("r11") _,
            options(nostack),
        );
        #[cfg(target_arch = "aarch64")]
        std::arch::asm!(
            "svc 0",
            in("x8") libc::SYS_clone3,
            inlateout("x0") &mut args as *mut libc::clone_args as i64 => pid,
            in("x1") std::mem::size_of::<libc::clone_args>(),
            options(nostack),
        );
    }
    if pid == 0 {
        // SAFETY: the child, while this thread is stopped.
        unsafe { child(&c) }
    }
    // SAFETY: the mask saved above.
    unsafe { libc::pthread_sigmask(libc::SIG_SETMASK, &old, std::ptr::null_mut()) };
    drop(err_w);
    if pid < 0 {
        return Err(io::Error::from_raw_os_error(-pid as i32));
    }
    let mut errno = [0u8; 4];
    if err_r.read_exact(&mut errno).is_ok() {
        // SAFETY: the child exited after writing its errno; reaped here.
        unsafe { libc::waitpid(pid as libc::pid_t, &mut 0, 0) };
        return Err(io::Error::from_raw_os_error(i32::from_ne_bytes(errno)));
    }
    Ok(pid as u32)
}

/// The child, until its exec, on the wrapper's memory: raw system calls only,
/// and nothing allocated. Its signal dispositions are its own copy (no
/// `CLONE_SIGHAND`), so the wrapper's handlers go back to the defaults here,
/// SIGPIPE's ignore with them, and its mask is empty, as std gives a command.
#[inline(never)]
unsafe fn child(c: &Child<'_>) -> ! {
    for sig in [libc::SIGTERM, libc::SIGCHLD, libc::SIGPIPE] {
        libc::signal(sig, libc::SIG_DFL);
    }
    for (fd, &from) in (0..).zip(&c.stdio) {
        if libc::dup2(from, fd) == -1 {
            fail(c.err, *libc::__errno_location());
        }
    }
    if let Some(d) = &c.exec.cwd {
        if libc::chdir(d.as_ptr()) == -1 {
            fail(c.err, *libc::__errno_location());
        }
    }
    if let Some(u) = c.umask {
        libc::umask(u as libc::mode_t);
    }
    let mut none: libc::sigset_t = std::mem::zeroed();
    libc::sigemptyset(&mut none);
    libc::pthread_sigmask(libc::SIG_SETMASK, &none, std::ptr::null_mut());
    // As execvp: a file that is not there, or not a directory's, is passed
    // over; EACCES is kept for the end; any other error ends the search.
    let mut last = libc::ENOENT;
    for f in &c.exec.files {
        libc::execve(f.as_ptr(), c.argv, c.envp);
        match *libc::__errno_location() {
            libc::ENOENT | libc::ENOTDIR => {}
            libc::EACCES => last = libc::EACCES,
            n => fail(c.err, n),
        }
    }
    fail(c.err, last)
}

/// The child's end: its errno to the parent, then exit.
unsafe fn fail(err: RawFd, n: libc::c_int) -> ! {
    libc::write(err, (&n as *const libc::c_int).cast(), 4);
    libc::_exit(127)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::File;

    fn run(argv: &[&str], umask: Option<u32>) -> io::Result<(String, i32)> {
        let argv: Vec<String> = argv.iter().map(|s| s.to_string()).collect();
        let e = Exec::new(&argv, Some(Path::new("/")))?;
        let (mut r, w) = std::io::pipe()?;
        let null = File::open("/dev/null")?;
        let stdio = [null.as_raw_fd(), w.as_raw_fd(), w.as_raw_fd()];
        let pid = spawn(&e, stdio, umask, None)?;
        drop(w);
        let mut out = String::new();
        r.read_to_string(&mut out)?;
        let mut status = 0;
        // SAFETY: this test's own child.
        unsafe { libc::waitpid(pid as libc::pid_t, &mut status, 0) };
        Ok((out, libc::WEXITSTATUS(status)))
    }

    /// A command is found on PATH, gets its umask, cwd, and stdio, and an
    /// exec that fails is the spawn's error, its child reaped.
    #[test]
    fn a_command_runs_with_its_umask_and_cwd_and_a_missing_one_fails_to_spawn() {
        let (out, code) = run(
            &["sh", "-c", "umask; pwd; echo err >&2; exit 3"],
            Some(0o027),
        )
        .unwrap();
        assert_eq!(out, "0027\n/\nerr\n");
        assert_eq!(code, 3);
        let e = run(&["theseus-no-such-program-7f3a"], None).unwrap_err();
        assert_eq!(e.raw_os_error(), Some(libc::ENOENT), "{e}");
        let e = run(&["/etc/passwd"], None).unwrap_err();
        assert_eq!(e.raw_os_error(), Some(libc::EACCES), "{e}");
    }
}
