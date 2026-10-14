//! `theseus task start` (theseus-7oq0w): one `task.start` request carrying the brief,
//! the home, the budget, the profile, the time as typed, and the quote files'
//! text; the short id printed; a refusal printed with its reason and exit 1; and,
//! inside a Theseus job, refused before anything is sent.

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

fn started() -> Value {
    json!({"task_id": "ses_0000ab1c2d3e4f", "session_id": "ses_0000ab1c2d3e4f",
           "short": "e4f9a1", "home_session": "ses_0000ffee0017", "limit_usd": 0.5})
}

#[test]
fn a_start_sends_the_brief_the_budget_and_the_quote_and_prints_the_short_id() {
    let (dir, sock, daemon) = one_request(started());
    let quote = dir.path().join("log.txt");
    std::fs::write(&quote, "ferry 7 was late").unwrap();
    let out = theseus(
        &sock,
        dir.path(),
        None,
        &[
            "task",
            "start",
            "Count",
            "the",
            "boats",
            "--budget",
            "0.5",
            "--profile",
            "glm",
            "--in-time",
            "2h",
            "--quote-file",
            quote.to_str().unwrap(),
        ],
    );
    let req = daemon.join().unwrap();
    assert_eq!(req["method"], "task.start");
    assert_eq!(
        req["params"],
        json!({"brief": "Count the boats", "budget_usd": 0.5, "profile": "glm",
               "start_when": "in 2h", "quotes": ["ferry 7 was late"]})
    );
    let said = String::from_utf8_lossy(&out.stdout);
    assert_eq!(out.status.code(), Some(0), "{said}");
    assert!(
        said.starts_with("task e4f9a1 started (running), $0.50"),
        "{said}"
    );
}

#[test]
fn a_start_at_a_time_and_in_a_conversation_sends_both() {
    let (dir, sock, daemon) = one_request(started());
    // `--in` with a whole session id needs no read before the call.
    let out = theseus(
        &sock,
        dir.path(),
        None,
        &[
            "task",
            "start",
            "Survey the north quay",
            "--at",
            "fri 9:00",
            "--in",
            "ses_0000ffee0017",
        ],
    );
    let req = daemon.join().unwrap();
    assert_eq!(
        req["params"],
        json!({"brief": "Survey the north quay", "home": "ses_0000ffee0017", "start_when": "fri 9:00"}),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
}

#[test]
fn a_refusal_is_printed_with_its_reason_and_exits_one() {
    let (dir, sock, daemon) = one_request(json!({
        "code": -32005,
        "message": "Not started: a budget of $9.00 is more than conversation 7fa001 has left: $4.00"
    }));
    let out = theseus(
        &sock,
        dir.path(),
        None,
        &["task", "start", "Count the boats", "--budget", "9"],
    );
    daemon.join().unwrap();
    assert_eq!(out.status.code(), Some(1));
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("has left: $4.00"), "{err}");
}

#[test]
fn inside_a_job_a_start_is_refused_before_anything_is_sent() {
    let dir = tempfile::tempdir().unwrap();
    let sock = dir.path().join("sock");
    let _never_asked = UnixListener::bind(&sock).unwrap();
    let out = theseus(
        &sock,
        dir.path(),
        Some("ses_0000aa1b2c3"),
        &["task", "start", "Count the boats"],
    );
    assert_eq!(out.status.code(), Some(1));
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("theseus task start refused"), "{err}");
    assert!(err.contains("Run it from your own shell"), "{err}");
}
