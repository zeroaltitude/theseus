//! `theseus digest` and `theseus status`'s last line (theseus-qkoi3), the real
//! binary against a scripted daemon on a Unix socket: what is asked (a time or
//! the machine's last look), what is printed, what is recorded as looked at, and
//! that nothing is recorded inside a job.

use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, Mutex};

use serde_json::{json, Value};

const THESEUS: &str = env!("CARGO_BIN_EXE_theseus");

fn at(h: u64, m: u64) -> u64 {
    // 2026-10-14 18:00 UTC-7 is 01:00 UTC the next day; the zone is pinned below.
    1_792_026_000_000 + h * 3_600_000 + m * 60_000
}

fn digest() -> Value {
    json!({
        "from": 500, "to": 900, "from_ledger": false,
        "now": {"needs_you": 1, "working": 0, "done": 3, "failed": 1, "total": 5},
        "questions": [{
            "id": "act_0000a1b2c3", "kind": "approval",
            "asked_by": {"id": "exe_t", "kind": "task", "title": "Kelp count"},
            "prompt": "fs.write create notes.txt (40 B)", "options": [], "floor": false,
            "may_answer": true, "asked_at_ms": at(0, 0)
        }],
        "lines": [
            {"at_ms": at(0, 20), "kind": "reported", "count": 1, "words": "📋 Beta summary reported",
             "work": {"id": "exe_b", "kind": "task", "title": "Beta summary"}},
            {"at_ms": at(0, 40), "kind": "answered", "count": 1, "words": "✓ you answered: fs.write notes.txt",
             "work": {"id": "act_1", "kind": "question", "title": "fs.write notes.txt"}}
        ]
    })
}

/// A daemon on `sock` that answers `work.digest` and records its params, and
/// an empty board for `executions.watch`.
fn daemon(sock: &Path) -> (std::thread::JoinHandle<()>, Arc<Mutex<Vec<Value>>>) {
    let listener = UnixListener::bind(sock).unwrap();
    let asked = Arc::new(Mutex::new(Vec::new()));
    let seen = asked.clone();
    listener.set_nonblocking(true).unwrap();
    let path = sock.to_path_buf();
    // Any number of connections (`status` opens several), until the socket's file is removed.
    let h = std::thread::spawn(move || {
        while path.exists() {
            match listener.accept() {
                Ok((s, _)) => {
                    s.set_nonblocking(false).unwrap();
                    let seen = seen.clone();
                    std::thread::spawn(move || serve(s, &seen));
                }
                Err(_) => std::thread::sleep(std::time::Duration::from_millis(5)),
            }
        }
    });
    (h, asked)
}

fn serve(s: UnixStream, asked: &Mutex<Vec<Value>>) {
    let mut reader = BufReader::new(&s);
    let mut line = String::new();
    while reader.read_line(&mut line).unwrap_or(0) > 0 {
        let req: Value = serde_json::from_str(&line).unwrap();
        line.clear();
        let result = match req["method"].as_str().unwrap() {
            "work.digest" => {
                asked.lock().unwrap().push(req["params"].clone());
                digest()
            }
            "executions.watch" => {
                json!({"position": 10, "executions": [], "confirms": [], "total": 0})
            }
            "status.short" => json!({"position": 10, "needs_you": 0, "working": 0, "failed": []}),
            "work.watch" => json!({"position": 10, "views": [], "total": 0}),
            _ => json!({}),
        };
        let answer = json!({"jsonrpc": "2.0", "id": req["id"], "result": result});
        (&s).write_all(format!("{answer}\n").as_bytes()).unwrap();
    }
}

struct World {
    dir: tempfile::TempDir,
}

impl World {
    fn new() -> Self {
        Self {
            dir: tempfile::tempdir().unwrap(),
        }
    }
    fn sock(&self) -> PathBuf {
        self.dir.path().join("sock")
    }
    fn seen(&self) -> PathBuf {
        self.dir.path().join("state/theseus/seen.json")
    }
    /// The machine looked at the board at `position`, at `at_ms`.
    fn looked(&self, position: u64, at_ms: u64) {
        std::fs::create_dir_all(self.seen().parent().unwrap()).unwrap();
        std::fs::write(
            self.seen(),
            json!({"version": 1, "board_read": true, "executions": {},
                   "looked": {"position": position, "at_ms": at_ms}})
            .to_string(),
        )
        .unwrap();
    }
    fn position(&self) -> u64 {
        let v: Value =
            serde_json::from_str(&std::fs::read_to_string(self.seen()).unwrap()).unwrap();
        v["looked"]["position"].as_u64().unwrap_or(0)
    }
    fn run(&self, args: &[&str], job: bool) -> (String, Vec<Value>) {
        let (d, asked) = daemon(&self.sock());
        let mut c = Command::new(THESEUS);
        c.arg("--socket")
            .arg(self.sock())
            .args(args)
            .env_remove("THESEUS_SOCKET")
            .env_remove("THESEUS_SESSION")
            .env_remove("TMUX")
            .env("XDG_STATE_HOME", self.dir.path().join("state"))
            .env("TZ", "America/Phoenix")
            .env("COLUMNS", "100");
        if job {
            c.env("THESEUS_SESSION", "ses_a_job");
        }
        let out = c.output().unwrap();
        let _ = std::fs::remove_file(self.sock());
        d.join().unwrap();
        assert!(
            out.status.success(),
            "{:?} {} {}",
            out.status,
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
        let asked = asked.lock().unwrap().clone();
        (String::from_utf8_lossy(&out.stdout).into_owned(), asked)
    }
}

#[test]
fn the_digest_since_a_time_asks_for_it_prints_the_lines_and_records_the_look() {
    let w = World::new();
    let (out, asked) = w.run(&["digest", "--since", "2h"], false);
    assert_eq!(asked.len(), 1);
    assert!(
        asked[0]["since_ms"].as_u64().unwrap() > 1_700_000_000_000,
        "{asked:?}"
    );
    assert!(asked[0].get("since").is_none() && asked[0].get("roots").is_none());
    let lines: Vec<&str> = out.lines().collect();
    assert!(
        lines[0].starts_with("◆ 1 reported, 1 answered by you since "),
        "{out}"
    );
    assert!(lines[1].ends_with("📋 Beta summary reported"), "{out}");
    assert!(
        lines[2].ends_with("✓ you answered: fs.write notes.txt"),
        "{out}"
    );
    assert_eq!(lines[3], "now: 1 need you, 1 failed of 5 on the board");
    assert_eq!(
        lines[4],
        "● fs.write create notes.txt (40 B)? · theseus answer a1b2c3"
    );
    assert_eq!(
        w.position(),
        900,
        "the board was shown: the look moved to its position"
    );
}

#[test]
fn since_seen_asks_from_the_last_look_and_a_session_scopes_it_and_records_nothing() {
    let w = World::new();
    w.looked(500, at(0, 0));
    let (_, asked) = w.run(&["digest", "--since-seen"], false);
    assert_eq!(asked, [json!({"since": 500})]);
    assert_eq!(w.position(), 900);
    // One task's timeline is not the board: asked of it, and no look recorded.
    let w = World::new();
    w.looked(500, at(0, 0));
    let (_, asked) = w.run(&["digest", "--session", "ses_task"], false);
    assert_eq!(asked, [json!({"since": 500, "roots": ["ses_task"]})]);
    assert_eq!(w.position(), 500);
}

#[test]
fn nothing_is_recorded_inside_a_job_and_json_is_the_answer_as_it_came() {
    let w = World::new();
    w.looked(500, at(0, 0));
    let (_, _) = w.run(&["digest"], true);
    assert_eq!(w.position(), 500, "a job's reading is not the operator's");
    let (out, _) = w.run(&["--json", "digest"], false);
    let v: Value = serde_json::from_str(&out).unwrap();
    assert_eq!(v["result"]["to"], 900);
}

#[test]
fn status_ends_with_one_line_when_something_happened_since_the_last_look() {
    let w = World::new();
    // Never looked: status starts nothing and says nothing of it.
    let (out, asked) = w.run(&["status"], false);
    assert!(asked.is_empty(), "{asked:?}");
    assert!(!out.contains("theseus digest"), "{out}");
    w.looked(500, at(0, 0));
    let (out, asked) = w.run(&["status"], false);
    assert_eq!(asked, [json!({"since": 500})]);
    let last = out.lines().last().unwrap();
    assert!(
        last.starts_with("◆ 1 reported, 1 answered by you since ")
            && last.ends_with(" · theseus digest"),
        "{out}"
    );
    // Status is not a look: the line is there until the digest is read.
    assert_eq!(w.position(), 500);
    // The prompt's and the script's forms say nothing of it.
    let (out, asked) = w.run(&["status", "--short"], false);
    assert!(asked.is_empty() && !out.contains("theseus digest"), "{out}");
}
