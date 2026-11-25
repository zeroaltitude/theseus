//! `theseus digest` and `theseus status`'s last line (theseus-qkoi3), the real
//! binary against a scripted daemon on a Unix socket: what is asked (a time or
//! the store's look, issue 46), what is printed, the look sent when the board
//! was shown, and that nothing is sent inside a job.

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

/// What the scripted daemon was asked: (method, params) of each `work.digest`
/// and `work.look`.
type Asked = Arc<Mutex<Vec<(String, Value)>>>;

/// A daemon on `sock` that answers `work.digest` (with the store's look, when
/// it has one) and `work.look` (or refuses it, when `refuse`), and records
/// their params, and an empty board for `executions.watch`.
fn daemon(sock: &Path, look: Option<Value>, refuse: bool) -> (std::thread::JoinHandle<()>, Asked) {
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
                    let look = look.clone();
                    std::thread::spawn(move || serve(s, &seen, look.as_ref(), refuse));
                }
                Err(_) => std::thread::sleep(std::time::Duration::from_millis(5)),
            }
        }
    });
    (h, asked)
}

fn serve(s: UnixStream, asked: &Mutex<Vec<(String, Value)>>, look: Option<&Value>, refuse: bool) {
    let mut reader = BufReader::new(&s);
    let mut line = String::new();
    while reader.read_line(&mut line).unwrap_or(0) > 0 {
        let req: Value = serde_json::from_str(&line).unwrap();
        line.clear();
        let m = req["method"].as_str().unwrap();
        let result = match m {
            "work.digest" | "work.look" => {
                asked
                    .lock()
                    .unwrap()
                    .push((m.to_string(), req["params"].clone()));
                if m == "work.look" {
                    json!({"look": {"position": req["params"]["position"], "at_ms": at(1, 0)}})
                } else {
                    let mut d = digest();
                    // A position past this store's head (900) tells nothing, as the daemon's does.
                    if req["params"]["since"].as_u64().is_some_and(|p| p > 900) {
                        d["lines"] = json!([]);
                    }
                    if let Some(l) = look {
                        d["look"] = l.clone();
                    }
                    d
                }
            }
            "executions.watch" => {
                json!({"position": 10, "executions": [], "confirms": [], "total": 0})
            }
            "status.short" => json!({"position": 10, "needs_you": 0, "working": 0, "failed": []}),
            "work.watch" => json!({"position": 10, "views": [], "total": 0}),
            _ => json!({}),
        };
        let answer = if refuse && m == "work.look" {
            json!({"jsonrpc": "2.0", "id": req["id"],
                   "error": {"code": -32000, "message": "the store is busy"}})
        } else {
            json!({"jsonrpc": "2.0", "id": req["id"], "result": result})
        };
        (&s).write_all(format!("{answer}\n").as_bytes()).unwrap();
    }
}

struct World {
    dir: tempfile::TempDir,
    /// The store's look, as `work.digest` answers it.
    look: Option<Value>,
    /// The daemon refuses `work.look`.
    refuse_look: bool,
}

/// What a run printed, the params of each `work.digest`, and of each `work.look`.
struct Ran {
    out: String,
    digests: Vec<Value>,
    looks: Vec<Value>,
}

impl World {
    fn new() -> Self {
        Self {
            dir: tempfile::tempdir().unwrap(),
            look: None,
            refuse_look: false,
        }
    }
    /// The store's look is at `position`, from `at_ms`.
    fn store_looked(&mut self, position: u64, at_ms: u64) {
        self.look = Some(json!({"position": position, "at_ms": at_ms}));
    }
    fn sock(&self) -> PathBuf {
        self.dir.path().join("sock")
    }
    fn seen(&self) -> PathBuf {
        self.dir.path().join("state/theseus/seen.json")
    }
    /// A seen file as an older build left it, with the machine's look at
    /// `position`, at `at_ms`.
    fn old_file_looked(&self, position: u64, at_ms: u64) {
        std::fs::create_dir_all(self.seen().parent().unwrap()).unwrap();
        std::fs::write(
            self.seen(),
            json!({"version": 1, "board_read": true, "executions": {},
                   "looked": {"position": position, "at_ms": at_ms}})
            .to_string(),
        )
        .unwrap();
    }
    fn run(&self, args: &[&str], job: bool) -> Ran {
        let (d, asked) = daemon(&self.sock(), self.look.clone(), self.refuse_look);
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
        let of = |m: &str| -> Vec<Value> {
            asked
                .iter()
                .filter(|(n, _)| n == m)
                .map(|(_, p)| p.clone())
                .collect()
        };
        Ran {
            out: String::from_utf8_lossy(&out.stdout).into_owned(),
            digests: of("work.digest"),
            looks: of("work.look"),
        }
    }
}

#[test]
fn the_digest_since_a_time_asks_for_it_prints_the_lines_and_records_the_look() {
    let w = World::new();
    let Ran {
        out,
        digests: asked,
        looks,
    } = w.run(&["digest", "--since", "2h"], false);
    assert_eq!(asked.len(), 1);
    assert!(
        asked[0]["since_ms"].as_u64().unwrap() > 1_700_000_000_000,
        "{asked:?}"
    );
    assert!(asked[0].get("since").is_none() && asked[0].get("roots").is_none());
    assert!(asked[0].get("since_look").is_none(), "{asked:?}");
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
        looks,
        [json!({"position": 900})],
        "the board was shown: the store's look moves to its position"
    );
}

#[test]
fn since_seen_asks_from_the_stores_look_and_a_session_scopes_it_and_records_nothing() {
    let mut w = World::new();
    w.store_looked(500, at(0, 0));
    let ran = w.run(&["digest", "--since-seen"], false);
    assert_eq!(ran.digests.len(), 1);
    assert_eq!(ran.digests[0]["since_look"], true, "{:?}", ran.digests);
    assert!(ran.digests[0].get("since").is_none(), "{:?}", ran.digests);
    assert!(
        ran.out
            .starts_with("◆ 1 reported, 1 answered by you since 18:00"),
        "the words are the store's look's time: {}",
        ran.out
    );
    assert_eq!(ran.looks, [json!({"position": 900})]);
    // One task's timeline is not the board: asked of it, and no look sent.
    let ran = w.run(&["digest", "--session", "ses_task"], false);
    assert_eq!(ran.digests[0]["since_look"], true);
    assert_eq!(ran.digests[0]["roots"], json!(["ses_task"]));
    assert_eq!(ran.looks, Vec::<Value>::new());
}

#[test]
fn nothing_is_recorded_inside_a_job_and_json_is_the_answer_as_it_came() {
    let mut w = World::new();
    w.store_looked(500, at(0, 0));
    let ran = w.run(&["digest"], true);
    assert_eq!(
        ran.looks,
        Vec::<Value>::new(),
        "a job's reading is not the operator's"
    );
    let ran = w.run(&["--json", "digest"], false);
    let v: Value = serde_json::from_str(&ran.out).unwrap();
    assert_eq!(v["result"]["to"], 900);
}

#[test]
fn status_ends_with_one_line_when_something_happened_since_the_last_look() {
    let mut w = World::new();
    // Never looked: status starts nothing and says nothing of it.
    let ran = w.run(&["status"], false);
    assert!(!ran.out.contains("theseus digest"), "{}", ran.out);
    assert_eq!(ran.looks, Vec::<Value>::new());
    w.store_looked(500, at(0, 0));
    let ran = w.run(&["status"], false);
    assert_eq!(ran.digests.len(), 1);
    assert_eq!(ran.digests[0]["since_look"], true, "{:?}", ran.digests);
    let last = ran.out.lines().last().unwrap();
    assert!(
        last.starts_with("◆ 1 reported, 1 answered by you since ")
            && last.ends_with(" · theseus digest"),
        "{}",
        ran.out
    );
    // Status is not a look: the line is there until the digest is read.
    assert_eq!(ran.looks, Vec::<Value>::new());
    // The prompt's and the script's forms say nothing of it.
    let ran = w.run(&["status", "--short"], false);
    assert!(
        ran.digests.is_empty() && !ran.out.contains("theseus digest"),
        "{}",
        ran.out
    );
}

/// The machine's old look points past this store's head (another store's, or a
/// newer history's before a restore): it no longer silences the digest. Its
/// time is carried over once, and the file keeps no look after.
#[test]
fn a_machine_file_past_the_stores_head_does_not_silence_the_digest() {
    let w = World::new();
    w.old_file_looked(999_999, at(0, 0));
    let ran = w.run(&["digest"], false);
    assert_eq!(ran.digests.len(), 1);
    assert!(ran.digests[0].get("since").is_none(), "{:?}", ran.digests);
    assert_eq!(ran.digests[0]["since_look"], true, "{:?}", ran.digests);
    assert_eq!(ran.digests[0]["since_ms"], at(0, 0), "{:?}", ran.digests);
    assert!(ran.out.contains("📋 Beta summary reported"), "{}", ran.out);
    assert_eq!(ran.looks, [json!({"position": 900})]);
    let file = std::fs::read_to_string(w.seen()).unwrap();
    assert!(!file.contains("looked"), "carried over once: {file}");
}

/// The time an older build kept in the seen file is the only copy of when the
/// owner last looked before the store kept its own (issue 46): another client's
/// write of the file keeps it, and so does a look the daemon refuses; the next
/// digest asks from it again, and the look the daemon takes hands it over.
#[test]
fn the_old_time_waits_for_a_look_the_daemon_takes() {
    let mut w = World::new();
    w.old_file_looked(999_999, at(0, 0));
    theseus_client::seen::record(&w.seen(), &[]).unwrap();
    w.refuse_look = true;
    let ran = w.run(&["digest"], false);
    assert_eq!(ran.digests[0]["since_ms"], at(0, 0), "{:?}", ran.digests);
    assert_eq!(ran.looks, [json!({"position": 900})]);
    w.refuse_look = false;
    let ran = w.run(&["digest"], false);
    assert_eq!(
        ran.digests[0]["since_ms"],
        at(0, 0),
        "a refused look kept it: {:?}",
        ran.digests
    );
    let file = std::fs::read_to_string(w.seen()).unwrap();
    assert!(!file.contains("looked"), "handed over: {file}");
}
