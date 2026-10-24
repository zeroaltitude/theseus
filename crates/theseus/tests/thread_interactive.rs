//! `watch --interactive` and threads (theseus-j0yh): the thread this shell chose is
//! watched at the start and named in the status line; `/t` lists the live threads,
//! `/t N` moves the watch to another thread's session, `/t home` back; a message
//! sent in a thread wears its tag. A watch with no current thread asks the daemon
//! nothing about threads.

use std::io::{BufRead, BufReader, Read, Write};
use std::os::unix::net::UnixListener;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde_json::{json, Value};

const THESEUS: &str = env!("CARGO_BIN_EXE_theseus");
/// 2026-09-21 14:13:20 UTC: the threads' activity is a minute before the test's "now"; the
/// status line reads the real clock, so the warm/cold word is not asserted here.
const NOW_MS: u64 = 1_790_000_000_000;

fn wait_until<T>(what: &str, mut ready: impl FnMut() -> Option<T>) -> T {
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        if let Some(t) = ready() {
            return t;
        }
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        std::thread::sleep(Duration::from_millis(20));
    }
}

fn thread(slug: &str, title: &str, session: Option<&str>) -> Value {
    let mut t = json!({"slug": slug, "title": title, "state": "active",
        "last_active_ms": NOW_MS, "created_at_ms": 1});
    if let Some(s) = session {
        t["session_id"] = json!(s);
    }
    json!({"thread": t, "context_tokens": 41_200, "today_usd": 0.0, "cache_ttl_secs": 3600})
}

fn session(id: &str) -> Value {
    json!({"session_id": id, "kind": "conversation", "label": null,
        "created_at_unix_ms": 1, "turns": 2, "profile": "sonnet"})
}

fn view(sid: &str) -> Value {
    json!({"position": 5, "at_ms": 1, "execution_id": format!("exe_{sid}"), "session_id": sid,
        "kind": "conversation", "state": "waiting", "pending": [], "turns": 2, "spent_usd": 0.0,
        "limit_usd": 10.0, "attention": {"level": "ready", "label": "ready", "since_ms": 0}})
}

struct Daemon {
    sock: PathBuf,
    got: Arc<Mutex<Vec<Value>>>,
}

impl Daemon {
    fn start(dir: &Path) -> Self {
        let sock = dir.join("sock");
        let listener = UnixListener::bind(&sock).unwrap();
        let got = Arc::new(Mutex::new(Vec::new()));
        let g = got.clone();
        std::thread::spawn(move || {
            let (s, _) = listener.accept().unwrap();
            for line in BufReader::new(&s).lines() {
                let Ok(line) = line else { break };
                let req: Value = serde_json::from_str(&line).unwrap();
                let p = &req["params"];
                let result = match req["method"].as_str().unwrap() {
                    "thread.show" => match p["slug"].as_str().unwrap() {
                        "pr" => thread("pr", "ex-ai pr", Some("ses_a")),
                        _ => thread("moons", "Moon table", None),
                    },
                    "thread.list" => json!({"threads": [
                        thread("pr", "ex-ai pr", Some("ses_a")),
                        thread("tides-log", "Tide log rewrite", Some("ses_b")),
                        thread("moons", "Moon table", None)], "total": 3}),
                    "session.wait" => json!({"reached": "settled", "already": true,
                        "execution": view(p["session_id"].as_str().unwrap()), "confirms": []}),
                    "session.list" => match p["ids"].as_array() {
                        Some(ids) => json!({"sessions": ids.iter()
                            .map(|i| session(i.as_str().unwrap())).collect::<Vec<_>>()}),
                        None => {
                            json!({"sessions": [session("ses_a"), session("ses_b"), session("ses_home")]})
                        }
                    },
                    "turn.submit" => {
                        json!({"session_id": "ses_new", "turn_id": "trn_1", "loops": 1,
                        "output": "Done.", "stop_reason": "end_turn", "provider_stop_reason": null,
                        "model": "invented-model", "usage": {"input_tokens": 1, "output_tokens": 1},
                        "elapsed_ms": 5})
                    }
                    _ => json!({}),
                };
                let answer = json!({"jsonrpc": "2.0", "id": req["id"], "result": result});
                g.lock().unwrap().push(req);
                if (&s).write_all(format!("{answer}\n").as_bytes()).is_err() {
                    break;
                }
            }
        });
        Self { sock, got }
    }

    fn requests(&self) -> Vec<Value> {
        self.got.lock().unwrap().clone()
    }

    fn methods(&self) -> Vec<String> {
        self.requests()
            .iter()
            .map(|r| r["method"].as_str().unwrap().to_string())
            .collect()
    }

    fn wait_for(&self, method: &str, n: usize) -> Vec<Value> {
        wait_until(&format!("{n} × {method}"), || {
            let of: Vec<Value> = self
                .requests()
                .into_iter()
                .filter(|r| r["method"] == method)
                .collect();
            (of.len() >= n).then_some(of)
        })
    }
}

struct Watch {
    child: Child,
    err: Arc<Mutex<String>>,
}

impl Drop for Watch {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

impl Watch {
    fn start(d: &Daemon, state: &Path, env: Option<&str>) -> Self {
        let mut cmd = Command::new(THESEUS);
        cmd.arg("--socket")
            .arg(&d.sock)
            .args(["watch", "--interactive", "--no-herdr"])
            .env("XDG_STATE_HOME", state)
            .env("COLUMNS", "100")
            .env_remove("THESEUS_SESSION")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        match env {
            Some(e) => cmd.env("THESEUS_THREAD", e),
            None => cmd.env_remove("THESEUS_THREAD"),
        };
        let mut child = cmd.spawn().unwrap();
        let err = Arc::new(Mutex::new(String::new()));
        let (e, mut stderr) = (err.clone(), child.stderr.take().unwrap());
        std::thread::spawn(move || {
            let mut buf = [0u8; 4096];
            while let Ok(n) = stderr.read(&mut buf) {
                if n == 0 {
                    break;
                }
                e.lock()
                    .unwrap()
                    .push_str(&String::from_utf8_lossy(&buf[..n]));
            }
        });
        Self { child, err }
    }

    fn line(&mut self, text: &str) {
        let stdin = self.child.stdin.as_mut().unwrap();
        writeln!(stdin, "{text}").unwrap();
        stdin.flush().unwrap();
    }

    fn wait_err(&self, text: &str) {
        wait_until(&format!("`{text}` on stderr"), || {
            self.err.lock().unwrap().contains(text).then_some(())
        });
    }
}

#[test]
fn the_current_thread_is_watched_named_listed_switched_and_tagged() {
    let dir = tempfile::tempdir().unwrap();
    let d = Daemon::start(dir.path());
    let mut w = Watch::start(&d, &dir.path().join("state"), Some("pr"));
    // The thread's own session is watched, and the status line names it.
    w.wait_err("[ex-ai pr · active · 41K ctx");
    let first = d.wait_for("session.watch", 1);
    assert_eq!(first[0]["params"]["session_id"], "ses_a");
    // `/t` lists the live threads, numbered, the current one marked.
    w.line("/t");
    w.wait_err("2   Tide log rewrite · active");
    w.wait_err(" 1 ▸ ex-ai pr · active");
    // `/t 2` moves the watch: the old session unwatched, the new one watched and read.
    w.line("/t 2");
    d.wait_for("session.unwatch", 1);
    let watched = d.wait_for("session.watch", 2);
    assert_eq!(watched[1]["params"]["session_id"], "ses_b");
    w.wait_err("[Tide log rewrite · active");
    // A message goes to the new session, with the tag.
    w.line("how is the log going");
    let turns = d.wait_for("turn.submit", 1);
    assert_eq!(turns[0]["params"]["session_id"], "ses_b");
    w.wait_err("→ Tide log rewrite");
    // `/t home` goes back to a session no thread holds.
    w.line("/t home");
    let watched = d.wait_for("session.watch", 3);
    assert_eq!(watched[2]["params"]["session_id"], "ses_home");
    w.wait_err("[home]");
    // A thread with no session opens one with its first message, on the thread.
    w.line("/t moons");
    w.wait_err("Moon table has no session yet");
    w.line("when is the next new moon");
    let turns = d.wait_for("turn.submit", 2);
    assert_eq!(turns[1]["params"]["thread"], "moons");
    assert!(
        turns[1]["params"].get("session_id").is_none(),
        "{}",
        turns[1]
    );
    // The session it opened is watched.
    let watched = d.wait_for("session.watch", 4);
    assert_eq!(watched[3]["params"]["session_id"], "ses_new");
}

#[test]
fn a_watch_with_no_current_thread_asks_nothing_about_threads() {
    let dir = tempfile::tempdir().unwrap();
    let d = Daemon::start(dir.path());
    let mut w = Watch::start(&d, &dir.path().join("state"), None);
    d.wait_for("session.list", 1);
    w.line("hello there");
    d.wait_for("turn.submit", 1);
    let methods = d.methods();
    assert!(
        methods.iter().all(|m| !m.starts_with("thread.")),
        "{methods:?}"
    );
    assert!(
        !w.err.lock().unwrap().contains("→"),
        "no tag outside a thread"
    );
}
