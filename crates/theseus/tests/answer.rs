//! `theseus answer ID OPTION` (theseus-ds5os): one `work.answer` request with
//! the id as it was typed (the daemon resolves a short end), the option and
//! the note; the daemon's answer printed, and the turn it resumes followed
//! unless `--no-wait`; a refusal or an ambiguous end printed with its reason
//! and exit 1; and, inside a Theseus job, refused before anything is sent.

use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixListener;
use std::process::Command;

use serde_json::{json, Value};

const THESEUS: &str = env!("CARGO_BIN_EXE_theseus");

/// A daemon that reads the one request it is sent and answers it with
/// `reply` (a result, or an error object).
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

fn theseus(sock: &std::path::Path, dir: &std::path::Path, args: &[&str]) -> std::process::Output {
    Command::new(THESEUS)
        .arg("--socket")
        .arg(sock)
        .args(args)
        // The CLI's seen file is the test's own, never the machine's (theseus-yus0).
        .env("XDG_STATE_HOME", dir.join("xdg-state"))
        .env_remove("THESEUS_SESSION")
        .output()
        .unwrap()
}

#[test]
fn an_answer_sends_the_end_the_option_and_the_note() {
    let (dir, sock, daemon) = one_request(json!({
        "question_id": "act_0c7e527d2a", "option": "decline"
    }));
    let out = theseus(
        &sock,
        dir.path(),
        &["answer", "27d2a", "decline", "--note", "not now"],
    );
    let req = daemon.join().unwrap();
    assert_eq!(req["method"], "work.answer");
    assert_eq!(
        req["params"],
        json!({"question_id": "27d2a", "option": "decline", "note": "not now", "watch": true})
    );
    let said = String::from_utf8_lossy(&out.stdout);
    assert_eq!(
        out.status.code(),
        Some(0),
        "{said} {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(said.trim(), "answered act_0c7e527d2a · decline");
}

#[test]
fn an_answer_that_resumes_work_says_which() {
    let (dir, sock, daemon) = one_request(json!({
        "question_id": "act_0c7e527d2a", "option": "approve",
        "resumes": {"id": "exe_0c7e527d2a", "kind": "task", "session_id": "ses_0c7e527d2a", "title": ""}
    }));
    let out = theseus(
        &sock,
        dir.path(),
        &["answer", "act_0c7e527d2a", "approve", "--no-wait"],
    );
    let req = daemon.join().unwrap();
    assert!(req["params"].get("watch").is_none(), "{req}");
    assert_eq!(
        String::from_utf8_lossy(&out.stdout).trim(),
        "answered act_0c7e527d2a · approve · ses_0c7e527d2a resumes"
    );
}

/// `answer` follows the turn it resumes (it asks the daemon to subscribe this
/// connection before the turn wakes) unless `--no-wait`: the same behaviour
/// `confirm` had. The scenarios in `golden.rs` hold the output.
#[test]
fn an_answer_that_resumes_work_follows_it() {
    let (dir, sock, daemon) = one_request(json!({
        "question_id": "act_0c7e527d2a", "option": "approve",
        "resumes": {"id": "exe_0c7e527d2a", "kind": "task", "session_id": "ses_0c7e527d2a", "title": ""}
    }));
    let out = theseus(&sock, dir.path(), &["answer", "act_0c7e527d2a", "approve"]);
    let req = daemon.join().unwrap();
    assert_eq!(req["params"]["watch"], true, "{req}");
    let said = String::from_utf8_lossy(&out.stderr);
    assert!(
        said.contains("following the resumed turn in ses_0c7e527d2a"),
        "{said}"
    );
}

#[test]
fn a_refusal_or_an_ambiguous_end_is_printed_with_its_reason_and_exits_1() {
    let why = "`b0c3` names 2 questions (act_aa11b0c3, act_77b0c3): give more of its id";
    let (dir, sock, daemon) = one_request(json!({"code": -32602, "message": why}));
    let out = theseus(&sock, dir.path(), &["answer", "b0c3", "approve"]);
    daemon.join().unwrap();
    let said = String::from_utf8_lossy(&out.stderr);
    assert_eq!(out.status.code(), Some(1), "{said}");
    assert!(said.contains(why), "{said}");
}

/// The operator's, like `theseus confirm`: refused inside a job before the
/// daemon is asked, saying why and where to run it.
#[test]
fn an_answer_is_refused_inside_a_job() {
    let dir = tempfile::tempdir().unwrap();
    let sock = dir.path().join("sock");
    // A daemon that accepts and would read: it is sent nothing.
    let _listener = UnixListener::bind(&sock).unwrap();
    let out = Command::new(THESEUS)
        .arg("--socket")
        .arg(&sock)
        .args(["answer", "7f3a", "approve"])
        .env("THESEUS_SESSION", "ses_0000aa1b2c3")
        .env("XDG_STATE_HOME", dir.path().join("xdg-state"))
        .output()
        .unwrap();
    let said = String::from_utf8_lossy(&out.stderr);
    assert_eq!(out.status.code(), Some(1), "{said}");
    assert!(said.contains("theseus answer refused"), "{said}");
    assert!(said.contains("Run it from your own shell."), "{said}");
}

/// A fired reminder is snoozed with `answer ID snooze WHEN` (the old `theseus snooze`): one
/// `wake.snooze`, a whole wake id marking it as fired, and the time as typed.
#[test]
fn a_snooze_is_answered_with_its_time() {
    let (dir, sock, daemon) = one_request(json!({"wake": {}}));
    let out = theseus(
        &sock,
        dir.path(),
        &["--json", "answer", "wak_0199aaaa3f9a1c", "snooze", "1h"],
    );
    let req = daemon.join().unwrap();
    assert_eq!(req["method"], "wake.snooze", "{req}");
    assert_eq!(req["params"]["wake"], "wak_0199aaaa3f9a1c");
    assert_eq!(req["params"]["when"], "1h");
    assert_eq!(req["params"]["fired"], true);
    assert_eq!(out.status.code(), Some(0), "{:?}", out);
}

/// The retired spellings (`snooze_1h`, `tomorrow_9`) and a time on any other answer are refused
/// before anything is sent, saying the one spelling.
#[test]
fn the_old_snooze_options_and_a_stray_time_are_refused_before_anything_is_sent() {
    for (args, says) in [
        (
            vec!["answer", "wak_0199aaaa3f9a1c", "snooze_1h"],
            "theseus answer wak_0199aaaa3f9a1c snooze 1h",
        ),
        (
            vec!["answer", "wak_0199aaaa3f9a1c", "tomorrow_9"],
            "snooze tomorrow 9:00",
        ),
        (
            vec!["answer", "act_0c7e527d2a", "approve", "1h"],
            "a time goes with `snooze` alone",
        ),
    ] {
        let dir = tempfile::tempdir().unwrap();
        let sock = dir.path().join("sock");
        let listener = UnixListener::bind(&sock).unwrap();
        let daemon = std::thread::spawn(move || {
            let (s, _) = listener.accept().unwrap();
            let mut sent = String::new();
            BufReader::new(&s).read_line(&mut sent).unwrap();
            sent
        });
        let out = theseus(&sock, dir.path(), &args);
        assert_eq!(daemon.join().unwrap(), "", "{args:?} sent a request");
        let said = String::from_utf8_lossy(&out.stderr);
        assert!(said.contains(says), "{args:?}: {said}");
        assert_eq!(out.status.code(), Some(1), "{said}");
    }
}
