//! Waking a follower: inotify on the WAL's directory, and a timer as the
//! backstop. A write to a segment, a new segment, or a cut tail wakes it at
//! once; the timer covers a filesystem that sends no events. Nothing polls.

use std::ffi::CString;
use std::io;
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

/// Why [`Waker::wait`] returned.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Wake {
    /// The directory changed: a segment was written, created, or cut, or the
    /// directory itself appeared.
    Changed,
    /// The backstop passed with no change seen.
    Timer,
    /// Another thread asked ([`Kicker::kick`]): a rebuild, or a stop.
    Kicked,
}

/// Wakes a [`Waker`] from another thread (an eventfd).
#[derive(Clone)]
pub struct Kicker(Arc<OwnedFd>);

impl Kicker {
    pub fn kick(&self) {
        let one = 1u64.to_ne_bytes();
        // SAFETY: eight bytes from a live buffer, the size an eventfd takes.
        // A full counter (never, at one per kick) would only fail the write,
        // and the waker is awake then anyway.
        unsafe { libc::write(self.0.as_raw_fd(), one.as_ptr().cast(), one.len()) };
    }
}

/// Writes and truncations of a segment (`IN_MODIFY`), a new one
/// (`IN_CREATE`, `IN_MOVED_TO`), its close after writing, and the directory
/// going away (a restore that replaces the store).
const MASK: u32 = libc::IN_MODIFY
    | libc::IN_CREATE
    | libc::IN_MOVED_TO
    | libc::IN_CLOSE_WRITE
    | libc::IN_DELETE_SELF
    | libc::IN_MOVE_SELF;

/// The size of `struct inotify_event` before its name.
const EVENT_HEADER: usize = 16;

/// How often a waker whose directory does not exist looks for it again.
const UNWATCHED_RETRY: Duration = Duration::from_secs(1);

pub struct Waker {
    fd: OwnedFd,
    kick: Arc<OwnedFd>,
    dir: PathBuf,
    watching: bool,
}

/// A descriptor just returned by a call that returns one or -1.
fn owned(raw: i32) -> io::Result<OwnedFd> {
    if raw < 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: `raw` was just opened, and nothing else owns it.
    Ok(unsafe { OwnedFd::from_raw_fd(raw) })
}

impl Waker {
    /// Watch `dir`. A directory that does not exist yet is watched once it
    /// does: until then [`Waker::wait`] looks for it once a second.
    pub fn new(dir: &Path) -> io::Result<Self> {
        // SAFETY: neither call takes a pointer; each returns a new
        // descriptor or -1.
        let fd = owned(unsafe { libc::inotify_init1(libc::IN_NONBLOCK | libc::IN_CLOEXEC) })?;
        let kick = owned(unsafe { libc::eventfd(0, libc::EFD_NONBLOCK | libc::EFD_CLOEXEC) })?;
        let mut w = Self {
            fd,
            kick: Arc::new(kick),
            dir: dir.to_path_buf(),
            watching: false,
        };
        w.watch();
        Ok(w)
    }

    /// A handle another thread wakes this waker with.
    pub fn kicker(&self) -> Kicker {
        Kicker(self.kick.clone())
    }

    /// Whether the directory is watched (it exists, and the watch took).
    pub fn watching(&self) -> bool {
        self.watching
    }

    fn watch(&mut self) {
        let Ok(path) = CString::new(self.dir.as_os_str().as_bytes()) else {
            return;
        };
        // SAFETY: `path` is NUL-terminated and outlives the call.
        let wd = unsafe { libc::inotify_add_watch(self.fd.as_raw_fd(), path.as_ptr(), MASK) };
        self.watching = wd >= 0;
    }

    /// Block until the directory changes or `backstop` passes. Events that
    /// came since the last wait are taken at once, so a change made between
    /// a read and this wait is never missed.
    pub fn wait(&mut self, backstop: Duration) -> io::Result<Wake> {
        if !self.watching {
            self.watch();
            if self.watching {
                // It appeared since: whatever is in it is new.
                return Ok(Wake::Changed);
            }
        }
        let mut pfds = [
            libc::pollfd {
                fd: self.fd.as_raw_fd(),
                events: libc::POLLIN,
                revents: 0,
            },
            libc::pollfd {
                fd: self.kick.as_raw_fd(),
                events: libc::POLLIN,
                revents: 0,
            },
        ];
        // Unwatched (no directory yet), it looks for the directory once a
        // second: nothing would wake it when the directory appears.
        let wait = if self.watching {
            backstop
        } else {
            backstop.min(UNWATCHED_RETRY)
        };
        let ms = i32::try_from(wait.as_millis()).unwrap_or(i32::MAX);
        // SAFETY: two valid pollfds, and the count says two.
        let n = unsafe { libc::poll(pfds.as_mut_ptr(), 2, ms) };
        if n < 0 {
            let e = io::Error::last_os_error();
            // A signal cut the wait short: look, as the timer would.
            return if e.kind() == io::ErrorKind::Interrupted {
                Ok(Wake::Timer)
            } else {
                Err(e)
            };
        }
        if n == 0 {
            return Ok(Wake::Timer);
        }
        let kicked = pfds[1].revents != 0;
        if kicked {
            let mut count = [0u8; 8];
            // SAFETY: `count` is valid for writes of eight bytes, the size an
            // eventfd reads; it resets the counter.
            unsafe {
                libc::read(
                    self.kick.as_raw_fd(),
                    count.as_mut_ptr().cast(),
                    count.len(),
                )
            };
        }
        if pfds[0].revents != 0 {
            self.drain()?;
            return Ok(Wake::Changed);
        }
        Ok(if kicked { Wake::Kicked } else { Wake::Timer })
    }

    /// Take every queued event. A removed watch (the directory deleted or
    /// moved) is watched again on the next wait.
    fn drain(&mut self) -> io::Result<()> {
        let mut buf = [0u8; 4096];
        loop {
            // SAFETY: `buf` is valid for writes of its whole length.
            let n = unsafe { libc::read(self.fd.as_raw_fd(), buf.as_mut_ptr().cast(), buf.len()) };
            if n < 0 {
                let e = io::Error::last_os_error();
                match e.kind() {
                    io::ErrorKind::WouldBlock => return Ok(()),
                    io::ErrorKind::Interrupted => continue,
                    _ => return Err(e),
                }
            }
            let n = n as usize;
            if n == 0 {
                return Ok(());
            }
            let mut i = 0usize;
            while i + EVENT_HEADER <= n {
                let field = |at: usize| {
                    u32::from_ne_bytes([buf[at], buf[at + 1], buf[at + 2], buf[at + 3]])
                };
                let mask = field(i + 4);
                let name_len = field(i + 12) as usize;
                if mask & (libc::IN_IGNORED | libc::IN_DELETE_SELF | libc::IN_MOVE_SELF) != 0 {
                    self.watching = false;
                }
                i += EVENT_HEADER + name_len;
            }
        }
    }
}
