//! The daemon's descriptors and its socket connections (theseus-7vtp): the
//! soft open-files limit raised at start, a ceiling on connections that sits
//! under it, and what an accept error does.
//!
//! A connection takes a descriptor, and so does everything else the daemon
//! holds: the store, a job's pipes, a terminal. Past the soft limit an accept
//! fails with EMFILE, and a serving loop that returns on that error ends the
//! daemon (1,017 open connections once did). So the limit is raised to the
//! hard one at start, a connection past a ceiling below it is answered with
//! one protocol error and closed, and an accept error is logged (one line a
//! second at most) and waited out for 50 ms, never returned.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use theseus_protocol::ConnectionsHealth;

/// Descriptors kept back from the ceiling for the store, jobs and terminals.
const RESERVE: u64 = 256;
/// The derived ceiling never passes this, whatever the limit.
const DERIVED_CAP: u64 = 8192;
/// How long an accept error waits before the next accept.
pub const ACCEPT_BACKOFF: Duration = Duration::from_millis(50);

/// `[server] max_connections` as it acts: the configured ceiling, or, at 0,
/// the soft limit less a reserve (half of it where the limit is under 512),
/// capped. Never below 1.
pub fn ceiling_for(configured: u64, soft_limit: u64) -> u64 {
    if configured > 0 {
        return configured;
    }
    let reserve = RESERVE.min(soft_limit / 2);
    soft_limit.saturating_sub(reserve).clamp(1, DERIVED_CAP)
}

/// The limit in effect, as `RLIMIT_NOFILE` says: `(soft, hard)`.
pub fn nofile_limits() -> (u64, u64) {
    let mut r = libc::rlimit {
        rlim_cur: 0,
        rlim_max: 0,
    };
    // SAFETY: `r` is a valid out-pointer for the call's duration.
    if unsafe { libc::getrlimit(libc::RLIMIT_NOFILE, &mut r) } != 0 {
        return (0, 0);
    }
    (r.rlim_cur, r.rlim_max)
}

/// Raise the soft `RLIMIT_NOFILE` to the hard limit, once, at start. Returns
/// what it was `(soft, hard)` and what it is now. The kernel caps the soft
/// limit at `fs.nr_open`, which can sit under an "unlimited" hard one, so a
/// refused raise tries a step down before it gives up.
pub fn raise_nofile() -> ((u64, u64), u64) {
    let (soft, hard) = nofile_limits();
    if soft == 0 || soft >= hard {
        return ((soft, hard), soft);
    }
    let mut want = hard;
    loop {
        let r = libc::rlimit {
            rlim_cur: want,
            rlim_max: hard,
        };
        // SAFETY: `r` is a valid in-pointer for the call's duration.
        if unsafe { libc::setrlimit(libc::RLIMIT_NOFILE, &r) } == 0 {
            return ((soft, hard), want);
        }
        // Over `nr_open`: halve toward what the kernel allows.
        want = soft + (want - soft) / 2;
        if want <= soft {
            return ((soft, hard), soft);
        }
    }
}

/// The socket connections held now, those refused at the ceiling, and the
/// limits they sit under. One counter: no lock on the accept path.
#[derive(Debug)]
pub struct Connections {
    held: AtomicU64,
    refused: AtomicU64,
    ceiling: AtomicU64,
    soft: AtomicU64,
    hard: AtomicU64,
}

impl Default for Connections {
    fn default() -> Self {
        Self {
            held: AtomicU64::new(0),
            refused: AtomicU64::new(0),
            ceiling: AtomicU64::new(u64::MAX),
            soft: AtomicU64::new(0),
            hard: AtomicU64::new(0),
        }
    }
}

/// A held connection's place under the ceiling, given back when it drops.
#[derive(Debug)]
pub struct Held(Arc<Connections>);

impl Drop for Held {
    fn drop(&mut self) {
        self.0.held.fetch_sub(1, Ordering::AcqRel);
    }
}

impl Connections {
    /// Set once at start, before the loop serves: the limits in effect and
    /// the ceiling they give (`ceiling_for`).
    pub fn set(&self, ceiling: u64, soft: u64, hard: u64) {
        self.ceiling.store(ceiling, Ordering::Relaxed);
        self.soft.store(soft, Ordering::Relaxed);
        self.hard.store(hard, Ordering::Relaxed);
    }

    /// A place for one more connection, or the ceiling it ran into.
    pub fn admit(self: &Arc<Self>) -> Result<Held, u64> {
        let ceiling = self.ceiling.load(Ordering::Relaxed);
        let before = self.held.fetch_add(1, Ordering::AcqRel);
        if before >= ceiling {
            self.held.fetch_sub(1, Ordering::AcqRel);
            self.refused.fetch_add(1, Ordering::Relaxed);
            return Err(ceiling);
        }
        Ok(Held(self.clone()))
    }

    pub fn status(&self) -> ConnectionsHealth {
        ConnectionsHealth {
            held: self.held.load(Ordering::Relaxed),
            ceiling: self.ceiling.load(Ordering::Relaxed),
            refused: self.refused.load(Ordering::Relaxed),
            fd_soft: self.soft.load(Ordering::Relaxed),
            fd_hard: self.hard.load(Ordering::Relaxed),
        }
    }
}

/// The one protocol frame a refused connection is told, as a line.
pub fn refusal_line(ceiling: u64) -> String {
    let frame = serde_json::json!({
        "jsonrpc": "2.0",
        "id": null,
        "error": {
            "code": theseus_protocol::error_code::LIMIT,
            "message": format!(
                "the daemon holds {ceiling} connections, its ceiling; close one and retry"
            ),
            "data": {"ceiling": ceiling},
        },
    });
    format!("{frame}\n")
}

/// What an accept loop does with an error: a warning, one a second at most
/// with the count of those it left out, then a wait of `ACCEPT_BACKOFF`.
/// The loop goes on; nothing here returns an error.
#[derive(Debug)]
pub struct AcceptErrors {
    /// Which listener, for the log line.
    what: &'static str,
    last: Mutex<Option<Instant>>,
    unsaid: AtomicU64,
    total: AtomicU64,
}

impl AcceptErrors {
    pub fn new(what: &'static str) -> Self {
        Self {
            what,
            last: Mutex::new(None),
            unsaid: AtomicU64::new(0),
            total: AtomicU64::new(0),
        }
    }

    /// Errors seen since the daemon started.
    pub fn total(&self) -> u64 {
        self.total.load(Ordering::Relaxed)
    }

    /// Note one error; true when this one was logged.
    pub fn note(&self, e: &std::io::Error) -> bool {
        self.total.fetch_add(1, Ordering::Relaxed);
        let now = Instant::now();
        let mut last = self.last.lock().unwrap_or_else(|p| p.into_inner());
        if last.is_some_and(|t| now.duration_since(t) < Duration::from_secs(1)) {
            self.unsaid.fetch_add(1, Ordering::Relaxed);
            return false;
        }
        *last = Some(now);
        let left_out = self.unsaid.swap(0, Ordering::Relaxed);
        tracing::warn!(
            listener = self.what,
            error = %e,
            errno = ?e.raw_os_error(),
            left_out,
            "accept failed; waiting 50 ms and serving on"
        );
        true
    }

    /// `note`, then the wait.
    pub async fn wait(&self, e: &std::io::Error) {
        self.note(e);
        tokio::time::sleep(ACCEPT_BACKOFF).await;
    }
}

/// The next accepted connection: an accept error is noted and waited out
/// (`AcceptErrors::wait`) and the accept tried again, so a serving loop that
/// takes its connections here never returns on one.
pub async fn accept_until_ok<T, F, Fut>(errors: &AcceptErrors, mut accept: F) -> T
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = std::io::Result<T>>,
{
    loop {
        match accept().await {
            Ok(t) => return t,
            Err(e) => errors.wait(&e).await,
        }
    }
}

/// Tell a connection past the ceiling why, and close it. A task does it, so
/// the accept loop never waits on a client; what the client sent is read and
/// dropped (for at most 250 ms) so the close is a FIN, not a reset that could
/// take the frame with it.
pub fn turn_away(stream: tokio::net::UnixStream, ceiling: u64) {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    tokio::spawn(async move {
        let mut io = stream;
        let line = refusal_line(ceiling);
        let _ = tokio::time::timeout(Duration::from_millis(250), async move {
            let _ = io.write_all(line.as_bytes()).await;
            let _ = io.shutdown().await;
            let mut sink = [0u8; 1024];
            while matches!(io.read(&mut sink).await, Ok(n) if n > 0) {}
        })
        .await;
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_derived_ceiling_sits_under_the_limit_and_never_passes_its_cap() {
        assert_eq!(ceiling_for(0, 1024), 768);
        assert_eq!(ceiling_for(0, 300), 150);
        assert_eq!(ceiling_for(0, 65536), DERIVED_CAP);
        assert_eq!(ceiling_for(0, 1), 1);
        assert_eq!(ceiling_for(5, 1024), 5);
    }

    #[test]
    fn past_the_ceiling_a_connection_is_refused_and_counted_and_a_drop_gives_its_place_back() {
        let c = Arc::new(Connections::default());
        c.set(2, 1024, 4096);
        let a = c.admit().unwrap();
        let _b = c.admit().unwrap();
        assert_eq!(c.admit().unwrap_err(), 2);
        assert_eq!(c.admit().unwrap_err(), 2);
        let s = c.status();
        assert_eq!((s.held, s.refused, s.ceiling), (2, 2, 2));
        assert_eq!((s.fd_soft, s.fd_hard), (1024, 4096));
        drop(a);
        assert!(c.admit().is_ok());
        assert_eq!(c.status().refused, 2);
    }

    #[test]
    fn the_refusal_is_one_protocol_error_line_that_says_why() {
        let line = refusal_line(768);
        assert!(line.ends_with('\n'));
        let v: serde_json::Value = serde_json::from_str(line.trim_end()).unwrap();
        assert_eq!(v["error"]["code"], theseus_protocol::error_code::LIMIT);
        assert_eq!(v["error"]["data"]["ceiling"], 768);
        assert_eq!(
            v["error"]["message"],
            "the daemon holds 768 connections, its ceiling; close one and retry"
        );
    }

    #[test]
    fn the_soft_limit_is_raised_to_the_hard_one() {
        let (_, hard) = nofile_limits();
        let ((_, _), now) = raise_nofile();
        // Or as far as the kernel allows: never lower than before.
        assert!(now >= 1 && (now == hard || nofile_limits().0 == now));
    }

    #[tokio::test(start_paused = true)]
    async fn an_accept_error_is_logged_once_a_second_and_the_loop_goes_on() {
        let errors = AcceptErrors::new("test");
        let calls = std::sync::atomic::AtomicU64::new(0);
        let got = accept_until_ok(&errors, || {
            let n = calls.fetch_add(1, Ordering::Relaxed);
            async move {
                if n < 30 {
                    Err(std::io::Error::from_raw_os_error(libc::EMFILE))
                } else {
                    Ok(n)
                }
            }
        })
        .await;
        // The error seen 30 times; the clock ran 30 x 50 ms = 1.5 s, so the
        // warning came at the first and once more past the second.
        assert_eq!(got, 30);
        assert_eq!(errors.total(), 30);
        assert!(errors.unsaid.load(Ordering::Relaxed) < 30);
    }

    #[test]
    fn one_error_in_a_second_logs_and_the_rest_wait_their_turn() {
        let errors = AcceptErrors::new("test");
        let e = std::io::Error::from_raw_os_error(libc::EMFILE);
        assert!(errors.note(&e));
        assert!(!errors.note(&e));
        assert!(!errors.note(&e));
        assert_eq!(errors.unsaid.load(Ordering::Relaxed), 2);
    }
}
