//! The herdr adapter end to end (theseus-l1l, design `stage2` §2.10 and §3.2):
//! the real binary against a scripted daemon and a fake herdr, whose socket
//! records every NDJSON line it receives. These hold the mapping from attention
//! to herdr's state, reports sent only on a change, `seq`, the release on every
//! way out (stdin's end, SIGTERM, Ctrl-C, a hangup; a lost daemon is no way out: the watch waits for it), and
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

#[path = "support/herdr_schema.rs"]
mod herdr_schema;

/// A fake herdr: one request per connection, answered by `answer` (a result,
/// or an error object), and recorded. A request of a shape herdr's own schema
/// does not take (`support/herdr_schema.rs`) is noted, and the test fails when
/// the fake is dropped with any noted (theseus-6ifz1).
struct FakeHerdr {
    sock: PathBuf,
    got: Arc<Mutex<Vec<Value>>>,
    /// When each request in `got` arrived.
    at: Arc<Mutex<Vec<Instant>>>,
    bad: Arc<Mutex<Vec<String>>>,
}

impl Drop for FakeHerdr {
    fn drop(&mut self) {
        if !std::thread::panicking() {
            let bad = self.bad.lock().unwrap();
            assert!(bad.is_empty(), "herdr's schema refused: {bad:#?}");
        }
    }
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
        let at = Arc::new(Mutex::new(Vec::new()));
        let bad = Arc::new(Mutex::new(Vec::new()));
        let schema = herdr_schema::Schema::load();
        let (g, t, b) = (got.clone(), at.clone(), bad.clone());
        std::thread::spawn(move || {
            for s in listener.incoming() {
                let Ok(s) = s else { break };
                let mut line = String::new();
                if BufReader::new(&s).read_line(&mut line).unwrap_or(0) == 0 {
                    continue;
                }
                let arrived = Instant::now();
                let req: Value = serde_json::from_str(&line).unwrap();
                let method = req["method"].as_str().unwrap_or_default();
                if let Err(e) = schema.check_request(method, &req["params"]) {
                    b.lock().unwrap().push(format!("{method}: {e}"));
                }
                let out = match answer(&req) {
                    Ok(r) => json!({"id": req["id"], "result": r}),
                    Err(e) => json!({"id": req["id"], "error": e}),
                };
                // The time first, so a reader that sees the request sees its time.
                t.lock().unwrap().push(arrived);
                g.lock().unwrap().push(req);
                let _ = writeln!(&s, "{out}");
            }
        });
        Self { sock, got, at, bad }
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

    /// Wait until the question has been asked `n` times: the prompt's own
    /// line, two spaces and the prompt. The start's help line repeats the
    /// prompt's words ("a question asks approve? [y/N/note]"), so a count of
    /// the words was one ahead, and a test typed its answer before the CLI
    /// asked again (theseus-jhie).
    fn wait_prompt(&self, n: usize) {
        const PROMPT: &str = "  approve? [y/N/note]";
        wait_until(&format!("{n} × the prompt `{PROMPT}` on stderr"), || {
            let err = self.err.lock().unwrap();
            (err.lines().filter(|l| *l == PROMPT).count() >= n).then_some(())
        });
    }

    fn start(d: &FakeDaemon, herdr: Option<&FakeHerdr>, extra: &[&str]) -> Self {
        Self::start_at(&d.sock, herdr, extra)
    }

    /// The watch of `S` over the daemon socket `sock`.
    fn start_at(sock: &Path, herdr: Option<&FakeHerdr>, extra: &[&str]) -> Self {
        let mut cmd = Command::new(THESEUS);
        cmd.arg("--socket")
            .arg(sock)
            .args(["watch", S])
            .args(extra)
            // The CLI's seen file is the test's own, never the machine's (theseus-yus0).
            .env("XDG_STATE_HOME", sock.with_file_name("xdg-state"))
            // Times are the test's own zone, never the machine's.
            .env("TZ", "<-07>7")
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

    /// Whether the watch still runs.
    fn child_alive(&self) -> bool {
        // SAFETY: signal 0 only asks whether the child this test spawned exists.
        unsafe { libc::kill(self.child.id() as i32, 0) == 0 }
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
/// first report, `seq` growing, and the release when the watch is ended (the
/// name is from when the daemon's close ended it; a lost daemon now only makes
/// the watch wait, theseus-qb1es).
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
        (Some(PANE), Some("custom:theseus"), Some("Tide notes"))
    );
    assert_eq!(got[1]["method"], "agent.rename");
    assert_eq!(
        got[1]["params"],
        json!({"target": PANE, "name": "theseus-q7f3k2-tide-notes"})
    );
    let meta = &got[2]["params"];
    // A session the operator labelled is titled by its label.
    assert_eq!(meta["title"], "Tide notes");
    assert_eq!(meta["display_agent"], "Tide notes");
    let tokens = &meta["tokens"];
    assert_eq!(
        (tokens["session"].as_str(), tokens["cost"].as_str()),
        (Some(S), Some("$0"))
    );
    assert_eq!(tokens["confirm"], Value::Null);
    assert_eq!(tokens["kind"], "conversation");

    // Each push waits for what it changes: the reporter may drop a report
    // that a later one, queued in the same batch, replaces.
    d.push(
        "execution.changed",
        view(11, "running", "working", "turn 2", None, 0.0),
    );
    herdr.wait_for("pane.report_agent", 2);
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
    // The second view's metadata (its label moved) comes first; the spend's is the one waited for.
    let spend = wait_until("the spend token", || {
        let got = herdr.requests();
        last_meta(&got)
            .filter(|m| m["params"]["tokens"]["cost"] == "$0.42")
            .map(|_| got)
    });
    assert_eq!(
        last_meta(&spend).unwrap()["params"]["tokens"]["cost"],
        "$0.42"
    );
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
    let blocked = wait_until("the confirm token", || {
        last_meta(&herdr.requests()).filter(|m| m["params"]["tokens"]["confirm"] == "act_m3")
    });
    assert_eq!(blocked["params"]["tokens"]["confirm"], "act_m3");
    d.push("execution.changed", view(14, s, l, lbl, None, 0.42));
    herdr.wait_for("pane.report_agent", 4);
    w.signal(libc::SIGTERM);
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
        (Some(PANE), Some("custom:theseus"), Some("Tide notes"))
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
    w.wait_prompt(1);
    herdr.wait_for("pane.report_metadata", 1);
    w.type_line("y");
    let answers = d.wait_for("action.confirm", 1);
    assert_eq!(
        answers[0]["params"],
        json!({"correlation_id": "act_m3", "approve": true, "note": null,
               "author": "theseus watch"})
    );
    d.push(
        "confirm.resolved",
        json!({"session_id": S, "correlation_id": "act_m3", "approved": true,
               "by": "theseus watch", "option": "approve"}),
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
            "work.watch",
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

/// theseus-nu3z: a message sent from a pane continues the session on the
/// profile its last turn ran on, as a continuation does, and not on the
/// daemon's live profile: a session started on GLM once ran its next turn on
/// Sonnet from a pane. The profile is read again at each new turn, so the
/// pane follows a session whose last turn another client ran elsewhere.
#[test]
fn a_message_continues_the_session_on_the_profile_of_its_last_turn() {
    let dir = tempfile::tempdir().unwrap();
    let profile = Arc::new(Mutex::new("glm".to_string()));
    let current = profile.clone();
    let d = FakeDaemon::start(dir.path(), move |req| {
        Some(Ok(match req["method"].as_str().unwrap() {
            "session.watch" => json!({"watching": true}),
            "session.wait" => json!({"reached": "settled", "already": true,
                "execution": view(10, "waiting", "ready", "ready", None, 0.0), "confirms": []}),
            "session.list" => json!({"sessions": [{
                "session_id": S, "kind": "conversation", "label": "Tide notes",
                "created_at_unix_ms": 1_759_300_000_000u64, "turns": 2,
                "title": "check the tide tables", "profile": *current.lock().unwrap()}]}),
            "turn.submit" => json!({}),
            other => {
                return Some(Err(
                    json!({"code": -32601, "message": format!("no {other}")}),
                ))
            }
        }))
    });
    let mut w = Watch::start(&d, None, &["--interactive"]);
    w.wait_err("a line you type is sent", 1);
    w.type_line("what is the tide at noon?");
    let first = d.wait_for("turn.submit", 1);
    assert_eq!(
        first[0]["params"],
        json!({"session_id": S, "input": "what is the tide at noon?",
               "author": "theseus watch", "profile": "glm", "carried": true})
    );
    // Another client's turn ran on another profile: the next message follows.
    *profile.lock().unwrap() = "sonnet".into();
    let mut next = view(11, "waiting", "ready", "ready", None, 0.0);
    next["turns"] = json!(3);
    d.push("execution.changed", next);
    d.wait_for("session.list", 2);
    std::thread::sleep(Duration::from_millis(300));
    w.type_line("and at midnight?");
    let both = d.wait_for("turn.submit", 2);
    assert_eq!(both[1]["params"]["profile"], json!("sonnet"));
    drop(w.child.stdin.take());
    let (code, err) = w.finish();
    assert_eq!(code, 0, "{err}");
}

/// A line that is not an answer word declines, with the line as the note; a
/// refused answer is said and asked again; `y` then approves.
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
            "session.list" => json!({"sessions": [{"session_id": S, "kind": "conversation",
                "label": null, "created_at_unix_ms": 1_759_300_000_000u64, "turns": 2}]}),
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
    w.wait_prompt(1);
    w.type_line("use the staging port");
    let first = d.wait_for("action.confirm", 1);
    assert_eq!(
        (
            first[0]["params"]["approve"].clone(),
            first[0]["params"]["note"].clone()
        ),
        (json!(false), json!("use the staging port"))
    );
    // The re-ask: typed before it, `y` would find no question open.
    w.wait_prompt(2);
    w.type_line("y");
    let both = d.wait_for("action.confirm", 2);
    assert_eq!(both[1]["params"]["approve"], json!(true));
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
    w.signal(libc::SIGTERM);
    let (code, err) = w.finish();
    assert_eq!(code, 0, "{err}");
    // A conversation's pane has no task header.
    assert!(!err.contains("⚙ task"), "{err}");
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

/// A task is named by its title (theseus-0n1v): the store labels every task
/// `task`, and the pane said `task` for each of them. Its title is the pane's
/// title and the words of its agent's name, cut at herdr's 32 characters.
#[test]
fn a_task_is_named_by_its_title_not_its_label() {
    let dir = tempfile::tempdir().unwrap();
    let herdr = FakeHerdr::start(dir.path());
    let d = FakeDaemon::start(dir.path(), move |req| {
        Some(Ok(match req["method"].as_str().unwrap() {
            "session.watch" => json!({"watching": true}),
            "session.wait" => json!({"reached": "settled", "already": true,
                                     "execution": view(10, "waiting", "ready", "ready", None, 0.0)}),
            "session.list" => json!({"sessions": [{"session_id": S, "kind": "task",
                "label": "task", "title": "Survey the north quay",
                "created_at_unix_ms": 1_759_300_000_000u64, "turns": 1}]}),
            _ => return None,
        }))
    });
    let w = Watch::start(&d, Some(&herdr), &[]);
    let got = herdr.wait_for("pane.report_metadata", 1);
    let rename = got.iter().find(|r| r["method"] == "agent.rename").unwrap();
    assert_eq!(rename["params"]["name"], "theseus-q7f3k2-survey-the-north");
    let meta = got
        .iter()
        .find(|r| r["method"] == "pane.report_metadata")
        .unwrap();
    assert_eq!(meta["params"]["title"], "Survey the north quay");
    w.signal(libc::SIGTERM);
    let (code, err) = w.finish();
    assert_eq!(code, 0, "{err}");
    // A task's pane says whose it is and the keys that leave it (theseus-m638c).
    assert!(
        err.contains("── ⚙ task q7f3k2 · prefix+m main · prefix+i inspect · prefix+p pause ──"),
        "{err}"
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
    // The session named whole is read once, so an unknown one is refused
    // (theseus-0n1v), then watched.
    assert_eq!(methods, [json!("session.list"), json!("session.watch")]);
}

/// The wall clock, in ms since the epoch.
fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64
}

/// The `notification.show` requests herdr received.
fn shown(got: &[Value]) -> Vec<Value> {
    got.iter()
        .filter(|r| r["method"] == "notification.show")
        .map(|r| r["params"].clone())
        .collect()
}

/// The notification policy's pings that the pane's state does not already
/// say (theseus-753z): a question's reminder at half its life is one
/// `notification.show`, once; the question itself is the pane's blocked
/// state, and sends none. Its life is 24 s, so its reminder falls outside
/// the burst window (10 s) the question opened.
#[test]
fn a_reminder_is_one_notification() {
    let dir = tempfile::tempdir().unwrap();
    let herdr = FakeHerdr::answering(dir.path(), |r| {
        Ok(if r["method"] == "notification.show" {
            json!({"type": "rate_limited"})
        } else {
            json!({"type": "ok"})
        })
    });
    let now = now_ms();
    let first = json!({"position": 10, "at_ms": now, "execution_id": X, "session_id": S,
        "kind": "conversation", "state": "waiting",
        "waiting_on": {"on": "confirm", "confirm_id": "cor_tide"},
        "pending": [{"correlation_id": "cor_tide", "tool": "proc.run",
                     "reason": "run the tide tables", "expires_at_ms": now + 24_000}],
        "turns": 2, "spent_usd": 0.0, "limit_usd": 10.0,
        "attention": {"level": "needs_you", "label": "confirm proc.run: run the tide tables",
                      "since_ms": now}});
    let d = daemon(dir.path(), first, vec![]);
    let w = Watch::start(&d, Some(&herdr), &[]);
    let got = herdr.wait_for("notification.show", 1);
    assert_eq!(
        reports(&got),
        [rep("blocked", "confirm proc.run: run the tide tables")]
    );
    assert_eq!(
        shown(&got),
        [json!({"title": "Tide notes", "sound": "request",
                "body": "● still waiting, 1 min left: Tide notes's proc.run"})]
    );
    assert!(now_ms() >= now + 12_000, "not before half its life");
    std::thread::sleep(Duration::from_millis(1_500));
    assert_eq!(shown(&herdr.requests()).len(), 1, "once");
    let err = w.err.lock().unwrap().clone();
    assert!(
        !err.contains("herdr did not"),
        "rate_limited is an answer: {err}"
    );
}

/// A wake's turn on the session says so in one notification; a spend change
/// sends no notification, only the metadata it sent before.
#[test]
fn a_wake_is_one_notification_and_spend_is_none() {
    let dir = tempfile::tempdir().unwrap();
    let herdr = FakeHerdr::start(dir.path());
    let (s, l, lbl) = READY;
    let d = daemon(dir.path(), view(10, s, l, lbl, None, 0.0), vec![]);
    let _w = Watch::start(&d, Some(&herdr), &[]);
    herdr.wait_for("pane.report_metadata", 1);
    d.push(
        "execution.changed",
        view(11, "running", "working", "turn 2", None, 0.0),
    );
    herdr.wait_for("pane.report_agent", 2);
    let before = herdr.requests().len();
    d.push(
        "execution.changed",
        view(12, "running", "working", "turn 2", None, 0.42),
    );
    let got = herdr.wait_for("pane.report_metadata", 2);
    std::thread::sleep(Duration::from_millis(300));
    assert_eq!(herdr.requests().len(), before + 1, "the metadata alone");
    assert!(shown(&got).is_empty());
    let mut queued = view(13, "queued", "working", "queued · wake", None, 0.42);
    queued["why"] = json!("wake");
    d.push("execution.changed", queued);
    let got = herdr.wait_for("notification.show", 1);
    assert_eq!(
        shown(&got),
        [json!({"title": "Tide notes", "sound": "request",
                "body": "⏰ Tide notes · its wake fired"})]
    );
}

/// theseus-rn7s: the prompt is the last line while a question waits. A turn
/// that parks on a question ends after the question was asked, and its
/// footer, printed below the prompt, used to leave the terminal ending on
/// the footer: the prompt is said again below whatever an event printed.
#[test]
fn the_prompt_is_the_last_line_after_a_parked_turns_footer() {
    let dir = tempfile::tempdir().unwrap();
    let d = FakeDaemon::start(dir.path(), move |req| {
        Some(Ok(match req["method"].as_str().unwrap() {
            "session.watch" => json!({"watching": true}),
            "session.wait" => json!({"reached": "blocked", "already": true,
                "execution": view(30, "waiting", "needs_you", "confirm proc.run",
                                  Some("act_m3"), 0.0),
                "confirms": [question("act_m3")]}),
            "session.list" => json!({"sessions": [{"session_id": S, "kind": "conversation",
                "label": null, "created_at_unix_ms": 1_759_300_000_000u64, "turns": 2}]}),
            _ => return None,
        }))
    });
    let mut w = Watch::start(&d, None, &["--interactive"]);
    w.wait_prompt(1);
    d.push(
        "turn.ended",
        json!({"session_id": S, "turn_id": "turn_m4p8z1", "loops": 1, "output": "",
               "stop_reason": "awaiting_confirm", "provider_stop_reason": "tool_use",
               "model": "glm-x", "provider": "zed", "profile": "quick",
               "usage": {"input_tokens": 10, "output_tokens": 5}, "elapsed_ms": 900,
               "cost_usd": 0.001, "tool_calls": 1, "continuation": false, "cites": [],
               "awaiting_confirm": "act_m3"}),
    );
    w.wait_err("· session ses_q7f3k2]", 1);
    w.wait_prompt(2);
    drop(w.child.stdin.take());
    let (code, err) = w.finish();
    assert_eq!(code, 0, "{err}");
    let lines: Vec<&str> = err.lines().filter(|l| !l.is_empty()).collect();
    assert_eq!(lines.last(), Some(&"  approve? [y/N/note]"), "{err}");
    let footer = lines
        .iter()
        .rposition(|l| l.contains("· session ses_q7f3k2]"));
    assert!(
        footer.is_some_and(|f| f + 1 < lines.len()),
        "the footer is above the prompt: {err}"
    );
}

// ---------------------------------------------------------------------------
// herdr's slots, and a watch that outlives its daemon (theseus-qb1es)
// ---------------------------------------------------------------------------

const TITLE: &str = "Write the harbour log needs attention";
const WRITTEN: u64 = 1_759_300_000_000;

/// The answer to a read of the task session: its title, its model, its
/// parent (`ses_dmroot`, whose label is `DM`), and when it was created.
fn task_info(ids: &Value) -> Value {
    if ids[0] == "ses_dmroot" {
        return json!({"sessions": [{"session_id": "ses_dmroot", "kind": "conversation",
            "label": "DM", "created_at_unix_ms": WRITTEN - 1000, "turns": 9}]});
    }
    json!({"sessions": [{"session_id": S, "kind": "task", "label": "task", "title": TITLE,
        "created_at_unix_ms": WRITTEN, "turns": 3, "model": "glm-5.3",
        "parent_session_id": "ses_dmroot"}]})
}

/// A view of the task, with the fields a state needs.
fn task_view(position: u64, state: &str, level: &str, label: &str, extra: Value) -> Value {
    let mut v = json!({"position": position, "at_ms": WRITTEN + position, "execution_id": X,
        "session_id": S, "kind": "task", "parent_session_id": "ses_dmroot", "state": state,
        "turns": 3, "spent_usd": 0.42, "limit_usd": 10.0,
        "attention": {"level": level, "label": label, "since_ms": 0}});
    for (k, e) in extra.as_object().into_iter().flatten() {
        v[k] = e.clone();
    }
    v
}

/// What the daemon answers a watch of the task.
fn task_answers(first: Value, req: &Value) -> Option<Result<Value, Value>> {
    Some(Ok(match req["method"].as_str().unwrap() {
        "session.watch" => json!({"watching": true}),
        "session.wait" => json!({"reached": "settled", "already": true, "execution": first,
                                 "confirms": []}),
        "session.list" => task_info(&req["params"]["ids"]),
        _ => return None,
    }))
}

/// A daemon that finds the task at `first`.
fn task_daemon(dir: &Path, first: Value) -> FakeDaemon {
    FakeDaemon::start(dir, move |req| task_answers(first.clone(), req))
}

/// The reports alone, whole. herdr's `pane.report_agent` takes no state label, so a report's
/// `state_labels` is joined here from the `pane.report_metadata` that follows it (the watch sends
/// the label with the metadata); a report not yet followed by one has none.
fn report_requests(got: &[Value]) -> Vec<Value> {
    got.iter()
        .enumerate()
        .filter(|(_, r)| r["method"] == "pane.report_agent")
        .map(|(i, r)| {
            let mut r = r.clone();
            let meta = got[i..].iter().find(|m| {
                m["method"] == "pane.report_metadata" && m["params"].get("state_labels").is_some()
            });
            if let Some(m) = meta {
                r["params"]["state_labels"] = m["params"]["state_labels"].clone();
            }
            r
        })
        .collect()
}

/// Wait for `n` reports, the last with its label.
fn labelled_reports(herdr: &FakeHerdr, n: usize) -> Vec<Value> {
    wait_until(&format!("{n} labelled reports"), || {
        let got = report_requests(&herdr.requests());
        (got.len() >= n && got[n - 1]["params"].get("state_labels").is_some()).then_some(got)
    })
}

/// A report's state and its one label (empty while the label has not come).
fn state_and_label(r: &Value) -> (String, String) {
    let labels = r["params"]["state_labels"]
        .as_object()
        .cloned()
        .unwrap_or_default();
    assert!(labels.len() <= 1, "one state's label: {r}");
    let label = labels
        .values()
        .next()
        .and_then(Value::as_str)
        .unwrap_or_default();
    (r["params"]["state"].as_str().unwrap().into(), label.into())
}

/// The last metadata herdr got.
fn last_meta(got: &[Value]) -> Option<Value> {
    got.iter()
        .rev()
        .find(|r| r["method"] == "pane.report_metadata")
        .cloned()
}

/// Every request herdr got obeys its limits: at most 16 tokens, each at most
/// 80 characters, an agent label of at most 22, and labels of at most 80.
fn within_herdrs_limits(got: &[Value]) {
    let chars = |v: &Value| v.as_str().map_or(0, |s| s.chars().count());
    for r in got {
        let p = &r["params"];
        if r["method"] == "pane.report_metadata" {
            let t = p["tokens"].as_object().unwrap();
            assert!(t.len() <= 16, "{} tokens: {r}", t.len());
            assert!(t.values().all(|v| chars(v) <= 80), "a long token: {r}");
            assert!(chars(&p["title"]) <= 80, "{r}");
        }
        if r["method"] == "pane.report_agent" {
            assert!(chars(&p["agent"]) <= 22, "{r}");
        }
        if let Some(labels) = p["state_labels"].as_object() {
            assert!(labels.values().all(|v| chars(v) <= 80), "{r}");
        }
    }
}

/// The ten tokens a task's first report carries, and what they say.
fn the_first_tokens_say(meta: &Value) {
    assert_eq!(meta["params"]["title"], TITLE);
    assert_eq!(meta["params"]["display_agent"], "Write the harbour log");
    let tok = &meta["params"]["tokens"];
    let names: Vec<&str> = tok
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    for want in [
        "task", "kind", "session", "confirm", "cost", "parent", "since", "doing", "model", "report",
    ] {
        assert!(names.contains(&want), "token {want} missing from {names:?}");
    }
    assert_eq!(tok["task"], TITLE);
    assert_eq!(tok["kind"], "task");
    assert_eq!(tok["session"], S);
    assert_eq!(tok["cost"], "$0.42");
    assert_eq!(tok["parent"], "↳ DM");
    assert_eq!(tok["since"], "23:26", "local time, at the test's zone");
    assert_eq!(tok["model"], "glm-5.3");
    assert_eq!(tok["confirm"], Value::Null);
    assert_eq!(tok["doing"], Value::Null);
}

/// A view, herdr's state for it, and its label (`state_labels`).
type Case = (Value, &'static str, &'static str);

/// The task in each state its labels word.
fn state_cases() -> Vec<Case> {
    let mut all = needs_you_cases();
    all.extend(working_cases());
    all
}

/// A question, a budget question, a failure and a block: all `blocked`.
fn needs_you_cases() -> Vec<Case> {
    let pending = |cid: &str, tool: &str, reason: &str, budget: bool| {
        json!({"pending": [{"correlation_id": cid, "tool": tool, "reason": reason,
                            "budget": budget}]})
    };
    let mut budget = pending("act_b1", "budget.reset", "", true);
    budget["spent_usd"] = json!(10.02);
    vec![
        (
            task_view(
                11,
                "waiting",
                "needs_you",
                "confirm fs.write: log.md",
                pending("act_w1", "fs.write", "log.md", false),
            ),
            "blocked",
            "needs you · fs.write log.md",
        ),
        (
            task_view(12, "waiting", "needs_you", "budget: $10.02 of $10", budget),
            "blocked",
            "budget · $10.02 of $10",
        ),
        (
            task_view(
                13,
                "failed",
                "needs_you",
                "failed: timeout (provider)",
                json!({"ended_reason": "timeout (provider)"}),
            ),
            "blocked",
            "failed · timeout (provider)",
        ),
        (
            task_view(
                14,
                "blocked",
                "needs_you",
                "blocked: no key",
                json!({"ended_reason": "no key for the sandbox"}),
            ),
            "blocked",
            "blocked · no key for the sandbox",
        ),
    ]
}

/// Running, queued, a job, a child and a sleep: all `working`.
fn working_cases() -> Vec<Case> {
    let past = |ms: u64| {
        let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH);
        u64::try_from(now.unwrap().as_millis()).unwrap() - ms
    };
    vec![
        (
            task_view(15, "running", "working", "turn 4", json!({})),
            "working",
            "turn 4",
        ),
        (
            task_view(
                16,
                "queued",
                "working",
                "queued · wake",
                json!({"why": "wake"}),
            ),
            "working",
            "queued · wake",
        ),
        (
            task_view(
                17,
                "waiting",
                "working",
                "waiting on 2 calls",
                json!({
                "waiting_on": {"on": "actions", "correlation_ids": ["a", "b"]},
                "attention": {"level": "working", "label": "waiting on 2 calls",
                              "since_ms": past(4 * 60_000 + 5_000)}}),
            ),
            "working",
            "job · 4m",
        ),
        (
            task_view(
                18,
                "waiting",
                "working",
                "waiting on task a1b2c3",
                json!({"waiting_on": {"on": "execution", "execution_id": "exe_a1b2c3"}}),
            ),
            "working",
            "waiting on task a1b2c3",
        ),
        (
            task_view(
                19,
                "waiting",
                "working",
                "sleeping until 16:00",
                json!({"waiting_on": {"on": "due_at", "at_ms": 1_759_330_000_000u64}}),
            ),
            "working",
            "sleeping until 16:00",
        ),
    ]
}

/// A call runs: the label and the `doing` token say what, and ending it
/// takes them back.
fn a_call_runs_and_ends(d: &FakeDaemon, herdr: &FakeHerdr) {
    let call = |kind: &str, extra: Value| {
        let mut p = json!({"session_id": S, "turn_id": "turn_a", "tool_use_id": "tu1",
                           "tool": "proc.run"});
        for (k, e) in extra.as_object().into_iter().flatten() {
            p[k] = e.clone();
        }
        d.push(kind, p);
    };
    let argv = json!(["cargo", "nextest", "run"]);
    call("tool.proposed", json!({"input": {"argv": argv}}));
    call("tool.started", json!({"backend": "job", "argv": argv}));
    let running = labelled_reports(herdr, 2);
    let want = (
        "working".to_string(),
        "turn 3 · proc.run cargo nextest run".to_string(),
    );
    assert_eq!(state_and_label(&running[1]), want);
    let doing = wait_until("the doing token", || {
        last_meta(&herdr.requests()).filter(|m| m["params"]["tokens"]["doing"].is_string())
    });
    let words = doing["params"]["tokens"]["doing"]
        .as_str()
        .unwrap()
        .to_string();
    assert!(
        words.starts_with("proc.run cargo nextest run · "),
        "{words}"
    );
    call("tool.ended", json!({"status": "ok"}));
    let ended = labelled_reports(herdr, 3);
    let want = ("working".to_string(), "turn 3".to_string());
    assert_eq!(state_and_label(&ended[2]), want);
}

/// Each state's labels, and the tokens beside them (theseus-qb1es): the agent
/// is the task's short name, cut at a word, so herdr's own toast reads "Write
/// the harbour log needs attention"; the state's label is worded once.
#[test]
fn every_state_has_its_label_and_the_tokens_beside_it() {
    let dir = tempfile::tempdir().unwrap();
    let herdr = FakeHerdr::start(dir.path());
    let d = task_daemon(
        dir.path(),
        task_view(10, "running", "working", "turn 3", json!({})),
    );
    let w = Watch::start(&d, Some(&herdr), &[]);
    let got = herdr.wait_for("pane.report_metadata", 1);
    let first = &report_requests(&got)[0];
    assert_eq!(state_and_label(first), ("working".into(), "turn 3".into()));
    // The agent is the short name, and so is what the sidebar row says.
    assert_eq!(first["params"]["agent"], "Write the harbour log");
    let meta = last_meta(&got).unwrap();
    assert_eq!(meta["params"]["display_agent"], "Write the harbour log");
    the_first_tokens_say(&meta);
    a_call_runs_and_ends(&d, &herdr);

    // Each state, one view after another.
    let mut n = 3;
    for (view, state, text) in state_cases() {
        d.push("execution.changed", view);
        n += 1;
        herdr.wait_for("pane.report_agent", n);
        let got = labelled_reports(&herdr, n);
        assert_eq!(
            state_and_label(&got[n - 1]),
            (state.into(), text.into()),
            "report {n}"
        );
    }
    // The confirm token followed the open question.
    let asked = herdr.requests().iter().any(|r| {
        r["method"] == "pane.report_metadata" && r["params"]["tokens"]["confirm"] == "act_w1"
    });
    assert!(asked, "the question's id was a token while it waited");

    // A finished task: its report's first line.
    d.push(
        "turn.ended",
        json!({"session_id": S, "turn_id": "turn_a", "loops": 2,
        "output": "Wrote the harbour log: 41 entries.\nThe rest is in log.md.",
        "stop_reason": "end_turn", "model": "glm-5.3",
        "usage": {"input_tokens": 1, "output_tokens": 1}, "elapsed_ms": 10, "cost_usd": null}),
    );
    d.push(
        "execution.changed",
        task_view(20, "complete", "idle", "complete", json!({})),
    );
    let done = wait_until("the done label", || {
        let got = report_requests(&herdr.requests());
        got.into_iter()
            .rev()
            .find(|r| r["params"]["state"] == "idle")
    });
    let want = (
        "idle".to_string(),
        "done · Wrote the harbour log: 41 entries.".to_string(),
    );
    assert_eq!(state_and_label(&done), want);
    let report = wait_until("the report token", || {
        last_meta(&herdr.requests()).filter(|m| m["params"]["tokens"]["report"].is_string())
    });
    assert_eq!(
        report["params"]["tokens"]["report"],
        "Wrote the harbour log: 41 entries."
    );
    within_herdrs_limits(&herdr.requests());
    w.signal(libc::SIGTERM);
    let (code, err) = w.finish();
    assert_eq!(code, 0, "{err}");
}

/// A process's CPU time so far, in clock ticks (user and system).
fn cpu_ticks(pid: u32) -> u64 {
    let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).unwrap();
    // After the command's closing parenthesis: state is field 3, utime 14, stime 15.
    let rest: Vec<&str> = stat
        .rsplit_once(')')
        .unwrap()
        .1
        .split_whitespace()
        .collect();
    rest[11].parse::<u64>().unwrap() + rest[12].parse::<u64>().unwrap()
}

/// What a scripted daemon answers a request: nothing (never answered), a
/// result, or an error.
type Answer = Option<Result<Value, Value>>;

/// A scripted daemon that goes away and comes back at the same socket, as a
/// restart does: `stop` closes its connection and removes its socket,
/// `restart` binds it again. It serves one connection at a time.
struct Restarting {
    sock: PathBuf,
    answer: Arc<dyn Fn(&Value) -> Answer + Send + Sync>,
    writer: Arc<Mutex<Option<UnixStream>>>,
    got: Arc<Mutex<Vec<Value>>>,
}

impl Restarting {
    fn start(dir: &Path, answer: impl Fn(&Value) -> Answer + Send + Sync + 'static) -> Self {
        let d = Self {
            sock: dir.join("theseusd.sock"),
            answer: Arc::new(answer),
            writer: Arc::new(Mutex::new(None)),
            got: Arc::new(Mutex::new(Vec::new())),
        };
        d.listen();
        d
    }

    fn listen(&self) {
        let listener = UnixListener::bind(&self.sock).unwrap();
        let (answer, writer, got) = (self.answer.clone(), self.writer.clone(), self.got.clone());
        std::thread::spawn(move || {
            for s in listener.incoming() {
                let Ok(s) = s else { break };
                *writer.lock().unwrap() = Some(s.try_clone().unwrap());
                for line in BufReader::new(&s).lines() {
                    let Ok(line) = line else { break };
                    let req: Value = serde_json::from_str(&line).unwrap();
                    let out = answer(&req).map(|a| match a {
                        Ok(r) => json!({"jsonrpc": "2.0", "id": req["id"], "result": r}),
                        Err(e) => json!({"jsonrpc": "2.0", "id": req["id"], "error": e}),
                    });
                    if let (Some(out), Some(w)) = (out, writer.lock().unwrap().as_mut()) {
                        let _ = writeln!(w, "{out}");
                    }
                    got.lock().unwrap().push(req);
                }
            }
        });
    }

    /// The daemon goes: the client reads EOF and finds no socket.
    fn stop(&self) {
        if let Some(w) = self.writer.lock().unwrap().take() {
            let _ = w.shutdown(Shutdown::Both);
        }
        std::fs::remove_file(&self.sock).unwrap();
    }

    fn restart(&self) {
        self.listen();
    }

    fn count(&self, method: &str) -> usize {
        let got = self.got.lock().unwrap();
        got.iter().filter(|r| r["method"] == method).count()
    }
}

/// A restart takes every pane's watch with it no more (theseus-qb1es): the
/// watch keeps its pane, says `· reconnecting` and never releases it, tries
/// again from 100 ms, reads the session once more when the daemon is back,
/// and says its state and tokens again, all within 2 s of the daemon's return.
#[test]
fn a_watch_keeps_its_pane_through_a_daemon_restart() {
    let dir = tempfile::tempdir().unwrap();
    let herdr = FakeHerdr::start(dir.path());
    let first = task_view(
        10,
        "waiting",
        "needs_you",
        "confirm fs.write: log.md",
        json!({"pending": [{"correlation_id": "act_w1", "tool": "fs.write", "reason": "log.md"}]}),
    );
    let d = Restarting::start(dir.path(), move |req| task_answers(first.clone(), req));
    let w = Watch::start_at(&d.sock, Some(&herdr), &["--interactive"]);
    herdr.wait_for("pane.report_metadata", 1);
    let asking = "needs you · fs.write log.md";
    let reports = || report_requests(&herdr.requests());
    assert_eq!(state_and_label(&reports()[0]).1, asking);
    let tokens_before = last_meta(&herdr.requests()).unwrap()["params"]["tokens"].clone();
    assert_eq!(d.count("session.wait"), 1);

    // The daemon goes. The pane says so, and is not released.
    d.stop();
    let away = wait_until("the reconnecting label", || {
        reports()
            .into_iter()
            .find(|r| state_and_label(r).1.ends_with("· reconnecting"))
    });
    assert_eq!(
        away["params"]["state"], "blocked",
        "the state stays as it was"
    );
    assert_eq!(state_and_label(&away).1, format!("{asking} · reconnecting"));
    // Waiting costs a timer, not a loop: the watch's CPU time over the outage.
    let cpu = cpu_ticks(w.child.id());
    std::thread::sleep(Duration::from_millis(600));
    let spent = cpu_ticks(w.child.id()) - cpu;
    assert!(
        spent <= 2,
        "{spent} ticks of CPU (10 ms each) while it waited"
    );
    assert!(w.child_alive(), "the watch waits for the daemon");
    let released = herdr
        .requests()
        .iter()
        .any(|r| r["method"] == "pane.release_agent");
    assert!(!released, "the pane is never released meanwhile");

    // It comes back: within 2 s, one re-read, and the labels and tokens again.
    let back = Instant::now();
    d.restart();
    let mended = wait_until("the label restored", || {
        reports()
            .into_iter()
            .rev()
            .find(|r| state_and_label(r).1 == asking)
            .filter(|r| r["params"]["seq"].as_u64() > away["params"]["seq"].as_u64())
    });
    let took = back.elapsed();
    assert!(took < Duration::from_secs(2), "mended in {took:?}");
    assert_eq!(mended["params"]["state"], "blocked");
    w.wait_err("the daemon is back", 1);
    assert_eq!(
        d.count("session.wait"),
        2,
        "read once at the start, once on return"
    );
    assert_eq!(d.count("session.watch"), 2);
    let tokens_after = wait_until("the tokens restored", || {
        let last = last_meta(&herdr.requests())?;
        (last["params"]["tokens"]["session"] == S).then(|| last["params"]["tokens"].clone())
    });
    assert_eq!(tokens_after, tokens_before, "every token as it was");
    assert!(w.child_alive());
    // Ended by hand, it releases.
    w.signal(libc::SIGTERM);
    herdr.wait_for("pane.release_agent", 1);
    let (code, err) = w.finish();
    assert_eq!(code, 0, "{err}");
    within_herdrs_limits(&herdr.requests());
}

/// A watch started on a session that already failed says why, in words, and
/// waits: it neither exits nor stays silent, and the pane says it failed.
#[test]
fn a_watch_on_a_failed_session_says_why_and_waits() {
    let dir = tempfile::tempdir().unwrap();
    let herdr = FakeHerdr::start(dir.path());
    let d = task_daemon(
        dir.path(),
        task_view(
            10,
            "failed",
            "needs_you",
            "failed: the provider refused the request (400)",
            json!({"ended_reason": "the provider refused the request (400)"}),
        ),
    );
    let w = Watch::start(&d, Some(&herdr), &["--interactive"]);
    w.wait_err(
        &format!("{S} failed: the provider refused the request (400)"),
        1,
    );
    let got = herdr.wait_for("pane.report_metadata", 1);
    let r = &report_requests(&got)[0];
    assert_eq!(r["params"]["state"], "blocked");
    assert_eq!(
        r["params"]["state_labels"],
        json!({"blocked": "failed · the provider refused the request (400)"})
    );
    std::thread::sleep(Duration::from_millis(300));
    assert!(w.child_alive(), "it waits");
    w.signal(libc::SIGTERM);
    let (code, err) = w.finish();
    assert_eq!(code, 0, "{err}");
}

/// When the nth report reached herdr, after `sent`.
fn nth_report_after(herdr: &FakeHerdr, n: usize, sent: Instant) -> Option<Duration> {
    let (at, got) = (herdr.at.lock().unwrap(), herdr.got.lock().unwrap());
    let mut reports = got
        .iter()
        .zip(at.iter())
        .filter(|(r, _)| r["method"] == "pane.report_agent");
    reports
        .nth(n)
        .map(|(_, t)| t.saturating_duration_since(sent))
}

/// From the daemon's frame to herdr's ear, 200 changes on the scripted rig:
/// the 99th percentile within 10 ms (FAST). A change is one report.
#[test]
fn a_change_reaches_herdr_within_ten_ms_at_the_99th_percentile() {
    let dir = tempfile::tempdir().unwrap();
    let herdr = FakeHerdr::start(dir.path());
    let d = task_daemon(
        dir.path(),
        task_view(10, "running", "working", "turn 1", json!({})),
    );
    let w = Watch::start(&d, Some(&herdr), &[]);
    herdr.wait_for("pane.report_metadata", 1);
    let mut lat = Vec::new();
    for i in 0..200u64 {
        let seen = report_requests(&herdr.requests()).len();
        let sent = Instant::now();
        let label = format!("turn {}", i + 2);
        d.push(
            "execution.changed",
            task_view(11 + i, "running", "working", &label, json!({})),
        );
        lat.push(wait_until("the report", || {
            nth_report_after(&herdr, seen, sent)
        }));
    }
    lat.sort();
    let p99 = lat[lat.len() * 99 / 100];
    eprintln!(
        "frame to herdr: p50 {:?}, p99 {p99:?}, max {:?}",
        lat[lat.len() / 2],
        lat[lat.len() - 1]
    );
    assert!(p99 <= Duration::from_millis(10), "p99 {p99:?}");
    w.signal(libc::SIGTERM);
    let (code, err) = w.finish();
    assert_eq!(code, 0, "{err}");
}

/// theseus-ypnxt.3's join: a line typed in an ended task's pane goes to the conversation that
/// started it, and the pane's label goes back to what the task's last view said; it was left
/// "working · sending your message" for good, since an ended task sends no further view.
#[test]
fn a_line_sent_on_to_main_does_not_leave_the_pane_sending() {
    let dir = tempfile::tempdir().unwrap();
    // The task's refusal of the line is held until herdr has heard "sending your message": the
    // label going back follows that answer, so the two reports can never share a batch for the
    // adapter to coalesce into one (theseus-fumkt).
    let (sending_tx, sending_rx) = std::sync::mpsc::channel::<()>();
    let sending_tx = Mutex::new(sending_tx);
    let herdr = FakeHerdr::answering(dir.path(), move |r| {
        if r["method"] == "pane.report_agent" && r["params"]["message"] == "sending your message" {
            let _ = sending_tx.lock().unwrap().send(());
        }
        Ok(json!({"type": "ok"}))
    });
    let main = "ses_main0a1b2c";
    let mut ended = view(30, "complete", "ready", "done", None, 0.4);
    ended["kind"] = json!("task");
    let sending_rx = Mutex::new(sending_rx);
    let d = FakeDaemon::start(dir.path(), move |req| {
        let p = &req["params"];
        let task = json!({"session_id": S, "kind": "task", "label": "task",
            "title": "Sound the north channel", "parent_session_id": main,
            "created_at_unix_ms": 1, "turns": 3, "profile": "sonnet"});
        let parent = json!({"session_id": main, "kind": "conversation", "label": "Harbour log",
            "created_at_unix_ms": 1, "turns": 2, "profile": "sonnet"});
        Some(match req["method"].as_str().unwrap() {
            "session.watch" => Ok(json!({"watching": true})),
            "session.wait" => Ok(json!({"reached": "settled", "already": true,
                "execution": ended.clone(), "confirms": []})),
            "session.list"
                if p["ids"]
                    .as_array()
                    .is_some_and(|i| i.contains(&json!(main))) =>
            {
                Ok(json!({"sessions": [parent]}))
            }
            "session.list" => Ok(json!({"sessions": [task]})),
            "turn.submit" if p["session_id"] == S => {
                // Unbounded: a report that never comes fails the waits below, and nextest
                // kills a test that hangs; a line every 30 s says what is awaited.
                let t0 = std::time::Instant::now();
                let rx = sending_rx.lock().unwrap();
                while let Err(std::sync::mpsc::RecvTimeoutError::Timeout) =
                    rx.recv_timeout(Duration::from_secs(30))
                {
                    eprintln!(
                        "waiting for herdr's \"sending your message\" ({} s)",
                        t0.elapsed().as_secs()
                    );
                }
                Err(json!({"code": -32000,
                    "message": "the task is complete", "data": {"class": "execution_complete"}}))
            }
            "turn.submit" => Ok(json!({"session_id": main, "turn_id": "trn_1", "loops": 1,
                "output": "Noted.", "stop_reason": "end_turn", "provider_stop_reason": null,
                "model": "invented-model", "usage": {"input_tokens": 1, "output_tokens": 1},
                "elapsed_ms": 5})),
            _ => Ok(json!({})),
        })
    });
    let mut w = Watch::start(&d, Some(&herdr), &["--interactive"]);
    herdr.wait_for("pane.report_agent", 1);
    w.type_line("hello");
    d.wait_for("turn.submit", 2);
    w.wait_err("main › Noted.", 1);
    herdr.wait_for("pane.report_agent", 3);
    drop(w.child.stdin.take());
    herdr.wait_for("pane.release_agent", 1);
    let (code, err) = w.finish();
    assert_eq!(code, 0, "{err}");
    let got = reports(&herdr.requests());
    assert_eq!(got[1], rep("working", "sending your message"), "{got:?}");
    assert_eq!(
        got.last(),
        got.first(),
        "back to the last view's label: {got:?}"
    );
}

/// One question is one block in a pane: its links line and its prompt print once per question, whether
/// the daemon tells the question again or other events come while it waits (theseus-dowi0).
#[test]
fn one_question_one_prompt_in_a_pane() {
    let dir = tempfile::tempdir().unwrap();
    let herdr = FakeHerdr::start(dir.path());
    let waiting = |position| {
        view(
            position,
            "waiting",
            "needs_you",
            "confirm proc.run: run the tide tables",
            Some("act_m3"),
            0.0,
        )
    };
    let d = daemon(dir.path(), waiting(20), vec![question("act_m3")]);
    let mut w = Watch::start(&d, Some(&herdr), &["--interactive"]);
    w.wait_prompt(1);
    // The same question again, and views of the same wait: nothing is asked twice.
    d.push("confirm.requested", question("act_m3"));
    d.push("execution.changed", waiting(21));
    d.push("execution.changed", waiting(22));
    // The pane has read them all when it answers a line typed after them.
    std::thread::sleep(Duration::from_millis(1000));
    w.type_line("y");
    d.wait_for("action.confirm", 1);
    drop(w.child.stdin.take());
    let (code, err) = w.finish();
    assert_eq!(code, 0, "{err}");
    let count = |f: &dyn Fn(&str) -> bool| err.lines().filter(|l| f(l)).count();
    assert_eq!(count(&|l| l == "  approve? [y/N/note]"), 1, "{err}");
    assert_eq!(count(&|l| l.contains("[answer]")), 1, "{err}");
    assert_eq!(count(&|l| l.contains("proc.run")), 1, "one card: {err}");
}

/// prefix+shift+s on a task's pane cancels the task the pane shows: the pane's `task` token is the
/// title, so the cancel names the pane's session (theseus-dtvqa).
#[test]
fn a_stop_cancels_the_task_a_pane_shows() {
    let dir = tempfile::tempdir().unwrap();
    let herdr = FakeHerdr::answering(dir.path(), |req| match req["method"].as_str() {
        Some("pane.get") => Ok(
            json!({"type": "pane_info", "pane": {"pane_id": PANE, "tokens": {
            "kind": "task", "session": "ses_01a1772ee8", "task": "[walkthrough] drift"}}}),
        ),
        _ => Ok(json!({"type": "ok"})),
    });
    let d = FakeDaemon::start(dir.path(), |req| {
        Some(match req["method"].as_str().unwrap() {
            "wake.cancel" => Err(json!({"code": -32002, "message": "no pending wake is named"})),
            "task.cancel" => Ok(json!({"task": {"short": "a1b2c3", "state": "cancelled"},
                "cancelled_actions": []})),
            other => Err(json!({"code": -32601, "message": format!("no {other}")})),
        })
    });
    let out = Command::new(THESEUS)
        .arg("--socket")
        .arg(&d.sock)
        .args([
            "--json",
            "herdr",
            "stop",
            "--pane",
            PANE,
            "--yes",
            "--herdr-socket",
        ])
        .arg(&herdr.sock)
        .env_remove("THESEUS_SOCKET")
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let cancel = d.wait_for("task.cancel", 1);
    assert_eq!(cancel[0]["params"]["task"], "ses_01a1772ee8");
}

/// A call that starts while a question waits prints below the question; in a herdr pane that
/// does not say the prompt again: the pane's prompt is one per question (theseus-xgfps).
#[test]
fn a_pane_says_its_prompt_once_when_events_print_below_the_question() {
    let dir = tempfile::tempdir().unwrap();
    let herdr = FakeHerdr::start(dir.path());
    let waiting = view(
        20,
        "waiting",
        "needs_you",
        "confirm proc.run: run the tide tables",
        Some("act_m3"),
        0.0,
    );
    let d = daemon(dir.path(), waiting, vec![question("act_m3")]);
    let mut w = Watch::start(&d, Some(&herdr), &["--interactive"]);
    w.wait_prompt(1);
    for n in 1..=2 {
        d.push(
            "tool.started",
            json!({"session_id": S, "turn_id": "trn_1", "tool_use_id": format!("tu_{n}"),
                "tool": "fs.read", "correlation_id": format!("act_r{n}"), "backend": "inproc"}),
        );
        w.wait_err("fs.read", n);
    }
    std::thread::sleep(Duration::from_millis(500));
    drop(w.child.stdin.take());
    let (_, err) = w.finish();
    let count = err
        .lines()
        .filter(|l| *l == "  approve? [y/N/note]")
        .count();
    assert_eq!(count, 1, "{err}");
}
