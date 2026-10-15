//! `theseus chat --plain` (theseus-mnd9g), and a pipe or `TERM=dumb`: line
//! mode, `watch --interactive` with the conversation's tasks told as lines.
//! The real binary against a scripted daemon on a Unix socket: a new
//! conversation is opened in this directory, the tasks' tree is watched as a
//! screen a person looks at, and five tasks print five titled `started`
//! lines, a question and the commands that answer it, a report and a failure.

use std::io::{BufRead, BufReader, Read, Write};
use std::os::unix::net::UnixListener;
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde_json::{json, Value};

const THESEUS: &str = env!("CARGO_BIN_EXE_theseus");

fn task(i: usize, title: &str, position: u64, extra: Value) -> Value {
    let mut v = json!({
        "id": format!("exe_t{i}"), "kind": "task", "position": position, "at_ms": 1,
        "root": "exe_conv", "parent": "exe_conv", "session_id": format!("ses_abc{i}def"),
        "title": title, "state": "running",
        "attention": {"level": "working", "label": "turn 1", "since_ms": 1},
        "started_at_ms": 1, "state_since_ms": 1
    });
    for (k, x) in extra.as_object().unwrap() {
        v[k] = x.clone();
    }
    v
}

type Asked = Arc<Mutex<Vec<(String, Value)>>>;

fn note(method: &str, params: Value) -> String {
    format!(
        "{}\n",
        json!({"jsonrpc": "2.0", "method": method, "params": params})
    )
}

/// The scripted daemon: one connection, the tree watched, then the task events.
fn serve(listener: UnixListener, seen: Asked) {
    let (s, _) = listener.accept().unwrap();
    let mut reader = BufReader::new(&s);
    let mut line = String::new();
    while reader.read_line(&mut line).unwrap_or(0) > 0 {
        let req: Value = serde_json::from_str(&line).unwrap();
        line.clear();
        let method = req["method"].as_str().unwrap().to_string();
        seen.lock()
            .unwrap()
            .push((method.clone(), req["params"].clone()));
        let result = match method.as_str() {
            "session.open" => {
                json!({"session_id": "ses_0123456789abcdef", "kind": "conversation",
                    "created_at_unix_ms": 1, "turns": 0})
            }
            "session.list" => json!({"sessions": [{"session_id": "ses_0123456789abcdef",
                    "kind": "conversation", "created_at_unix_ms": 1, "turns": 0}]}),
            "session.wait" => json!({"reached": "settled", "already": true, "confirms": []}),
            "work.watch" => json!({"position": 5, "views": [], "total": 0}),
            _ => json!({}),
        };
        let a = json!({"jsonrpc": "2.0", "id": req["id"], "result": result});
        (&s).write_all(format!("{a}\n").as_bytes()).unwrap();
        if method == "work.watch" {
            let names = [
                "Paint log",
                "Buoy survey",
                "Tide tables",
                "Rigging check",
                "Weather brief",
            ];
            for (i, n) in names.iter().enumerate() {
                let v = task(i, n, 10 + i as u64, json!({}));
                let c = note(
                    "work.changed",
                    json!({"position": 10 + i, "at_ms": 1, "views": [v]}),
                );
                (&s).write_all(c.as_bytes()).unwrap();
            }
            let mut ask = task(
                0,
                "Paint log",
                20,
                json!({
                    "state": "needs_you",
                    "attention": {"level": "needs_you", "label": "confirm fs.write", "since_ms": 1},
                    "question": {"id": "act_x2a329e", "kind": "approval", "prompt": "write regatta/paint-log.md (37 B)",
                        "asked_by": {"id": "exe_t0", "kind": "task", "title": "Paint log"},
                        "options": [
                            {"id": "approve", "label": "Approve", "style": "primary", "takes_note": false, "effect": "x"},
                            {"id": "decline", "label": "Decline", "style": "plain", "takes_note": true, "effect": "x"}],
                        "floor": false, "may_answer": true}
                }),
            );
            ask["position"] = json!(20);
            (&s).write_all(
                note(
                    "work.changed",
                    json!({"position": 20, "at_ms": 1, "views": [ask]}),
                )
                .as_bytes(),
            )
            .unwrap();
            let done = task(
                4,
                "Weather brief",
                21,
                json!({
                    "state": "done", "attention": {"level": "idle", "label": "complete", "since_ms": 2},
                    "report": {"outcome": "done", "first_line": "Fair winds Saturday"}}),
            );
            (&s).write_all(
                note(
                    "work.changed",
                    json!({"position": 21, "at_ms": 2, "views": [done]}),
                )
                .as_bytes(),
            )
            .unwrap();
            let failed = task(
                3,
                "Rigging check",
                22,
                json!({
                    "state": "failed", "attention": {"level": "needs_you", "label": "failed: x", "since_ms": 2},
                    "report": {"outcome": "failed", "reason": "the provider refused the request (400)"}}),
            );
            (&s).write_all(
                note(
                    "work.changed",
                    json!({"position": 22, "at_ms": 2, "views": [failed]}),
                )
                .as_bytes(),
            )
            .unwrap();
        }
    }
}

#[test]
fn plain_chat_opens_a_conversation_and_tells_each_task_event_as_a_line() {
    let dir = tempfile::tempdir().unwrap();
    let sock = dir.path().join("sock");
    let listener = UnixListener::bind(&sock).unwrap();
    let asked: Asked = Arc::default();
    let seen = asked.clone();
    let daemon = std::thread::spawn(move || serve(listener, seen));

    let mut child = Command::new(THESEUS)
        .args(["--socket"])
        .arg(&sock)
        .args(["chat", "--plain"])
        .env_remove("THESEUS_SOCKET")
        .env_remove("THESEUS_SESSION")
        .env("XDG_STATE_HOME", dir.path().join("state"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    // Read stderr until the last event is told, then end the input.
    let mut err = child.stderr.take().unwrap();
    let stdin = child.stdin.take().unwrap();
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let mut buf = [0u8; 4096];
        while let Ok(n) = err.read(&mut buf) {
            if n == 0 || tx.send(buf[..n].to_vec()).is_err() {
                break;
            }
        }
    });
    let mut text = String::new();
    let end = Instant::now() + Duration::from_secs(20);
    while !text.contains("Rigging check failed") && Instant::now() < end {
        if let Ok(b) = rx.recv_timeout(Duration::from_millis(200)) {
            text.push_str(&String::from_utf8_lossy(&b));
        }
    }
    drop(stdin);
    let _ = child.wait();
    drop(daemon);

    let started: Vec<&str> = text.lines().filter(|l| l.contains("started:")).collect();
    assert_eq!(started.len(), 5, "five titled started lines:\n{text}");
    assert!(started[0].starts_with("◐ started: Paint log ("), "{text}");
    assert!(
        text.contains("● Paint log asks: write regatta/paint-log.md (37 B)?"),
        "{text}"
    );
    assert!(
        text.contains("  theseus answer 2a329e approve · theseus answer 2a329e decline --note …"),
        "{text}"
    );
    assert!(
        text.contains("✓ Weather brief reported: Fair winds Saturday"),
        "{text}"
    );
    assert!(
        text.contains("✗ Rigging check failed: the provider refused the request (400)"),
        "{text}"
    );
    let asked = asked.lock().unwrap();
    let opened = asked
        .iter()
        .find(|(m, _)| m == "session.open")
        .expect("a new conversation");
    assert!(opened.1["dir"].is_string(), "{opened:?}");
    let watch = asked
        .iter()
        .find(|(m, _)| m == "work.watch")
        .expect("the tree");
    assert_eq!(watch.1["roots"], json!(["ses_0123456789abcdef"]));
    assert_eq!(watch.1["interactive"], json!(true));
}
