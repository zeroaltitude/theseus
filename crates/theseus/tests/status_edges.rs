//! The edges of `theseus status` that a scripted daemon can't reach by answering
//! (theseus-lweh review): a daemon that connects and never answers, a socket
//! file left by a dead daemon, and the terminal tab cleared when a signal ends
//! `--watch --tab`, on a pseudo-terminal that is the child's controlling one.

use std::io::{BufRead, BufReader, Read, Write};
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::os::unix::net::UnixListener;
use std::os::unix::process::CommandExt;
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

const THESEUS: &str = env!("CARGO_BIN_EXE_theseus");

fn theseus(sock: &Path, args: &[&str]) -> Command {
    let mut c = Command::new(THESEUS);
    c.arg("--socket")
        .arg(sock)
        .args(args)
        .env_remove("THESEUS_SOCKET")
        .env_remove("THESEUS_SESSION")
        .env_remove("TMUX")
        // The seen file (the diamond, and what the long form records) is the
        // test's own, beside its socket, never the machine's.
        .env("XDG_STATE_HOME", sock.parent().unwrap().join("state"));
    c
}

/// A daemon that connects and never answers: `--short` ends silently with exit
/// 1 after its 400 ms (theseus-lweh's review: a prompt or a status bar runs it
/// on every draw; it waited 2 s until then), and the long form, which a person
/// asked for, after its two seconds, saying so on stderr.
#[test]
fn a_daemon_that_never_answers_ends_short_silently_with_exit_1_after_two_seconds() {
    let dir = tempfile::tempdir().unwrap();
    let sock = dir.path().join("sock");
    // Listening, so the connect succeeds; nothing accepts or reads.
    let _listener = UnixListener::bind(&sock).unwrap();
    let t = Instant::now();
    let out = theseus(&sock, &["status", "--short"]).output().unwrap();
    let took = t.elapsed();
    assert_eq!(out.status.code(), Some(1));
    assert!(out.stdout.is_empty() && out.stderr.is_empty(), "{out:?}");
    // The process's own start and end are inside `took`: the upper bound tells
    // the 400 ms wait from the old 2 s one, not the wait's precision.
    assert!(
        took >= Duration::from_millis(380) && took < Duration::from_millis(1900),
        "took {took:?}"
    );
    // The long form waits its two seconds, and says so, on stderr.
    let t = Instant::now();
    let out = theseus(&sock, &["status"]).output().unwrap();
    assert!(
        t.elapsed() >= Duration::from_millis(1900),
        "took {:?}",
        t.elapsed()
    );
    assert_eq!(out.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&out.stderr).contains("did not answer"));
}

#[test]
fn a_socket_file_left_by_a_dead_daemon_is_exit_3_at_once() {
    let dir = tempfile::tempdir().unwrap();
    let sock = dir.path().join("sock");
    drop(UnixListener::bind(&sock).unwrap());
    assert!(sock.exists());
    let t = Instant::now();
    let out = theseus(&sock, &["status", "--short"]).output().unwrap();
    assert_eq!(out.status.code(), Some(3));
    assert!(out.stdout.is_empty() && out.stderr.is_empty(), "{out:?}");
    assert!(t.elapsed() < Duration::from_millis(500));
}

/// A pseudo-terminal: the master to read what the child writes to its
/// controlling terminal, and the slave to make that terminal.
fn pty() -> (std::fs::File, OwnedFd) {
    let (mut m, mut s) = (0, 0);
    // SAFETY: openpty fills two fds we own from here on.
    let rc = unsafe {
        libc::openpty(
            &mut m,
            &mut s,
            std::ptr::null_mut(),
            std::ptr::null(),
            std::ptr::null(),
        )
    };
    assert_eq!(rc, 0);
    // SAFETY: both are fresh, valid fds.
    unsafe { (std::fs::File::from_raw_fd(m), OwnedFd::from_raw_fd(s)) }
}

/// A daemon that answers `executions.watch` with one working task and holds the
/// connection open.
fn working_daemon(sock: &Path) -> std::thread::JoinHandle<()> {
    let listener = UnixListener::bind(sock).unwrap();
    std::thread::spawn(move || {
        let Ok((s, _)) = listener.accept() else {
            return;
        };
        let mut line = String::new();
        BufReader::new(&s).read_line(&mut line).unwrap();
        let req: serde_json::Value = serde_json::from_str(&line).unwrap();
        let view = serde_json::json!({
            "position": 11, "at_ms": 1, "execution_id": "exe_a", "session_id": "ses_a",
            "kind": "task", "state": "running", "pending": [], "turns": 1,
            "attention": {"level": "working", "label": "turn 1", "since_ms": 1},
        });
        let result = serde_json::json!({"position": 10, "executions": [view],
            "confirms": [], "total": 1});
        let answer = serde_json::json!({"jsonrpc": "2.0", "id": req["id"], "result": result});
        (&s).write_all(format!("{answer}\n").as_bytes()).unwrap();
        std::thread::sleep(Duration::from_secs(5));
    })
}

/// `status --short --watch --tab`, in a new session whose controlling terminal
/// is the pty's slave.
fn watch_on_a_tty(sock: &Path, slave: &OwnedFd) -> Child {
    let fd = slave.as_raw_fd();
    let mut c = theseus(sock, &["status", "--short", "--watch", "--tab"]);
    c.stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    // SAFETY: only async-signal-safe calls between fork and exec.
    unsafe {
        c.pre_exec(move || {
            if libc::setsid() < 0 || libc::ioctl(fd, libc::TIOCSCTTY, 0) < 0 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }
    c.spawn().unwrap()
}

#[test]
fn a_signal_ends_watch_tab_with_the_tab_cleared() {
    for sig in [libc::SIGINT, libc::SIGTERM, libc::SIGHUP] {
        let dir = tempfile::tempdir().unwrap();
        let sock = dir.path().join("sock");
        let _daemon = working_daemon(&sock);
        let (mut master, slave) = pty();
        let mut child = watch_on_a_tty(&sock, &slave);
        drop(slave);
        // Working: the ring is on.
        std::thread::sleep(Duration::from_millis(800));
        // SAFETY: the child's pid is ours.
        unsafe { libc::kill(child.id() as i32, sig) };
        let status = child.wait().unwrap();
        let mut got = Vec::new();
        // The child is gone and so is the slave: the master reads to its end.
        let _ = master.read_to_end(&mut got);
        let got = String::from_utf8_lossy(&got);
        let (ring, clear) = (got.find("\x1b]9;4;3;0\x07"), got.rfind("\x1b]9;4;0;0\x07"));
        assert!(
            matches!((ring, clear), (Some(r), Some(c)) if r < c),
            "signal {sig}: want the ring, then the tab cleared: {got:?}"
        );
        assert_eq!(status.code(), Some(0), "signal {sig}: {status:?}");
    }
}
