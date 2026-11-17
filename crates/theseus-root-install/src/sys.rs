//! The few system calls the helper makes, wrapped. After the first open of a directory every name is resolved
//! relative to a descriptor and with `O_NOFOLLOW`, so a path component that is replaced afterwards changes nothing
//! the helper holds.

use std::ffi::CString;
use std::fs::File;
use std::io;
use std::os::fd::{FromRawFd, OwnedFd, RawFd};

fn cstr(name: &str) -> io::Result<CString> {
    CString::new(name).map_err(|_| io::Error::from_raw_os_error(libc::EINVAL))
}

fn cvt(r: libc::c_int) -> io::Result<libc::c_int> {
    if r < 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(r)
    }
}

/// The anchor of a walk: opened by its whole path once, then never by path again.
pub fn open_path(path: &str, flags: libc::c_int) -> io::Result<OwnedFd> {
    let c = cstr(path)?;
    // SAFETY: `c` is a NUL-terminated string that outlives the call.
    let fd = cvt(unsafe { libc::open(c.as_ptr(), flags | libc::O_CLOEXEC) })?;
    // SAFETY: `open` returned a new descriptor that nothing else owns.
    Ok(unsafe { OwnedFd::from_raw_fd(fd) })
}

pub fn openat(dir: RawFd, name: &str, flags: libc::c_int, mode: u32) -> io::Result<OwnedFd> {
    let c = cstr(name)?;
    // SAFETY: as `open_path`; `dir` is a descriptor the caller holds.
    let fd = cvt(unsafe {
        libc::openat(
            dir,
            c.as_ptr(),
            flags | libc::O_CLOEXEC,
            mode as libc::c_uint,
        )
    })?;
    // SAFETY: a new descriptor that nothing else owns.
    Ok(unsafe { OwnedFd::from_raw_fd(fd) })
}

pub fn file(fd: OwnedFd) -> File {
    File::from(fd)
}

pub fn fstat(fd: RawFd) -> io::Result<libc::stat> {
    // SAFETY: `st` is zeroed memory of the right type, filled by the call.
    let mut st: libc::stat = unsafe { std::mem::zeroed() };
    cvt(unsafe { libc::fstat(fd, &raw mut st) })?;
    Ok(st)
}

/// `lstat` of `name` in `dir`: the link itself, never what it points at. None when it does not exist.
pub fn lstat_at(dir: RawFd, name: &str) -> io::Result<Option<libc::stat>> {
    let c = cstr(name)?;
    // SAFETY: as `fstat`.
    let mut st: libc::stat = unsafe { std::mem::zeroed() };
    let r = unsafe { libc::fstatat(dir, c.as_ptr(), &raw mut st, libc::AT_SYMLINK_NOFOLLOW) };
    if r == 0 {
        return Ok(Some(st));
    }
    let e = io::Error::last_os_error();
    if e.raw_os_error() == Some(libc::ENOENT) {
        Ok(None)
    } else {
        Err(e)
    }
}

pub fn mkdirat(dir: RawFd, name: &str, mode: u32) -> io::Result<()> {
    let c = cstr(name)?;
    // SAFETY: as `openat`.
    cvt(unsafe { libc::mkdirat(dir, c.as_ptr(), mode as libc::mode_t) }).map(drop)
}

pub fn unlinkat(dir: RawFd, name: &str, remove_dir: bool) -> io::Result<()> {
    let c = cstr(name)?;
    let flags = if remove_dir { libc::AT_REMOVEDIR } else { 0 };
    // SAFETY: as `openat`.
    cvt(unsafe { libc::unlinkat(dir, c.as_ptr(), flags) }).map(drop)
}

/// A rename that fails, and changes nothing, when `to` exists (`RENAME_NOREPLACE`).
pub fn rename_noreplace(dir: RawFd, from: &str, to: &str) -> io::Result<()> {
    let (f, t) = (cstr(from)?, cstr(to)?);
    // SAFETY: both names are NUL-terminated and outlive the call; the flag is the kernel's own.
    let r = unsafe {
        libc::syscall(
            libc::SYS_renameat2,
            dir,
            f.as_ptr(),
            dir,
            t.as_ptr(),
            libc::RENAME_NOREPLACE,
        )
    };
    if r < 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}

pub fn fchmod(fd: RawFd, mode: u32) -> io::Result<()> {
    // SAFETY: a descriptor the caller holds.
    cvt(unsafe { libc::fchmod(fd, mode as libc::mode_t) }).map(drop)
}

pub fn fchown(fd: RawFd, uid: u32, gid: u32) -> io::Result<()> {
    // SAFETY: as `fchmod`.
    cvt(unsafe { libc::fchown(fd, uid, gid) }).map(drop)
}

pub fn fsync(fd: RawFd) -> io::Result<()> {
    // SAFETY: as `fchmod`.
    cvt(unsafe { libc::fsync(fd) }).map(drop)
}

pub fn euid() -> u32 {
    // SAFETY: no arguments, no failure.
    unsafe { libc::geteuid() }
}

pub fn set_umask(mask: u32) {
    // SAFETY: no failure.
    unsafe { libc::umask(mask as libc::mode_t) };
}

/// The names in a directory, but `.` and `..`, sorted. Read from a duplicate of the descriptor.
pub fn list(fd: RawFd) -> io::Result<Vec<String>> {
    // SAFETY: `dup` of a descriptor the caller holds; `fdopendir` takes the duplicate and `closedir` closes it.
    let dup = cvt(unsafe { libc::fcntl(fd, libc::F_DUPFD_CLOEXEC, 0) })?;
    let dir = unsafe { libc::fdopendir(dup) };
    if dir.is_null() {
        let e = io::Error::last_os_error();
        // SAFETY: `fdopendir` failed, so the duplicate is still ours.
        unsafe { libc::close(dup) };
        return Err(e);
    }
    let mut names = Vec::new();
    loop {
        // SAFETY: `dir` is open; `readdir` returns null at the end, and the entry it returns is valid until the
        // next call, which comes after the name is copied.
        let ent = unsafe { libc::readdir(dir) };
        if ent.is_null() {
            break;
        }
        let name = unsafe { std::ffi::CStr::from_ptr((*ent).d_name.as_ptr()) };
        let name = name.to_string_lossy().into_owned();
        if name != "." && name != ".." {
            names.push(name);
        }
    }
    // SAFETY: closes `dir` and the duplicate with it.
    unsafe { libc::closedir(dir) };
    names.sort();
    Ok(names)
}

/// One line to the system log, as an authentication-facility notice.
pub fn syslog(line: &str) {
    let Ok(c) = CString::new(line.replace('\0', " ")) else {
        return;
    };
    let ident = b"theseus-root-install\0";
    // SAFETY: `ident` is static and NUL-terminated (openlog keeps the pointer); the format is a literal `%s`.
    unsafe {
        libc::openlog(ident.as_ptr().cast(), libc::LOG_PID, libc::LOG_AUTH);
        libc::syslog(
            libc::LOG_AUTH | libc::LOG_NOTICE,
            c"%s".as_ptr(),
            c.as_ptr(),
        );
        libc::closelog();
    }
}
