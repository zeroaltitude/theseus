//! `theseus history` records what it showed in the machine's seen file
//! (theseus-yus0), after its output, and never fails for it.

use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixListener;
use std::process::Command;

use serde_json::{json, Value};

const THESEUS: &str = env!("CARGO_BIN_EXE_theseus");

fn view(position: u64) -> Value {
    json!({
        "position": position, "execution_id": "exe_1", "session_id": "ses_1",
        "kind": "conversation", "state": "waiting", "turns": 2,
        "attention": {"level": "ready", "label": "ready", "since_ms": 0},
    })
}

/// A daemon that answers `session.history` and `executions.watch`, for one
/// connection; it returns the methods it was asked, in order.
fn daemon(sock: &std::path::Path) -> std::thread::JoinHandle<Vec<String>> {
    let listener = UnixListener::bind(sock).unwrap();
    std::thread::spawn(move || {
        let (s, _) = listener.accept().unwrap();
        let mut asked = Vec::new();
        for line in BufReader::new(&s).lines() {
            let req: Value = serde_json::from_str(&line.unwrap()).unwrap();
            let method = req["method"].as_str().unwrap().to_string();
            let result = match method.as_str() {
                "session.history" => json!({
                    "session": {"session_id": "ses_1", "kind": "conversation", "label": null,
                        "created_at_unix_ms": 0, "turns": 2},
                    "nodes": [],
                }),
                _ => json!({"position": 42, "executions": [view(42)], "confirms": [], "total": 1}),
            };
            asked.push(method);
            let reply = json!({"jsonrpc": "2.0", "id": req["id"], "result": result});
            (&s).write_all(format!("{reply}\n").as_bytes()).unwrap();
        }
        asked
    })
}

fn history(state: &std::path::Path) -> (std::process::Output, Vec<String>) {
    let dir = tempfile::tempdir().unwrap();
    let sock = dir.path().join("sock");
    let d = daemon(&sock);
    let out = Command::new(THESEUS)
        .arg("--socket")
        .arg(&sock)
        .args(["history", "ses_1"])
        .env("XDG_STATE_HOME", state)
        .output()
        .unwrap();
    (out, d.join().unwrap())
}

#[test]
fn history_records_the_session_it_printed_at_its_position() {
    let state = tempfile::tempdir().unwrap();
    let (out, asked) = history(state.path());
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(
        asked,
        ["session.history", "executions.watch"],
        "the position is asked after the history"
    );
    let text = std::fs::read_to_string(state.path().join("theseus/seen.json")).unwrap();
    let file: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(file["executions"]["exe_1"]["displayed"], 42);
}

#[test]
fn a_seen_file_that_cannot_be_written_does_not_fail_the_command() {
    // The state directory is a file, so nothing can be created under it.
    let dir = tempfile::tempdir().unwrap();
    let blocked = dir.path().join("state");
    std::fs::write(&blocked, "x").unwrap();
    let (out, _) = history(&blocked);
    let said = String::from_utf8_lossy(&out.stderr);
    assert!(out.status.success(), "{said}");
    assert!(said.contains("could not record"), "{said}");
    assert_eq!(said.lines().count(), 1, "one line at most: {said}");
}
