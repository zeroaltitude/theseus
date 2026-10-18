//! The CLI's side of a refused answer (theseus-6qy): `theseus answer` prints
//! the reason the daemon gives, and exits 1. An id comes with its answer.

use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixListener;
use std::process::Command;

use serde_json::{json, Value};

const THESEUS: &str = env!("CARGO_BIN_EXE_theseus");

const WHY: &str = "the answer from sock#3 does not count: from a Theseus job's process (job \
                   act_job, pid 4242, theseus). It keeps waiting for an answer that does";

/// A daemon that refuses the one answer it is sent, as a job's is refused.
#[test]
fn a_refused_answer_is_printed_with_its_reason_and_exits_1() {
    let dir = tempfile::tempdir().unwrap();
    let sock = dir.path().join("sock");
    let listener = UnixListener::bind(&sock).unwrap();
    let daemon = std::thread::spawn(move || {
        let (s, _) = listener.accept().unwrap();
        let mut line = String::new();
        BufReader::new(&s).read_line(&mut line).unwrap();
        let req: Value = serde_json::from_str(&line).unwrap();
        let refusal = json!({"jsonrpc": "2.0", "id": req["id"], "error": {
            "code": -32005, "message": WHY,
            "data": {"who": "sock#3", "via": "cli", "why": "from a Theseus job's process"}}});
        (&s).write_all(format!("{refusal}\n").as_bytes()).unwrap();
        req
    });
    let out = Command::new(THESEUS)
        .arg("--socket")
        .arg(&sock)
        .args(["answer", "act_1", "approve", "--no-wait"])
        // The CLI's seen file is the test's own, never the machine's (theseus-yus0).
        .env("XDG_STATE_HOME", dir.path().join("xdg-state"))
        .output()
        .unwrap();
    let req = daemon.join().unwrap();
    assert_eq!(req["method"], "work.answer");
    assert_eq!(
        (&req["params"]["question_id"], &req["params"]["option"]),
        (&json!("act_1"), &json!("approve"))
    );
    let said = String::from_utf8_lossy(&out.stderr);
    assert_eq!(out.status.code(), Some(1), "{said}");
    assert!(said.contains(WHY), "{said}");
    assert!(said.contains("-32005"), "{said}");
}

#[test]
fn a_question_without_its_answer_is_refused_before_anything_is_sent() {
    let out = Command::new(THESEUS)
        .args(["answer", "act_1"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&out.stderr).contains("required arguments were not provided"));
}
