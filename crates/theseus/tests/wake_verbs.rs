//! `theseus wake pause|resume|snooze|cancel ID` (the old top-level `pause`, `resume`, `snooze`
//! and `cancel` of a wake): each is one request of the wake method it always was, with the id
//! as typed, against a scripted daemon.

use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixListener;
use std::process::Command;

use serde_json::{json, Value};

const THESEUS: &str = env!("CARGO_BIN_EXE_theseus");

/// Run `theseus --json ARGS` against a daemon that answers its first request with `reply`
/// and returns that request.
fn run(args: &[&str], reply: Value) -> (Value, std::process::Output) {
    let dir = tempfile::tempdir().unwrap();
    let sock = dir.path().join("sock");
    let listener = UnixListener::bind(&sock).unwrap();
    let daemon = std::thread::spawn(move || {
        let (s, _) = listener.accept().unwrap();
        let mut line = String::new();
        BufReader::new(&s).read_line(&mut line).unwrap();
        let req: Value = serde_json::from_str(&line).unwrap();
        let answer = json!({"jsonrpc": "2.0", "id": req["id"], "result": reply});
        (&s).write_all(format!("{answer}\n").as_bytes()).unwrap();
        req
    });
    let out = Command::new(THESEUS)
        .arg("--socket")
        .arg(&sock)
        .arg("--json")
        .args(args)
        .env("XDG_STATE_HOME", dir.path().join("xdg-state"))
        .env_remove("THESEUS_SESSION")
        .output()
        .unwrap();
    (daemon.join().unwrap(), out)
}

#[test]
fn pause_takes_an_until_and_resume_a_name() {
    let (req, out) = run(
        &["wake", "pause", "3f9a1c", "--until", "fri 9:00"],
        json!({}),
    );
    assert_eq!(req["method"], "wake.pause", "{req}");
    assert_eq!(req["params"]["wake"], "3f9a1c");
    assert_eq!(req["params"]["until"], "fri 9:00");
    assert_eq!(out.status.code(), Some(0));
    let (req, _) = run(&["wake", "resume", "3f9a1c"], json!({}));
    assert_eq!(req["method"], "wake.resume", "{req}");
    assert_eq!(req["params"]["wake"], "3f9a1c");
}

#[test]
fn snooze_moves_a_pending_wake_and_cancel_names_a_wake_first() {
    let (req, _) = run(&["wake", "snooze", "3f9a1c", "tomorrow 9:00"], json!({}));
    assert_eq!(req["method"], "wake.snooze", "{req}");
    assert_eq!(req["params"]["when"], "tomorrow 9:00");
    assert_ne!(req["params"]["fired"], true);
    let (req, _) = run(&["wake", "cancel", "3f9a1c"], json!({}));
    assert_eq!(req["method"], "wake.cancel", "{req}");
    assert_eq!(req["params"]["wake"], "3f9a1c");
}

#[test]
fn the_old_top_level_verbs_are_gone() {
    for old in [
        "pause", "resume", "snooze", "done", "wakes", "work", "watches", "voice", "notices",
        "index",
    ] {
        let out = Command::new(THESEUS).arg(old).output().unwrap();
        assert_eq!(out.status.code(), Some(2), "`theseus {old}` still parses");
    }
}
