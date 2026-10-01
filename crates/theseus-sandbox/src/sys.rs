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
pub(crate) fn close_range(first: u32, last: u32, cloexec: bool) -> io::Result<()> {
    const CLOSE_RANGE_CLOEXEC: libc::c_uint = 1 << 2;
    let flags = if cloexec { CLOSE_RANGE_CLOEXEC } else { 0 };
    cvt_long(unsafe { libc::syscall(libc::SYS_close_range, first, last, flags) }).map(drop)
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
