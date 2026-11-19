//! The daemon's log and its store's writer, each held where a stalled disk would hold it, with the real `theseusd`
//! (debug builds only: the holds are a debug build's plant, `theseus_store::hold_by_fifo`). `health` answers while
//! the log writer is held, a turn is taken then, and `health` and another session's read answer while the log writer
//! AND the WAL writer are held together and a turn waits for the write. The daemon runs on ONE runtime worker, so a
//! thread that waited on either would hold every other request with it.

mod common;

use std::ffi::CString;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use common::Served;
use serde_json::{json, Value};

/// How long `health` may take with the writers held. Held, they never come back inside it: the bound is the test's
/// patience, and an answer that needed a writer would be no answer in it.
const ANSWER_WITHIN: Duration = Duration::from_secs(5);

/// A FIFO at `path`, which arms the hold named by the variable that points at it.
fn arm(path: &Path) {
    let c = CString::new(path.to_str().unwrap()).unwrap();
    // SAFETY: a valid path, a mode.
    assert_eq!(unsafe { libc::mkfifo(c.as_ptr(), 0o600) }, 0, "mkfifo");
}

/// Wait until the held thread has arrived (`<path>.in`).
fn arrived(path: &Path, what: &str) {
    let marker = PathBuf::from(format!("{}.in", path.display()));
    let t0 = Instant::now();
    while !marker.exists() {
        assert!(
            t0.elapsed() < Duration::from_secs(30),
            "{what} was never held"
        );
        std::thread::sleep(Duration::from_millis(5));
    }
}

/// Let the held thread go, and keep the end open, so a later write passes at once.
fn release(path: &Path) -> std::fs::File {
    std::fs::OpenOptions::new().write(true).open(path).unwrap()
}

/// One call on the socket, waiting at most `bound` for the answer: its result, or why not.
fn rpc(d: &Served, method: &str, params: Value, bound: Duration) -> Result<Value, String> {
    let s = UnixStream::connect(d.path("sock")).map_err(|e| e.to_string())?;
    s.set_read_timeout(Some(bound)).unwrap();
    let req = json!({"jsonrpc": "2.0", "id": 1, "method": method, "params": params});
    (&s).write_all(format!("{req}\n").as_bytes())
        .map_err(|e| e.to_string())?;
    let line = BufReader::new(&s)
        .lines()
        .next()
        .ok_or("the connection closed")?
        .map_err(|e| format!("no answer in {bound:?}: {e}"))?;
    let v: Value = serde_json::from_str(&line).map_err(|e| e.to_string())?;
    Ok(v["result"].clone())
}

/// `health`, within `bound`.
fn health(d: &Served, bound: Duration) -> Result<Value, String> {
    rpc(d, "health", Value::Null, bound)
}

struct Held {
    daemon: Served,
    wal: PathBuf,
    log: PathBuf,
    _fifos: tempfile::TempDir,
}

fn daemon(one_worker: bool) -> Held {
    let fifos = tempfile::tempdir().unwrap();
    let (wal, log) = (fifos.path().join("wal"), fifos.path().join("log"));
    let hook = {
        let (wal, log) = (wal.clone(), log.clone());
        move |c: &mut std::process::Command| {
            c.env("THESEUS_TEST_WAL_HOLD", &wal)
                .env("THESEUS_TEST_LOG_HOLD", &log)
                .env("THESEUS_LOG", "debug");
            if one_worker {
                c.env("TOKIO_WORKER_THREADS", "1");
            }
        }
    };
    let daemon = Served::start_with(|_| {}, |_| {}, hook);
    Held {
        daemon,
        wal,
        log,
        _fifos: fifos,
    }
}

/// A turn's submit, answered (this daemon has no model to give it: the answer may be an error, and it is an answer).
fn submit(d: &Served, input: &str) -> Duration {
    let t0 = Instant::now();
    let _ = d.call(
        "turn.submit",
        json!({"input": input, "author": "test", "attachments": []}),
    );
    t0.elapsed()
}

/// The log writer is held where a stalled destination holds it: `health` answers, and so does a turn's submit.
#[test]
fn health_and_a_turn_are_taken_while_the_logs_write_is_held() {
    let h = daemon(true);
    arm(&h.log);
    // Lines for the writer to take: the first one it takes is where it is held.
    for i in 0..5 {
        let _ = health(&h.daemon, ANSWER_WITHIN);
        std::thread::sleep(Duration::from_millis(5 * i));
    }
    submit(&h.daemon, "one");
    arrived(&h.log, "the log's write");
    for _ in 0..20 {
        let t0 = Instant::now();
        health(&h.daemon, ANSWER_WITHIN).unwrap();
        assert!(t0.elapsed() < ANSWER_WITHIN, "health waited for the log");
    }
    let took = submit(&h.daemon, "two");
    assert!(took < ANSWER_WITHIN, "a turn waited for the log: {took:?}");
    let _end = release(&h.log);
}

/// Both held, on ONE runtime worker: the WAL writer inside a frame's write (with its lock) and the log writer inside
/// its write, while a turn's submit waits for the write. `health` answers, and so does a read of another session,
/// within the bound: the submit's wait for the writer (its staging asks the log's position under the writer's lock)
/// is made off the worker. Let go, the waiting turn goes through, and its session holds its execution.
#[test]
fn health_and_another_session_answer_while_a_turn_waits_on_the_held_writers_on_one_worker() {
    let h = daemon(true);
    submit(&h.daemon, "first");
    let listed = rpc(&h.daemon, "session.list", json!({}), ANSWER_WITHIN).unwrap();
    let other = listed["sessions"][0]["session_id"]
        .as_str()
        .unwrap()
        .to_string();
    arm(&h.log);
    arm(&h.wal);
    // A turn writes frames: its submit is held with the writer, on a thread of its own.
    std::thread::scope(|s| {
        let held = s.spawn(|| submit(&h.daemon, "writes"));
        arrived(&h.wal, "the store's write");
        // The log writer is held by the lines a daemon says as it goes.
        for _ in 0..5 {
            let _ = health(&h.daemon, ANSWER_WITHIN);
        }
        arrived(&h.log, "the log's write");
        for _ in 0..20 {
            let t0 = Instant::now();
            let said = health(&h.daemon, ANSWER_WITHIN).unwrap();
            assert!(
                t0.elapsed() < ANSWER_WITHIN,
                "health took {:?} with the writers held",
                t0.elapsed()
            );
            assert!(said["name"].is_string(), "{said}");
            let read = rpc(
                &h.daemon,
                "session.history",
                json!({"session_id": other}),
                ANSWER_WITHIN,
            )
            .unwrap();
            assert!(read["nodes"].is_array(), "another session's read: {read}");
        }
        let (_a, _b) = (release(&h.wal), release(&h.log));
        held.join().unwrap();
    });
    // The turn that waited went through: both sessions are there, and the second holds its execution.
    let listed = rpc(&h.daemon, "session.list", json!({}), ANSWER_WITHIN).unwrap();
    let sessions = listed["sessions"].as_array().unwrap();
    assert_eq!(sessions.len(), 2, "{listed}");
    let newer = sessions
        .iter()
        .map(|s| s["session_id"].as_str().unwrap())
        .find(|id| *id != other)
        .unwrap();
    let history = rpc(
        &h.daemon,
        "session.history",
        json!({"session_id": newer}),
        ANSWER_WITHIN,
    )
    .unwrap();
    // This daemon has no model, so the session holds no node; it holds the execution the turn staged and committed.
    assert!(history["session"]["execution_id"].is_string(), "{history}");
}

/// A start that fails still says what it logged before, and then why: the queue is written out before the error that
/// ends the start is printed, in the order the lines were made. The log's write is held until the start has failed
/// (the FIFO), so a start that did not wait for the queue would end with the line unwritten.
#[test]
fn a_start_that_fails_says_what_it_logged_and_then_why() {
    let home = tempfile::tempdir().unwrap();
    let hold = home.path().join("log-hold");
    arm(&hold);
    let mut child = std::process::Command::new(env!("CARGO_BIN_EXE_theseusd"))
        .args(["--config", "/nonexistent/theseus.toml", "--state-dir"])
        .arg(home.path().join("state"))
        .arg("--socket")
        .arg(home.path().join("sock"))
        .env("HOME", home.path())
        .env("THESEUS_LOG", "info")
        .env("THESEUS_TEST_LOG_HOLD", &hold)
        .env_remove("OP_SERVICE_ACCOUNT_TOKEN")
        .env_remove("THESEUS_OP_TOKEN_FILE")
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    arrived(&hold, "the first line's write");
    // The line is held, and the start has failed: a start that waits for its queue is still here a moment later,
    // one that does not has ended with the line unwritten.
    std::thread::sleep(Duration::from_millis(500));
    assert!(
        child.try_wait().unwrap().is_none(),
        "the failed start ended with its log's first line unwritten"
    );
    let _end = release(&hold);
    let t0 = Instant::now();
    let status = loop {
        if let Some(status) = child.try_wait().unwrap() {
            break status;
        }
        if t0.elapsed() > Duration::from_secs(20) {
            let _ = child.kill();
            panic!("the failed start did not end");
        }
        std::thread::sleep(Duration::from_millis(5));
    };
    assert!(!status.success());
    let mut err = String::new();
    std::io::Read::read_to_string(&mut child.stderr.take().unwrap(), &mut err).unwrap();
    let logged = err.find("no 1Password access");
    let why = err.find("Error: reading config file /nonexistent/theseus.toml");
    assert!(
        matches!((logged, why), (Some(a), Some(b)) if a < b),
        "the line logged first, then the error: {err}"
    );
}
