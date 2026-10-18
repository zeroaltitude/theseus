//! `theseus task show|pause|resume` (theseus-ypnxt.1, C), the real binary against a daemon
//! that reads the one request it is sent and answers it.

use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixListener;
use std::process::Command;

use serde_json::{json, Value};

const THESEUS: &str = env!("CARGO_BIN_EXE_theseus");

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
        .env("TZ", "America/Phoenix")
        .env_remove("THESEUS_SESSION");
    if let Some(j) = job {
        c.env("THESEUS_SESSION", j);
    }
    c.output().unwrap()
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64
}

/// A task as `task.list` answers it for an `id` read.
fn shown(state: &str, pending: u32, paused: bool, report: Option<&str>) -> Value {
    let task = json!({
        "task_id": "ses_0000000000a1b2c3", "short": "a1b2c3", "execution_id": "exe_a1b2c3",
        "parent_session_id": "ses_home", "parent_execution_id": "exe_home",
        "title": "Paint the quay", "state": state, "spent_usd": 0.4, "limit_usd": 2.0,
        "cost_usd": 0.4, "turns": 3, "pending_confirms": pending, "target": "discord:dm:42",
        "created_at_ms": now_ms() - 3_600_000, "updated_at_ms": now_ms() - 60_000
    });
    let record = json!({
        "id": "tsk_0000000000a1b2c3", "version": 2, "title": "Paint the quay",
        "objective": "Paint the railings along the quay.", "state": "in_progress",
        "owner": "agent", "session": "ses_0000000000a1b2c3",
        "origin": {"session": "ses_home", "principal": "operator"},
        "created_at_ms": 1, "updated_at_ms": 2
    });
    json!({"tasks": [task], "records": [record], "details": [{
        "task_id": "ses_0000000000a1b2c3", "report_line": report.map(|r| r.lines().next().unwrap()),
        "report": report, "paused": paused}]})
}

fn stdout(o: &std::process::Output) -> String {
    String::from_utf8_lossy(&o.stdout).into_owned()
}

/// `task show` asks `task.list` for the task by the id as typed, and prints the record whole; a
/// running, an asking and an ended task each show their own lines.
#[test]
fn show_reads_the_task_by_id_and_prints_it_whole() {
    let cases = [
        (
            shown("running", 0, false, None),
            vec![
                "◐ Paint the quay  ·  running",
                "theseus task pause a1b2c3",
                "theseus cancel a1b2c3",
            ],
            vec!["Report"],
        ),
        (
            shown("running", 1, false, None),
            vec!["theseus answer ID …"],
            vec![],
        ),
        (
            shown("complete", 0, false, Some("Painted it.\nTwo coats, green.")),
            vec![
                "✓ Paint the quay  ·  done",
                "Report\n  Painted it.\n  Two coats, green.",
                "theseus chat --task a1b2c3",
            ],
            vec!["theseus cancel"],
        ),
    ];
    for (reply, has, lacks) in cases {
        let (dir, sock, daemon) = one_request(reply);
        let out = theseus(&sock, dir.path(), None, &["task", "show", "a1b2c3"]);
        let req = daemon.join().unwrap();
        assert_eq!(req["method"], "task.list");
        assert_eq!(req["params"], json!({"id": "a1b2c3"}));
        let text = stdout(&out);
        for w in has {
            assert!(text.contains(w), "missing {w:?} in:\n{text}");
        }
        for w in lacks {
            assert!(!text.contains(w), "unexpected {w:?} in:\n{text}");
        }
        assert!(out.status.success());
    }
}

#[test]
fn show_json_prints_the_answer() {
    let (dir, sock, daemon) = one_request(shown("running", 0, false, None));
    let out = theseus(
        &sock,
        dir.path(),
        None,
        &["--json", "task", "show", "a1b2c3"],
    );
    daemon.join().unwrap();
    let v: Value = serde_json::from_str(&stdout(&out)).unwrap();
    assert_eq!(v["records"][0]["id"], "tsk_0000000000a1b2c3");
}

/// `pause` sends `task.pause` and `resume` sends `task.resume`, each with the id as typed, and the
/// words say what happened.
#[test]
fn pause_and_resume_send_their_own_methods() {
    for (verb, method, result, says) in [
        (
            "pause",
            "task.pause",
            json!({"task_id": "ses_0000000000a1b2c3", "paused": true, "changed": true}),
            "Pausing a1b2c3",
        ),
        (
            "pause",
            "task.pause",
            json!({"task_id": "ses_0000000000a1b2c3", "paused": true, "changed": false}),
            "a1b2c3 is paused already",
        ),
        (
            "resume",
            "task.resume",
            json!({"task_id": "ses_0000000000a1b2c3", "paused": false, "changed": true}),
            "Resuming a1b2c3",
        ),
    ] {
        let (dir, sock, daemon) = one_request(result);
        let out = theseus(&sock, dir.path(), None, &["task", verb, "2a1b2c3"]);
        let req = daemon.join().unwrap();
        assert_eq!(req["method"], method);
        assert_eq!(req["params"]["task"], "2a1b2c3");
        assert!(stdout(&out).contains(says), "{}", stdout(&out));
    }
}

/// An ended task's refusal is printed as the daemon says it, and exits 1.
#[test]
fn an_ended_tasks_refusal_is_printed_as_the_daemon_says_it() {
    let (dir, sock, daemon) = one_request(
        json!({"code": -32602, "message": "Task ses_0000000000a1b2c3 has ended (complete)."}),
    );
    let out = theseus(&sock, dir.path(), None, &["task", "pause", "a1b2c3"]);
    daemon.join().unwrap();
    assert!(!out.status.success());
    assert!(
        String::from_utf8_lossy(&out.stderr)
            .contains("Task ses_0000000000a1b2c3 has ended (complete)."),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
}

/// Inside a job, pause and resume are refused before anything is sent: they are the operator's.
#[test]
fn a_job_cannot_pause_or_resume() {
    for verb in ["pause", "resume"] {
        let dir = tempfile::tempdir().unwrap();
        let sock = dir.path().join("sock");
        let listener = UnixListener::bind(&sock).unwrap();
        let out = theseus(
            &sock,
            dir.path(),
            Some("ses_job"),
            &["task", verb, "a1b2c3"],
        );
        assert!(!out.status.success());
        assert!(
            String::from_utf8_lossy(&out.stderr).contains(&format!("theseus task {verb} refused"))
        );
        // The CLI may have connected, but it sent nothing.
        listener.set_nonblocking(true).unwrap();
        if let Ok((mut s, _)) = listener.accept() {
            let mut sent = String::new();
            std::io::Read::read_to_string(&mut s, &mut sent).unwrap();
            assert_eq!(sent, "", "nothing was sent");
        }
    }
}
