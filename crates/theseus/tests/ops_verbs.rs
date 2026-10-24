//! The Operations page's verbs from a shell (theseus-5iqf8): `task rehome`, `task reschedule`
//! and `sessions model`: the one request each sends, the line each prints, a `refused` answer
//! (ok on the wire) exiting 1 with the daemon's reason, and a job's shell refused first.

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

const SES: &str = "ses_0000ffee0017";

#[test]
fn rehome_sends_the_task_and_the_home_and_prints_from_to_and_the_note() {
    let (dir, sock, daemon) = one_request(json!({
        "task": "ses_0000ab1c2d3e4f", "from": "discord:dm:42", "to": null,
        "note": "its budget stays carved from its original parent (exe_1)"}));
    let out = theseus(
        &sock,
        dir.path(),
        None,
        &["task", "rehome", "e4f9a1", "thread:harbor"],
    );
    let req = daemon.join().unwrap();
    assert_eq!(req["method"], "task.rehome");
    assert_eq!(
        req["params"],
        json!({"task": "e4f9a1", "home": "thread:harbor"})
    );
    let said = String::from_utf8_lossy(&out.stdout);
    assert_eq!(
        out.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        said.contains("goes to the board alone (was discord:dm:42)"),
        "{said}"
    );
    assert!(
        said.contains("stays carved from its original parent"),
        "{said}"
    );
}

#[test]
fn rehome_to_a_conversation_sends_its_session_id() {
    let (dir, sock, daemon) = one_request(json!({
        "task": "ses_0000ab1c2d3e4f", "from": null, "to": "discord:dm:42", "note": "n"}));
    theseus(&sock, dir.path(), None, &["task", "rehome", "e4f9a1", SES]);
    let req = daemon.join().unwrap();
    assert_eq!(req["params"], json!({"task": "e4f9a1", "home": SES}));
}

#[test]
fn reschedule_sends_start_when_as_task_start_does_and_prints_the_undo() {
    let (dir, sock, daemon) = one_request(json!({
        "task": "ses_0000ab1c2d3e4f", "starts_at_ms": 1_800_000_000_000u64,
        "previous": 1_700_000_000_000u64}));
    let out = theseus(
        &sock,
        dir.path(),
        None,
        &["task", "reschedule", "e4f9a1", "--in-time", "2h"],
    );
    let req = daemon.join().unwrap();
    assert_eq!(req["method"], "task.reschedule");
    assert_eq!(
        req["params"],
        json!({"task": "e4f9a1", "start_when": "in 2h"})
    );
    let said = String::from_utf8_lossy(&out.stdout);
    assert!(
        said.contains("will start at")
            && said.contains("(was ")
            && said.contains("to undo: theseus task reschedule e4f9a1 --at"),
        "{said}"
    );
    // The undo's time is one `--at` reads back (wake::parse_at): RFC 3339 with its offset.
    let undo = said
        .split("--at \"")
        .nth(1)
        .and_then(|t| t.split('"').next())
        .unwrap_or_default();
    let shape = "dddd-dd-ddTdd:dd:dd+dd:dd";
    assert!(
        undo.len() == shape.len()
            && undo.chars().zip(shape.chars()).all(|(c, s)| match s {
                'd' => c.is_ascii_digit(),
                '+' => c == '+' || c == '-',
                s => c == s,
            }),
        "the undo's --at is not a time --at reads: {undo:?} in {said}"
    );

    let (dir, sock, daemon) = one_request(json!({
        "task": "ses_0000ab1c2d3e4f", "starts_at_ms": 1u64, "previous": 2u64}));
    theseus(
        &sock,
        dir.path(),
        None,
        &["task", "reschedule", "e4f9a1", "--at", "fri 9:00"],
    );
    assert_eq!(
        daemon.join().unwrap()["params"],
        json!({"task": "e4f9a1", "start_when": "fri 9:00"})
    );
}

#[test]
fn reschedule_needs_exactly_one_time() {
    let dir = tempfile::tempdir().unwrap();
    let sock = dir.path().join("sock");
    let _never = UnixListener::bind(&sock).unwrap();
    for args in [
        vec!["task", "reschedule", "e4f9a1"],
        vec![
            "task",
            "reschedule",
            "e4f9a1",
            "--at",
            "9:00",
            "--in-time",
            "2h",
        ],
    ] {
        let out = theseus(&sock, dir.path(), None, &args);
        assert_eq!(out.status.code(), Some(2), "{args:?}");
    }
}

fn held(profile: Option<&str>) -> Value {
    json!({"session_id": SES, "profile": profile, "previous": null})
}

#[test]
fn sessions_model_sends_the_profile_and_off_sends_null() {
    let (dir, sock, daemon) = one_request(held(Some("opus")));
    let out = theseus(&sock, dir.path(), None, &["sessions", "model", SES, "opus"]);
    let req = daemon.join().unwrap();
    assert_eq!(req["method"], "session.profile");
    assert_eq!(req["params"], json!({"session_id": SES, "profile": "opus"}));
    let said = String::from_utf8_lossy(&out.stdout);
    assert!(said.contains("run on opus from the next one"), "{said}");

    let (dir, sock, daemon) = one_request(held(None));
    let out = theseus(
        &sock,
        dir.path(),
        None,
        &["sessions", "model", SES, "--off"],
    );
    let req = daemon.join().unwrap();
    assert!(req["params"].get("profile").is_none(), "{}", req["params"]);
    assert_eq!(req["params"]["session_id"], SES);
    assert_eq!(out.status.code(), Some(0));
    assert!(String::from_utf8_lossy(&out.stdout).contains("held to no profile"));
}

#[test]
fn sessions_model_needs_exactly_one_of_a_profile_and_off() {
    let dir = tempfile::tempdir().unwrap();
    let sock = dir.path().join("sock");
    let _never = UnixListener::bind(&sock).unwrap();
    for args in [
        vec!["sessions", "model", SES],
        vec!["sessions", "model", SES, "opus", "--off"],
    ] {
        assert_eq!(
            theseus(&sock, dir.path(), None, &args).status.code(),
            Some(2),
            "{args:?}"
        );
    }
}

#[test]
fn a_refused_answer_prints_the_reason_and_exits_one() {
    let (dir, sock, daemon) = one_request(json!({
        "session_id": SES, "profile": null, "previous": null,
        "refused": "this conversation holds private content, and `glm` is not a model of US origin",
        "note": "nothing was written"}));
    let out = theseus(&sock, dir.path(), None, &["sessions", "model", SES, "glm"]);
    daemon.join().unwrap();
    assert_eq!(out.status.code(), Some(1));
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("not a model of US origin"), "{err}");
    assert!(String::from_utf8_lossy(&out.stdout).contains("nothing was written"));
}

#[test]
fn inside_a_job_each_verb_is_refused_before_anything_is_sent() {
    let dir = tempfile::tempdir().unwrap();
    let sock = dir.path().join("sock");
    let _never = UnixListener::bind(&sock).unwrap();
    for (args, command) in [
        (
            vec!["task", "rehome", "e4f9a1", SES],
            "theseus task rehome refused",
        ),
        (
            vec!["task", "reschedule", "e4f9a1", "--at", "9:00"],
            "theseus task reschedule refused",
        ),
        (
            vec!["sessions", "model", SES, "--off"],
            "theseus sessions model refused",
        ),
    ] {
        let out = theseus(&sock, dir.path(), Some("ses_0000aa1b2c3"), &args);
        assert_eq!(out.status.code(), Some(1), "{args:?}");
        let err = String::from_utf8_lossy(&out.stderr);
        assert!(err.contains(command), "{err}");
    }
}
