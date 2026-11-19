//! The daemon's log and its store's writer, each held where a stalled disk would hold it, with the real `theseusd`
//! (debug builds only: the holds are a debug build's plant, `theseus_store::hold_by_fifo`). `health` answers while
//! the log writer is held (the daemon on ONE runtime worker, so a thread that waited on the log would hold every
//! other request with it), a turn is taken then, and `health` answers while the log writer AND the WAL writer are
//! held together (on the daemon's own workers: the turn that opens a session asks the log's position under the
//! writer's lock, on a worker, and a held writer holds that worker; with one worker that is the whole daemon, which
//! is why this test does not run on one).

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

/// `health` on the socket, waiting at most `bound` for the answer: its result, or why not.
fn health(d: &Served, bound: Duration) -> Result<Value, String> {
    let s = UnixStream::connect(d.path("sock")).map_err(|e| e.to_string())?;
    s.set_read_timeout(Some(bound)).unwrap();
    let req = json!({"jsonrpc": "2.0", "id": 1, "method": "health"});
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

/// Both held: the WAL writer inside a frame's write (with its lock) and the log writer inside its write. `health`
/// answers within the bound.
#[test]
fn health_answers_with_the_wal_writer_and_the_log_writer_both_held() {
    let h = daemon(false);
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
        }
        let (_a, _b) = (release(&h.wal), release(&h.log));
        held.join().unwrap();
    });
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
