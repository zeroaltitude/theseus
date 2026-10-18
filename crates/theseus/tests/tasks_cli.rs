//! `theseus tasks` and `theseus tasks --watch` (theseus-mnd9g), the real binary against
//! a scripted daemon on a Unix socket: one `work.watch` read draws the tree,
//! `--graph` and `--json` keep the task list's own, `--watch` and `work` tell
//! the daemon a person is looking and print a line for each task event.

use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::Path;
use std::process::Command;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde_json::{json, Value};

const THESEUS: &str = env!("CARGO_BIN_EXE_theseus");

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64
}

fn ago(ms: u64) -> u64 {
    now_ms() - ms
}

fn view(v: Value) -> Value {
    let mut base = json!({
        "id": "exe_1", "kind": "task", "position": 9, "at_ms": ago(1000), "root": "ses_conv",
        "parent": "ses_conv", "title": "", "state": "running",
        "attention": {"level": "working", "label": "turn 1", "since_ms": ago(60_000)},
        "started_at_ms": ago(60_000), "state_since_ms": ago(60_000)
    });
    for (k, x) in v.as_object().unwrap() {
        base[k] = x.clone();
    }
    base
}

/// A conversation with an asking task, a working one and a done one.
fn board() -> Vec<Value> {
    vec![
        view(json!({
            "id": "exe_conv", "kind": "conversation", "session_id": "ses_conv", "parent": null,
            "title": "Regatta prep", "state": "waiting",
            "attention": {"level": "working", "label": "waiting on 3 tasks", "since_ms": ago(90_000)},
            "started_at_ms": ago(900_000)
        })),
        view(json!({
            "id": "exe_paint", "session_id": "ses_paint2a329e", "title": "Paint log",
            "state": "needs_you",
            "attention": {"level": "needs_you", "label": "confirm fs.write", "since_ms": ago(50_000)},
            "question": {
                "id": "act_x2a329e", "kind": "approval", "tool": "fs.write",
                "prompt": "write regatta/paint-log.md (37 B)",
                "asked_by": {"id": "exe_paint", "kind": "task", "title": "Paint log"},
                "options": [
                    {"id": "approve", "label": "Approve", "style": "primary", "takes_note": false, "effect": "the call runs"},
                    {"id": "decline", "label": "Decline", "style": "plain", "takes_note": true, "effect": "the call does not run"}],
                "floor": false, "may_answer": true, "asked_at_ms": ago(50_000)
            }
        })),
        view(json!({
            "id": "exe_tide", "session_id": "ses_tide77c0de", "title": "Tide tables",
            "doing": {"what": "thinking", "label": "turn 1", "since_ms": ago(31_000), "turn": 1, "loop_index": 0}
        })),
        view(json!({
            "id": "exe_wx", "session_id": "ses_wxbeef01", "title": "Weather brief",
            "state": "done", "ended_at_ms": ago(10_000),
            "attention": {"level": "idle", "label": "complete", "since_ms": ago(10_000)},
            "report": {"outcome": "done", "first_line": "Fair winds Saturday"}
        })),
    ]
}

/// What a scripted daemon was asked.
type Asked = Arc<Mutex<Vec<(String, Value)>>>;

/// Serve one connection: answer by method, then send `then` after the answer
/// to `work.watch`, and hold the connection `hold` (zero: close at once).
fn serve(s: UnixStream, answers: &[(&str, Value)], then: &[Value], hold: Duration, asked: &Asked) {
    let mut reader = BufReader::new(&s);
    let mut line = String::new();
    while reader.read_line(&mut line).unwrap_or(0) > 0 {
        let req: Value = serde_json::from_str(&line).unwrap();
        line.clear();
        let method = req["method"].as_str().unwrap().to_string();
        asked
            .lock()
            .unwrap()
            .push((method.clone(), req["params"].clone()));
        let result = answers
            .iter()
            .find(|(m, _)| *m == method)
            .map_or(json!({}), |(_, r)| r.clone());
        let answer = json!({"jsonrpc": "2.0", "id": req["id"], "result": result});
        (&s).write_all(format!("{answer}\n").as_bytes()).unwrap();
        if method == "work.watch" {
            for n in then {
                if (&s).write_all(format!("{n}\n").as_bytes()).is_err() {
                    return;
                }
            }
            if hold.is_zero() {
                return;
            }
            std::thread::sleep(hold);
            return;
        }
    }
}

fn changed(views: Vec<Value>, position: u64) -> Value {
    json!({"jsonrpc": "2.0", "method": "work.changed",
        "params": {"position": position, "at_ms": now_ms(), "views": views}})
}

fn theseus(sock: &Path, args: &[&str]) -> Command {
    let mut c = Command::new(THESEUS);
    c.arg("--socket")
        .arg(sock)
        .args(args)
        .env_remove("THESEUS_SOCKET")
        .env_remove("THESEUS_SESSION")
        .env("XDG_STATE_HOME", sock.parent().unwrap().join("state"))
        .env("TZ", "America/Phoenix")
        .env("COLUMNS", "100");
    c
}

fn run(
    args: &[&str],
    answers: Vec<(&'static str, Value)>,
    then: Vec<Value>,
) -> (String, String, Vec<(String, Value)>) {
    let dir = tempfile::tempdir().unwrap();
    let sock = dir.path().join("sock");
    let listener = UnixListener::bind(&sock).unwrap();
    let asked: Asked = Arc::default();
    let seen = asked.clone();
    let daemon = std::thread::spawn(move || {
        let (s, _) = listener.accept().unwrap();
        serve(s, &answers, &then, Duration::ZERO, &seen);
    });
    let out = theseus(&sock, args).output().unwrap();
    daemon.join().unwrap();
    let asked = asked.lock().unwrap().clone();
    (
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
        asked,
    )
}

fn snapshot() -> Value {
    json!({"position": 10, "views": board(), "total": 1})
}

/// One `work.watch` read draws the tree; the person is not looking (a plain
/// `tasks` is no screen), nothing else is asked but the watch's end.
#[test]
fn tasks_is_one_read_and_a_tree() {
    let (out, _, asked) = run(&["tasks"], vec![("work.watch", snapshot())], vec![]);
    let reads: Vec<&(String, Value)> = asked.iter().filter(|(m, _)| m == "work.watch").collect();
    assert_eq!(reads.len(), 1, "{asked:?}");
    assert_eq!(reads[0].1["interactive"], json!(false), "{:?}", reads[0].1);
    assert_eq!(reads[0].1["limit"], json!(20));
    assert!(
        !asked.iter().any(|(m, _)| m == "task.list"),
        "the tree is the board's alone: {asked:?}"
    );
    let lines: Vec<&str> = out.lines().collect();
    assert!(lines[0].ends_with("Regatta prep"), "{out}");
    let rows: Vec<&&str> = lines
        .iter()
        .filter(|l| l.contains("Paint log") || l.contains("Tide tables") || l.contains("Weather"))
        .collect();
    assert_eq!(rows.len(), 3, "one line a task: {out}");
    assert!(
        out.contains("asks: write regatta/paint-log.md (37 B)"),
        "{out}"
    );
    assert!(
        out.contains("theseus answer 2a329e approve · theseus answer 2a329e decline --note …"),
        "the command beneath: {out}"
    );
    assert!(out.contains("model 0:3"), "{out}");
    assert!(out.contains("Fair winds Saturday"), "{out}");
    assert_eq!(
        lines.last().copied(),
        Some("1 needs you · 1 working · 1 done"),
        "{out}"
    );
    assert!(lines.iter().all(|l| l.chars().count() <= 100), "{out}");
}

/// `--json` and `--graph` are the task list's own, as before.
#[test]
fn json_and_graph_keep_the_task_list() {
    let list = json!({"tasks": [], "records": []});
    for args in [&["tasks", "--json"][..], &["tasks", "--graph"][..]] {
        let (out, _, asked) = run(args, vec![("task.list", list.clone())], vec![]);
        let methods: Vec<&str> = asked.iter().map(|(m, _)| m.as_str()).collect();
        assert_eq!(methods, ["task.list"], "{args:?}");
        if args.contains(&"--json") {
            assert_eq!(out.trim(), list.to_string());
        } else {
            assert_eq!(out, "no tasks\n");
        }
    }
}

/// `--session` names the tree's one root.
#[test]
fn a_session_narrows_the_watch_to_its_root() {
    let (_, _, asked) = run(
        &["tasks", "--session", "ses_0123456789abcdef"],
        vec![("work.watch", snapshot())],
        vec![],
    );
    let read = asked.iter().find(|(m, _)| m == "work.watch").unwrap();
    assert_eq!(
        read.1["roots"],
        json!(["ses_0123456789abcdef"]),
        "{asked:?}"
    );
}

/// `--watch` is a screen a person looks at, and says each task event: a new
/// task started, a question asked, a report, a failure.
#[test]
fn watch_says_each_task_event_and_is_a_screen() {
    let started = view(json!({
        "id": "exe_buoy", "session_id": "ses_buoy5a877c", "title": "Buoy survey",
        "position": 11
    }));
    let failed = view(json!({
        "id": "exe_rig", "session_id": "ses_rig0f1e2d", "title": "Rigging check",
        "state": "failed", "position": 12,
        "attention": {"level": "needs_you", "label": "failed: the provider refused the request (400)", "since_ms": ago(1000)},
        "report": {"outcome": "failed", "reason": "the provider refused the request (400)"}
    }));
    let args = &["tasks", "--watch"][..];
    let (out, _, asked) = run(
        args,
        vec![("work.watch", snapshot())],
        vec![changed(vec![started], 11), changed(vec![failed], 12)],
    );
    let read = asked.iter().find(|(m, _)| m == "work.watch").unwrap();
    assert_eq!(read.1["interactive"], json!(true), "{args:?}");
    assert!(
        out.contains("◐ started: Buoy survey (5a877c)"),
        "{args:?}: {out}"
    );
    assert!(
        out.contains("✗ Rigging check failed: the provider refused the request (400)"),
        "{args:?}: {out}"
    );
    // A failure names the two verbs that deal with it.
    assert!(
        out.contains("  theseus retry 0f1e2d · theseus dismiss 0f1e2d"),
        "{out}"
    );
    // The tree first, the events after it.
    let tree = out.find("Regatta prep").unwrap();
    assert!(tree < out.find("started:").unwrap(), "{out}");
}

/// `theseus tasks --watch` (the old `theseus work`, theseus-6kgp8): it keeps the connection, tells the daemon a
/// person is looking, and prints what the work board showed: the thread's group, the job's id for `peek`
/// and `attach`, a line for the job's change and the thread's.
#[test]
fn work_is_tasks_watch_and_keeps_the_threads_and_the_job_ids() {
    let mut views = board();
    views[0]["thread"] = json!("regatta");
    views[2]["thread"] = json!("regatta");
    let job = |position: u64, at: u64, last: &str| {
        view(json!({
            "id": "act_job_c0ffee", "kind": "job", "parent": "exe_tide", "title": "cargo build",
            "position": position, "at_ms": at,
            "doing": {"what": "job", "label": "cargo build", "since_ms": ago(5_000), "turn": 1,
                "loop_index": 0, "last_line": last},
            "attention": {"level": "working", "label": "running", "since_ms": ago(5_000)}
        }))
    };
    views.push(job(10, ago(2_000), "Compiling theseus"));
    let snap = json!({"position": 10, "views": views, "total": 1, "threads": [
        {"slug": "regatta", "title": "Regatta thread", "state": "active", "session_id": "ses_conv",
         "last_active_ms": ago(60_000)}]});
    let moved = json!({"slug": "regatta", "title": "Regatta thread, renamed", "state": "active",
        "session_id": "ses_conv", "last_active_ms": ago(1_000)});
    let after = {
        let mut n = changed(vec![job(12, now_ms(), "Finished dev profile")], 12);
        n["params"]["threads"] = json!([moved]);
        n
    };
    let (out, _, asked) = run(
        &["tasks", "--watch"],
        vec![("work.watch", snap)],
        vec![after],
    );
    let read = asked.iter().find(|(m, _)| m == "work.watch").unwrap();
    assert_eq!(
        read.1["interactive"],
        json!(true),
        "work watches: {asked:?}"
    );
    assert!(out.starts_with("Threads: 1 live"), "{out}");
    assert!(out.contains("Regatta thread"), "{out}");
    assert!(
        out.contains("Jobs (theseus peek JOB · theseus attach JOB):\n  c0ffee  ◐ cargo build"),
        "{out}"
    );
    assert!(out.contains("@12 job c0ffee"), "{out}");
    assert!(out.contains("› Finished dev profile"), "{out}");
    assert!(
        out.contains("@12 ") && out.contains("Regatta thread, renamed"),
        "{out}"
    );
}
