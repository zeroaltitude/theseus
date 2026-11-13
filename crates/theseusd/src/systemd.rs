//! Socket activation and `sd_notify` (theseus-r8yj, row B7).
//!
//! Two halves of the same conversation with an init system, both of them
//! protocol over environment variables and a datagram socket, so neither
//! needs a dependency: systemd's own documentation (`sd_notify(3)`,
//! `sd_listen_fds(3)`) is the whole specification.
//!
//! **READY, at the listener.** `Type=exec` meant `systemctl start theseusd`
//! returned when the *process* existed, which is before the store is open and
//! before anything can answer on the socket. A unit of `Type=notify` returns
//! when the daemon says `READY=1`, and the daemon says it at the one honest
//! point: once the RPC listener is up, so a client that connects will be
//! served (the maintainer's decision, 1.0.6: "READY when the RPC listener
//! accepts"). Not at process start, and not at the store's open — an open
//! store that is not yet listening still refuses every client.
//!
//! **WATCHDOG, only if asked.** `WATCHDOG_USEC` is systemd's own configured
//! interval (`WatchdogSec=` in the unit); the daemon pings at half of it, as
//! the documentation asks. **Nothing in the units this binary installs sets
//! `WatchdogSec=`**, so no daemon gets a watchdog unless its operator asks
//! for one in a drop-in: a watchdog is systemd killing and restarting a
//! daemon it judges hung, and that is not a default this row takes on its
//! own. With the variable unset, [`Notify::watchdog`] does nothing and the
//! ping task is never spawned.
//!
//! **LISTEN_FDS, the passed socket.** With socket activation the listening
//! socket is made by systemd and handed over as file descriptor
//! [`SD_LISTEN_FDS_START`]; the daemon adopts it instead of binding one of
//! its own, and leaves its path alone at the stop, because systemd owns it.
//!
//! **One read, at the start, and then the variables are gone.** Everything
//! here is read by [`Systemd::from_env`], once, from the start path, which
//! removes all five names ([`VARS`]) as it reads them: left in place they
//! would be inherited by every job the daemon launches, and a job that sees
//! `LISTEN_FDS` could take the daemon's own listener for its own.

use std::os::fd::{FromRawFd, RawFd};
use std::time::Duration;

use anyhow::{Context, Result};

/// The first file descriptor systemd passes (`SD_LISTEN_FDS_START`).
pub(crate) const SD_LISTEN_FDS_START: RawFd = 3;

/// Every variable the init system sets for this protocol. All five are
/// removed by [`Systemd::from_env`] as it reads the four it uses; the census
/// `every_variable_this_module_reads_is_listed` holds the list to the code.
pub(crate) const VARS: [&str; 5] = [
    "LISTEN_PID",
    "LISTEN_FDS",
    "LISTEN_FDNAMES",
    "NOTIFY_SOCKET",
    "WATCHDOG_USEC",
];

/// What the init system told this start: the socket it passed, if any, and
/// where to report. Read once, by [`Systemd::from_env`].
pub(crate) struct Systemd {
    /// The listening socket systemd passed, adopted as it is. `None` when
    /// this start was not socket-activated, and then the caller binds its
    /// own.
    pub(crate) listener: Option<std::os::unix::net::UnixListener>,
    /// Shared, because the watchdog's ping task holds it for as long as the
    /// daemon serves.
    pub(crate) notify: std::sync::Arc<Notify>,
}

/// Whether this process was started with `LISTEN_FDS` meant for it, and how
/// many descriptors it was given. Pure, so the whole table is a unit test:
/// `LISTEN_PID` must name this process (systemd sets it so that a child which
/// inherited the variables does not adopt its parent's socket), and
/// `LISTEN_FDS` must parse to at least one.
fn listen_fds_of(listen_pid: Option<&str>, listen_fds: Option<&str>, me: u32) -> Option<u32> {
    let pid: u32 = listen_pid?.trim().parse().ok()?;
    if pid != me {
        return None;
    }
    let n: u32 = listen_fds?.trim().parse().ok()?;
    (n >= 1).then_some(n)
}

impl Systemd {
    /// Read the environment once and clear it. Call this from the start path,
    /// before the runtime's workers and before any child exists: that is what
    /// makes the removal safe and what keeps the variables out of every job.
    ///
    /// It never fails the start on its own: a notify socket that cannot be
    /// opened is a warning and nothing is reported, since a start that can
    /// serve should serve. A passed descriptor that is *not* a listening Unix
    /// socket is the one error returned, because adopting it would leave the
    /// daemon serving nothing.
    pub(crate) fn from_env() -> Result<Systemd> {
        let listen_pid = var("LISTEN_PID");
        let listen_fds = var("LISTEN_FDS");
        let notify_socket = var("NOTIFY_SOCKET");
        let watchdog_usec = var("WATCHDOG_USEC");
        // `LISTEN_FDNAMES` is read by nothing here (one socket needs no name)
        // and is cleared with the rest, so no job inherits it either.
        for v in VARS {
            std::env::remove_var(v);
        }
        let listener = match listen_fds_of(
            listen_pid.as_deref(),
            listen_fds.as_deref(),
            std::process::id(),
        ) {
            Some(1) => Some(adopt()?),
            // More than one is refused rather than guessed at: this daemon
            // listens on one socket, and picking the first of several would
            // be a guess about a unit nobody has written.
            Some(n) => anyhow::bail!(
                "systemd passed {n} listening sockets; theseusd listens on one, so name one \
                 socket unit for it"
            ),
            None => None,
        };
        Ok(Systemd {
            listener,
            notify: std::sync::Arc::new(Notify::of(
                notify_socket.as_deref(),
                watchdog_usec.as_deref(),
            )),
        })
    }
}

/// Take descriptor [`SD_LISTEN_FDS_START`] as the listener. It arrives
/// blocking and without `FD_CLOEXEC`: both are wrong here, so it is set
/// close-on-exec, so a job never inherits the daemon's listener, and
/// non-blocking, which tokio's registration requires.
fn adopt() -> Result<std::os::unix::net::UnixListener> {
    cloexec(SD_LISTEN_FDS_START).context("the passed socket's close-on-exec flag")?;
    nonblocking(SD_LISTEN_FDS_START).context("the passed socket's non-blocking flag")?;
    // SAFETY: systemd passed this descriptor to this process; nothing else in
    // this binary touches it; `from_env` runs once on the start path, before
    // any child, and has already removed the variables that name it, so no
    // second call can take it again.
    let listener = unsafe { std::os::unix::net::UnixListener::from_raw_fd(SD_LISTEN_FDS_START) };
    listener
        .local_addr()
        .context("the descriptor systemd passed is not a listening Unix socket")?;
    Ok(listener)
}

/// The init system's notification socket.
///
/// Unconnected when `NOTIFY_SOCKET` is unset — every start that is not an
/// init system's — and then every method here does nothing at all, which is
/// what keeps this module out of the way of a daemon started from a shell.
pub(crate) struct Notify {
    socket: Option<std::os::unix::net::UnixDatagram>,
    /// `WATCHDOG_USEC`, systemd's own configured interval.
    watchdog_usec: Option<u64>,
}

impl Notify {
    fn of(addr: Option<&str>, watchdog_usec: Option<&str>) -> Notify {
        let socket = addr.and_then(|a| match connect(a) {
            Ok(s) => Some(s),
            Err(e) => {
                tracing::warn!(error = %format!("{e:#}"),
                    "the init system's notify socket could not be opened: nothing is reported to it");
                None
            }
        });
        Notify {
            socket,
            watchdog_usec: watchdog_usec.and_then(|u| u.trim().parse().ok()),
        }
    }

    /// Send one line, best effort: a notification nobody is listening for is
    /// not worth a start's failure, and a `Type=notify` unit whose READY is
    /// lost fails on its own timeout, where the operator can see it.
    fn send(&self, line: &str) {
        let Some(s) = &self.socket else { return };
        if let Err(e) = s.send(line.as_bytes()) {
            tracing::debug!(error = %e, line, "the init system did not take a notification");
        }
    }

    /// `READY=1`: the daemon can answer. Said once the RPC listener is up,
    /// never before.
    pub(crate) fn ready(&self, status: &str) {
        self.send(&format!("READY=1\nSTATUS={}\n", one_line(status)));
    }

    /// `STOPPING=1`: a clean stop has begun, so systemd does not wait out its
    /// timeout and does not read the exit as a crash.
    pub(crate) fn stopping(&self) {
        self.send("STOPPING=1\nSTATUS=stopping\n");
    }

    /// How often `WATCHDOG=1` must be sent: half of systemd's own interval,
    /// as `sd_notify(3)` asks. `None` when the unit asked for no watchdog,
    /// which is every unit this binary installs.
    pub(crate) fn watchdog_every(&self) -> Option<Duration> {
        let usec = self.watchdog_usec.filter(|u| *u > 0)?;
        Some(Duration::from_micros(usec / 2))
    }

    /// One `WATCHDOG=1`.
    pub(crate) fn watchdog(&self) {
        if self.watchdog_usec.is_some() {
            self.send("WATCHDOG=1\n");
        }
    }
}

/// A status line: `sd_notify`'s protocol is newline-separated assignments, so
/// a newline inside a value would read as another assignment.
fn one_line(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Connect to `NOTIFY_SOCKET`: a path, or an abstract-namespace name when it
/// begins with `@`, which is what systemd uses for a user manager.
fn connect(addr: &str) -> Result<std::os::unix::net::UnixDatagram> {
    use std::os::linux::net::SocketAddrExt;
    let s = std::os::unix::net::UnixDatagram::unbound()?;
    // Non-blocking, so no notification can ever hold a runtime worker: a full
    // receive buffer gives `WouldBlock`, which `send` logs and drops.
    s.set_nonblocking(true)?;
    match addr.strip_prefix('@') {
        Some(name) => {
            let a = std::os::unix::net::SocketAddr::from_abstract_name(name.as_bytes())?;
            s.connect_addr(&a)?;
        }
        None => s.connect(addr)?,
    }
    Ok(s)
}

fn var(name: &str) -> Option<String> {
    std::env::var(name).ok().filter(|v| !v.is_empty())
}

/// Set `FD_CLOEXEC` on a descriptor, so the daemon's children never hold it.
fn cloexec(fd: RawFd) -> Result<()> {
    let flags = unsafe { libc::fcntl(fd, libc::F_GETFD) };
    if flags < 0 {
        return Err(std::io::Error::last_os_error().into());
    }
    if unsafe { libc::fcntl(fd, libc::F_SETFD, flags | libc::FD_CLOEXEC) } < 0 {
        return Err(std::io::Error::last_os_error().into());
    }
    Ok(())
}

/// Set `O_NONBLOCK`, which tokio's registration of the listener requires.
fn nonblocking(fd: RawFd) -> Result<()> {
    let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
    if flags < 0 {
        return Err(std::io::Error::last_os_error().into());
    }
    if unsafe { libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) } < 0 {
        return Err(std::io::Error::last_os_error().into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `LISTEN_FDS` is this process's only when `LISTEN_PID` names it: the
    /// whole point of that variable is that a child which inherited the pair
    /// does not adopt its parent's socket.
    #[test]
    fn listen_fds_are_this_processs_only_when_listen_pid_names_it() {
        assert_eq!(listen_fds_of(Some("41"), Some("1"), 41), Some(1));
        assert_eq!(listen_fds_of(Some("41"), Some("3"), 41), Some(3));
        // A child's inheritance: the pid is its parent's.
        assert_eq!(listen_fds_of(Some("41"), Some("1"), 42), None);
        // Nothing passed, or nonsense passed.
        assert_eq!(listen_fds_of(None, Some("1"), 41), None);
        assert_eq!(listen_fds_of(Some("41"), None, 41), None);
        assert_eq!(listen_fds_of(Some("41"), Some("0"), 41), None);
        assert_eq!(listen_fds_of(Some("x"), Some("1"), 41), None);
        assert_eq!(listen_fds_of(Some("41"), Some("x"), 41), None);
        // Whitespace, as a shell-set variable can carry.
        assert_eq!(listen_fds_of(Some(" 41 "), Some(" 2 "), 41), Some(2));
    }

    /// With no `NOTIFY_SOCKET` nothing is connected, every send does nothing,
    /// and no watchdog is asked for: a daemon started from a shell behaves
    /// exactly as it did before this row.
    #[test]
    fn a_start_outside_an_init_system_notifies_nothing() {
        let n = Notify::of(None, None);
        assert!(n.socket.is_none() && n.watchdog_every().is_none());
        n.ready("serving");
        n.watchdog();
        n.stopping();
    }

    /// The ping interval is half systemd's own, and a unit that asked for no
    /// watchdog gets none — which is every unit this binary installs.
    #[test]
    fn the_watchdog_pings_at_half_the_interval_systemd_set() {
        let of = |usec: Option<&str>| Notify::of(None, usec).watchdog_every();
        assert_eq!(of(Some("30000000")), Some(Duration::from_secs(15)));
        assert_eq!(of(Some("1000000")), Some(Duration::from_millis(500)));
        assert_eq!(of(Some("0")), None);
        assert_eq!(of(Some("not a number")), None);
        assert_eq!(of(None), None);
    }

    /// A status cannot break the protocol's newline separation: a line break
    /// inside a value would read as another assignment.
    #[test]
    fn a_status_cannot_break_the_protocols_lines() {
        assert_eq!(one_line("serving on\nthe socket"), "serving on the socket");
        assert_eq!(one_line("  two   words  "), "two words");
    }

    /// A real notify socket takes the lines, in order, byte for byte: the one
    /// test that proves the protocol rather than the wording. Its `@` form is
    /// proved beside it, since that is what a user manager uses.
    #[test]
    fn the_lines_reach_a_real_notify_socket() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("notify");
        let listening = std::os::unix::net::UnixDatagram::bind(&path).unwrap();
        let n = Notify::of(path.to_str(), Some("20000000"));
        assert!(n.socket.is_some(), "the notify socket did not open");
        n.ready("serving on a socket");
        n.watchdog();
        n.stopping();
        let mut got = Vec::new();
        for _ in 0..3 {
            let mut buf = [0u8; 256];
            let read = listening.recv(&mut buf).unwrap();
            got.push(String::from_utf8(buf[..read].to_vec()).unwrap());
        }
        assert_eq!(
            got,
            vec![
                "READY=1\nSTATUS=serving on a socket\n".to_string(),
                "WATCHDOG=1\n".to_string(),
                "STOPPING=1\nSTATUS=stopping\n".to_string(),
            ]
        );
    }

    /// The abstract-namespace form, which a systemd user manager passes.
    #[test]
    fn an_abstract_notify_socket_is_connected_too() {
        use std::os::linux::net::SocketAddrExt;
        let name = format!("theseus-notify-test-{}", std::process::id());
        let addr = std::os::unix::net::SocketAddr::from_abstract_name(name.as_bytes()).unwrap();
        let listening = std::os::unix::net::UnixDatagram::bind_addr(&addr).unwrap();
        let n = Notify::of(Some(&format!("@{name}")), None);
        assert!(
            n.socket.is_some(),
            "the abstract notify socket did not open"
        );
        n.ready("serving");
        let mut buf = [0u8; 256];
        let read = listening.recv(&mut buf).unwrap();
        assert_eq!(
            String::from_utf8(buf[..read].to_vec()).unwrap(),
            "READY=1\nSTATUS=serving\n"
        );
    }

    /// Every name this module reads is on [`VARS`], and the removal is the
    /// one loop over [`VARS`]: a name read outside the list would survive
    /// into every job's environment, which is the bug this census exists to
    /// stop.
    #[test]
    fn every_variable_this_module_reads_is_listed() {
        let src = include_str!("systemd.rs");
        assert_eq!(VARS.len(), 5);
        for v in VARS {
            assert!(
                src.contains(&format!("\"{v}\"")),
                "{v} is not in the source"
            );
        }
        // Four of the five have a reader; `LISTEN_FDNAMES` is cleared only.
        let readers = VARS
            .iter()
            .filter(|v| src.contains(&format!("var(\"{v}\")")))
            .count();
        assert_eq!(readers, 4, "a reader was added or lost");
        assert!(!src.contains("var(\"LISTEN_FDNAMES\")"));
        // `std::env::var` is called in exactly one place, `fn var`, and
        // `remove_var` in exactly one, the loop above. The needles are
        // assembled so this test's own source does not match itself.
        let reader = concat!("std::env::", "var(");
        let remover = concat!("std::env::", "remove_var(");
        assert_eq!(src.matches(reader).count(), 1, "{reader} is called twice");
        assert_eq!(src.matches(remover).count(), 1, "{remover} is called twice");
    }
}
