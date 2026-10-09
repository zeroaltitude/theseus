//! `theseus status` and `theseus wait --any` (theseus-lweh), the real binary
//! against a scripted daemon on a Unix socket: the long and short forms (with
//! the zone and the width pinned), the silence of `--short`, a daemon that is
//! down, `--watch` through a restart, and `wait --any`.

use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use serde_json::{json, Value};

const THESEUS: &str = env!("CARGO_BIN_EXE_theseus");

/// A moment well in the past, so that a row's age is the same minute-and-second
/// in every run: `now` is the clock, and a golden cannot pin it. Rows carry
/// their `since_ms` as an offset before the real now, and the goldens leave
/// the ages out by asking for `--json` or by matching around them.
fn ago(ms: u64) -> u64 {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64;
    now - ms
}

fn view(id: &str, state: &str, level: &str, label: &str, since: u64, position: u64) -> Value {
    json!({
        "position": position, "at_ms": since,
        "execution_id": format!("exe_{id}"), "session_id": format!("ses_{id}"),
        "kind": "task", "state": state, "pending": [], "turns": 1,
        "attention": {"level": level, "label": label, "since_ms": since},
    })
}

fn asking(id: &str, since: u64, position: u64) -> Value {
    let mut v = view(
        id,
        "waiting",
        "needs_you",
        "confirm fs.write: write regatta/paint-log.md",
        since,
        position,
    );
    v["pending"] = json!([{"correlation_id": format!("act_{id}"), "tool": "fs.write",
        "reason": "write regatta/paint-log.md"}]);
    v
}

fn snapshot(views: &[Value]) -> Value {
    json!({"position": 10, "executions": views, "confirms": [], "total": views.len()})
}

/// Answer each request on `s` from `answers` by method; send `then` after the
/// answer to `executions.watch`, each after its pause; then hold the
/// connection open for `hold`, or close it at once when `hold` is zero.
fn serve(s: UnixStream, answers: &[(&str, Value)], then: &[(Duration, Value)], hold: Duration) {
    let mut reader = BufReader::new(&s);
    let mut line = String::new();
    while reader.read_line(&mut line).unwrap_or(0) > 0 {
        let req: Value = serde_json::from_str(&line).unwrap();
        line.clear();
        let method = req["method"].as_str().unwrap().to_string();
        let result = answers
            .iter()
            .find(|(m, _)| *m == method)
            .map_or(json!({}), |(_, r)| r.clone());
        let answer = json!({"jsonrpc": "2.0", "id": req["id"], "result": result});
        (&s).write_all(format!("{answer}\n").as_bytes()).unwrap();
        if method == "executions.watch" {
            for (pause, n) in then {
                std::thread::sleep(*pause);
                let n = json!({"jsonrpc": "2.0", "method": "execution.changed", "params": n});
                if (&s).write_all(format!("{n}\n").as_bytes()).is_err() {
                    return;
                }
            }
            if !hold.is_zero() {
                std::thread::sleep(hold);
                return;
            }
        }
    }
}

fn theseus(sock: &Path, args: &[&str]) -> Command {
    let mut c = Command::new(THESEUS);
    c.arg("--socket")
        .arg(sock)
        .args(args)
        .env_remove("THESEUS_SOCKET")
        .env_remove("THESEUS_SESSION")
        .env_remove("TMUX")
        .env("TZ", "America/Phoenix")
        .env("COLUMNS", "100");
    c
}

/// Run `args` against a daemon that serves one connection as `serve` does.
fn run(args: &[&str], answers: Vec<(&'static str, Value)>) -> (String, String, i32) {
    let dir = tempfile::tempdir().unwrap();
    let sock = dir.path().join("sock");
    let listener = UnixListener::bind(&sock).unwrap();
    let daemon = std::thread::spawn(move || {
        let (s, _) = listener.accept().unwrap();
        serve(s, &answers, &[], Duration::ZERO);
    });
    let out = theseus(&sock, args).output().unwrap();
    daemon.join().unwrap();
    (
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
        out.status.code().unwrap_or(-1),
    )
}

fn five() -> Vec<Value> {
    vec![
        view("e527d2", "running", "working", "turn 5", ago(134_000), 11),
        asking("2a329e", ago(240_000), 12),
        view(
            "5a877c",
            "failed",
            "needs_you",
            "failed: the provider refused the request (400)",
            1_759_300_000_000,
            13,
        ),
        view("87e9c6", "running", "working", "turn 2", ago(31_000), 14),
    ]
}

fn titles() -> Value {
    let s = |id: &str, label: &str| {
        json!({"session_id": format!("ses_{id}"), "kind": "task", "label": label,
               "created_at_unix_ms": 1, "turns": 1})
    };
    json!({"sessions": [
        s("2a329e", "Paint the south buoy red and log it."),
        s("5a877c", "Read the tide gauge at the pier."),
        s("87e9c6", "Fetch Saturday's forecast for the harbour."),
    ]})
}

#[test]
fn the_short_form_counts_and_the_long_form_lists() {
    let (out, _, code) = run(
        &["status", "--short"],
        vec![("executions.watch", snapshot(&five()))],
    );
    assert_eq!((out.as_str(), code), ("●1 ◐2 ✗1\n", 0));

    let (out, _, code) = run(
        &["status"],
        vec![
            ("executions.watch", snapshot(&five())),
            ("session.list", titles()),
        ],
    );
    assert_eq!(code, 0);
    let lines: Vec<&str> = out.lines().collect();
    // The failure's time is a local one: 2025-10-01 06:26:40 UTC is 23:26 in Phoenix the day before.
    let want_head = "● 1 needs you · ◐ 2 working · ✗ 1 failed";
    assert!(
        lines[0].starts_with("theseus ") && lines[0].ends_with(want_head),
        "{out}"
    );
    assert!(
        lines[1].starts_with("● 2a329e  Paint the south buoy red and log it."),
        "{out}"
    );
    assert!(
        lines[1].contains("confirm fs.write: write regatta/paint-log.md · 4:0"),
        "{out}"
    );
    assert_eq!(lines[2], "         theseus confirm act_2a329e");
    assert!(
        lines[3].starts_with("✗ 5a877c  Read the tide gauge at the pier.")
            && lines[3].ends_with("failed 23:26: the provider refused the request (400)"),
        "{out}"
    );
    assert!(
        lines[5].starts_with("◐ 87e9c6  Fetch Saturday's forecast for the h"),
        "{out}"
    );
    assert!(
        lines[4].starts_with("◐ e527d2  (task)") && lines[4].contains("turn 5 · 2:1"),
        "{out}"
    );
    assert_eq!(lines.len(), 6, "{out}");
}

#[test]
fn the_short_form_is_silent_when_nothing_works_or_waits() {
    let idle = vec![view("q1", "waiting", "ready", "ready", ago(1000), 11)];
    let (out, err, code) = run(
        &["status", "--short"],
        vec![("executions.watch", snapshot(&idle))],
    );
    assert_eq!((out.as_str(), err.as_str(), code), ("", "", 0));
}

#[test]
fn the_long_form_cuts_the_title_to_the_terminal() {
    let dir = tempfile::tempdir().unwrap();
    let sock = dir.path().join("sock");
    let listener = UnixListener::bind(&sock).unwrap();
    let daemon = std::thread::spawn(move || {
        let (s, _) = listener.accept().unwrap();
        serve(
            s,
            &[
                ("executions.watch", snapshot(&five())),
                ("session.list", titles()),
            ],
            &[],
            Duration::ZERO,
        );
    });
    let out = theseus(&sock, &["status"])
        .env("COLUMNS", "60")
        .output()
        .unwrap();
    daemon.join().unwrap();
    let out = String::from_utf8_lossy(&out.stdout);
    for line in out.lines().skip(1) {
        if !line.starts_with("         ") {
            assert!(line.chars().count() <= 60, "{line}");
        }
    }
    assert!(out.contains('…'), "{out}");
}

#[test]
fn a_daemon_that_is_down_prints_nothing_and_exits_3_at_once() {
    let dir = tempfile::tempdir().unwrap();
    let sock = dir.path().join("no-such-socket");
    let t = Instant::now();
    let out = theseus(&sock, &["status", "--short"]).output().unwrap();
    let took = t.elapsed();
    assert_eq!(out.status.code(), Some(3));
    assert!(out.stdout.is_empty() && out.stderr.is_empty(), "{out:?}");
    assert!(took < Duration::from_millis(100), "took {took:?}");
    // The long form says why on stderr, and exits 3 too.
    let out = theseus(&sock, &["status"]).output().unwrap();
    assert_eq!(out.status.code(), Some(3));
    assert!(String::from_utf8_lossy(&out.stderr).contains("connecting to theseusd"));
}

/// Read lines from a child's stdout as they come, each with a deadline.
struct Lines {
    rx: std::sync::mpsc::Receiver<String>,
}

impl Lines {
    fn of(child: &mut std::process::Child) -> Self {
        let out = child.stdout.take().unwrap();
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            for l in BufReader::new(out).lines().map_while(Result::ok) {
                if tx.send(l).is_err() {
                    break;
                }
            }
        });
        Self { rx }
    }

    fn next(&self) -> String {
        self.rx
            .recv_timeout(Duration::from_secs(20))
            .expect("a line")
    }

    fn none_within(&self, d: Duration) -> bool {
        self.rx.recv_timeout(d).is_err()
    }
}

#[test]
fn watch_prints_each_change_once_and_goes_on_through_a_restart() {
    let dir = tempfile::tempdir().unwrap();
    let sock = dir.path().join("sock");
    let listener = UnixListener::bind(&sock).unwrap();
    let daemon = std::thread::spawn(move || {
        // The first daemon: one working task, then a second, then it goes away.
        let (s, _) = listener.accept().unwrap();
        serve(
            s,
            &[(
                "executions.watch",
                snapshot(&[view("a", "running", "working", "turn 1", ago(5000), 11)]),
            )],
            &[(
                Duration::from_millis(200),
                view("b", "running", "working", "turn 1", ago(1000), 12),
            )],
            Duration::from_millis(200),
        );
        // The restarted daemon: `a` is waiting on you.
        let (s, _) = listener.accept().unwrap();
        serve(
            s,
            &[("executions.watch", snapshot(&[asking("a", ago(5000), 3)]))],
            &[],
            Duration::from_secs(2),
        );
    });
    let mut child = theseus(&sock, &["status", "--short", "--watch"])
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let lines = Lines::of(&mut child);
    let got = [lines.next(), lines.next(), lines.next(), lines.next()];
    assert_eq!(got, ["◐1", "◐2", "", "●1"]);
    // Nothing more: no timer prints.
    assert!(lines.none_within(Duration::from_millis(800)));
    child.kill().unwrap();
    child.wait().unwrap();
    drop(lines);
    daemon.join().unwrap();
}

#[test]
fn watch_tab_writes_its_sequences_to_the_terminal_and_clears_on_exit() {
    // No controlling terminal in a test: the tab says so and the lines go on.
    let dir = tempfile::tempdir().unwrap();
    let sock = dir.path().join("sock");
    let listener = UnixListener::bind(&sock).unwrap();
    let daemon = std::thread::spawn(move || {
        let (s, _) = listener.accept().unwrap();
        serve(
            s,
            &[(
                "executions.watch",
                snapshot(&[view("a", "running", "working", "turn 1", ago(5000), 11)]),
            )],
            &[],
            Duration::from_secs(2),
        );
    });
    let mut child = theseus(&sock, &["status", "--short", "--watch", "--tab"])
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let lines = Lines::of(&mut child);
    assert_eq!(lines.next(), "◐1");
    child.kill().unwrap();
    child.wait().unwrap();
    drop(daemon);
}

#[test]
fn wait_any_returns_on_the_approval_and_exits_4_at_its_timeout() {
    let dir = tempfile::tempdir().unwrap();
    let sock = dir.path().join("sock");
    let listener = UnixListener::bind(&sock).unwrap();
    let daemon = std::thread::spawn(move || {
        for _ in 0..1 {
            let (s, _) = listener.accept().unwrap();
            serve(
                s,
                &[(
                    "executions.watch",
                    snapshot(&[view("a", "running", "working", "turn 1", ago(5000), 11)]),
                )],
                &[
                    (
                        Duration::from_millis(100),
                        view("b", "running", "working", "turn 1", ago(1000), 12),
                    ),
                    (Duration::from_millis(100), asking("b", ago(10), 13)),
                ],
                Duration::from_secs(3),
            );
        }
    });
    let t = Instant::now();
    let out = theseus(&sock, &["wait", "--any", "--until", "blocked"])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert_eq!(out.status.code(), Some(0), "{stdout}");
    assert!(
        stdout.starts_with("blocked\t13\tses_b\ttask\twaiting\t● confirm fs.write"),
        "{stdout}"
    );
    assert!(t.elapsed() < Duration::from_secs(3));

    // Nothing needs you in time: exit 4.
    let (idle_dir, idle) = (
        tempfile::tempdir().unwrap(),
        vec![view("a", "running", "working", "turn 1", ago(5000), 11)],
    );
    let sock2 = idle_dir.path().join("sock");
    let l2 = UnixListener::bind(&sock2).unwrap();
    let d2 = std::thread::spawn(move || {
        let (s, _) = l2.accept().unwrap();
        serve(
            s,
            &[("executions.watch", snapshot(&idle))],
            &[],
            Duration::from_secs(3),
        );
    });
    let out = theseus(&sock2, &["wait", "--any", "--timeout", "1s"])
        .output()
        .unwrap();
    assert_eq!(
        out.status.code(),
        Some(4),
        "{}",
        String::from_utf8_lossy(&out.stdout)
    );
    assert!(String::from_utf8_lossy(&out.stdout).starts_with("timeout"));
    drop((daemon, d2));
}
