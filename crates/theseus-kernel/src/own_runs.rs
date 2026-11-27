//! The daemon's own L1 runs (issue 63): a command a thread of the daemon starts in L1 and waits for itself
//! (`probe_run`: a self build's gate, a watch's look, a replay), instead of a job wrapper's. Such a run's thread
//! is on the runtime's blocking pool, and a runtime's drop waits for every thread of its pool: a run that does not
//! end by itself held a stop or a restart for as long as it ran (a self build's gate, half an hour).
//!
//! The daemon's stop asks every run here to end ([`end_all`]): each one's wait stops its job, every process of it,
//! as at its deadline (`job_l1::stop`: SIGTERM to its init, then the kill of its pid namespace), and returns. The
//! ask is kept, so a run that starts after it ends at its first look. Each wait polls the ask's pipe beside its init's
//! pidfd, and the pipe is never emptied: once written, every wait wakes at once.

use std::os::fd::{AsRawFd, OwnedFd, RawFd};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

/// The ask: 0, none; otherwise the grace in milliseconds, plus one.
static ASKED: AtomicU64 = AtomicU64::new(0);

/// The ask's pipe, made at the first run: its read end in every wait's poll, a byte in it once asked.
static PIPE: OnceLock<Option<(OwnedFd, OwnedFd)>> = OnceLock::new();

/// The runs going now, in the order they started.
static RUNNING: Mutex<Vec<Run>> = Mutex::new(Vec::new());

/// One run going now: what it is, and its init's pid.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Run {
    /// Its kind, as its caller names it (`self-build`, `watch`, `replay`).
    pub kind: String,
    /// Its init's pid, in the daemon's pid namespace.
    pub pid: u32,
}

impl Run {
    /// It in a few words: its kind and its short id, the init's pid.
    pub fn words(&self) -> String {
        format!("the {} run (job {})", self.kind, self.pid)
    }
}

fn pipe() -> Option<&'static (OwnedFd, OwnedFd)> {
    PIPE.get_or_init(|| {
        let mut fds = [0 as libc::c_int; 2];
        // SAFETY: pipe2 fills the two descriptors, which the OwnedFds own from here on.
        if unsafe { libc::pipe2(fds.as_mut_ptr(), libc::O_CLOEXEC | libc::O_NONBLOCK) } == -1 {
            return None;
        }
        use std::os::fd::FromRawFd;
        // SAFETY: two new descriptors, each owned once.
        Some(unsafe { (OwnedFd::from_raw_fd(fds[0]), OwnedFd::from_raw_fd(fds[1])) })
    })
    .as_ref()
}

/// Ask every run, going now or started later, to end within `grace` of its next look.
pub fn end_all(grace: Duration) {
    let ms = u64::try_from(grace.as_millis()).unwrap_or(u64::MAX - 1);
    let _ = ASKED.compare_exchange(
        0,
        ms.saturating_add(1).min(u64::MAX - 1),
        Ordering::SeqCst,
        Ordering::SeqCst,
    );
    if let Some((_, w)) = pipe() {
        // SAFETY: one byte written to the pipe's nonblocking write end; a full pipe already wakes every wait.
        let _ = unsafe { libc::write(w.as_raw_fd(), [1u8].as_ptr().cast(), 1) };
    }
}

/// The grace a run has to end in, once asked.
pub(crate) fn asked() -> Option<Duration> {
    match ASKED.load(Ordering::SeqCst) {
        0 => None,
        ms => Some(Duration::from_millis(ms - 1)),
    }
}

/// Whether this daemon's stop has asked its own runs to end: a run that ended since is no result to record.
pub fn stopping() -> bool {
    asked().is_some()
}

/// The pipe a wait polls beside its init's pidfd, once a run has made it: readable once asked. A wrapper process
/// starts no run of its own and never has one.
pub(crate) fn wake_fd() -> Option<RawFd> {
    PIPE.get()?.as_ref().map(|(r, _)| r.as_raw_fd())
}

/// The runs going now.
pub fn running() -> Vec<Run> {
    RUNNING.lock().map(|r| r.clone()).unwrap_or_default()
}

/// A run, listed while it is held.
pub(crate) struct Held(Run);

/// List `kind`'s run, whose init is `pid`, until the returned guard drops.
pub(crate) fn hold(kind: &str, pid: u32) -> Held {
    let run = Run {
        kind: kind.to_string(),
        pid,
    };
    let _ = pipe();
    if let Ok(mut r) = RUNNING.lock() {
        r.push(run.clone());
    }
    Held(run)
}

impl Drop for Held {
    fn drop(&mut self) {
        if let Ok(mut r) = RUNNING.lock() {
            if let Some(i) = r.iter().position(|x| *x == self.0) {
                r.remove(i);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A run is listed while held, by its kind and its init, and gone once its guard drops.
    #[test]
    fn a_run_is_listed_while_it_is_held() {
        let a = hold("self-build", 4242);
        assert!(running().contains(&Run {
            kind: "self-build".into(),
            pid: 4242
        }));
        assert_eq!(
            running().last().map(Run::words).as_deref(),
            Some("the self-build run (job 4242)")
        );
        drop(a);
        assert!(!running().iter().any(|r| r.pid == 4242));
    }

    /// Once asked, the ask holds its grace, and the pipe a wait polls is readable and stays so.
    #[test]
    fn the_ask_holds_its_grace_and_wakes_every_poll() {
        assert_eq!(asked(), None);
        end_all(Duration::from_millis(1500));
        assert_eq!(asked(), Some(Duration::from_millis(1500)));
        let _held = hold("watch", 1);
        let fd = wake_fd().expect("the pipe");
        for _ in 0..2 {
            let mut p = libc::pollfd {
                fd,
                events: libc::POLLIN,
                revents: 0,
            };
            // SAFETY: one poll over one live descriptor.
            assert_eq!(unsafe { libc::poll(&mut p, 1, 0) }, 1, "readable");
        }
    }
}
