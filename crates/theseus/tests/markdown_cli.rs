//! A reply drawn as markdown on a terminal, and not anywhere else (theseus-rn7s):
//! the real binary against a scripted daemon, with stdout a pseudo-terminal or a
//! pipe. A pipe, `NO_COLOR` and `--plain` print the text as it came, with no
//! escape sequence; a terminal gets the heading, the emphasis, the list and the
//! code drawn, wrapped at words at `COLUMNS`.

use std::io::{BufRead, BufReader, Read, Write};
use std::os::fd::{FromRawFd, OwnedFd};
use std::os::unix::net::UnixListener;
use std::process::{Command, Stdio};

use serde_json::{json, Value};

const THESEUS: &str = env!("CARGO_BIN_EXE_theseus");
const S: &str = "ses_q7f3k2";
const T: &str = "turn_m4p8z1";

const REPLY: &str = "# Port Wenlow tides\n\n\
A **small library** for an invented harbour's tide table, which reads `HH:MM height` lines and finds the waters.\n\n\
- one *unit test* each\n- a slow survey\n\n```rust\nlet x = 1;\n```\n";

/// How the child's stdout is attached, and its environment.
struct Run<'a> {
    tty: bool,
    env: &'a [(&'a str, &'a str)],
    args: &'a [&'a str],
}

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

/// Runs the CLI against a daemon that answers each request of `script`
/// (method, result, notifications before it) in order. Returns what the child
/// wrote to its stdout, the terminal's carriage returns taken off.
fn run(r: &Run, script: Vec<(&'static str, Value, Vec<Value>)>) -> String {
    let dir = tempfile::tempdir().unwrap();
    let sock = dir.path().join("sock");
    let listener = UnixListener::bind(&sock).unwrap();
    let daemon = std::thread::spawn(move || {
        let (s, _) = listener.accept().unwrap();
        let mut reader = BufReader::new(&s);
        let mut script = script.into_iter();
        s.set_read_timeout(Some(std::time::Duration::from_millis(400)))
            .unwrap();
        let mut line = String::new();
        while reader.read_line(&mut line).unwrap_or(0) > 0 {
            let req: Value = serde_json::from_str(&line).unwrap();
            line.clear();
            let method = req["method"].as_str().unwrap_or("?").to_string();
            let mut out = String::new();
            let result = if method == "session.wait" {
                json!({"reached": "timeout", "already": false, "confirms": []})
            } else {
                let (want, result, before) = script.next().expect("a request past the script");
                assert_eq!(method, want);
                for n in before {
                    out.push_str(&format!("{n}\n"));
                }
                result
            };
            out.push_str(&format!(
                "{}\n",
                json!({"jsonrpc": "2.0", "id": req["id"], "result": result})
            ));
            (&s).write_all(out.as_bytes()).unwrap();
        }
    });
    let mut cmd = Command::new(THESEUS);
    cmd.arg("--socket")
        .arg(&sock)
        .args(r.args)
        .env("XDG_STATE_HOME", dir.path().join("state"))
        .env_remove("THESEUS_SOCKET")
        .env_remove("THESEUS_SESSION")
        .env_remove("NO_COLOR")
        .env("TERM", "xterm-256color")
        .env("TZ", "<-07>7")
        .stdin(Stdio::null())
        .stderr(Stdio::null());
    for (k, v) in r.env {
        cmd.env(k, v);
    }
    let got = if r.tty {
        let (mut master, slave) = pty();
        let mut child = cmd.stdout(Stdio::from(slave)).spawn().unwrap();
        child.wait().unwrap();
        // The child and its copy of the slave are gone: the master reads to its end.
        drop(cmd);
        let mut buf = Vec::new();
        let _ = master.read_to_end(&mut buf);
        String::from_utf8_lossy(&buf).replace("\r\n", "\n")
    } else {
        let out = cmd.stdout(Stdio::piped()).output().unwrap();
        String::from_utf8_lossy(&out.stdout).into_owned()
    };
    daemon.join().unwrap();
    got
}

fn result(output: &str) -> Value {
    json!({"session_id": S, "turn_id": T, "loops": 1, "output": output, "stop_reason": "end_turn",
           "provider_stop_reason": "end_turn", "model": "glm-x", "provider": "zed", "profile": "quick",
           "usage": {"input_tokens": 10, "output_tokens": 5}, "elapsed_ms": 900, "cost_usd": 0.001,
           "tool_calls": 0, "continuation": false, "cites": []})
}

fn deltas(text: &str, piece: usize) -> Vec<Value> {
    let chars: Vec<char> = text.chars().collect();
    chars
        .chunks(piece)
        .map(|c| {
            let t: String = c.iter().collect();
            json!({"jsonrpc": "2.0", "method": "model.delta",
                   "params": {"turn_id": T, "loop_index": 0, "text": t}})
        })
        .collect()
}

fn ask() -> Vec<(&'static str, Value, Vec<Value>)> {
    vec![("turn.submit", result(REPLY), deltas(REPLY, 7))]
}

#[test]
fn a_pipe_gets_the_text_as_it_came_with_no_escape() {
    let out = run(
        &Run {
            tty: false,
            env: &[("COLUMNS", "60")],
            args: &["ask", "tides?"],
        },
        ask(),
    );
    assert_eq!(out, REPLY);
    assert!(!out.contains('\x1b'));
}

#[test]
fn a_terminal_gets_the_reply_drawn_at_its_width() {
    let out = run(
        &Run {
            tty: true,
            env: &[("COLUMNS", "60")],
            args: &["ask", "tides?"],
        },
        ask(),
    );
    assert!(
        out.starts_with("\x1b[1;4mPort Wenlow tides\x1b[0m\n\n"),
        "{out:?}"
    );
    assert!(out.contains("A \x1b[1msmall library\x1b[0m for"), "{out:?}");
    assert!(out.contains("\x1b[36mHH:MM height\x1b[0m"), "{out:?}");
    assert!(
        out.contains("\x1b[90m• \x1b[0mone \x1b[3munit test\x1b[0m each\n"),
        "{out:?}"
    );
    assert!(
        out.contains("  \x1b[35mlet\x1b[0m x = \x1b[33m1\x1b[0m;\n"),
        "{out:?}"
    );
    assert!(!out.contains("**") && !out.contains("```"), "{out:?}");
    // Wrapped at words, a column short of the terminal's width, never inside one.
    let plain = strip(&out);
    for l in plain.lines() {
        assert!(l.chars().count() <= 59, "{l:?}");
    }
    assert!(
        plain.contains("tide table, which\nreads HH:MM height lines and finds the waters.\n"),
        "{plain:?}"
    );
}

/// `NO_COLOR` (set and not empty), `TERM=dumb` and `--plain` each print the
/// text as it came, though stdout is a terminal.
#[test]
fn no_color_dumb_and_plain_print_no_escape_on_a_terminal() {
    type Case<'a> = (&'a [(&'a str, &'a str)], &'a [&'a str]);
    let cases: [Case; 3] = [
        (&[("NO_COLOR", "1")], &["ask", "tides?"]),
        (&[("TERM", "dumb")], &["ask", "tides?"]),
        (&[], &["--plain", "ask", "tides?"]),
    ];
    for (env, args) in cases {
        let out = run(
            &Run {
                tty: true,
                env,
                args,
            },
            ask(),
        );
        assert_eq!(out, REPLY, "{env:?} {args:?}");
    }
    // An empty NO_COLOR does not count (no-color.org).
    let out = run(
        &Run {
            tty: true,
            env: &[("NO_COLOR", "")],
            args: &["ask", "x"],
        },
        ask(),
    );
    assert!(out.contains('\x1b'));
}

#[test]
fn no_stream_draws_the_whole_reply_on_a_terminal_too() {
    let script = vec![("turn.submit", result(REPLY), vec![])];
    let out = run(
        &Run {
            tty: true,
            env: &[("COLUMNS", "60")],
            args: &["--no-stream", "ask", "x"],
        },
        script.clone(),
    );
    assert!(
        out.starts_with("\x1b[1;4mPort Wenlow tides\x1b[0m\n\n"),
        "{out:?}"
    );
    let piped = run(
        &Run {
            tty: false,
            env: &[],
            args: &["--no-stream", "ask", "x"],
        },
        script,
    );
    assert_eq!(piped, format!("{REPLY}\n"));
}

fn history() -> Value {
    json!({
        "session": {"session_id": S, "kind": "conversation", "label": null,
                    "created_at_unix_ms": 1_759_300_000_000u64, "turns": 1,
                    "usage": {"input_tokens": 1, "output_tokens": 1}, "execution_id": null,
                    "execution_state": null, "last_active_ms": 1_759_300_900_000u64,
                    "cost_usd": 0.0, "tool_calls": 0, "model": "glm-x", "title": "Pier"},
        "nodes": [{"node_id": "msg_aaa111", "kind": "assistant_message", "session_id": S,
                   "position": 9, "at_unix_ms": 1_759_300_100_000u64, "turn_id": T,
                   "text": REPLY, "detail": {"model": "glm-x", "stop_reason": "end_turn",
                   "usage": {"input_tokens": 3, "output_tokens": 2}, "tool_calls": []}, "bytes": 24}],
        "pending_confirms": [],
        "cites": [],
    })
}

#[test]
fn history_draws_a_reply_on_a_terminal_and_prints_the_text_on_a_pipe() {
    let script = vec![("session.history", history(), vec![])];
    let tty = run(
        &Run {
            tty: true,
            env: &[("COLUMNS", "60")],
            args: &["history", S],
        },
        script.clone(),
    );
    assert!(tty.contains("\x1b[1;4mPort Wenlow tides\x1b[0m"), "{tty:?}");
    assert!(tty.contains("    A \x1b[1msmall library\x1b[0m"), "{tty:?}");
    let plain = strip(&tty);
    assert!(
        plain.contains("Theseus (glm-x):\n    Port Wenlow tides\n"),
        "{plain:?}"
    );
    assert!(plain.contains("↳ end_turn · in 3 out 2"), "{plain:?}");
    let piped = run(
        &Run {
            tty: false,
            env: &[],
            args: &["history", S],
        },
        script,
    );
    assert!(!piped.contains('\x1b'));
    assert!(
        piped.contains("Theseus (glm-x): # Port Wenlow tides ⏎  ⏎ A **small library**"),
        "{piped:?}"
    );
}

/// The text of a capture without its SGR and OSC 8 sequences.
fn strip(s: &str) -> String {
    let mut out = String::new();
    let mut it = s.chars().peekable();
    while let Some(c) = it.next() {
        if c != '\x1b' {
            out.push(c);
        } else if it.peek() == Some(&'[') {
            for d in it.by_ref() {
                if d.is_ascii_alphabetic() {
                    break;
                }
            }
        } else if it.peek() == Some(&']') {
            for d in it.by_ref() {
                if d == '\\' {
                    break;
                }
            }
        }
    }
    out
}
