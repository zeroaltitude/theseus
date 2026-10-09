//! What `history`, `watch` and `confirm` record in the machine's seen file,
//! and when (theseus-yus0): after their output and never before it, only
//! when they showed the session's end, on Ctrl-C for `watch`, and nothing for
//! a watch the daemon closed. Review tests beside `tests/seen.rs`.

use std::fs::File;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixListener;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};

use serde_json::{json, Value};

const THESEUS: &str = env!("CARGO_BIN_EXE_theseus");

fn view(position: u64) -> Value {
    json!({
        "position": position, "execution_id": "exe_1", "session_id": "ses_1",
        "kind": "conversation", "state": "waiting", "turns": 2,
        "attention": {"level": "ready", "label": "ready", "since_ms": 0},
    })
}

/// What the stand-in daemon saw: each method asked, and whether the command's
/// stdout (a file) already held bytes when the method arrived.
type Asked = Arc<Mutex<Vec<(String, bool)>>>;

struct Rig {
    dir: tempfile::TempDir,
    asked: Asked,
    daemon: Option<std::thread::JoinHandle<()>>,
}

impl Rig {
    fn join(&mut self) {
        self.daemon.take().unwrap().join().unwrap();
    }
    fn sock(&self) -> PathBuf {
        self.dir.path().join("sock")
    }
    fn state(&self) -> PathBuf {
        self.dir.path().join("state")
    }
    fn seen_file(&self) -> PathBuf {
        self.state().join("theseus/seen.json")
    }
    fn out(&self) -> PathBuf {
        self.dir.path().join("stdout")
    }
    fn asked(&self) -> Vec<String> {
        self.asked
            .lock()
            .unwrap()
            .iter()
            .map(|(m, _)| m.clone())
            .filter(|m| m != "session.list")
            .collect()
    }
}

/// A stand-in daemon for one connection. `history` is `session.history`'s
/// result; `close_after` makes it drop the connection after answering that
/// method (a daemon that closes a watch).
fn rig(history: Value, confirms: Value, close_after: Option<&'static str>) -> Rig {
    let dir = tempfile::tempdir().unwrap();
    let sock = dir.path().join("sock");
    let out = dir.path().join("stdout");
    let listener = UnixListener::bind(&sock).unwrap();
    let asked: Asked = Arc::default();
    let log = asked.clone();
    let daemon = std::thread::spawn(move || {
        let (s, _) = listener.accept().unwrap();
        for line in BufReader::new(&s).lines() {
            let Ok(line) = line else { break };
            let req: Value = serde_json::from_str(&line).unwrap();
            let method = req["method"].as_str().unwrap().to_string();
            let printed = std::fs::metadata(&out).is_ok_and(|m| m.len() > 0);
            log.lock().unwrap().push((method.clone(), printed));
            let result = match method.as_str() {
                "session.history" => history.clone(),
                "confirm.list" => confirms.clone(),
                "session.watch" => json!({}),
                // A command that names its session whole reads it alone
                // (client-names' `resolve::existing_session`).
                "session.list" => json!({"sessions": [history["session"].clone()]}),
                _ => {
                    json!({"reached": "settled", "already": true, "execution": view(42), "confirms": []})
                }
            };
            let reply = json!({"jsonrpc": "2.0", "id": req["id"], "result": result});
            (&s).write_all(format!("{reply}\n").as_bytes()).unwrap();
            if close_after == Some(method.as_str()) {
                break;
            }
        }
    });
    Rig {
        dir,
        asked,
        daemon: Some(daemon),
    }
}

fn history_result() -> Value {
    json!({
        "session": {"session_id": "ses_1", "kind": "conversation", "label": null,
            "created_at_unix_ms": 0, "turns": 2},
        "nodes": [],
    })
}

fn command(rig: &Rig, args: &[&str]) -> Command {
    let mut cmd = Command::new(THESEUS);
    cmd.arg("--socket")
        .arg(rig.sock())
        .args(args)
        .env("XDG_STATE_HOME", rig.state())
        .env_remove("THESEUS_SESSION")
        .stdout(File::create(rig.out()).unwrap());
    cmd
}

fn recorded(path: &Path) -> Option<u64> {
    let text = std::fs::read_to_string(path).ok()?;
    let file: Value = serde_json::from_str(&text).unwrap();
    file["executions"]["exe_1"]["displayed"].as_u64()
}

/// The record comes after the output: when the snapshot that gives its
/// position is asked, the command's stdout already holds its text.
#[test]
fn history_asks_for_the_position_only_after_it_printed() {
    let mut r = rig(history_result(), json!({"confirms": []}), None);
    let out = command(&r, &["history", "ses_1"]).output().unwrap();
    assert!(out.status.success());
    r.join();
    assert_eq!(
        *r.asked.lock().unwrap(),
        [
            ("session.history".to_string(), false),
            ("session.wait".to_string(), true)
        ],
        "stdout held the page when the snapshot was asked"
    );
    assert_eq!(recorded(&r.seen_file()), Some(42));
}

#[test]
fn a_page_back_records_nothing() {
    let mut r = rig(history_result(), json!({"confirms": []}), None);
    let out = command(&r, &["history", "ses_1", "--before", "10"])
        .output()
        .unwrap();
    assert!(out.status.success());
    r.join();
    assert_eq!(r.asked(), ["session.history"]);
    assert!(!r.seen_file().exists());
}

#[test]
fn a_page_with_more_to_come_records_nothing() {
    let mut h = history_result();
    h["next"] = json!(5);
    let mut r = rig(h, json!({"confirms": []}), None);
    let out = command(&r, &["history", "ses_1", "--after", "0"])
        .output()
        .unwrap();
    assert!(out.status.success());
    r.join();
    assert_eq!(r.asked(), ["session.history"]);
    assert!(!r.seen_file().exists());
}

#[test]
fn the_last_page_forward_records() {
    let mut r = rig(history_result(), json!({"confirms": []}), None);
    let out = command(&r, &["history", "ses_1", "--after", "3"])
        .output()
        .unwrap();
    assert!(out.status.success());
    r.join();
    assert_eq!(recorded(&r.seen_file()), Some(42));
}

/// Ctrl-C ends a watch with exit 0, after it recorded what it showed.
#[test]
fn a_watch_stopped_with_ctrl_c_records_and_exits_zero() {
    let mut r = rig(history_result(), json!({"confirms": []}), None);
    let mut child = command(&r, &["watch", "ses_1"])
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut err = BufReader::new(child.stderr.take().unwrap());
    let mut line = String::new();
    err.read_line(&mut line).unwrap();
    assert!(line.starts_with("watching ses_1"), "{line}");
    // SAFETY: a signal to the child this test started.
    unsafe { libc::kill(child.id() as i32, libc::SIGINT) };
    let status = child.wait().unwrap();
    r.join();
    assert_eq!(status.code(), Some(0), "a watch ends 0 on Ctrl-C");
    assert_eq!(r.asked(), ["session.watch", "session.wait"]);
    assert_eq!(recorded(&r.seen_file()), Some(42));
}

/// A watch the daemon closes has nothing to ask: nothing is recorded.
#[test]
fn a_watch_the_daemon_closes_records_nothing() {
    let mut r = rig(
        history_result(),
        json!({"confirms": []}),
        Some("session.watch"),
    );
    let out = command(&r, &["watch", "ses_1"]).output().unwrap();
    r.join();
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(r.asked(), ["session.watch"]);
    assert!(!r.seen_file().exists());
}

#[test]
fn confirm_records_the_sessions_it_listed_after_printing() {
    let confirm = json!({"correlation_id": "act_k4", "session_id": "ses_1", "execution_id": "exe_1",
        "tool": "fs.write", "input": {}, "reason": "outside the roots", "by": "operator",
        "requested_at_ms": 1u64, "expires_at_ms": 0u64});
    let mut r = rig(history_result(), json!({"confirms": [confirm]}), None);
    let out = command(&r, &["confirm"]).output().unwrap();
    assert!(out.status.success());
    r.join();
    assert_eq!(
        *r.asked.lock().unwrap(),
        [
            ("confirm.list".to_string(), false),
            ("session.wait".to_string(), true)
        ]
    );
    assert_eq!(recorded(&r.seen_file()), Some(42));
}

/// Nothing waiting, nothing listed, nothing asked.
#[test]
fn confirm_with_nothing_waiting_asks_for_no_snapshot() {
    let mut r = rig(history_result(), json!({"confirms": []}), None);
    let out = command(&r, &["confirm"]).output().unwrap();
    assert!(out.status.success());
    r.join();
    assert_eq!(r.asked(), ["confirm.list"]);
}

/// An agent's reading inside a job is not the operator's: nothing is recorded.
#[test]
fn a_command_run_inside_a_job_records_nothing() {
    let mut r = rig(history_result(), json!({"confirms": []}), None);
    let out = command(&r, &["history", "ses_1"])
        .env("THESEUS_SESSION", "ses_job")
        .output()
        .unwrap();
    assert!(out.status.success());
    r.join();
    assert_eq!(r.asked(), ["session.history"]);
    assert!(!r.seen_file().exists());
}
