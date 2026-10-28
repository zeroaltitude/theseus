//! A task's pane answers every question and never loses a message (theseus-ypnxt.3): the real
//! `theseus watch --interactive` against a scripted daemon. A question with options (`owner_ask`)
//! is answered by its number with `work.answer`, a plain approval still goes out as
//! `action.confirm`, and a line typed after the task ended goes to the conversation that started
//! it, with the redirect line every surface says. Run with `--nocapture` to read the panes.

use std::io::{BufRead, BufReader, Read, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde_json::{json, Value};

const THESEUS: &str = env!("CARGO_BIN_EXE_theseus");
const TASK: &str = "ses_task9f2c1d";
const MAIN: &str = "ses_main0a1b2c";

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

fn session(id: &str) -> Value {
    if id == TASK {
        return json!({"session_id": id, "kind": "task", "label": "task", "title": "Sound the north channel",
            "parent_session_id": MAIN, "created_at_unix_ms": 1, "turns": 3, "profile": "sonnet"});
    }
    json!({"session_id": id, "kind": "conversation", "label": "Harbour log",
        "created_at_unix_ms": 1, "turns": 2, "profile": "sonnet"})
}

fn view(sid: &str, state: &str) -> Value {
    let kind = if sid == TASK { "task" } else { "conversation" };
    json!({"position": 5, "at_ms": 1, "execution_id": format!("exe_{sid}"), "session_id": sid,
        "kind": kind, "state": state, "pending": [], "turns": 3, "spent_usd": 0.1,
        "limit_usd": 2.0, "attention": {"level": "ready", "label": "ready", "since_ms": 0}})
}

/// A question with three options, as the daemon words it.
fn ask() -> Value {
    let option = |i: usize, label: &str| {
        json!({"id": format!("choice:{i}"), "label": label, "style": "plain",
               "takes_note": false, "effect": "answers with it"})
    };
    json!({"correlation_id": "act_00reef", "session_id": TASK, "execution_id": "exe_task",
        "tool": "owner_ask", "reason": "the task asks you",  "by": "operator",
        "input": {"question": "Which quay first?", "options": [
            {"label": "North quay"}, {"label": "East quay", "detail": "the tide is in"},
            {"label": "Skip both"}]},
        "requested_at_ms": 1_759_300_600_000u64, "expires_at_ms": 1_759_302_400_000u64,
        "options": [option(0, "North quay"), option(1, "East quay"), option(2, "Skip both"),
            {"id": "choice:other", "label": "Something else", "style": "plain",
             "takes_note": true, "effect": "answers in your words"}]})
}

/// A plain approval, as `herdr.rs` pins it.
fn approval() -> Value {
    json!({"correlation_id": "act_m3", "session_id": TASK, "execution_id": "exe_task",
        "tool": "proc.run", "input": {"argv": ["tide", "--tables"]}, "reason": "run the tide tables",
        "by": "operator", "requested_at_ms": 1_759_300_600_000u64,
        "expires_at_ms": 1_759_301_500_000u64})
}

struct Daemon {
    sock: PathBuf,
    got: Arc<Mutex<Vec<Value>>>,
    out: Arc<Mutex<Option<UnixStream>>>,
}

impl Daemon {
    /// A daemon whose task session is in `state`, asking `confirms`; a message to the task is
    /// refused as an ended task's is, and one to main is answered.
    fn start(dir: &Path, state: &'static str, confirms: Vec<Value>) -> Self {
        let sock = dir.join("sock");
        let listener = UnixListener::bind(&sock).unwrap();
        let got = Arc::new(Mutex::new(Vec::new()));
        let g = got.clone();
        let out = Arc::new(Mutex::new(None));
        let o = out.clone();
        std::thread::spawn(move || {
            let (s, _) = listener.accept().unwrap();
            *o.lock().unwrap() = s.try_clone().ok();
            for line in BufReader::new(&s).lines() {
                let Ok(line) = line else { break };
                let req: Value = serde_json::from_str(&line).unwrap();
                let p = &req["params"];
                let answer = match req["method"].as_str().unwrap() {
                    "session.wait" => Ok(json!({"reached": "settled", "already": true,
                        "execution": view(p["session_id"].as_str().unwrap(), state),
                        "confirms": confirms})),
                    "session.list" => Ok(json!({"sessions": p["ids"].as_array().map_or_else(
                        || vec![session(TASK)],
                        |ids| ids.iter().map(|i| session(i.as_str().unwrap())).collect())})),
                    "work.answer" => {
                        Ok(json!({"question_id": p["question_id"], "option": p["option"]}))
                    }
                    "action.confirm" => Ok(json!({"correlation_id": p["correlation_id"],
                        "approved": p["approve"], "session_id": TASK, "execution_id": "exe_task"})),
                    "turn.submit" if p["session_id"] == TASK => Err(json!({"code": -32000,
                        "message": "the task is complete", "data": {"class": "execution_complete"}})),
                    "turn.submit" => Ok(json!({"session_id": MAIN, "turn_id": "trn_1", "loops": 1,
                        "output": "Noted: the north channel waits.", "stop_reason": "end_turn",
                        "provider_stop_reason": null, "model": "invented-model",
                        "usage": {"input_tokens": 1, "output_tokens": 1}, "elapsed_ms": 5})),
                    _ => Ok(json!({})),
                };
                let out = match answer {
                    Ok(r) => json!({"jsonrpc": "2.0", "id": req["id"], "result": r}),
                    Err(e) => json!({"jsonrpc": "2.0", "id": req["id"], "error": e}),
                };
                g.lock().unwrap().push(req);
                if (&s).write_all(format!("{out}\n").as_bytes()).is_err() {
                    break;
                }
            }
        });
        Self { sock, got, out }
    }

    /// A notification the daemon sends the pane, as an event of the task.
    fn push(&self, method: &str, params: Value) {
        let line = json!({"jsonrpc": "2.0", "method": method, "params": params});
        let out = self.out.lock().unwrap();
        let mut s = out.as_ref().expect("the pane connected");
        writeln!(s, "{line}").unwrap();
        s.flush().unwrap();
    }

    fn wait_for(&self, method: &str, n: usize) -> Vec<Value> {
        wait_until(&format!("{n} × {method}"), || {
            let of: Vec<Value> = self
                .got
                .lock()
                .unwrap()
                .iter()
                .filter(|r| r["method"] == method)
                .cloned()
                .collect();
            (of.len() >= n).then_some(of)
        })
    }

    fn count(&self, method: &str) -> usize {
        self.got
            .lock()
            .unwrap()
            .iter()
            .filter(|r| r["method"] == method)
            .count()
    }
}

struct Pane {
    child: Child,
    err: Arc<Mutex<String>>,
}

impl Drop for Pane {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

impl Pane {
    fn start(d: &Daemon, state: &Path) -> Self {
        let mut child = Command::new(THESEUS)
            .arg("--socket")
            .arg(&d.sock)
            .args(["watch", TASK, "--interactive", "--no-herdr"])
            .env("XDG_STATE_HOME", state)
            .env("COLUMNS", "100")
            .env_remove("THESEUS_SESSION")
            .env_remove("THESEUS_THREAD")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
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

    fn seen(&self) -> String {
        self.err.lock().unwrap().clone()
    }

    /// Wait until stderr holds `text` `n` times.
    fn wait_err(&self, text: &str, n: usize) {
        wait_until(&format!("{n} × `{text}` on stderr"), || {
            (self.seen().matches(text).count() >= n).then_some(())
        });
    }

    /// What the pane showed, for a reader of `--nocapture`.
    fn transcript(&self, title: &str) {
        eprintln!("--- {title} ---\n{}--- end ---", self.seen());
    }
}

/// A question with options: the pane numbers them, `2` answers with the second option by
/// `work.answer`, and neither a `y` nor a number past the last option sends anything.
#[test]
fn an_owner_ask_is_answered_by_its_number() {
    let dir = tempfile::tempdir().unwrap();
    let d = Daemon::start(dir.path(), "waiting", vec![ask()]);
    let mut w = Pane::start(&d, &dir.path().join("state"));
    w.wait_err("choose? [1-3, or your own words]", 1);
    let shown = w.seen();
    for line in [
        "? Which quay first?",
        "1. North quay",
        "2. East quay — the tide is in",
        "3. Skip both",
    ] {
        assert!(shown.contains(line), "{line}: {shown}");
    }
    // A yes chooses nothing, and neither does a place past the last option.
    w.line("y");
    w.wait_err("choose? [1-3, or your own words]", 2);
    w.line("4");
    w.wait_err("choose? [1-3, or your own words]", 3);
    assert_eq!((d.count("work.answer"), d.count("action.confirm")), (0, 0));
    w.line("2");
    let answers = d.wait_for("work.answer", 1);
    assert_eq!(
        answers[0]["params"],
        json!({"question_id": "act_00reef", "option": "choice:1", "author": "theseus watch"})
    );
    assert_eq!(
        d.count("action.confirm"),
        0,
        "the daemon refuses a yes to a question with options"
    );
    w.transcript("an owner_ask answered by 2");
}

/// Events print below a question that waits, and the pane says its prompt again after each so
/// the line to answer on is the last: the question's block (`? question`, the numbered options)
/// is said once, when it is first asked, and only the `choose?` line comes again.
#[test]
fn a_question_block_prints_once_whatever_prints_below_it() {
    let dir = tempfile::tempdir().unwrap();
    let d = Daemon::start(dir.path(), "waiting", vec![ask()]);
    let w = Pane::start(&d, &dir.path().join("state"));
    w.wait_err("choose? [1-3, or your own words]", 1);
    for n in 1..=2 {
        d.push(
            "tool.started",
            json!({"session_id": TASK, "turn_id": "trn_1", "tool_use_id": format!("tu_{n}"),
                "tool": "fs.read", "correlation_id": format!("act_r{n}"), "backend": "inproc"}),
        );
        w.wait_err("choose? [1-3, or your own words]", 1 + n);
    }
    let shown = w.seen();
    assert_eq!(shown.matches("fs.read").count(), 2, "{shown}");
    for line in ["? Which quay first?", "1. North quay", "3. Skip both"] {
        assert_eq!(shown.matches(line).count(), 1, "{line}: {shown}");
    }
    w.transcript("a question's block once");
}

/// Words of the owner's own answer `other`, with the words as the note.
#[test]
fn other_words_answer_with_the_owners_own() {
    let dir = tempfile::tempdir().unwrap();
    let d = Daemon::start(dir.path(), "waiting", vec![ask()]);
    let mut w = Pane::start(&d, &dir.path().join("state"));
    w.wait_err("choose? [1-3, or your own words]", 1);
    w.line("the west quay, after the tide");
    let answers = d.wait_for("work.answer", 1);
    assert_eq!(
        (
            answers[0]["params"]["option"].clone(),
            answers[0]["params"]["note"].clone()
        ),
        (
            json!("choice:other"),
            json!("the west quay, after the tide")
        )
    );
}

/// A plain approval still goes out as `action.confirm`, with the params it always had.
#[test]
fn a_plain_approval_still_goes_out_as_action_confirm() {
    let dir = tempfile::tempdir().unwrap();
    let d = Daemon::start(dir.path(), "waiting", vec![approval()]);
    let mut w = Pane::start(&d, &dir.path().join("state"));
    w.wait_err("approve? [y/N/note]", 1);
    w.line("y");
    let answers = d.wait_for("action.confirm", 1);
    assert_eq!(
        answers[0]["params"],
        json!({"correlation_id": "act_m3", "approve": true, "note": null,
               "author": "theseus watch"})
    );
    assert_eq!(d.count("work.answer"), 0);
}

/// After the task ended, a message goes to the conversation that started it, with the redirect
/// line; a bare answer word is not a message and is not sent.
#[test]
fn a_message_after_the_task_ended_goes_to_main() {
    let dir = tempfile::tempdir().unwrap();
    let d = Daemon::start(dir.path(), "complete", vec![]);
    let mut w = Pane::start(&d, &dir.path().join("state"));
    w.wait_err("watching", 1);
    w.line("hello");
    let sent = d.wait_for("turn.submit", 2);
    assert_eq!(sent[0]["params"]["session_id"], TASK);
    assert_eq!(sent[1]["params"]["session_id"], MAIN);
    let line = "[Written to the task “Sound the north channel” (…9f2c1d), and the task is over] ";
    assert_eq!(sent[1]["params"]["input"], format!("{line}hello"));
    w.wait_err("main › Noted: the north channel waits.", 1);
    let shown = w.seen();
    assert!(shown.contains(line.trim_end()), "{shown}");
    assert!(!shown.contains("was not sent"), "{shown}");
    // `y` after the end is an answer to nothing, not a message.
    w.line("y");
    w.wait_err("nothing waits for an answer now", 1);
    assert_eq!(d.count("turn.submit"), 2);
    w.transcript("a message after the task ended");
}
