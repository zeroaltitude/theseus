//! Socket activation and `sd_notify`, against the real binary
//! (theseus-r8yj, row B7).
//!
//! The unit tests of the protocol itself are `src/systemd.rs`'s. These two
//! prove the thing that matters about a `Type=notify` daemon, which no unit
//! test can: that `READY=1` arrives, and that **the daemon answers on its
//! socket the moment it does** — not when the process exists. And that a
//! socket this test bound and passed as file descriptor 3, as a systemd
//! socket unit does, is the socket the daemon serves on, with none of its
//! own bound.
//!
//! `LISTEN_PID` must hold the daemon's own pid, and a pid is not known before
//! the fork, so the daemon is started the way a unit would start it:
//! `sh -c 'LISTEN_PID=$$ … exec theseusd …'`. `exec` keeps the shell's pid,
//! so the variable names the daemon. The descriptor is put on 3 in a
//! `pre_exec`, with `dup2`, which allocates nothing.

mod common;

use std::io::{BufRead, Write};
use std::os::fd::IntoRawFd;
use std::os::unix::net::{UnixDatagram, UnixListener, UnixStream};
use std::os::unix::process::CommandExt;
use std::path::Path;
use std::process::Command;
use std::time::Duration;

use common::{safe_note, Daemon};

/// How long a notification may take to arrive on a loaded machine. A missing
/// one fails the test on this timeout rather than hanging it.
const WAIT: Duration = Duration::from_secs(30);

/// A daemon started as a unit of `Type=notify` would start it.
struct Started {
    dir: tempfile::TempDir,
    daemon: Daemon,
    /// Where the daemon reports: every line it sent, in order.
    notify: UnixDatagram,
}

impl Started {
    /// `pass_socket`: bind the daemon's socket here first and hand it over as
    /// descriptor 3, as a socket unit does. `watchdog_usec`: what
    /// `WatchdogSec=` would set.
    fn new(pass_socket: bool, watchdog_usec: Option<&str>) -> Started {
        use std::os::unix::fs::PermissionsExt;
        let theseusd = std::path::PathBuf::from(env!("CARGO_BIN_EXE_theseusd"));
        let dir = tempfile::tempdir().unwrap();
        let path = |p: &str| dir.path().join(p);
        std::fs::create_dir_all(path("bin")).unwrap();
        std::fs::create_dir_all(path("projects")).unwrap();
        std::fs::write(path("bin/op"), "#!/bin/sh\nexit 1\n").unwrap();
        std::fs::set_permissions(path("bin/op"), std::fs::Permissions::from_mode(0o755)).unwrap();
        std::fs::write(
            path("config.toml"),
            safe_note(&theseusd, &path("projects"), 100.0),
        )
        .unwrap();
        // The notify socket is the init system's end; the daemon connects to
        // it and sends. A short name, since a Unix path has about 100 bytes.
        let notify = UnixDatagram::bind(path("n")).unwrap();
        notify.set_read_timeout(Some(WAIT)).unwrap();
        let log = std::fs::File::create(path("theseusd.log")).unwrap();

        // `exec` keeps the shell's pid, so `$$` is the daemon's own.
        let mut script = String::from("LISTEN_PID=$$");
        if pass_socket {
            script.push_str(" LISTEN_FDS=1");
        }
        script.push_str(" exec \"$0\" --config \"$1\" --state-dir \"$2\" --socket \"$3\"");
        let mut cmd = Command::new("sh");
        cmd.arg("-c")
            .arg(&script)
            .arg(&theseusd)
            .arg(path("config.toml"))
            .arg(path("state"))
            .arg(path("sock"))
            .env(
                "PATH",
                format!(
                    "{}:{}",
                    path("bin").display(),
                    std::env::var("PATH").unwrap_or_default()
                ),
            )
            .env("OP_SERVICE_ACCOUNT_TOKEN", "test-not-a-token")
            .env("NOTIFY_SOCKET", path("n"))
            .env_remove("THESEUS_OP_TOKEN_FILE")
            .env_remove("THESEUS_CONFIG")
            .env_remove("THESEUS_STATE_DIR")
            .env_remove("THESEUS_SOCKET")
            .env_remove("THESEUS_OPERATOR_UMASK")
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(log);
        match watchdog_usec {
            Some(u) => cmd.env("WATCHDOG_USEC", u),
            None => cmd.env_remove("WATCHDOG_USEC"),
        };
        if pass_socket {
            let fd = UnixListener::bind(path("sock")).unwrap().into_raw_fd();
            // SAFETY: `dup2` is async-signal-safe and allocates nothing,
            // which is all a `pre_exec` closure may do.
            unsafe {
                cmd.pre_exec(move || match libc::dup2(fd, 3) {
                    -1 => Err(std::io::Error::last_os_error()),
                    _ => Ok(()),
                });
            }
        }
        Started {
            dir,
            daemon: Daemon::spawn(&mut cmd),
            notify,
        }
    }

    fn path(&self, p: &str) -> std::path::PathBuf {
        self.dir.path().join(p)
    }

    fn log(&self) -> String {
        std::fs::read_to_string(self.path("theseusd.log")).unwrap_or_default()
    }

    /// The next line the daemon reported, or a panic naming the log.
    fn reported(&mut self) -> String {
        let mut buf = [0u8; 1024];
        match self.notify.recv(&mut buf) {
            Ok(n) => String::from_utf8_lossy(&buf[..n]).into_owned(),
            Err(e) => {
                if let Some(status) = self.daemon.try_wait() {
                    panic!("theseusd exited ({status}):\n{}", self.log());
                }
                panic!("nothing was reported ({e}):\n{}", self.log());
            }
        }
    }
}

/// One JSON-RPC call on a socket: the result, or the error as text. The
/// daemon's own `Served::call`, which cannot be used here because `Served`
/// starts the daemon itself.
fn call(sock: &Path, method: &str) -> Result<serde_json::Value, String> {
    let s = UnixStream::connect(sock).map_err(|e| e.to_string())?;
    s.set_read_timeout(Some(Duration::from_secs(30))).unwrap();
    let req = serde_json::json!({"jsonrpc": "2.0", "id": 1, "method": method, "params": null});
    (&s).write_all(format!("{req}\n").as_bytes())
        .map_err(|e| e.to_string())?;
    for line in std::io::BufReader::new(&s).lines() {
        let v: serde_json::Value =
            serde_json::from_str(&line.map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
        if v["id"] == 1 {
            return match v.get("error") {
                Some(e) if !e.is_null() => Err(e.to_string()),
                _ => Ok(v["result"].clone()),
            };
        }
    }
    Err("the connection closed".into())
}

/// `READY=1` means the daemon can answer: the very first `health` call after
/// it arrives is answered, with no retry loop at all. That is the whole
/// point of `Type=notify` over `Type=exec`, and the one claim a unit test
/// cannot make.
///
/// Then `WATCHDOG=1` within the interval systemd set, and `STOPPING=1` at a
/// clean stop.
#[test]
fn ready_means_the_daemon_answers_and_the_watchdog_follows() {
    // Two seconds, so the first ping is due about a second after READY.
    let mut s = Started::new(false, Some("2000000"));
    let ready = s.reported();
    assert!(
        ready.starts_with("READY=1\nSTATUS=serving on "),
        "{ready:?}\n{}",
        s.log()
    );
    assert!(ready.ends_with("/sock\n"), "{ready:?}");
    // No loop, no sleep: if READY were sent before the listener was up, this
    // would fail, which is the regression this test exists to catch.
    let health = call(&s.path("sock"), "health")
        .unwrap_or_else(|e| panic!("the daemon said READY and did not answer: {e}\n{}", s.log()));
    assert_eq!(health["name"], "theseus", "{health}");

    // The ping, within the interval. `reported` waits out `WAIT` before it
    // fails, so a slow machine does not redden this.
    let ping = s.reported();
    assert_eq!(ping, "WATCHDOG=1\n", "{}", s.log());

    // A clean stop says so, so systemd waits out no timeout.
    call(&s.path("sock"), "shutdown").unwrap();
    let stopping = loop {
        let line = s.reported();
        if line != "WATCHDOG=1\n" {
            break line;
        }
    };
    assert_eq!(stopping, "STOPPING=1\nSTATUS=stopping\n", "{}", s.log());
}

/// A socket bound here and passed as descriptor 3 is the socket the daemon
/// serves on: it binds none of its own, and it leaves the path alone at its
/// stop, because a socket unit owns it.
///
/// The positive half is the answer on that very socket — the one this test
/// made, with its own inode — so an empty or replaced socket cannot pass.
#[test]
fn the_socket_systemd_passed_is_the_one_it_serves_on() {
    let mut s = Started::new(true, None);
    let inode = |p: &std::path::Path| {
        use std::os::unix::fs::MetadataExt;
        std::fs::metadata(p).unwrap().ino()
    };
    let before = inode(&s.path("sock"));
    let ready = s.reported();
    assert!(ready.starts_with("READY=1"), "{ready:?}\n{}", s.log());
    // The daemon answers on the socket this test bound, which it did not
    // unlink and re-bind: same path, same inode.
    let health = call(&s.path("sock"), "health")
        .unwrap_or_else(|e| panic!("the passed socket did not answer: {e}\n{}", s.log()));
    assert_eq!(health["name"], "theseus", "{health}");
    assert_eq!(before, inode(&s.path("sock")), "the daemon re-bound it");
    // And the daemon says so itself: its `socket` startup phase records that
    // this start was activated rather than bound.
    let phase = health["startup"]
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["name"] == "socket")
        .expect("the socket phase")
        .clone();
    assert_eq!(
        phase["detail"]["activated"],
        serde_json::json!(true),
        "{phase}"
    );

    // And a passed socket is not the daemon's to remove at its stop.
    call(&s.path("sock"), "shutdown").unwrap();
    let deadline = std::time::Instant::now() + WAIT;
    while s.daemon.try_wait().is_none() {
        assert!(
            std::time::Instant::now() < deadline,
            "no exit:\n{}",
            s.log()
        );
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(
        s.path("sock").exists(),
        "the daemon removed the socket systemd owns:\n{}",
        s.log()
    );
}
