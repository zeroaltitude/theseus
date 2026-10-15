//! `theseus mode` (theseus-lyxy): one `session.mode` request naming the
//! session (a whole id) and the mode; the answer's line and checklist
//! printed; a refusal printed with its reason and exit 1; and, inside a
//! Theseus job, refused before anything is sent.

use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixListener;
use std::process::Command;

use serde_json::{json, Value};

const THESEUS: &str = env!("CARGO_BIN_EXE_theseus");

/// A daemon that reads the one request it is sent and answers it.
fn one_request(
    reply: Value,
) -> (
    tempfile::TempDir,
    std::path::PathBuf,
    std::thread::JoinHandle<Value>,
) {
    let dir = tempfile::tempdir().unwrap();
    let sock = dir.path().join("sock");
    let listener = UnixListener::bind(&sock).unwrap();
    let daemon = std::thread::spawn(move || {
        let (s, _) = listener.accept().unwrap();
        let mut line = String::new();
        BufReader::new(&s).read_line(&mut line).unwrap();
        let req: Value = serde_json::from_str(&line).unwrap();
        let mut answer = json!({"jsonrpc": "2.0", "id": req["id"]});
        match reply.get("code") {
            Some(_) => answer["error"] = reply,
            None => answer["result"] = reply,
        }
        (&s).write_all(format!("{answer}\n").as_bytes()).unwrap();
        req
    });
    (dir, sock, daemon)
}

fn theseus(
    sock: &std::path::Path,
    dir: &std::path::Path,
    job: Option<&str>,
    args: &[&str],
) -> std::process::Output {
    let mut c = Command::new(THESEUS);
    c.arg("--socket")
        .arg(sock)
        .args(args)
        .env("XDG_STATE_HOME", dir.join("xdg-state"))
        .env_remove("THESEUS_SESSION");
    if let Some(j) = job {
        c.env("THESEUS_SESSION", j);
    }
    c.output().unwrap()
}

const SID: &str = "ses_0000ab1c2d3e4f";

#[test]
fn a_mode_is_one_request_and_its_line_and_checklist_are_printed() {
    let (dir, sock, daemon) = one_request(json!({
        "session_id": SID, "mode": "plan", "previous": "ask",
        "steps": [{"text": "add it", "state": "done"}, {"text": "test it", "state": "doing"}],
    }));
    let out = theseus(&sock, dir.path(), None, &["mode", "plan", SID]);
    assert!(out.status.success(), "{out:?}");
    let req = daemon.join().unwrap();
    assert_eq!(req["method"], "session.mode");
    assert_eq!(req["params"], json!({"session_id": SID, "mode": "plan"}));
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(
        text.starts_with(
            "plan: planning, read-only until you approve a plan · plan 1/2"
        ),
        "{text}"
    );
    assert!(
        text.contains("✔ 1. add it") && text.contains("◼ 2. test it"),
        "{text}"
    );
}

#[test]
fn a_refusal_is_printed_with_its_reason_and_exits_1() {
    let (dir, sock, daemon) = one_request(json!({
        "code": -32001,
        "message": "a shared place has no mode but ask. The conversation is in ask.",
    }));
    let out = theseus(&sock, dir.path(), None, &["mode", "auto", SID]);
    daemon.join().unwrap();
    assert_eq!(out.status.code(), Some(1), "{out:?}");
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("no mode but ask"), "{err}");
}

#[test]
fn inside_a_job_it_is_refused_before_anything_is_sent() {
    let dir = tempfile::tempdir().unwrap();
    let sock = dir.path().join("sock");
    let listener = UnixListener::bind(&sock).unwrap();
    listener.set_nonblocking(true).unwrap();
    let out = theseus(
        &sock,
        dir.path(),
        Some("ses_0000aa1b2c3"),
        &["mode", "auto", SID],
    );
    assert!(!out.status.success());
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(
        err.contains("theseus mode refused: it is the operator's to run"),
        "{err}"
    );
    // The CLI may have connected, but it wrote no request.
    if let Ok((mut s, _)) = listener.accept() {
        use std::io::Read;
        s.set_nonblocking(true).unwrap();
        let mut sent = Vec::new();
        let _ = s.read_to_end(&mut sent);
        assert!(sent.is_empty(), "nothing was sent: {sent:?}");
    }
}

#[test]
fn a_mode_the_daemon_does_not_have_is_a_usage_error() {
    let dir = tempfile::tempdir().unwrap();
    let sock = dir.path().join("sock");
    let out = theseus(&sock, dir.path(), None, &["mode", "yolo", SID]);
    assert_eq!(out.status.code(), Some(2), "{out:?}");
}
