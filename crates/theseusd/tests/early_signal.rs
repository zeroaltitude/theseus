//! A signal in the start's first moments (theseus-6mxq), with the real
//! `theseusd`. SIGINT and SIGTERM were registered only once the socket was
//! bound and `after_serving` spawned, so one sent just after the `serving`
//! line took its default action: the daemon killed by the signal, with no
//! stopping row and no checkpoint, and the next start replayed the tail.
//! Now both are registered before the core is built, for the socket and
//! `--stdio` alike, and every signal from then on is a clean stop.

mod common;

use std::io::{BufRead, BufReader};
use std::os::unix::process::ExitStatusExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use serde_json::{json, Value};

use common::{safe_note, Daemon};

/// Where in the start a run sends its signal: on the kernel's first startup
/// step (the core being built, the first line after the signals'
/// registration), on `serving protocol` (the socket bound, and nothing
/// registered before the fix), or on the `serving` line (the report).
const TRIGGERS: [&str; 3] = [
    "kernel startup step step=1",
    "serving protocol",
    "serving serving_ms=",
];

struct Rig {
    dir: tempfile::TempDir,
}

impl Rig {
    fn new() -> Self {
        use std::os::unix::fs::PermissionsExt;
        let theseusd = PathBuf::from(env!("CARGO_BIN_EXE_theseusd"));
        let dir = tempfile::tempdir().unwrap();
        let path = |p: &str| dir.path().join(p);
        std::fs::create_dir_all(path("bin")).unwrap();
        std::fs::create_dir_all(path("projects")).unwrap();
        std::fs::write(path("bin/op"), "#!/bin/sh\nexit 1\n").unwrap();
        std::fs::set_permissions(path("bin/op"), std::fs::Permissions::from_mode(0o755)).unwrap();
        let note = safe_note(&theseusd, &path("projects"), 100.0);
        std::fs::write(path("config.toml"), note).unwrap();
        Self { dir }
    }

    fn path(&self, p: &str) -> PathBuf {
        self.dir.path().join(p)
    }

    fn command(&self) -> Command {
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_theseusd"));
        cmd.arg("--config")
            .arg(self.path("config.toml"))
            .arg("--state-dir")
            .arg(self.path("state"))
            .arg("--socket")
            .arg(self.path("sock"))
            .env(
                "PATH",
                format!(
                    "{}:{}",
                    self.path("bin").display(),
                    std::env::var("PATH").unwrap_or_default()
                ),
            )
            .env("OP_SERVICE_ACCOUNT_TOKEN", "test-not-a-token")
            .env_remove("THESEUS_LOG")
            .env_remove("THESEUS_OP_TOKEN_FILE")
            .env_remove("THESEUS_CONFIG")
            .env_remove("THESEUS_STATE_DIR")
            .env_remove("THESEUS_SOCKET")
            .env_remove("THESEUS_OPERATOR_UMASK")
            .env_remove("THESEUS_TEST_PANIC");
        cmd
    }

    /// One start that gets `signal` the moment its log shows `trigger`:
    /// its exit status and its log.
    fn signalled(
        &self,
        stdio: bool,
        trigger: &str,
        signal: i32,
    ) -> (std::process::ExitStatus, String) {
        let mut cmd = self.command();
        if stdio {
            // Its one client holds the pipes open, so only the signal ends it.
            cmd.arg("--stdio")
                .stdin(Stdio::piped())
                .stdout(Stdio::piped());
        } else {
            cmd.stdin(Stdio::null()).stdout(Stdio::null());
        }
        let mut d = Daemon::spawn(cmd.stderr(Stdio::piped()));
        let stderr = d.stderr();
        let pid = d.id() as i32;
        // The log's lines, read on a thread of their own, so a start that
        // never writes the trigger fails the test instead of hanging it.
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            for line in BufReader::new(stderr).lines() {
                let Ok(line) = line else { break };
                if tx.send(plain(&line)).is_err() {
                    break;
                }
            }
        });
        let mut log = String::new();
        loop {
            let Ok(line) = rx.recv_timeout(Duration::from_secs(30)) else {
                panic!("no {trigger:?} in the log in 30 s:\n{log}");
            };
            log.push_str(&line);
            log.push('\n');
            if line.contains(trigger) {
                // SAFETY: kill with plain integers, to the pid this test spawned.
                assert_eq!(unsafe { libc::kill(pid, signal) }, 0);
                break;
            }
        }
        let t0 = Instant::now();
        let status = loop {
            if let Some(s) = d.try_wait() {
                break s;
            }
            assert!(
                t0.elapsed() < Duration::from_secs(30),
                "no stop in 30 s:\n{log}"
            );
            std::thread::sleep(Duration::from_millis(5));
        };
        // The daemon has exited: its log ends once every line is read.
        while let Ok(line) = rx.recv_timeout(Duration::from_secs(5)) {
            log.push_str(&line);
            log.push('\n');
        }
        (status, log)
    }
}

/// A log line without its colours' escapes.
fn plain(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let mut chars = line.chars();
    while let Some(c) = chars.next() {
        if c == '\u{1b}' {
            for c in chars.by_ref() {
                if c.is_ascii_alphabetic() {
                    break;
                }
            }
        } else {
            out.push(c);
        }
    }
    out
}

/// The store's phase of a start: what its open replayed and repaired.
fn store_phase(h: &Value) -> Value {
    h["startup"]
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["name"] == "store")
        .map(|p| p["detail"].clone())
        .unwrap()
}

/// One JSON-RPC call on the socket in `dir`.
fn call(dir: &Path, method: &str, params: Value) -> Result<Value, String> {
    use std::io::Write;
    let s = std::os::unix::net::UnixStream::connect(dir.join("sock")).map_err(|e| e.to_string())?;
    s.set_read_timeout(Some(Duration::from_secs(30))).unwrap();
    let req = json!({"jsonrpc": "2.0", "id": 1, "method": method, "params": params});
    (&s).write_all(format!("{req}\n").as_bytes())
        .map_err(|e| e.to_string())?;
    for line in BufReader::new(&s).lines() {
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

/// The runs' signals as each `server.stopping` row names them, from a socket
/// start on the rig's store, and that start's store phase.
fn stopped(rig: &Rig) -> (Vec<String>, Value) {
    let mut d = Daemon::spawn(
        rig.command()
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null()),
    );
    let t0 = Instant::now();
    let h = loop {
        if let Ok(h) = call(rig.dir.path(), "health", Value::Null) {
            break h;
        }
        assert!(d.try_wait().is_none(), "the check's start exited");
        assert!(t0.elapsed() < Duration::from_secs(15), "no health in 15 s");
        std::thread::sleep(Duration::from_millis(10));
    };
    let rows = call(rig.dir.path(), "ledger.tail", json!({"n": 5000})).unwrap();
    let signals = rows["rows"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|r| r["kind"] == "server.stopping")
        .map(|r| r["data"]["signal"].as_str().unwrap_or("").to_string())
        .collect();
    (signals, store_phase(&h))
}

/// Many starts of each daemon, each sent SIGINT or SIGTERM at once at a
/// point of its start: every one exits 0 having said it stopped on the
/// signal, every one's `server.stopping` row names it, and a start after the
/// last replays nothing and repairs nothing.
fn every_early_signal_is_a_clean_stop(stdio: bool) {
    let rig = Rig::new();
    let mut sent = Vec::new();
    for run in 0..24 {
        let trigger = TRIGGERS[run % TRIGGERS.len()];
        let (signal, name) = if run % 2 == 0 {
            (libc::SIGINT, "SIGINT")
        } else {
            (libc::SIGTERM, "SIGTERM")
        };
        let (status, log) = rig.signalled(stdio, trigger, signal);
        assert!(
            status.success(),
            "run {run}, {name} on {trigger:?}: {status} (killed by {:?}); the log:\n{log}",
            status.signal()
        );
        assert!(
            log.contains("stopping on a signal"),
            "run {run}, {name} on {trigger:?}: no clean stop in the log:\n{log}"
        );
        sent.push(name.to_string());
    }
    if stdio {
        // The stdio daemon's store is `store-stdio`: one more stdio start
        // and its health tell the same.
        return stdio_rows(&rig, &sent);
    }
    let (signals, store) = stopped(&rig);
    assert_eq!(signals, sent, "one stopping row per run, naming its signal");
    assert_eq!(
        (&store["replayed_into_index"], &store["index_repaired"]),
        (&json!(0), &json!(false)),
        "the last stop closed the store: {store}"
    );
}

/// The `--stdio` daemon's rows, read over its own pipes.
fn stdio_rows(rig: &Rig, sent: &[String]) {
    let mut d = Daemon::spawn(
        rig.command()
            .arg("--stdio")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null()),
    );
    let mut c = common::stdio::StdioClient::new(&mut d);
    let h = c.call("health", Value::Null).unwrap();
    let store = store_phase(&h);
    let rows = c.call("ledger.tail", json!({"n": 5000})).unwrap();
    let signals: Vec<String> = rows["rows"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|r| r["kind"] == "server.stopping")
        .map(|r| r["data"]["signal"].as_str().unwrap_or("").to_string())
        .collect();
    assert_eq!(signals, sent, "one stopping row per run, naming its signal");
    assert_eq!(
        (&store["replayed_into_index"], &store["index_repaired"]),
        (&json!(0), &json!(false)),
        "the last stop closed the store: {store}"
    );
}

/// The store phase of the next start in the same mode, which is then shut
/// down over the protocol: what the last stop left to replay.
fn next_start(rig: &Rig, stdio: bool) -> Value {
    let wait = |d: &mut Daemon| {
        let t0 = Instant::now();
        while d.try_wait().is_none() {
            assert!(t0.elapsed() < Duration::from_secs(15), "no stop in 15 s");
            std::thread::sleep(Duration::from_millis(5));
        }
    };
    if stdio {
        let mut d = Daemon::spawn(
            rig.command()
                .arg("--stdio")
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::null()),
        );
        let mut c = common::stdio::StdioClient::new(&mut d);
        let store = store_phase(&c.call("health", Value::Null).unwrap());
        c.call("shutdown", Value::Null).unwrap();
        drop(c);
        wait(&mut d);
        return store;
    }
    let mut d = Daemon::spawn(
        rig.command()
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null()),
    );
    let t0 = Instant::now();
    let h = loop {
        if let Ok(h) = call(rig.dir.path(), "health", Value::Null) {
            break h;
        }
        assert!(d.try_wait().is_none(), "the check's start exited");
        assert!(t0.elapsed() < Duration::from_secs(15), "no health in 15 s");
        std::thread::sleep(Duration::from_millis(10));
    };
    call(rig.dir.path(), "shutdown", Value::Null).unwrap();
    wait(&mut d);
    store_phase(&h)
}

/// A stop on the `serving` line, checked after every run: the work after
/// serving (`announce_serving`'s frame, the driver's `driver.started`) wrote
/// on its own time, so a frame of it could land after the stop's last
/// checkpoint and the next start replayed it. Each now goes through the
/// core's late-row gate (theseus-81kk).
fn every_stop_on_serving_leaves_nothing_to_replay(stdio: bool) {
    let rig = Rig::new();
    for run in 0..30 {
        let signal = if run % 2 == 0 {
            libc::SIGINT
        } else {
            libc::SIGTERM
        };
        let (status, log) = rig.signalled(stdio, "serving serving_ms=", signal);
        assert!(status.success(), "run {run}: {status}; the log:\n{log}");
        let store = next_start(&rig, stdio);
        assert_eq!(
            (&store["replayed_into_index"], &store["index_repaired"]),
            (&json!(0), &json!(false)),
            "run {run}: the stop left a frame after its last checkpoint: {store}; its log:\n{log}"
        );
    }
}

#[test]
fn a_stop_on_serving_leaves_nothing_to_replay() {
    every_stop_on_serving_leaves_nothing_to_replay(false);
}

#[test]
fn a_stdio_stop_on_serving_leaves_nothing_to_replay() {
    every_stop_on_serving_leaves_nothing_to_replay(true);
}

#[test]
fn a_signal_just_after_serving_is_a_clean_stop() {
    every_early_signal_is_a_clean_stop(false);
}

#[test]
fn a_stdio_daemons_early_signal_is_a_clean_stop() {
    every_early_signal_is_a_clean_stop(true);
}
