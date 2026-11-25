//! `theseus told undo POST` (issue 57): it sends `told.undo` with the note's
//! full post id and prints what the note says now, plain or `--json`, as the
//! daemon (which masks it) sent it; a refusal from the core is its error;
//! inside a job the CLI refuses it before anything is sent, as every operator
//! verb. Each runs the real binary against a scripted daemon on a Unix socket.

use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixListener;
use std::process::Command;

use serde_json::{json, Value};

const THESEUS: &str = env!("CARGO_BIN_EXE_theseus");

/// What `theseus --socket <sock> told undo POST` sent, printed and exited
/// with, against a daemon that answers `reply` (a whole response but its id),
/// inside the job `job` names when it does.
fn run(post: &str, reply: Value, job: Option<&str>) -> (Vec<Value>, String, String, i32) {
    run_with(&[], post, reply, job)
}

/// `run`, with `flags` before the command.
fn run_with(
    flags: &[&str],
    post: &str,
    reply: Value,
    job: Option<&str>,
) -> (Vec<Value>, String, String, i32) {
    let dir = tempfile::tempdir().unwrap();
    let sock = dir.path().join("sock");
    let listener = UnixListener::bind(&sock).unwrap();
    let daemon = std::thread::spawn(move || {
        let (s, _) = listener.accept().unwrap();
        let mut seen = Vec::new();
        let mut line = String::new();
        if BufReader::new(&s).read_line(&mut line).unwrap_or(0) == 0 {
            return seen;
        }
        let req: Value = serde_json::from_str(&line).unwrap();
        let mut a = reply;
        a["jsonrpc"] = json!("2.0");
        a["id"] = req["id"].clone();
        (&s).write_all(format!("{a}\n").as_bytes()).unwrap();
        seen.push(req);
        seen
    });
    let mut cmd = Command::new(THESEUS);
    cmd.arg("--socket")
        .arg(&sock)
        .args(flags)
        .args(["told", "undo", post])
        .env("XDG_STATE_HOME", dir.path().join("state"))
        .env_remove("THESEUS_SOCKET")
        .env_remove("THESEUS_SESSION");
    if let Some(job) = job {
        cmd.env("THESEUS_SESSION", job);
    }
    let out = cmd.output().unwrap();
    let seen = daemon.join().unwrap();
    (
        seen,
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
        out.status.code().unwrap_or(-1),
    )
}

#[test]
fn told_undo_sends_the_door_the_post_and_prints_the_note_now() {
    let text = concat!(
        "ℹ️ FYI · Theseus took something back\n",
        "In “Tides”, without asking you\n",
        "↩️ Undone by the CLI."
    );
    let (seen, out, err, code) = run(
        "out_0000aa1b2c3d",
        json!({"result": {"text": text, "found": false}}),
        None,
    );
    assert_eq!(code, 0, "{err}");
    assert_eq!(seen.len(), 1);
    assert_eq!(seen[0]["method"], "told.undo");
    assert_eq!(seen[0]["params"], json!({"post": "out_0000aa1b2c3d"}));
    assert_eq!(out, format!("Undone. The note now says:\n{text}\n"));
}

#[test]
fn a_second_undo_is_the_cores_refusal() {
    let (seen, _, err, code) = run(
        "out_0001",
        json!({"error": {"code": -32602,
            "message": "it was already undone by the CLI: nothing was done",
            "data": {"undone_by": "the CLI"}}}),
        None,
    );
    assert_eq!(seen.len(), 1);
    assert_ne!(code, 0);
    assert!(err.contains("already undone by the CLI"), "{err}");
}

#[test]
fn inside_a_job_told_undo_is_refused_before_anything_is_sent() {
    let (seen, out, err, code) = run(
        "out_0001",
        json!({"result": {"text": "", "found": false}}),
        Some("ses_0000aa1b2c3"),
    );
    assert!(seen.is_empty(), "{seen:?}");
    assert_ne!(code, 0);
    assert!(out.is_empty(), "{out}");
    assert!(err.contains("theseus told undo refused"), "{err}");
    assert!(err.contains("THESEUS_SESSION=ses_0000aa1b2c3"), "{err}");
}

/// `--json` prints the daemon's answer as it came: the daemon masked it
/// (`rpc::told::tests::told_undos_copies_over_the_rpc_are_masked` proves that
/// against the real core), and the CLI adds nothing raw.
#[test]
fn told_undo_json_prints_the_daemons_masked_answer_as_it_came() {
    let answer = json!({"text": "ℹ️ FYI · Took back [redacted:told_test_secret]\n↩️ Undone by ana.",
                        "found": false});
    let (_, out, err, code) = run_with(
        &["--json"],
        "out_0000aa1b2c3d",
        json!({"result": answer}),
        None,
    );
    assert_eq!(code, 0, "{err}");
    let printed: Value = serde_json::from_str(out.trim()).unwrap();
    assert_eq!(printed, answer);
}
