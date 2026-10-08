//! Thin wrappers over the system calls the sandbox makes, each an
//! `io::Result`, and the failure type that names the stage it came from.

use std::ffi::{CStr, CString};
use std::io;
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd, RawFd};
use std::os::unix::ffi::OsStrExt;
use std::path::Path;

/// A step of the setup that failed, and why: "mounting /proc: EPERM".
#[derive(Debug)]
pub(crate) struct Failure {
    pub stage: String,
    pub error: io::Error,
}

impl std::fmt::Display for Failure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.stage, self.error)
    }
}

/// Names the stage an `io::Error` came from.
pub(crate) trait Stage<T> {
    fn stage(self, stage: impl Into<String>) -> Result<T, Failure>;
}

impl<T> Stage<T> for io::Result<T> {
    fn stage(self, stage: impl Into<String>) -> Result<T, Failure> {
        self.map_err(|error| Failure {
            stage: stage.into(),
            error,
        })
    }
}

pub(crate) fn cvt(r: libc::c_int) -> io::Result<libc::c_int> {
    if r == -1 {
        Err(io::Error::last_os_error())
    } else {
        Ok(r)
    }
}

pub(crate) fn cvt_long(r: libc::c_long) -> io::Result<libc::c_long> {
    if r == -1 {
        Err(io::Error::last_os_error())
    } else {
        Ok(r)
    }
}

pub(crate) fn cpath(p: &Path) -> io::Result<CString> {
    cbytes(p.as_os_str().as_bytes())
}

pub(crate) fn cbytes(b: &[u8]) -> io::Result<CString> {
    CString::new(b).map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "a NUL byte inside"))
}

fn opt(c: Option<&CStr>) -> *const libc::c_char {
    c.map_or(std::ptr::null(), CStr::as_ptr)
}

pub(crate) fn mount(
    source: Option<&CStr>,
    target: &CStr,
    fstype: Option<&CStr>,
    flags: libc::c_ulong,
    data: Option<&CStr>,
) -> io::Result<()> {
    cvt(unsafe {
        libc::mount(
            opt(source),
            target.as_ptr(),
            opt(fstype),
            flags,
            opt(data).cast(),
        )
    })
    .map(drop)
}

pub(crate) fn umount2(target: &CStr, flags: libc::c_int) -> io::Result<()> {
    cvt(unsafe { libc::umount2(target.as_ptr(), flags) }).map(drop)
}

pub(crate) fn pivot_root(new_root: &CStr, put_old: &CStr) -> io::Result<()> {
    cvt_long(unsafe { libc::syscall(libc::SYS_pivot_root, new_root.as_ptr(), put_old.as_ptr()) })
        .map(drop)
}

pub(crate) const MOUNT_ATTR_RDONLY: u64 = 0x1;
pub(crate) const MOUNT_ATTR_NOSUID: u64 = 0x2;
pub(crate) const MOUNT_ATTR_NODEV: u64 = 0x4;
pub(crate) const MOUNT_ATTR_NOEXEC: u64 = 0x8;
const AT_RECURSIVE: libc::c_uint = 0x8000;

#[repr(C)]
struct MountAttr {
    attr_set: u64,
    attr_clr: u64,
    propagation: u64,
    userns_fd: u64,
}

/// Sets mount attributes on `path`'s mount, and with `recursive` on every
/// mount below it too (Linux 5.12). Setting an attribute never needs one a
/// locked mount holds to be cleared, so it works on the host's binds.
pub(crate) fn mount_setattr(path: &CStr, recursive: bool, set: u64) -> io::Result<()> {
    let attr = MountAttr {
        attr_set: set,
        attr_clr: 0,
        propagation: 0,
        userns_fd: 0,
    };
    let flags = if recursive { AT_RECURSIVE } else { 0 };
    cvt_long(unsafe {
        libc::syscall(
            libc::SYS_mount_setattr,
            libc::AT_FDCWD,
            path.as_ptr(),
            flags,
            &attr as *const MountAttr,
            std::mem::size_of::<MountAttr>(),
        )
    })
    .map(drop)
}

/// `close_range(2)`: closes `first..=last`, or with `cloexec` marks them
/// close-on-exec instead.
/// Where the host refuses it (Linux before 5.9, or a container's seccomp
/// profile that answers ENOSYS or EPERM), the same one descriptor at a time
/// (theseus-f7tz).
pub(crate) fn close_range(first: u32, last: u32, cloexec: bool) -> io::Result<()> {
    const CLOSE_RANGE_CLOEXEC: libc::c_uint = 1 << 2;
    let flags = if cloexec { CLOSE_RANGE_CLOEXEC } else { 0 };
    match cvt_long(unsafe { libc::syscall(libc::SYS_close_range, first, last, flags) }) {
        Err(e) if matches!(e.raw_os_error(), Some(libc::ENOSYS | libc::EPERM)) => {
            each_open(first, last, |fd| unsafe {
                match cloexec {
                    true => libc::fcntl(fd, libc::F_SETFD, libc::FD_CLOEXEC),
                    false => libc::close(fd),
                };
            })
        }
        r => r.map(drop),
    }
}

/// `f` on each open descriptor in `first..=last`, read from `/proc/self/fd`,
/// or, with no `/proc`, on each below the open-file limit. Raw calls on the
/// stack alone: the init runs it between its fork and the command's exec.
fn each_open(first: u32, last: u32, mut f: impl FnMut(libc::c_int)) -> io::Result<()> {
    let dir = unsafe {
        libc::open(
            c"/proc/self/fd".as_ptr(),
            libc::O_RDONLY | libc::O_DIRECTORY | libc::O_CLOEXEC,
        )
    };
    if dir == -1 {
        let mut lim: libc::rlimit = unsafe { std::mem::zeroed() };
        cvt(unsafe { libc::getrlimit(libc::RLIMIT_NOFILE, &mut lim) })?;
        // An unlimited one stops at the kernel's default `nr_open`.
        let top = (lim.rlim_cur as u64).min(1 << 20);
        (u64::from(first)..top.min(u64::from(last) + 1)).for_each(|fd| f(fd as libc::c_int));
        return Ok(());
    }
    // The listing is read whole before any is acted on, so a close never
    // moves what is still to be read; more than `found` holds are taken by
    // further passes, from past the last one taken.
    let mut buf = [0u8; 4096];
    let mut found = [0 as libc::c_int; 256];
    let mut from = u64::from(first);
    loop {
        let mut n_found = 0;
        let mut full = false;
        unsafe { libc::lseek(dir, 0, libc::SEEK_SET) };
        'read: loop {
            let n =
                unsafe { libc::syscall(libc::SYS_getdents64, dir, buf.as_mut_ptr(), buf.len()) };
            if n <= 0 {
                break;
            }
            let mut at = 0usize;
            while at < n as usize {
                // linux_dirent64: ino (8), off (8), reclen (2), type (1), name.
                let reclen = u16::from_ne_bytes([buf[at + 16], buf[at + 17]]) as usize;
                let name = &buf[at + 19..at + reclen];
                at += reclen;
                let mut digits = name.iter().take_while(|b| b.is_ascii_digit());
                let Some(fd) = digits.try_fold(0u64, |n, &b| {
                    n.checked_mul(10)?.checked_add(u64::from(b - b'0'))
                }) else {
                    continue;
                };
                if !name[0].is_ascii_digit()
                    || fd < from
                    || fd > u64::from(last)
                    || fd == dir as u64
                {
                    continue;
                }
                if n_found == found.len() {
                    full = true;
                    break 'read;
                }
                found[n_found] = fd as libc::c_int;
                n_found += 1;
            }
        }
        found[..n_found].iter().for_each(|&fd| f(fd));
        match found[..n_found].iter().max() {
            Some(&top) if full => from = top as u64 + 1,
            _ => break,
        }
    }
    unsafe { libc::close(dir) };
    Ok(())
}

pub(crate) fn pidfd_send_signal(pidfd: RawFd, sig: libc::c_int) -> io::Result<()> {
    cvt_long(unsafe {
        libc::syscall(
            libc::SYS_pidfd_send_signal,
            pidfd,
            sig,
            std::ptr::null::<libc::siginfo_t>(),
            0,
        )
    })
    .map(drop)
}

/// A pipe, both ends close-on-exec: (read, write).
pub(crate) fn pipe() -> io::Result<(OwnedFd, OwnedFd)> {
    let mut fds = [0; 2];
    cvt(unsafe { libc::pipe2(fds.as_mut_ptr(), libc::O_CLOEXEC) })?;
    Ok(unsafe { (OwnedFd::from_raw_fd(fds[0]), OwnedFd::from_raw_fd(fds[1])) })
}

/// A connected pair of Unix stream sockets, both close-on-exec.
pub(crate) fn socketpair() -> io::Result<(OwnedFd, OwnedFd)> {
    let mut fds = [0; 2];
    cvt(unsafe {
        libc::socketpair(
            libc::AF_UNIX,
            libc::SOCK_STREAM | libc::SOCK_CLOEXEC,
            0,
            fds.as_mut_ptr(),
        )
    })?;
    Ok(unsafe { (OwnedFd::from_raw_fd(fds[0]), OwnedFd::from_raw_fd(fds[1])) })
}

/// The same descriptor at `min` or above, close-on-exec.
pub(crate) fn dup_above(fd: &OwnedFd, min: RawFd) -> io::Result<OwnedFd> {
    let new = cvt(unsafe { libc::fcntl(fd.as_raw_fd(), libc::F_DUPFD_CLOEXEC, min) })?;
    Ok(unsafe { OwnedFd::from_raw_fd(new) })
}

/// Sends `fd` over the Unix socket `sock` (`SCM_RIGHTS`), with one byte.
pub(crate) fn send_fd(sock: RawFd, fd: RawFd) -> io::Result<()> {
    let mut byte = *b"L";
    let mut iov = libc::iovec {
        iov_base: byte.as_mut_ptr().cast(),
        iov_len: 1,
    };
    let space = unsafe { libc::CMSG_SPACE(std::mem::size_of::<RawFd>() as u32) } as usize;
    let mut control = vec![0u8; space];
    let mut msg: libc::msghdr = unsafe { std::mem::zeroed() };
    msg.msg_iov = &mut iov;
    msg.msg_iovlen = 1;
    msg.msg_control = control.as_mut_ptr().cast();
    msg.msg_controllen = space as _;
    unsafe {
        let cmsg = libc::CMSG_FIRSTHDR(&msg);
        (*cmsg).cmsg_level = libc::SOL_SOCKET;
        (*cmsg).cmsg_type = libc::SCM_RIGHTS;
        (*cmsg).cmsg_len = libc::CMSG_LEN(std::mem::size_of::<RawFd>() as u32) as _;
        std::ptr::write_unaligned(libc::CMSG_DATA(cmsg).cast::<RawFd>(), fd);
    }
    loop {
        let n = unsafe { libc::sendmsg(sock, &msg, libc::MSG_NOSIGNAL) };
        if n == 1 {
            return Ok(());
        }
        let e = io::Error::last_os_error();
        if n == -1 && e.kind() == io::ErrorKind::Interrupted {
            continue;
        }
        return Err(if n == -1 {
            e
        } else {
            io::Error::other("short send")
        });
    }
}

/// Receives one descriptor sent by `send_fd`, close-on-exec.
pub(crate) fn recv_fd(sock: RawFd) -> io::Result<OwnedFd> {
    let mut byte = [0u8];
    let mut iov = libc::iovec {
        iov_base: byte.as_mut_ptr().cast(),
        iov_len: 1,
    };
    let space = unsafe { libc::CMSG_SPACE(std::mem::size_of::<RawFd>() as u32) } as usize;
    let mut control = vec![0u8; space];
    let mut msg: libc::msghdr = unsafe { std::mem::zeroed() };
    msg.msg_iov = &mut iov;
    msg.msg_iovlen = 1;
    msg.msg_control = control.as_mut_ptr().cast();
    msg.msg_controllen = space as _;
    let n = loop {
        let n = unsafe { libc::recvmsg(sock, &mut msg, libc::MSG_CMSG_CLOEXEC) };
        if n == -1 && io::Error::last_os_error().kind() == io::ErrorKind::Interrupted {
            continue;
        }
        break n;
    };
    if n == -1 {
        return Err(io::Error::last_os_error());
    }
    unsafe {
        let cmsg = libc::CMSG_FIRSTHDR(&msg);
        if n != 1
            || cmsg.is_null()
            || (*cmsg).cmsg_level != libc::SOL_SOCKET
            || (*cmsg).cmsg_type != libc::SCM_RIGHTS
        {
            return Err(io::Error::other("no descriptor arrived"));
        }
        let fd = std::ptr::read_unaligned(libc::CMSG_DATA(cmsg).cast::<RawFd>());
        Ok(OwnedFd::from_raw_fd(fd))
    }
}

/// The errno of the last failed call, for code that may not allocate.
pub(crate) fn errno() -> libc::c_int {
    unsafe { *libc::__errno_location() }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Where the host refuses `close_range` (ENOSYS, or an older profile's
    /// EPERM), it marks and closes the range one descriptor at a time, and
    /// nothing outside it (theseus-f7tz).
    #[test]
    fn close_range_marks_and_closes_one_by_one_where_the_host_refuses_it() {
        for refusal in [libc::ENOSYS, libc::EPERM] {
            std::thread::spawn(move || {
                crate::seccomp::refuse_here(&[(libc::SYS_close_range, refusal)]).unwrap();
                let null = std::fs::File::open("/dev/null").unwrap();
                // Four descriptors in a row, high enough to be this test's.
                let fds: Vec<RawFd> = (0..4)
                    .map(|i| cvt(unsafe { libc::dup2(null.as_raw_fd(), 700 + i) }).unwrap())
                    .collect();
                let flags = |fd| unsafe { libc::fcntl(fd, libc::F_GETFD) };
                close_range(701, 702, true).unwrap();
                assert_eq!(
                    fds.iter().map(|&fd| flags(fd)).collect::<Vec<_>>(),
                    [0, 1, 1, 0]
                );
                close_range(701, 702, false).unwrap();
                assert_eq!(
                    fds.iter().map(|&fd| flags(fd)).collect::<Vec<_>>(),
                    [0, -1, -1, 0]
                );
                assert!(unsafe { libc::syscall(libc::SYS_close_range, 700, 700, 0) } == -1);
                assert_eq!(errno(), refusal, "the host's refusal stood in");
                unsafe { libc::close(700) };
                unsafe { libc::close(703) };
            })
            .join()
            .unwrap();
        }
    }
}
