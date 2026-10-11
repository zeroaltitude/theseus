//! `theseus postmortem` (theseus-m2tiu), the real binary:
//! - against a scripted daemon: the window read a page at a time, the default
//!   window, what `--session` and `--task` send, `--out`, `--json`, and the
//!   daemon's refusal said as it is;
//! - against a recorded incident (`tests/fixtures/postmortem/`), with no
//!   daemon: its markdown and its `--json` against goldens, and every fact the
//!   incident's script caused appearing once, in order.

use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, Mutex};

use serde_json::{json, Value};

const THESEUS: &str = env!("CARGO_BIN_EXE_theseus");

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64
}

fn row(position: u64, at: u64, kind: &str, session: &str, turn: &str, data: Value) -> Value {
    json!({"position": position, "at_unix_ms": at, "kind": kind, "session_id": session,
           "turn_id": turn, "data": data})
}

/// A daemon that answers each `ledger.window` with the next of `pages`, and
/// keeps every request's params.
struct Scripted {
    _dir: tempfile::TempDir,
    sock: PathBuf,
    asked: Arc<Mutex<Vec<Value>>>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl Scripted {
    fn start(pages: Vec<Result<Value, &'static str>>) -> Self {
        let dir = tempfile::tempdir().unwrap();
        let sock = dir.path().join("sock");
        let listener = UnixListener::bind(&sock).unwrap();
        let asked: Arc<Mutex<Vec<Value>>> = Arc::default();
        let seen = asked.clone();
        let thread = std::thread::spawn(move || {
            let (s, _) = listener.accept().unwrap();
            serve(s, pages, &seen);
        });
        Self {
            _dir: dir,
            sock,
            asked,
            thread: Some(thread),
        }
    }

    fn run(&mut self, args: &[&str]) -> (String, String, i32) {
        let out = Command::new(THESEUS)
            .arg("--socket")
            .arg(&self.sock)
            .args(args)
            .env_remove("THESEUS_SOCKET")
            .env_remove("THESEUS_SESSION")
            .env("TZ", "UTC")
            .output()
            .unwrap();
        if let Some(t) = self.thread.take() {
            t.join().unwrap();
        }
        (
            String::from_utf8_lossy(&out.stdout).into_owned(),
            String::from_utf8_lossy(&out.stderr).into_owned(),
            out.status.code().unwrap_or(-1),
        )
    }
}

fn serve(s: UnixStream, mut pages: Vec<Result<Value, &'static str>>, seen: &Mutex<Vec<Value>>) {
    let mut reader = BufReader::new(&s);
    let mut line = String::new();
    pages.reverse();
    while reader.read_line(&mut line).unwrap_or(0) > 0 {
        let req: Value = serde_json::from_str(&line).unwrap();
        line.clear();
        let answer = if req["method"] == "ledger.window" {
            seen.lock().unwrap().push(req["params"].clone());
            match pages.pop().expect("a page for the request") {
                Ok(result) => json!({"jsonrpc": "2.0", "id": req["id"], "result": result}),
                Err(why) => json!({"jsonrpc": "2.0", "id": req["id"],
                    "error": {"code": -32602, "message": why}}),
            }
        } else {
            json!({"jsonrpc": "2.0", "id": req["id"], "result": {}})
        };
        (&s).write_all(format!("{answer}\n").as_bytes()).unwrap();
    }
}

fn page(rows: Vec<Value>, next: Option<u64>, scope: &[&str]) -> Value {
    let mut p = json!({"rows": rows, "scope": scope, "now_ms": now_ms()});
    if let Some(n) = next {
        p["next"] = json!(n);
    }
    p
}

/// The window is read a page at a time from where the last page ended, and
/// the report holds the rows of every page. With no flags it is the last 12
/// hours.
#[test]
fn the_window_is_read_a_page_at_a_time() {
    let now = now_ms();
    let t = now - 3_600_000;
    let first = vec![
        row(
            10,
            t,
            "turn.started",
            "ses_aaaa1111",
            "turn_one",
            json!({"author": "owner"}),
        ),
        row(
            11,
            t + 1_000,
            "turn.ended",
            "ses_aaaa1111",
            "turn_one",
            json!({"loops": 1, "tool_calls": 0, "cost_usd": 0.01}),
        ),
    ];
    let second = vec![
        row(
            20,
            t + 5_000,
            "turn.started",
            "ses_aaaa1111",
            "turn_two",
            json!({"author": "owner"}),
        ),
        row(
            21,
            t + 6_000,
            "turn.failed",
            "ses_aaaa1111",
            "turn_two",
            json!({"loops": 1, "reason": "provider: overloaded"}),
        ),
    ];
    let mut d = Scripted::start(vec![
        Ok(page(first, Some(11), &[])),
        Ok(page(second, None, &[])),
    ]);
    let (out, err, code) = d.run(&["postmortem"]);
    assert_eq!(code, 0, "{err}");
    let asked = d.asked.lock().unwrap().clone();
    assert_eq!(asked.len(), 2, "{asked:?}");
    assert!(asked[0].get("after").is_none(), "{asked:?}");
    assert_eq!(asked[1]["after"], 11, "{asked:?}");
    let since = asked[0]["since_ms"].as_u64().unwrap();
    assert!(
        since.abs_diff(now - 12 * 3_600_000) < 10_000,
        "the last 12 hours: {since} for now {now}"
    );
    assert!(asked[0].get("session_id").is_none());
    for fact in [
        "turn_one",
        "turn_two FAILED",
        "provider: overloaded",
        "## Summary",
    ] {
        assert!(out.contains(fact), "`{fact}` is in:\n{out}");
    }
}

/// `--session` and `--task` name what the daemon narrows to, as given when
/// whole, and the report says what it was narrowed to.
#[test]
fn a_session_and_a_task_narrow_the_window() {
    let t = now_ms() - 1_000;
    let rows = vec![row(
        5,
        t,
        "turn.started",
        "ses_aaaa1111",
        "turn_one",
        json!({"author": "owner"}),
    )];
    let mut d = Scripted::start(vec![Ok(page(
        rows.clone(),
        None,
        &["ses_aaaa1111", "ses_task0001"],
    ))]);
    let (out, err, code) = d.run(&["postmortem", "--session", "ses_aaaa1111", "--since", "2h"]);
    assert_eq!(code, 0, "{err}");
    assert_eq!(d.asked.lock().unwrap()[0]["session_id"], "ses_aaaa1111");
    assert!(
        out.contains("narrowed to ses_aaaa1111, ses_task0001"),
        "{out}"
    );
    assert!(out.contains("### ses_aaaa1111"), "{out}");

    let mut d = Scripted::start(vec![Ok(page(rows, None, &["ses_task0001"]))]);
    let (_, err, code) = d.run(&["postmortem", "--task", "tsk_task0001"]);
    assert_eq!(code, 0, "{err}");
    assert_eq!(d.asked.lock().unwrap()[0]["session_id"], "tsk_task0001");
}

/// `--out` writes the file and prints nothing; `--json` is the same facts as
/// data; the daemon's refusal is said as it is, with exit 1.
#[test]
fn out_json_and_a_refusal() {
    let t = now_ms() - 1_000;
    let rows = vec![row(
        5,
        t,
        "turn.started",
        "ses_aaaa1111",
        "turn_one",
        json!({"author": "owner"}),
    )];
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("pm.md");
    let mut d = Scripted::start(vec![Ok(page(rows.clone(), None, &[]))]);
    let (out, err, code) = d.run(&["postmortem", "--out", file.to_str().unwrap()]);
    assert_eq!((code, out.as_str()), (0, ""), "{err}");
    assert!(std::fs::read_to_string(&file)
        .unwrap()
        .contains("# Postmortem"));

    let mut d = Scripted::start(vec![Ok(page(rows, None, &[]))]);
    let (out, err, code) = d.run(&["--json", "postmortem"]);
    assert_eq!(code, 0, "{err}");
    let v: Value = serde_json::from_str(&out).unwrap();
    assert_eq!(v["summary"]["turns"], 1);
    assert!(v["timeline"].is_array() && v["cost"].is_object() && v["open"].is_object());

    let mut d = Scripted::start(vec![Err(
        "the Discord binding reads no private text: name the session",
    )]);
    let (_, err, code) = d.run(&["postmortem"]);
    assert_eq!(code, 1);
    assert!(err.contains("name the session"), "{err}");
}

/// A window whose index is still being built after a start says so.
#[test]
fn a_window_not_yet_indexed_asks_to_try_again() {
    let mut d = Scripted::start(vec![Ok(
        json!({"rows": [], "building": true, "now_ms": now_ms()}),
    )]);
    let (_, err, code) = d.run(&["postmortem"]);
    assert_eq!(code, 1);
    assert!(err.contains("still being built"), "{err}");
}

/// Rows kept by `--rows-out` report offline, the same as the daemon's pages.
#[test]
fn kept_rows_report_with_no_daemon() {
    let t = now_ms() - 5_000;
    let rows = vec![
        row(
            5,
            t,
            "turn.started",
            "ses_aaaa1111",
            "turn_one",
            json!({"author": "owner"}),
        ),
        row(
            6,
            t + 900,
            "turn.ended",
            "ses_aaaa1111",
            "turn_one",
            json!({"loops": 1, "tool_calls": 0, "cost_usd": 0.01}),
        ),
    ];
    let dir = tempfile::tempdir().unwrap();
    let kept = dir.path().join("rows.jsonl");
    let mut d = Scripted::start(vec![Ok(page(rows, None, &[]))]);
    let (online, err, code) = d.run(&["postmortem", "--rows-out", kept.to_str().unwrap()]);
    assert_eq!(code, 0, "{err}");
    let out = Command::new(THESEUS)
        .args(["--socket", "/nonexistent/sock", "postmortem", "--from-rows"])
        .arg(&kept)
        .args(["--since", "1h"])
        .env("TZ", "UTC")
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let offline = String::from_utf8_lossy(&out.stdout);
    // The window's own words differ (the end is the last row's, not now); the facts do not.
    for fact in ["turn_one", "1 turn run, 0 failed"] {
        assert!(offline.contains(fact) && online.contains(fact), "`{fact}`");
    }
}

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/postmortem")
        .join(name)
}

/// The recorded incident, reported with no daemon, at a fixed zone.
fn report_of_incident(extra: &[&str]) -> String {
    let out = Command::new(THESEUS)
        .args(["--socket", "/nonexistent/sock"])
        .args(extra)
        .arg("postmortem")
        .arg("--from-rows")
        .arg(fixture("incident.jsonl"))
        .env("TZ", "<-07>7")
        .env_remove("THESEUS_SESSION")
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout).unwrap()
}

/// `THESEUS_GOLDEN=write` rewrites a golden: for a change to the report you
/// mean, with the diff read.
fn golden(name: &str, got: &str) {
    let path = fixture(name);
    if std::env::var("THESEUS_GOLDEN").as_deref() == Ok("write") {
        std::fs::write(&path, got).unwrap();
        return;
    }
    let want = std::fs::read_to_string(&path).unwrap();
    assert!(
        want == got,
        "{name} differs from the report; THESEUS_GOLDEN=write rewrites it:\n{got}"
    );
}

#[test]
fn the_incidents_markdown_is_its_golden() {
    golden("incident.md", &report_of_incident(&[]));
}

#[test]
fn the_incidents_json_is_its_golden() {
    let json = report_of_incident(&["--json"]);
    let v: Value = serde_json::from_str(&json).unwrap();
    assert_eq!(v["summary"]["restarts"], 1);
    golden("incident.json", &json);
}

/// Every fact the incident's script caused is in the timeline, as many times
/// as it happened, in the order it happened.
#[test]
fn every_fact_the_script_caused_appears_once_in_order() {
    let md = report_of_incident(&[]);
    let timeline = md
        .split("## Timeline")
        .nth(1)
        .and_then(|t| t.split("## Cost").next())
        .unwrap();
    // (what the script did, how it reads, how many times it happened)
    #[rustfmt::skip]
    let facts = [
        ("a tool failed", "tool call `fs.read` failed", 1),
        ("a question was asked and answered", "approved by the CLI from cli after 4.0 s", 1),
        ("a task was created", "created: Run the long job and report.", 1),
        ("and opened", "task opened, limit $25.00", 1),
        ("the kill cut two turns", "interrupted by a restart of the daemon", 2),
        ("the daemon came back", "daemon started again after it was killed", 1),
        ("two turns resumed", "started (resumed after a restart)", 2),
        ("a question expired", "expired unanswered after 25.0 s", 1),
        ("the long command ran 33 s", "tool call `proc.run` ran 33.0 s", 1),
        ("the task ended", "ended complete", 1),
    ];
    let mut at = 0;
    for (what, words, times) in facts {
        assert_eq!(
            timeline.matches(words).count(),
            times,
            "{what}: `{words}`\n{timeline}"
        );
        let first = timeline.find(words).unwrap();
        assert!(first >= at, "{what} is out of order:\n{timeline}");
        at = first;
    }
    for section in [
        "## Summary",
        "## Restarts",
        "## Cost",
        "## Asked and answered",
        "## Open at the end",
    ] {
        assert_eq!(md.matches(section).count(), 1, "{section}");
    }
    // The sections come in the order the document is specified in.
    let order: Vec<usize> = [
        "## Summary",
        "## Restarts",
        "## Timeline",
        "## Cost",
        "## Asked and answered",
        "## Open at the end",
    ]
    .iter()
    .map(|s| md.find(s).unwrap())
    .collect();
    assert!(order.windows(2).all(|w| w[0] < w[1]), "{order:?}");
    // The summary is five lines at most.
    let summary = md
        .split("## Summary")
        .nth(1)
        .unwrap()
        .split("## Restarts")
        .next()
        .unwrap();
    assert!(summary.lines().filter(|l| l.starts_with("- ")).count() <= 5);
}
