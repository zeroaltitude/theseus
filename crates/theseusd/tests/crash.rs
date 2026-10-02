//! The crash file (Review 2's consideration 1, theseus-xonq), on the real
//! binary. A debug build whose `THESEUS_TEST_PANIC` names `after_serving`
//! panics once it serves, and aborts, as the release build's
//! `panic = "abort"` does. Its panic hook writes the crash file first. The
//! next start takes the file, says so in its log and a `server.crashed` row,
//! and health reports the crash; the start after that still reports it, as
//! not its own.

mod common;

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

use serde_json::{json, Value};

use common::{safe_note, Daemon};

/// A daemon on `safe_note` in `dir`, as `common::Served` starts one, with
/// the planted panic when `panic` is set.
fn spawn(theseusd: &Path, dir: &Path, panic: bool, log: &str) -> Daemon {
    let path = |p: &str| dir.join(p);
    let mut cmd = Command::new(theseusd);
    cmd.arg("--config")
        .arg(path("config.toml"))
        .arg("--state-dir")
        .arg(path("state"))
        .arg("--socket")
        .arg(path("sock"))
        .env(
            "PATH",
            format!(
                "{}:{}",
                path("bin").display(),
                std::env::var("PATH").unwrap_or_default()
            ),
        )
        .env("OP_SERVICE_ACCOUNT_TOKEN", "test-not-a-token")
        .env_remove("THESEUS_OP_TOKEN_FILE")
        .env_remove("THESEUS_CONFIG")
        .env_remove("THESEUS_STATE_DIR")
        .env_remove("THESEUS_SOCKET")
        .env_remove("THESEUS_OPERATOR_UMASK")
        .env_remove("THESEUS_TEST_PANIC")
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::fs::File::create(path(log)).unwrap());
    if panic {
        cmd.env("THESEUS_TEST_PANIC", "after_serving");
    }
    Daemon::spawn(&mut cmd)
}

/// One JSON-RPC call on the socket: its result.
fn call(dir: &Path, method: &str, params: Value) -> Result<Value, String> {
    use std::io::{BufRead, Write};
    let s = std::os::unix::net::UnixStream::connect(dir.join("sock")).map_err(|e| e.to_string())?;
    s.set_read_timeout(Some(Duration::from_secs(30))).unwrap();
    let req = json!({"jsonrpc": "2.0", "id": 1, "method": method, "params": params});
    (&s).write_all(format!("{req}\n").as_bytes())
        .map_err(|e| e.to_string())?;
    for line in std::io::BufReader::new(&s).lines() {
        let v: Value =
            serde_json::from_str(&line.map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
        if v["id"] == 1 {
            return match v.get("error") {
                Some(e) if !e.is_null() => Err(e.to_string()),
                _ => Ok(v["result"].clone()),
            };
        }
    }
    Err("the connection closed".into())
}

/// Health, once the daemon answers.
fn health(dir: &Path, d: &mut Daemon, log: &str) -> Value {
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        if let Ok(h) = call(dir, "health", Value::Null) {
            return h;
        }
        if let Some(status) = d.try_wait() {
            panic!(
                "theseusd exited ({status}):\n{}",
                std::fs::read_to_string(dir.join(log)).unwrap_or_default()
            );
        }
        assert!(Instant::now() < deadline, "no health in 15 s");
        std::thread::sleep(Duration::from_millis(5));
    }
}

fn crashed_rows(dir: &Path) -> Vec<Value> {
    let tail = call(
        dir,
        "ledger.tail",
        json!({"n": 200, "kind": "server.crashed"}),
    )
    .unwrap();
    tail["rows"].as_array().cloned().unwrap_or_default()
}

#[test]
fn a_panic_leaves_a_crash_file_and_the_next_start_reports_it() {
    use std::os::unix::fs::PermissionsExt;
    let theseusd = PathBuf::from(env!("CARGO_BIN_EXE_theseusd"));
    let dir = tempfile::tempdir().unwrap();
    let d = dir.path();
    std::fs::create_dir_all(d.join("bin")).unwrap();
    std::fs::create_dir_all(d.join("projects")).unwrap();
    std::fs::write(d.join("bin/op"), "#!/bin/sh\nexit 1\n").unwrap();
    std::fs::set_permissions(d.join("bin/op"), std::fs::Permissions::from_mode(0o755)).unwrap();
    std::fs::write(
        d.join("config.toml"),
        safe_note(&theseusd, &d.join("projects"), 100.0),
    )
    .unwrap();

    let crash = the_planted_panic_aborts(&theseusd, d);
    the_next_start_reports_it(&theseusd, d, &crash);

    // 3. A clean start after it still reports the last crash, as not its own.
    let mut third = spawn(&theseusd, d, false, "third.log");
    let mut h = health(d, &mut third, "third.log");
    let deadline = Instant::now() + Duration::from_secs(15);
    while h["crash"].is_null() {
        assert!(Instant::now() < deadline, "no crash in health: {h}");
        std::thread::sleep(Duration::from_millis(20));
        h = call(d, "health", Value::Null).unwrap();
    }
    assert_eq!(h["crash"]["this_start"], false);
    assert_eq!(h["crash"]["location"], crash["location"]);
    assert_eq!(crashed_rows(d).len(), 1, "no second row");
}

/// The first start serves, then panics, and the abort ends it; the crash
/// file says what panicked. Returns the file's content.
fn the_planted_panic_aborts(theseusd: &Path, d: &Path) -> Value {
    let mut first = spawn(theseusd, d, true, "first.log");
    let deadline = Instant::now() + Duration::from_secs(15);
    let status = loop {
        if let Some(s) = first.try_wait() {
            break s;
        }
        assert!(
            Instant::now() < deadline,
            "the planted panic did not end the daemon in 15 s:\n{}",
            std::fs::read_to_string(d.join("first.log")).unwrap_or_default()
        );
        std::thread::sleep(Duration::from_millis(10));
    };
    use std::os::unix::process::ExitStatusExt;
    assert_eq!(status.signal(), Some(libc_sigabrt()), "aborted: {status}");
    let file = d.join("state/crash-socket.json");
    let crash: Value = serde_json::from_slice(&std::fs::read(&file).unwrap()).unwrap();
    assert!(
        crash["message"]
            .as_str()
            .is_some_and(|m| m.contains("a planted panic at after_serving")),
        "{crash}"
    );
    assert!(
        crash["location"]
            .as_str()
            .is_some_and(|l| l.contains("crash.rs")),
        "{crash}"
    );
    assert!(
        crash["thread"].as_str().is_some_and(|t| !t.is_empty()),
        "{crash}"
    );
    assert_eq!(crash["mode"], "socket");
    let first_log = std::fs::read_to_string(d.join("first.log")).unwrap();
    assert!(
        first_log.contains("a planted panic at after_serving"),
        "the hook it replaced still prints the panic:\n{first_log}"
    );
    crash
}

/// The next start takes the file, says so, and health reports it; then a
/// clean stop.
fn the_next_start_reports_it(theseusd: &Path, d: &Path, crash: &Value) {
    let file = d.join("state/crash-socket.json");
    let mut second = spawn(theseusd, d, false, "second.log");
    let h = health(d, &mut second, "second.log");
    let deadline = Instant::now() + Duration::from_secs(15);
    let h = loop {
        // The file is taken after serving: health has it once it is.
        if h["crash"]["this_start"] == true {
            break h;
        }
        let h2 = call(d, "health", Value::Null).unwrap();
        if h2["crash"]["this_start"] == true {
            break h2;
        }
        assert!(Instant::now() < deadline, "no crash in health: {h2}");
        std::thread::sleep(Duration::from_millis(20));
    };
    assert_eq!(h["crash"]["location"], crash["location"]);
    assert_eq!(h["crash"]["thread"], crash["thread"]);
    assert_eq!(h["crash"]["pid"], crash["pid"]);
    assert!(
        h["crash"].get("message").is_none(),
        "the message stays in the file"
    );
    let kept = PathBuf::from(h["crash"]["file"].as_str().unwrap());
    assert!(kept.starts_with(d.join("state/crashes")), "{kept:?}");
    assert!(kept.exists() && !file.exists(), "moved into crashes/");
    let rows = crashed_rows(d);
    assert_eq!(rows.len(), 1, "{rows:?}");
    assert_eq!(rows[0]["data"]["location"], crash["location"]);
    assert!(rows[0]["data"].get("message").is_none(), "{rows:?}");
    let second_log = std::fs::read_to_string(d.join("second.log")).unwrap();
    assert!(
        second_log.contains("the last run crashed"),
        "the start says it found one:\n{second_log}"
    );
    call(d, "shutdown", Value::Null).unwrap();
    let deadline = Instant::now() + Duration::from_secs(15);
    while second.try_wait().is_none() {
        assert!(Instant::now() < deadline, "no clean stop in 15 s");
        std::thread::sleep(Duration::from_millis(10));
    }
}

/// SIGABRT, which `std::process::abort` raises.
fn libc_sigabrt() -> i32 {
    6
}
