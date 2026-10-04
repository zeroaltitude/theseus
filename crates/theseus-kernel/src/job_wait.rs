//! The wrapper's wait on its command (Tier 7.1; review F2): asleep until
//! something happens, instead of a look every 20 ms.
//!
//! The command's pidfd reads as ready once it exits. A wrapper process also
//! has a wake pipe, which its SIGTERM and SIGCHLD handlers write a byte to:
//! a stop asked (18a), or a child's exit, an orphan reparented to the wrapper
//! among them, which the look after the wake reaps as before. Each wait polls
//! both, with the time left before the deadline as its timeout, so a
//! running job's wrapper wakes only when there is something to do. A signal
//! that comes between a look and the poll is not lost, whichever thread took
//! it: its byte waits in the pipe, and the poll returns at once. A test's
//! thread has no handlers and no pipe: it polls the pidfd alone.

use std::os::fd::{AsRawFd, BorrowedFd, FromRawFd, OwnedFd};
use std::sync::atomic::{AtomicI32, Ordering};
use std::time::Duration;

/// How often a wait looks where the kernel gives no pidfd (before Linux
/// 5.3): every wait did before 7.1.
pub(crate) const NO_PIDFD_LOOK: Duration = Duration::from_millis(20);

/// The wake pipe's ends: -1 until `arm` makes it, and always in a test's
/// thread.
static READ: AtomicI32 = AtomicI32::new(-1);
static WRITE: AtomicI32 = AtomicI32::new(-1);

/// Make the wake pipe and catch SIGCHLD into it: a wrapper process, once,
/// before its command starts. With `SA_RESTART`, so a child's exit never
/// interrupts the copy's reads on another thread; a poll is never restarted,
/// and the pipe wakes it anyway.
pub(crate) fn arm() -> Result<(), String> {
    let mut fds = [0 as libc::c_int; 2];
    // SAFETY: pipe2 fills the two descriptors, which live as long as the
    // process: the handlers write to one from any thread.
    if unsafe { libc::pipe2(fds.as_mut_ptr(), libc::O_CLOEXEC | libc::O_NONBLOCK) } == -1 {
        return Err(format!(
            "the wake pipe: {}",
            std::io::Error::last_os_error()
        ));
    }
    READ.store(fds[0], Ordering::SeqCst);
    WRITE.store(fds[1], Ordering::SeqCst);
    // SAFETY: a handler that only writes a byte to a pipe, installed for SIGCHLD.
    unsafe {
        let mut sa: libc::sigaction = std::mem::zeroed();
        sa.sa_sigaction = on_sigchld as *const () as usize;
        sa.sa_flags = libc::SA_RESTART | libc::SA_NOCLDSTOP;
        libc::sigemptyset(&mut sa.sa_mask);
        if libc::sigaction(libc::SIGCHLD, &sa, std::ptr::null_mut()) == -1 {
            return Err(format!(
                "catching SIGCHLD: {}",
                std::io::Error::last_os_error()
            ));
        }
    }
    Ok(())
}

/// A byte into the wake pipe, from a signal handler: `write` is
/// async-signal-safe, and errno is kept for the code the signal interrupted.
/// A full pipe already holds a wake, so a failed write loses nothing.
pub(crate) fn poke() {
    let fd = WRITE.load(Ordering::SeqCst);
    if fd < 0 {
        return;
    }
    // SAFETY: errno read and restored around one write of one byte.
    unsafe {
        let errno = *libc::__errno_location();
        let _ = libc::write(fd, [1u8].as_ptr().cast(), 1);
        *libc::__errno_location() = errno;
    }
}

extern "C" fn on_sigchld(_: libc::c_int) {
    poke();
}

/// A pidfd for process `pid`, or `None` where the kernel has none: the
/// caller then looks every `NO_PIDFD_LOOK`.
pub(crate) fn pidfd(pid: u32) -> Option<OwnedFd> {
    // SAFETY: pidfd_open takes a pid and flags and returns a new descriptor,
    // which the OwnedFd closes.
    let fd = unsafe { libc::syscall(libc::SYS_pidfd_open, pid as libc::pid_t, 0) };
    (fd >= 0).then(|| unsafe { OwnedFd::from_raw_fd(fd as i32) })
}

/// Sleep until `pidfd`'s process has exited, the wake pipe holds a byte, or
/// `timeout` has passed, then empty the pipe: the caller looks at what woke
/// it. A signal that interrupts the poll returns too.
pub(crate) fn wait(pidfd: BorrowedFd<'_>, timeout: Duration) {
    let wake = READ.load(Ordering::SeqCst);
    let mut fds = [
        libc::pollfd {
            fd: pidfd.as_raw_fd(),
            events: libc::POLLIN,
            revents: 0,
        },
        libc::pollfd {
            fd: wake,
            events: libc::POLLIN,
            revents: 0,
        },
    ];
    let n = if wake >= 0 { 2 } else { 1 };
    // Rounded up, so a wait short of a millisecond sleeps instead of spinning.
    let ms = timeout.as_micros().div_ceil(1000).min(i32::MAX as u128) as libc::c_int;
    // SAFETY: poll over the `n` descriptors above, alive for the call.
    unsafe { libc::poll(fds.as_mut_ptr(), n, ms) };
    if wake >= 0 && fds[1].revents != 0 {
        let mut buf = [0u8; 64];
        // SAFETY: reads from the nonblocking pipe until it is empty.
        while unsafe { libc::read(wake, buf.as_mut_ptr().cast(), buf.len()) } > 0 {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::fd::AsFd;
    use std::time::Instant;

    /// A pidfd wakes the wait when its process exits, long before the
    /// timeout, and a live process's wait lasts the timeout.
    #[test]
    fn a_pidfd_wakes_the_wait_at_the_exit_and_not_before() {
        let mut child = std::process::Command::new("sleep")
            .arg("0.2")
            .spawn()
            .unwrap();
        let fd = pidfd(child.id()).expect("this kernel has pidfds");
        let t0 = Instant::now();
        wait(fd.as_fd(), Duration::from_millis(50));
        let first = t0.elapsed();
        assert!(
            first >= Duration::from_millis(45) && child.try_wait().unwrap().is_none(),
            "a live process: the timeout ({first:?})"
        );
        wait(fd.as_fd(), Duration::from_secs(10));
        let woke = t0.elapsed();
        assert!(
            woke < Duration::from_secs(5),
            "woken by the exit, not the timeout ({woke:?})"
        );
        assert!(child.wait().unwrap().success());
    }
}
