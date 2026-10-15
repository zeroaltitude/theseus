//! The chat, the real binary, on a pseudo-terminal (theseus-cyha): the first
//! screen, a bracketed paste of 60 lines becoming one message, and the
//! terminal put back after SIGTERM, a panic and a second Ctrl-C. A fake
//! daemon on a Unix socket answers the chat's start and records what it is
//! sent. The screen is read back by `vt100`.

use std::io::{BufRead, BufReader, Read, Write};
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::os::unix::net::UnixListener;
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde_json::{json, Value};

const TUI: &str = env!("CARGO_BIN_EXE_theseus-tui");

/// A fake daemon: answers the chat's start, records every request.
struct Daemon {
    requests: Arc<Mutex<Vec<Value>>>,
}

impl Daemon {
    fn start(socket: &Path) -> Self {
        let listener = UnixListener::bind(socket).unwrap();
        let requests = Arc::new(Mutex::new(Vec::new()));
        let seen = requests.clone();
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(stream) = stream else { break };
                let seen = seen.clone();
                std::thread::spawn(move || {
                    let mut out = stream.try_clone().unwrap();
                    for line in BufReader::new(stream).lines() {
                        let Ok(line) = line else { break };
                        let req: Value = serde_json::from_str(&line).unwrap();
                        seen.lock().unwrap().push(req.clone());
                        let result = match req["method"].as_str().unwrap_or_default() {
                            "health" => json!({"model": "claude-sonnet-5-5", "profile": "p"}),
                            "session.open" => json!({
                                "session_id": "ses_0123456789abcdef", "kind": "conversation",
                                "created_at_unix_ms": 1, "turns": 0,
                            }),
                            "session.list" => json!({"sessions": []}),
                            _ => json!({}),
                        };
                        let reply = json!({"jsonrpc": "2.0", "id": req["id"], "result": result});
                        if writeln!(out, "{reply}").is_err() {
                            break;
                        }
                    }
                });
            }
        });
        Self { requests }
    }

    fn asked(&self, method: &str) -> Vec<Value> {
        self.requests
            .lock()
            .unwrap()
            .iter()
            .filter(|r| r["method"] == method)
            .map(|r| r["params"].clone())
            .collect()
    }
}

/// The chat on a terminal of 80 by 24.
struct Chat {
    child: Child,
    master: std::fs::File,
    screen: vt100::Parser,
    raw: Vec<u8>,
    _dir: tempfile::TempDir,
    daemon: Daemon,
}

impl Chat {
    fn start(args: &[&str]) -> Self {
        let dir = tempfile::tempdir().unwrap();
        let socket = dir.path().join("t.sock");
        let daemon = Daemon::start(&socket);
        let (mut m, mut s) = (0, 0);
        let size = libc::winsize {
            ws_row: 24,
            ws_col: 80,
            ws_xpixel: 0,
            ws_ypixel: 0,
        };
        // SAFETY: openpty writes two descriptors, which are ours to own.
        let rc = unsafe {
            libc::openpty(
                &mut m,
                &mut s,
                std::ptr::null_mut(),
                std::ptr::null(),
                &size,
            )
        };
        assert_eq!(rc, 0);
        // SAFETY: fresh descriptors from openpty.
        let (master, slave) = unsafe { (std::fs::File::from_raw_fd(m), OwnedFd::from_raw_fd(s)) };
        let child = Command::new(TUI)
            .arg("--socket")
            .arg(&socket)
            .args(args)
            .env("TERM", "xterm-256color")
            .env("HOME", dir.path())
            .env("XDG_STATE_HOME", dir.path().join("state"))
            .env("NO_COLOR", "1")
            .env_remove("KITTY_WINDOW_ID")
            .current_dir(dir.path())
            .stdin(Stdio::from(slave.try_clone().unwrap()))
            .stdout(Stdio::from(slave))
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        // Reads must not block the test: the master is read non-blocking.
        // SAFETY: fcntl on a descriptor we own.
        unsafe {
            let fl = libc::fcntl(master.as_raw_fd(), libc::F_GETFL);
            libc::fcntl(master.as_raw_fd(), libc::F_SETFL, fl | libc::O_NONBLOCK);
        }
        Self {
            child,
            master,
            screen: vt100::Parser::new(24, 80, 0),
            raw: Vec::new(),
            _dir: dir,
            daemon,
        }
    }

    /// Read what the chat wrote, for up to `ms`.
    fn drain(&mut self, ms: u64) {
        let end = Instant::now() + Duration::from_millis(ms);
        let mut buf = [0u8; 8192];
        while Instant::now() < end {
            match self.master.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => {
                    self.raw.extend_from_slice(&buf[..n]);
                    self.screen.process(&buf[..n]);
                }
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    std::thread::sleep(Duration::from_millis(5));
                }
                Err(_) => break,
            }
        }
    }

    fn until(&mut self, what: &str, cond: impl Fn(&Chat) -> bool) {
        let end = Instant::now() + Duration::from_secs(10);
        while Instant::now() < end {
            self.drain(30);
            if cond(self) {
                return;
            }
        }
        panic!(
            "timed out waiting for {what}; the screen:\n{}",
            self.screen.screen().contents()
        );
    }

    fn shows(&mut self, text: &str) {
        let t = text.to_string();
        self.until(&format!("the screen to show {t:?}"), move |c| {
            c.screen.screen().contents().contains(&t)
        });
    }

    fn send(&mut self, bytes: &[u8]) {
        self.master.write_all(bytes).unwrap();
    }

    /// Whether the terminal is in line mode with echo, as a shell leaves it.
    fn cooked(&self) -> bool {
        // SAFETY: tcgetattr into a zeroed termios, on a descriptor we own.
        unsafe {
            let mut t: libc::termios = std::mem::zeroed();
            assert_eq!(libc::tcgetattr(self.master.as_raw_fd(), &mut t), 0);
            t.c_lflag & libc::ICANON != 0 && t.c_lflag & libc::ECHO != 0
        }
    }

    fn raw_now(&self) -> bool {
        !self.cooked()
    }

    fn wait_exit(&mut self) -> Option<std::process::ExitStatus> {
        let end = Instant::now() + Duration::from_secs(10);
        while Instant::now() < end {
            self.drain(20);
            if let Some(s) = self.child.try_wait().unwrap() {
                self.drain(100);
                return Some(s);
            }
        }
        None
    }
}

impl Drop for Chat {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn restored(raw: &[u8]) -> bool {
    // Bracketed paste turned off, and the cursor shown, after it was turned on.
    let text = String::from_utf8_lossy(raw);
    let on = text.rfind("\x1b[?2004h");
    let off = text.rfind("\x1b[?2004l");
    matches!((on, off), (Some(a), Some(b)) if b > a) && text.contains("\x1b[?25h")
}

#[test]
fn the_first_screen_names_the_model_and_the_directory_and_takes_input() {
    let mut c = Chat::start(&["chat"]);
    c.shows("? help");
    let screen = c.screen.screen().contents();
    assert!(screen.starts_with("theseus · Sonnet 5.5 · "), "{screen}");
    assert!(screen.contains("$0.00"), "the status line: {screen}");
    assert!(c.raw_now(), "raw mode while the chat runs");
    c.until("the session to be opened", |c| {
        !c.daemon.asked("session.open").is_empty()
    });
    let open = c.daemon.asked("session.open");
    assert_eq!(open.len(), 1);
    assert!(
        open[0]["dir"].as_str().unwrap().len() > 1,
        "the directory rode in: {open:?}"
    );
}

#[test]
fn a_bracketed_paste_of_sixty_lines_is_one_chip_and_one_message() {
    let mut c = Chat::start(&["chat"]);
    c.shows("? help");
    let log: String = (1..=60)
        .map(|n| format!("{n:02} 12:{n:02}:07 line\n"))
        .collect();
    c.send(format!("\x1b[200~{log}\x1b[201~").as_bytes());
    c.shows("[pasted 60 lines");
    assert!(
        !c.screen.screen().contents().contains("12:30:07"),
        "folded, not shown"
    );
    c.send(b"what happened?\r");
    c.until("the message", |c| !c.daemon.asked("turn.submit").is_empty());
    std::thread::sleep(Duration::from_millis(200));
    let sent = c.daemon.asked("turn.submit");
    assert_eq!(sent.len(), 1, "one message: {sent:?}");
    assert_eq!(sent[0]["input"], format!("{log}what happened?"));
}

#[test]
fn sigterm_puts_the_terminal_back() {
    let mut c = Chat::start(&["chat"]);
    c.shows("? help");
    assert!(c.raw_now());
    // SAFETY: a signal to a child we started.
    unsafe { libc::kill(c.child.id() as i32, libc::SIGTERM) };
    let status = c.wait_exit().expect("the chat ends on SIGTERM");
    assert!(status.success(), "{status:?}");
    assert!(restored(&c.raw), "bracketed paste off and the cursor shown");
    assert!(c.cooked(), "the terminal is back in line mode");
}

#[test]
fn sighup_puts_the_terminal_back() {
    let mut c = Chat::start(&["chat"]);
    c.shows("? help");
    // SAFETY: a signal to a child we started.
    unsafe { libc::kill(c.child.id() as i32, libc::SIGHUP) };
    assert!(c.wait_exit().is_some());
    assert!(restored(&c.raw) && c.cooked());
}

#[cfg(debug_assertions)]
#[test]
fn a_panic_puts_the_terminal_back() {
    let mut c = Chat::start(&["chat"]);
    c.shows("? help");
    c.send(b"/panic\r");
    let status = c.wait_exit().expect("the chat ends on a panic");
    assert_eq!(status.code(), Some(101), "a panic: {status:?}");
    assert!(restored(&c.raw), "bracketed paste off and the cursor shown");
    assert!(c.cooked(), "the terminal is back in line mode");
}

#[test]
fn two_ctrl_c_leave_saying_the_conversation_goes_on() {
    let mut c = Chat::start(&["chat"]);
    c.shows("? help");
    c.send(b"half\x03");
    c.until("the line to clear", |c| {
        !c.screen.screen().contents().contains("half")
    });
    c.send(b"\x03");
    c.shows("ctrl+c again to leave");
    c.send(b"\x03");
    let status = c.wait_exit().expect("the second ctrl+c leaves");
    assert!(status.success());
    let text = String::from_utf8_lossy(&c.raw).into_owned();
    assert!(
        text.contains("left · the conversation goes on · theseus -c · theseus chat abcdef"),
        "{text}"
    );
    assert!(restored(&c.raw) && c.cooked());
}

#[test]
fn no_daemon_is_said_in_a_line_and_exits_3_before_the_terminal_is_touched() {
    let dir = tempfile::tempdir().unwrap();
    let out = Command::new(TUI)
        .arg("--socket")
        .arg(dir.path().join("nothing.sock"))
        .arg("chat")
        .stdin(Stdio::null())
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(3));
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(
        err.starts_with("theseus-tui: cannot reach theseusd at "),
        "{err}"
    );
    assert!(err.contains("Start it with `theseusd`"), "{err}");
    assert!(out.stdout.is_empty(), "nothing was drawn");
}
