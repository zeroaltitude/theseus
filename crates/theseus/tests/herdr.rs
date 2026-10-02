//! The herdr adapter end to end (theseus-l1l, design `stage2` §2.10 and §3.2):
//! the real binary against a scripted daemon and a fake herdr, whose socket
//! records every NDJSON line it receives. These hold the mapping from attention
//! to herdr's state, reports sent only on a change, `seq`, the release on every
//! way out (the daemon's end, stdin's end, SIGTERM, Ctrl-C, a hangup), and
//! `--interactive`'s answers and messages.

use std::io::{BufRead, BufReader, Write};
use std::net::Shutdown;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde_json::{json, Value};

const THESEUS: &str = env!("CARGO_BIN_EXE_theseus");
const S: &str = "ses_q7f3k2";
const X: &str = "exe_q7f3k2";
const PANE: &str = "w1:p2";

/// How long a test waits for something to arrive before it fails.
const PATIENCE: Duration = Duration::from_secs(20);

/// Wait until `ready` says yes, or fail saying `what`.
fn wait_until<T>(what: &str, mut ready: impl FnMut() -> Option<T>) -> T {
    let deadline = Instant::now() + PATIENCE;
    loop {
        if let Some(t) = ready() {
            return t;
        }
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// A fake herdr: one request per connection, answered by `answer` (a result,
/// or an error object), and recorded.
struct FakeHerdr {
    sock: PathBuf,
    got: Arc<Mutex<Vec<Value>>>,
}

impl FakeHerdr {
    fn start(dir: &Path) -> Self {
        Self::answering(dir, |_| Ok(json!({"type": "ok"})))
    }

    fn answering(
        dir: &Path,
        answer: impl Fn(&Value) -> Result<Value, Value> + Send + 'static,
    ) -> Self {
        let sock = dir.join("herdr.sock");
        let listener = UnixListener::bind(&sock).unwrap();
        let got = Arc::new(Mutex::new(Vec::new()));
        let g = got.clone();
        std::thread::spawn(move || {
            for s in listener.incoming() {
                let Ok(s) = s else { break };
                let mut line = String::new();
                if BufReader::new(&s).read_line(&mut line).unwrap_or(0) == 0 {
                    continue;
                }
                let req: Value = serde_json::from_str(&line).unwrap();
                let out = match answer(&req) {
                    Ok(r) => json!({"id": req["id"], "result": r}),
                    Err(e) => json!({"id": req["id"], "error": e}),
                };
                g.lock().unwrap().push(req);
                let _ = writeln!(&s, "{out}");
            }
        });
        Self { sock, got }
    }

    fn requests(&self) -> Vec<Value> {
        self.got.lock().unwrap().clone()
    }

    /// The requests once there are at least `n` of `method`.
    fn wait_for(&self, method: &str, n: usize) -> Vec<Value> {
        wait_until(&format!("{n} × {method} at herdr"), || {
            let got = self.requests();
            (got.iter().filter(|r| r["method"] == method).count() >= n).then_some(got)
        })
    }
}

/// The reports alone, as (state, message).
fn reports(got: &[Value]) -> Vec<(String, String)> {
    got.iter()
        .filter(|r| r["method"] == "pane.report_agent")
        .map(|r| {
            (
                r["params"]["state"].as_str().unwrap().to_string(),
                r["params"]["message"].as_str().unwrap().to_string(),
            )
        })
        .collect()
}

fn rep(state: &str, message: &str) -> (String, String) {
    (state.to_string(), message.to_string())
}

/// A scripted daemon: one connection; each request is answered by `answer`
/// (none: never answered), then recorded, so a notification the test pushes
/// after seeing a request comes after its answer. The test pushes
/// notifications.
struct FakeDaemon {
    sock: PathBuf,
    writer: Arc<Mutex<Option<UnixStream>>>,
    got: Arc<Mutex<Vec<Value>>>,
}

impl FakeDaemon {
    fn start(
        dir: &Path,
        answer: impl Fn(&Value) -> Option<Result<Value, Value>> + Send + 'static,
    ) -> Self {
        let sock = dir.join("theseusd.sock");
        let listener = UnixListener::bind(&sock).unwrap();
        let writer: Arc<Mutex<Option<UnixStream>>> = Arc::new(Mutex::new(None));
        let got = Arc::new(Mutex::new(Vec::new()));
        let (w, g) = (writer.clone(), got.clone());
        std::thread::spawn(move || {
            let (s, _) = listener.accept().unwrap();
            *w.lock().unwrap() = Some(s.try_clone().unwrap());
            for line in BufReader::new(&s).lines() {
                let Ok(line) = line else { break };
                let req: Value = serde_json::from_str(&line).unwrap();
                let out = match answer(&req) {
                    Some(Ok(r)) => Some(json!({"jsonrpc": "2.0", "id": req["id"], "result": r})),
                    Some(Err(e)) => Some(json!({"jsonrpc": "2.0", "id": req["id"], "error": e})),
                    None => None,
                };
                if let (Some(out), Some(w)) = (out, w.lock().unwrap().as_mut()) {
                    let _ = writeln!(w, "{out}");
                }
                g.lock().unwrap().push(req);
            }
        });
        Self { sock, writer, got }
    }

    fn push(&self, method: &str, params: Value) {
        let note = json!({"jsonrpc": "2.0", "method": method, "params": params});
        let mut w = self.writer.lock().unwrap();
        writeln!(w.as_mut().expect("connected"), "{note}").unwrap();
    }

    /// The daemon's end: the client reads EOF.
    fn close(&self) {
        if let Some(w) = self.writer.lock().unwrap().take() {
            let _ = w.shutdown(Shutdown::Both);
        }
    }

    fn requests(&self) -> Vec<Value> {
        self.got.lock().unwrap().clone()
    }

    fn wait_for(&self, method: &str, n: usize) -> Vec<Value> {
        wait_until(&format!("{n} × {method} at the daemon"), || {
            let got = self.requests();
            let of: Vec<Value> = got.into_iter().filter(|r| r["method"] == method).collect();
            (of.len() >= n).then_some(of)
        })
    }
}

/// A question: `proc.run` waiting for approval.
fn question(cid: &str) -> Value {
    json!({"correlation_id": cid, "session_id": S, "execution_id": X, "tool": "proc.run",
           "input": {"argv": ["tide", "--tables"]}, "reason": "run the tide tables",
           "by": "operator", "requested_at_ms": 1_759_300_600_000u64,
           "expires_at_ms": 1_759_301_500_000u64})
}

/// The session's execution as `execution.changed` carries it.
fn view(
    position: u64,
    state: &str,
    level: &str,
    label: &str,
    pending: Option<&str>,
    spent: f64,
) -> Value {
    let pending: Vec<Value> = pending
        .map(|cid| json!({"correlation_id": cid, "tool": "proc.run", "reason": "run the tide tables"}))
        .into_iter()
        .collect();
    json!({"position": position, "at_ms": 1_759_300_000_000u64 + position, "execution_id": X,
           "session_id": S, "kind": "conversation", "state": state, "pending": pending,
           "turns": 2, "spent_usd": spent, "limit_usd": 10.0,
           "attention": {"level": level, "label": label, "since_ms": 0}})
}

const READY: (&str, &str, &str) = ("waiting", "ready", "ready");

/// A daemon whose first read finds the session at `first`, with `confirms`
/// waiting; it answers every answer and message.
fn daemon(dir: &Path, first: Value, confirms: Vec<Value>) -> FakeDaemon {
    FakeDaemon::start(dir, move |req| {
        let p = &req["params"];
        Some(Ok(match req["method"].as_str().unwrap() {
            "session.watch" => json!({"watching": true}),
            "session.wait" => {
                json!({"reached": "settled", "already": true, "execution": first, "confirms": confirms})
            }
            "session.list" => json!({"sessions": [{
                "session_id": S, "kind": "conversation", "label": "Tide notes",
                "created_at_unix_ms": 1_759_300_000_000u64, "turns": 2,
                "title": "check the tide tables"}]}),
            "action.confirm" => json!({"correlation_id": p["correlation_id"],
                "approved": p["approve"], "session_id": S, "execution_id": X, "resumes": true}),
            "turn.submit" => json!({}),
            other => {
                return Some(Err(
                    json!({"code": -32601, "message": format!("no {other}")}),
                ))
            }
        }))
    })
}

/// The watch under test, killed if the test fails first. Its stderr is read as
/// it comes, so a test can wait for a prompt.
struct Watch {
    child: Child,
    err: Arc<Mutex<String>>,
    reader: Option<std::thread::JoinHandle<()>>,
}

impl Drop for Watch {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

impl Watch {
    /// Wait until its stderr holds `text` `n` times.
    fn wait_err(&self, text: &str, n: usize) {
        wait_until(&format!("{n} × `{text}` on stderr"), || {
            (self.err.lock().unwrap().matches(text).count() >= n).then_some(())
        });
    }

    fn start(d: &FakeDaemon, herdr: Option<&FakeHerdr>, extra: &[&str]) -> Self {
        let mut cmd = Command::new(THESEUS);
        cmd.arg("--socket")
            .arg(&d.sock)
            .args(["watch", S])
            .args(extra)
            .env_remove("THESEUS_SOCKET")
            .env_remove("HERDR_ENV")
            .env_remove("HERDR_PANE_ID")
            .env_remove("HERDR_SOCKET_PATH")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        if let Some(h) = herdr {
            cmd.env("HERDR_ENV", "1")
                .env("HERDR_PANE_ID", PANE)
                .env("HERDR_SOCKET_PATH", &h.sock);
        }
        let mut child = cmd.spawn().unwrap();
        let err = Arc::new(Mutex::new(String::new()));
        let (e, mut stderr) = (err.clone(), child.stderr.take().unwrap());
        let reader = std::thread::spawn(move || {
            let mut buf = [0u8; 4096];
            while let Ok(n) = std::io::Read::read(&mut stderr, &mut buf) {
                if n == 0 {
                    break;
                }
                e.lock()
                    .unwrap()
                    .push_str(&String::from_utf8_lossy(&buf[..n]));
            }
        });
        Self {
            child,
            err,
            reader: Some(reader),
        }
    }

    fn type_line(&mut self, line: &str) {
        let stdin = self.child.stdin.as_mut().unwrap();
        writeln!(stdin, "{line}").unwrap();
        stdin.flush().unwrap();
    }

    fn signal(&self, sig: i32) {
        // SAFETY: a signal to the child this test spawned and still holds.
        unsafe {
            libc::kill(self.child.id() as i32, sig);
        }
    }

    /// Its exit code and stderr, once it exits by itself.
    fn finish(mut self) -> (i32, String) {
        let code = wait_until("the watch to exit", || self.child.try_wait().unwrap());
        if let Some(r) = self.reader.take() {
            let _ = r.join();
        }
        let err = self.err.lock().unwrap().clone();
        (code.code().unwrap_or(-1), err)
    }
}

/// Every `seq` herdr received, in the order it came, only grows.
fn seqs_grow(got: &[Value]) {
    let seqs: Vec<u64> = got
        .iter()
        .filter(|r| r["method"] != "pane.report_metadata")
        .filter_map(|r| r["params"]["seq"].as_u64())
        .collect();
    assert!(seqs.len() >= 2, "{got:?}");
    assert!(
        seqs.windows(2).all(|w| w[1] > w[0]),
        "seq only grows: {seqs:?}"
    );
}

/// The reporter (design §2.10, §3.2 row 11a): each level as herdr's state, a
/// report only when the state or message changes (an old view, the same view
/// again, and a spend-only view change no state), the pane named after its
/// first report, `seq` growing, and the release when the daemon closes.
#[test]
#[expect(clippy::too_many_lines, reason = "shape budget: split it")]
fn the_reporter_maps_reports_only_changes_and_releases_when_the_daemon_ends() {
    let dir = tempfile::tempdir().unwrap();
    let herdr = FakeHerdr::start(dir.path());
    let (s, l, lbl) = READY;
    let d = daemon(dir.path(), view(10, s, l, lbl, None, 0.0), vec![]);
    let w = Watch::start(&d, Some(&herdr), &[]);
    let got = herdr.wait_for("pane.report_metadata", 1);
    assert_eq!(reports(&got), [rep("idle", "ready")]);
    let first = &got[0];
    assert_eq!(first["method"], "pane.report_agent");
    assert_eq!(
        (
            first["params"]["pane_id"].as_str(),
            first["params"]["source"].as_str(),
            first["params"]["agent"].as_str()
        ),
        (Some(PANE), Some("custom:theseus"), Some("theseus"))
    );
    assert_eq!(got[1]["method"], "agent.rename");
    assert_eq!(
        got[1]["params"],
        json!({"target": PANE, "name": "theseus-q7f3k2-tide-notes"})
    );
    let meta = &got[2]["params"];
    // A session the operator labelled is titled by its label.
    assert_eq!(meta["title"], "Tide notes");
    assert_eq!(meta["display_agent"], "theseus: ready");
    assert_eq!(
        meta["tokens"],
        json!({"session": S, "cost": "$0", "confirm": null})
    );

    // Each push waits for what it changes: the reporter may drop a report
    // that a later one, queued in the same batch, replaces.
    d.push(
        "execution.changed",
        view(11, "running", "working", "turn 2", None, 0.0),
    );
    herdr.wait_for("pane.report_metadata", 2);
    // Old, then the same again, then spend alone: no report.
    d.push(
        "execution.changed",
        view(10, "running", "needs_you", "an old view", None, 0.0),
    );
    d.push(
        "execution.changed",
        view(11, "running", "working", "turn 2", None, 0.0),
    );
    d.push(
        "execution.changed",
        view(12, "running", "working", "turn 2", None, 0.42),
    );
    let spend = herdr.wait_for("pane.report_metadata", 3);
    assert_eq!(spend.last().unwrap()["params"]["tokens"]["cost"], "$0.42");
    assert_eq!(reports(&spend).len(), 2, "spend alone is no report");
    d.push("confirm.requested", question("act_m3"));
    d.push(
        "execution.changed",
        view(
            13,
            "waiting",
            "needs_you",
            "confirm proc.run: run the tide tables",
            Some("act_m3"),
            0.42,
        ),
    );
    let blocked = herdr.wait_for("pane.report_metadata", 4);
    assert_eq!(
        blocked.last().unwrap()["params"]["tokens"]["confirm"],
        "act_m3"
    );
    d.push("execution.changed", view(14, s, l, lbl, None, 0.42));
    herdr.wait_for("pane.report_agent", 4);
    d.close();
    herdr.wait_for("pane.release_agent", 1);
    let (code, err) = w.finish();
    assert_eq!(code, 0, "{err}");
    assert!(
        err.contains("reporting its state to herdr's pane w1:p2"),
        "{err}"
    );
    let got = herdr.requests();
    assert_eq!(
        reports(&got),
        [
            rep("idle", "ready"),
            rep("working", "turn 2"),
            rep("blocked", "confirm proc.run: run the tide tables"),
            rep("idle", "ready"),
        ]
    );
    seqs_grow(&got);
    let release = got.len() - 2;
    assert_eq!(got[release]["method"], "pane.release_agent", "{got:?}");
    assert_eq!(
        (
            got[release]["params"]["pane_id"].as_str(),
            got[release]["params"]["source"].as_str(),
            got[release]["params"]["agent"].as_str()
        ),
        (Some(PANE), Some("custom:theseus"), Some("theseus"))
    );
    let cleared = &got[release + 1];
    assert_eq!(cleared["method"], "pane.report_metadata");
    assert_eq!(
        cleared["params"]["tokens"]["confirm"],
        Value::Null,
        "the question's token is cleared"
    );
    assert_eq!(
        cleared["params"]["tokens"]["session"], S,
        "the pane keeps its session"
    );
}

/// SIGTERM, Ctrl-C (SIGINT), and a hangup each release the pane, and the
/// watch exits cleanly.
#[test]
fn a_signal_releases_the_pane() {
    for sig in [libc::SIGTERM, libc::SIGINT, libc::SIGHUP] {
        let dir = tempfile::tempdir().unwrap();
        let herdr = FakeHerdr::start(dir.path());
        let (s, l, lbl) = READY;
        let d = daemon(dir.path(), view(10, s, l, lbl, None, 0.0), vec![]);
        let w = Watch::start(&d, Some(&herdr), &[]);
        herdr.wait_for("pane.report_metadata", 1);
        w.signal(sig);
        herdr.wait_for("pane.release_agent", 1);
        let (code, err) = w.finish();
        assert_eq!(code, 0, "signal {sig}: {err}");
    }
}

/// `--interactive` (§2.10): the question already waiting is asked; `y`
/// approves it as `theseus watch`; any other line is a message, and the pane
/// says working at once; a bare `y` with nothing waiting is held back; the
/// end of stdin ends the watch and releases the pane.
#[test]
fn interactive_answers_the_question_and_sends_messages() {
    let dir = tempfile::tempdir().unwrap();
    let herdr = FakeHerdr::start(dir.path());
    let d = daemon(
        dir.path(),
        view(
            20,
            "waiting",
            "needs_you",
            "confirm proc.run: run the tide tables",
            Some("act_m3"),
            0.0,
        ),
        vec![question("act_m3")],
    );
    let mut w = Watch::start(&d, Some(&herdr), &["--interactive"]);
    w.wait_err("approve? [y/N/t/note]", 1);
    herdr.wait_for("pane.report_metadata", 1);
    w.type_line("y");
    let answers = d.wait_for("action.confirm", 1);
    assert_eq!(
        answers[0]["params"],
        json!({"correlation_id": "act_m3", "approve": true, "trust": false, "note": null,
               "author": "theseus watch"})
    );
    d.push(
        "confirm.resolved",
        json!({"session_id": S, "correlation_id": "act_m3", "approved": true,
               "by": "theseus watch"}),
    );
    d.push(
        "execution.changed",
        view(21, "running", "working", "turn 3", None, 0.0),
    );
    herdr.wait_for("pane.report_agent", 2);
    d.push(
        "execution.changed",
        view(22, "waiting", "ready", "ready", None, 0.0),
    );
    herdr.wait_for("pane.report_agent", 3);
    w.type_line("check the tide at noon");
    let turns = d.wait_for("turn.submit", 1);
    assert_eq!(
        turns[0]["params"],
        json!({"session_id": S, "input": "check the tide at noon", "author": "theseus watch"})
    );
    herdr.wait_for("pane.report_agent", 4);
    w.type_line("y");
    w.type_line("n");
    w.wait_err("nothing waits for an answer now", 2);
    drop(w.child.stdin.take());
    herdr.wait_for("pane.release_agent", 1);
    let (code, err) = w.finish();
    assert_eq!(code, 0, "{err}");
    assert_eq!(
        reports(&herdr.requests()),
        [
            rep("blocked", "confirm proc.run: run the tide tables"),
            rep("working", "turn 3"),
            rep("idle", "ready"),
            rep("working", "sending your message"),
        ]
    );
    let methods: Vec<String> = d
        .requests()
        .iter()
        .map(|r| r["method"].as_str().unwrap().to_string())
        .collect();
    assert_eq!(
        methods,
        [
            "session.watch",
            "session.wait",
            "session.list",
            "action.confirm",
            "turn.submit"
        ],
        "a bare answer with nothing waiting is neither an answer nor a message"
    );
    assert!(
        err.contains("? proc.run waits for you in ses_q7f3k2: run the tide tables"),
        "the question waiting at the start is shown whole: {err}"
    );
}

/// A line that is not an answer word declines, with the line as the note; a
/// refused answer is said and asked again; `t` approves and trusts.
#[test]
fn a_note_declines_and_a_refused_answer_is_asked_again() {
    let dir = tempfile::tempdir().unwrap();
    let r = Arc::new(Mutex::new(false));
    let d = FakeDaemon::start(dir.path(), move |req| {
        let p = &req["params"];
        Some(Ok(match req["method"].as_str().unwrap() {
            "session.watch" => json!({"watching": true}),
            "session.wait" => json!({"reached": "blocked", "already": true,
                "execution": view(30, "waiting", "needs_you", "confirm proc.run",
                                  Some("act_m3"), 0.0),
                "confirms": [question("act_m3")]}),
            "session.list" => json!({"sessions": []}),
            "action.confirm" => {
                let mut once = r.lock().unwrap();
                if !*once {
                    *once = true;
                    return Some(Err(json!({"code": -32005,
                        "message": "approvals come from the operator's channels"})));
                }
                json!({"correlation_id": p["correlation_id"], "approved": p["approve"],
                       "session_id": S, "execution_id": X, "resumes": true})
            }
            _ => return None,
        }))
    });
    let mut w = Watch::start(&d, None, &["--interactive"]);
    w.wait_err("approve? [y/N/t/note]", 1);
    w.type_line("use the staging port");
    let first = d.wait_for("action.confirm", 1);
    assert_eq!(
        (
            first[0]["params"]["approve"].clone(),
            first[0]["params"]["note"].clone()
        ),
        (json!(false), json!("use the staging port"))
    );
    w.wait_err("approve? [y/N/t/note]", 2);
    w.type_line("t");
    let both = d.wait_for("action.confirm", 2);
    assert_eq!(
        (
            both[1]["params"]["approve"].clone(),
            both[1]["params"]["trust"].clone()
        ),
        (json!(true), json!(true))
    );
    drop(w.child.stdin.take());
    let (code, err) = w.finish();
    assert_eq!(code, 0, "{err}");
    assert!(
        err.contains(
            "the answer to act_m3 was refused: approvals come from the operator's channels"
        ),
        "{err}"
    );
}

/// A session with no label is titled by its title, which it may not have at
/// its first turn's start: the watch asks again at each new turn until it
/// has one, and then no more.
#[test]
fn an_unlabelled_session_is_titled_once_it_has_a_title() {
    let dir = tempfile::tempdir().unwrap();
    let herdr = FakeHerdr::start(dir.path());
    let n = Mutex::new(0u32);
    let d = FakeDaemon::start(dir.path(), move |req| {
        Some(Ok(match req["method"].as_str().unwrap() {
            "session.watch" => json!({"watching": true}),
            "session.wait" => json!({"reached": "settled", "already": true,
                                     "execution": view(10, "waiting", "ready", "ready", None, 0.0)}),
            "session.list" => {
                let mut n = n.lock().unwrap();
                *n += 1;
                // The first read, and the ask at turn 1, find no title yet.
                let title = (*n >= 3).then_some("check the tide tables");
                json!({"sessions": [{"session_id": S, "kind": "conversation", "label": null,
                       "created_at_unix_ms": 1_759_300_000_000u64, "turns": 1, "title": title}]})
            }
            _ => return None,
        }))
    });
    let w = Watch::start(&d, Some(&herdr), &[]);
    herdr.wait_for("pane.report_metadata", 1);
    let turn = |position: u64, turns: u64, label: &str| {
        let mut v = view(position, "running", "working", label, None, 0.0);
        v["turns"] = json!(turns);
        v
    };
    // herdr has heard a report with this message.
    let reported = |message: &str| {
        wait_until(&format!("a report of `{message}`"), || {
            reports(&herdr.requests())
                .iter()
                .any(|(_, m)| m == message)
                .then_some(())
        })
    };
    d.push("execution.changed", turn(11, 1, "turn 1"));
    d.wait_for("session.list", 2);
    reported("turn 1");
    d.push("execution.changed", turn(12, 1, "turn 1 · tools"));
    reported("turn 1 · tools");
    d.push("execution.changed", turn(13, 2, "turn 2"));
    d.wait_for("session.list", 3);
    wait_until("the title at herdr", || {
        herdr
            .requests()
            .iter()
            .any(|r| r["params"]["title"] == "check the tide tables")
            .then_some(())
    });
    d.push("execution.changed", turn(14, 3, "turn 3"));
    reported("turn 3");
    d.close();
    let (code, err) = w.finish();
    assert_eq!(code, 0, "{err}");
    let asked = d
        .requests()
        .iter()
        .filter(|r| r["method"] == "session.list")
        .count();
    assert_eq!(
        asked, 3,
        "asked at the start, at turn 1 (no title yet), and at turn 2; not at turn 1 again, nor once titled"
    );
}

/// `--no-herdr`, inside a pane: herdr hears nothing, and the watch is the
/// plain one, which sends only `session.watch`.
#[test]
fn no_herdr_reports_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let herdr = FakeHerdr::start(dir.path());
    let d = daemon(dir.path(), json!(null), vec![]);
    let w = Watch::start(&d, Some(&herdr), &["--no-herdr"]);
    d.wait_for("session.watch", 1);
    d.push(
        "turn.started",
        json!({"session_id": S, "turn_id": "turn_m4p8z1", "continuation": false}),
    );
    w.wait_err("── turn turn_m4p8z1", 1);
    d.close();
    let (code, err) = w.finish();
    assert_eq!(code, 0, "{err}");
    assert!(herdr.requests().is_empty(), "{:?}", herdr.requests());
    let methods: Vec<Value> = d.requests().iter().map(|r| r["method"].clone()).collect();
    assert_eq!(methods, [json!("session.watch")]);
}
