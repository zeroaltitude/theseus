//! `theseus herdr answer` end to end (theseus-pkaq): the real binary against a scripted daemon and a
//! fake herdr. These hold which question the card opens on (the named one, the focused pane's, the
//! oldest), that an answer is one `work.answer` with author `herdr`, the note path, that a question
//! answered elsewhere meanwhile is said, and that nothing waiting says so.

use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixListener;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};

use serde_json::{json, Value};

const THESEUS: &str = env!("CARGO_BIN_EXE_theseus");

fn question(cid: &str, session: &str, asked: u64, tool: &str) -> Value {
    json!({"correlation_id": cid, "session_id": session, "execution_id": "exe_q7f3k2",
           "tool": tool, "input": {"path": "log.md"}, "reason": "write under /tide",
           "by": "policy", "requested_at_ms": asked, "expires_at_ms": 0,
           "task": {"task_id": "tsk_1", "short": &session[4..], "title": format!("Task {session}")}})
}

/// What a scripted daemon does besides answering the card's reads.
#[derive(Clone, Default)]
struct Script {
    questions: Vec<Value>,
    /// `work.answer` is refused with this message.
    refuse: Option<&'static str>,
    /// After `session.watch`, tell the card this question was answered elsewhere.
    resolve_elsewhere: Option<&'static str>,
}

struct Daemon {
    requests: Arc<Mutex<Vec<Value>>>,
    sock: PathBuf,
    _dir: tempfile::TempDir,
}

fn daemon(script: Script) -> Daemon {
    let dir = tempfile::tempdir().unwrap();
    let sock = dir.path().join("sock");
    let listener = UnixListener::bind(&sock).unwrap();
    let requests = Arc::new(Mutex::new(Vec::new()));
    let log = requests.clone();
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(s) = stream else { return };
            let (script, log) = (script.clone(), log.clone());
            std::thread::spawn(move || {
                let mut qs = script.questions.clone();
                let mut w = s.try_clone().unwrap();
                for line in BufReader::new(s).lines() {
                    let Ok(line) = line else { return };
                    let req: Value = serde_json::from_str(&line).unwrap();
                    log.lock().unwrap().push(req.clone());
                    let mut reply = |r: Result<Value, Value>| {
                        let mut a = json!({"jsonrpc": "2.0", "id": req["id"]});
                        match r {
                            Ok(v) => a["result"] = v,
                            Err(e) => a["error"] = e,
                        }
                        writeln!(w, "{a}").unwrap();
                    };
                    match req["method"].as_str().unwrap() {
                        "confirm.list" => reply(Ok(json!({"confirms": qs}))),
                        "session.list" => reply(Ok(json!({"sessions": []}))),
                        "session.watch" => {
                            reply(Ok(json!({})));
                            if let Some(cid) = script.resolve_elsewhere {
                                let sid = qs
                                    .iter()
                                    .find(|q| q["correlation_id"] == cid)
                                    .map(|q| q["session_id"].clone())
                                    .unwrap_or(json!("ses_x"));
                                let n = json!({"jsonrpc": "2.0", "method": "confirm.resolved",
                                    "params": {"session_id": sid, "correlation_id": cid,
                                               "approved": true, "by": "discord"}});
                                writeln!(w, "{n}").unwrap();
                                qs.retain(|q| q["correlation_id"] != cid);
                            }
                        }
                        "work.answer" => match script.refuse {
                            Some(m) => reply(Err(json!({"code": -32000, "message": m}))),
                            None => {
                                let id = req["params"]["question_id"].clone();
                                qs.retain(|q| q["correlation_id"] != id);
                                reply(Ok(json!({"question_id": id,
                                                "option": req["params"]["option"]})));
                            }
                        },
                        other => panic!("unexpected request {other}"),
                    }
                }
            });
        }
    });
    Daemon {
        requests,
        sock,
        _dir: dir,
    }
}

impl Daemon {
    fn answers(&self) -> Vec<Value> {
        self.requests
            .lock()
            .unwrap()
            .iter()
            .filter(|r| r["method"] == "work.answer")
            .map(|r| r["params"].clone())
            .collect()
    }
}

/// A fake herdr that answers `pane.get` with the tokens it is given.
fn herdr(confirm_token: Option<&str>) -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let sock = dir.path().join("herdr.sock");
    let listener = UnixListener::bind(&sock).unwrap();
    let token = confirm_token.map(str::to_string);
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(s) = stream else { return };
            let token = token.clone();
            std::thread::spawn(move || {
                let mut line = String::new();
                BufReader::new(&s).read_line(&mut line).unwrap();
                let req: Value = serde_json::from_str(&line).unwrap();
                assert_eq!(req["method"], "pane.get");
                let tokens = match &token {
                    Some(t) => json!({"confirm": t}),
                    None => json!({}),
                };
                let a = json!({"id": req["id"], "result": {"type": "pane_info",
                    "pane": {"pane_id": "w1:p2", "tokens": tokens}}});
                writeln!(&s, "{a}").unwrap();
            });
        }
    });
    (dir, sock)
}

/// Run the card with `input` on its stdin; its stdout.
fn card(sock: &Path, args: &[&str], input: &str) -> (String, i32) {
    let mut child = Command::new(THESEUS)
        .arg("--socket")
        .arg(sock)
        .args(["herdr", "answer"])
        .args(args)
        .env_remove("THESEUS_SESSION")
        .env("XDG_STATE_HOME", sock.parent().unwrap().join("xdg"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(input.as_bytes())
        .unwrap();
    let out = child.wait_with_output().unwrap();
    (
        String::from_utf8_lossy(&out.stdout).into_owned() + &String::from_utf8_lossy(&out.stderr),
        out.status.code().unwrap_or(-1),
    )
}

fn three() -> Vec<Value> {
    vec![
        question("act_0000bbbb22", "ses_bbbb22", 2_000, "proc.run"),
        question("act_0000aaaa11", "ses_aaaa11", 1_000, "fs.write"),
        question("act_0000cccc33", "ses_cccc33", 3_000, "fs.read"),
    ]
}

/// With no pane and no name the card opens on the question that has waited longest, and `y`
/// answers it: one `work.answer` with author `herdr`, and the others are offered after it.
#[test]
fn the_card_opens_on_the_oldest_and_y_answers_it() {
    let d = daemon(Script {
        questions: three(),
        ..Script::default()
    });
    let (out, code) = card(&d.sock, &["--oldest"], "y\nq\n");
    assert_eq!(code, 0, "{out}");
    assert!(out.contains("Task ses_aaaa11  ·  aaaa11"), "{out}");
    assert!(out.contains("2 more wait:"), "{out}");
    assert_eq!(
        d.answers(),
        [json!({"question_id": "act_0000aaaa11", "option": "approve", "author": "herdr"})]
    );
    assert!(out.contains("answered act_0000aaaa11 · approve"), "{out}");
}

/// The focused pane's `confirm` token picks the question; a named question beats it.
#[test]
fn the_focused_panes_question_is_the_one_shown() {
    let d = daemon(Script {
        questions: three(),
        ..Script::default()
    });
    let (_h, hs) = herdr(Some("act_0000cccc33"));
    let (out, code) = card(
        &d.sock,
        &["--pane", "w1:p2", "--herdr-socket", hs.to_str().unwrap()],
        "n\nq\n",
    );
    assert_eq!(code, 0, "{out}");
    assert!(out.contains("Task ses_cccc33  ·  cccc33"), "{out}");
    assert_eq!(
        d.answers(),
        [json!({"question_id": "act_0000cccc33", "option": "decline", "author": "herdr"})]
    );
    // A pane with no question: the oldest.
    let d = daemon(Script {
        questions: three(),
        ..Script::default()
    });
    let (_h, hs) = herdr(None);
    let (out, _) = card(
        &d.sock,
        &["--pane", "w1:p2", "--herdr-socket", hs.to_str().unwrap()],
        "q\n",
    );
    assert!(out.contains("aaaa11"), "{out}");
    // A named one beats the pane's.
    let (_h, hs) = herdr(Some("act_0000cccc33"));
    let (out, _) = card(
        &d.sock,
        &[
            "--pane",
            "w1:p2",
            "--herdr-socket",
            hs.to_str().unwrap(),
            "bbbb22",
        ],
        "q\n",
    );
    assert!(out.contains("Task ses_bbbb22"), "{out}");
}

/// Words that are no key decline, with the words as the note.
#[test]
fn words_decline_with_a_note() {
    let d = daemon(Script {
        questions: vec![question("act_0000aaaa11", "ses_aaaa11", 1, "fs.write")],
        ..Script::default()
    });
    let (out, code) = card(&d.sock, &["--oldest"], "use the staging port\n");
    assert_eq!(code, 0, "{out}");
    assert_eq!(
        d.answers(),
        [json!({"question_id": "act_0000aaaa11", "option": "decline",
                "note": "use the staging port", "author": "herdr"})]
    );
}

/// Nothing waiting is said, and a named question that is gone is said and not replaced.
#[test]
fn nothing_waits_and_a_gone_question_are_said() {
    let d = daemon(Script::default());
    let (out, code) = card(&d.sock, &["--oldest"], "");
    assert_eq!(code, 0);
    assert!(out.contains("nothing waits for you"), "{out}");
    let d = daemon(Script {
        questions: three(),
        ..Script::default()
    });
    let (out, _) = card(&d.sock, &["act_0000gone99"], "");
    assert!(out.contains("act_0000gone99 no longer waits"), "{out}");
    assert!(d.answers().is_empty());
}

/// A question answered elsewhere while the card is open is said, and the card closes when it was
/// the only one.
#[test]
fn a_question_answered_elsewhere_meanwhile_is_said() {
    let d = daemon(Script {
        questions: vec![question("act_0000aaaa11", "ses_aaaa11", 1, "fs.write")],
        resolve_elsewhere: Some("act_0000aaaa11"),
        ..Script::default()
    });
    // Stdin stays open (no line): only the daemon's notice can end the card.
    let mut child = Command::new(THESEUS)
        .arg("--socket")
        .arg(&d.sock)
        .args(["herdr", "answer", "--oldest"])
        .env_remove("THESEUS_SESSION")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let _stdin = child.stdin.take();
    let out = child.wait_with_output().unwrap();
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(
        text.contains("act_0000aaaa11 was answered elsewhere by discord"),
        "{text}"
    );
    assert!(d.answers().is_empty());
}

/// An answer the daemon refuses (answered meanwhile) is said, with the question found gone.
#[test]
fn a_refused_answer_is_said() {
    let d = daemon(Script {
        questions: vec![question("act_0000aaaa11", "ses_aaaa11", 1, "fs.write")],
        refuse: Some("act_0000aaaa11 was already answered"),
        ..Script::default()
    });
    let (out, _) = card(&d.sock, &["--oldest"], "y\nq\n");
    assert!(out.contains("already answered"), "{out}");
    assert_eq!(d.answers().len(), 1);
}
