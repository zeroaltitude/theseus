//! The index tender with the real `theseusd` and the real `theseus-index`
//! beside it (roadmap row 51; M6 §2.2), on BM25 alone: its model's files are
//! never where it looks. It starts after serving; a kill restarts it after
//! its backoff, which doubles; a restart in place takes the running tender
//! over with no second; and a stop does not wait for it. Each daemon is held
//! by a guard that kills and reaps it, and each tender this file sees is
//! killed at its end.

mod common;

use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use common::{Daemon, Served};
use serde_json::{json, Value};
use theseus_core::config_copy;

/// Where the tender would find its model: nowhere, so no test loads one.
const NO_MODELS: &str = "/nonexistent/theseus-test-models";

/// The tender's binary, beside the daemon's: the workspace's build of it.
fn tender_bin() -> PathBuf {
    let p = Path::new(env!("CARGO_BIN_EXE_theseusd")).with_file_name("theseus-index");
    assert!(
        p.is_file(),
        "no theseus-index at {}: build the workspace (`cargo nextest run --workspace` builds it)",
        p.display()
    );
    p
}

fn index_on(t: &mut toml::Table) {
    let index = t
        .entry("index")
        .or_insert_with(|| toml::Value::Table(Default::default()))
        .as_table_mut()
        .unwrap();
    index.insert("enabled".into(), true.into());
    index.insert("weights_dir".into(), NO_MODELS.into());
}

/// The process's state letter, `None` once it is gone.
fn state(pid: u32) -> Option<char> {
    let s = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    s[s.rfind(')')? + 2..].chars().next()
}

fn ended(pid: u32) -> bool {
    matches!(state(pid), None | Some('Z') | Some('X'))
}

fn signal(pid: u32, sig: i32) {
    // SAFETY: plain integers; each pid is a tender this test saw its daemon
    // start, and its daemon reaps it, so it is not yet reused.
    unsafe {
        libc::kill(pid as libc::pid_t, sig);
    }
}

/// The tenders among `daemon`'s children, by their command lines.
fn tenders_of(daemon: u32) -> Vec<u32> {
    let mut out = Vec::new();
    for t in std::fs::read_dir(format!("/proc/{daemon}/task"))
        .into_iter()
        .flatten()
        .flatten()
    {
        let kids = std::fs::read_to_string(t.path().join("children")).unwrap_or_default();
        for pid in kids
            .split_whitespace()
            .filter_map(|p| p.parse::<u32>().ok())
        {
            let cmd = std::fs::read(format!("/proc/{pid}/cmdline")).unwrap_or_default();
            let argv0 = cmd.split(|&b| b == 0).next().unwrap_or_default();
            if argv0.ends_with(b"theseus-index") && !ended(pid) {
                out.push(pid);
            }
        }
    }
    out
}

/// Kills, at the end of a test, every tender it saw, whatever happened.
struct Reap(Vec<u32>);

impl Drop for Reap {
    fn drop(&mut self) {
        for &pid in &self.0 {
            if !ended(pid) {
                signal(pid, libc::SIGCONT);
                signal(pid, libc::SIGKILL);
            }
        }
    }
}

/// `health` until `ok` holds, at most 20 s.
fn until(s: &Served, what: &str, ok: impl Fn(&Value) -> bool) -> Value {
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        if let Ok(h) = s.call("health", Value::Null) {
            if ok(&h) {
                return h;
            }
        }
        assert!(Instant::now() < deadline, "no {what} in 20 s:\n{}", s.log());
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn tender(h: &Value) -> &Value {
    &h["children"]["tenders"][0]
}

fn running(h: &Value) -> Option<u32> {
    let t = tender(h);
    (t["state"] == "running")
        .then(|| t["pid"].as_u64())?
        .map(|p| p as u32)
}

/// `index.tender` rows, and the position of the first `server.serving`.
fn rows(s: &Served) -> (Vec<Value>, u64) {
    let t = s.call("ledger.tail", json!({"n": 1000})).unwrap();
    let all = t["rows"].as_array().unwrap();
    let serving = all
        .iter()
        .find(|r| r["kind"] == "server.serving")
        .and_then(|r| r["position"].as_u64())
        .unwrap_or(u64::MAX);
    let tender = all
        .iter()
        .filter(|r| r["kind"] == "index.tender")
        .cloned()
        .collect();
    (tender, serving)
}

/// It starts after serving, 2 s after (its `started` row follows
/// `server.serving` in the WAL; health says `starting` meanwhile), and
/// indexes the store; a SIGKILL restarts it 1 s later, and a second, at once,
/// 2 s later; a stop sends it SIGTERM.
#[test]
fn the_tender_starts_after_serving_and_a_kill_restarts_it_after_its_backoff() {
    tender_bin();
    let s = Served::start(|_| {}, index_on);
    let mut seen = Reap(Vec::new());
    let h = until(&s, "the wait after serving", |h| {
        h["index"]["state"] == "starting"
    });
    assert_eq!(
        h["index"]["why"], "it starts 2 s after the daemon serves",
        "{}",
        h["index"]
    );
    assert!(running(&h).is_none(), "{}", h["index"]);
    let h = until(&s, "the tender's index ready", |h| {
        running(h).is_some() && h["index"]["state"] == "ready"
    });
    // Its `started` row is 2 s after `server.serving`, by the daemon's clock.
    let tail = s.call("ledger.tail", json!({"n": 1000})).unwrap();
    let at = |kind: &str| {
        tail["rows"]
            .as_array()
            .unwrap()
            .iter()
            .find(|r| r["kind"] == kind)
            .and_then(|r| r["at_unix_ms"].as_u64())
            .unwrap()
    };
    let after = at("index.tender") - at("server.serving");
    assert!(after >= 1990, "it started {after} ms after serving");
    let first = running(&h).unwrap();
    seen.0.push(first);
    assert_eq!(h["index"]["status"]["mode"], "bm25_only", "{}", h["index"]);
    assert_eq!(h["index"]["status"]["pid"], first);
    assert_eq!(tenders_of(s.daemon.id()), [first]);
    let (started, serving) = rows(&s);
    assert_eq!(started.len(), 1, "{started:?}");
    assert_eq!(started[0]["data"]["event"], "started");
    assert!(
        started[0]["position"].as_u64().unwrap() > serving,
        "the tender started after serving: {started:?}, serving at {serving}"
    );

    let killed = Instant::now();
    signal(first, libc::SIGKILL);
    let h = until(&s, "a second tender", |h| {
        running(h).is_some_and(|p| p != first)
    });
    let waited = killed.elapsed();
    let second = running(&h).unwrap();
    seen.0.push(second);
    assert!(waited >= Duration::from_millis(950), "{waited:?}");
    assert_eq!(tender(&h)["restarts"], 1);
    assert_eq!(tender(&h)["last_exit"], "signal 9");

    let killed = Instant::now();
    signal(second, libc::SIGKILL);
    let h = until(&s, "a third tender", |h| {
        running(h).is_some_and(|p| p != second)
    });
    let waited = killed.elapsed();
    seen.0.push(running(&h).unwrap());
    assert!(
        waited >= Duration::from_millis(1950),
        "the wait doubled: {waited:?}"
    );
    assert_eq!(tender(&h)["restarts"], 2);
    assert_eq!(tender(&h)["backoff_ms"], 2000);
    // Each row is written off the runtime's workers, a moment after.
    let t0 = Instant::now();
    let events = loop {
        let events: Vec<Value> = rows(&s)
            .0
            .iter()
            .map(|r| r["data"]["event"].clone())
            .collect();
        if events.len() >= 5 || t0.elapsed() > Duration::from_secs(5) {
            break events;
        }
        std::thread::sleep(Duration::from_millis(10));
    };
    assert_eq!(
        events,
        ["started", "exited", "started", "exited", "started"],
        "{}",
        s.log()
    );
    // A stop: SIGTERM, and the tender is gone.
    let third = *seen.0.last().unwrap();
    let _ = s.call("shutdown", Value::Null);
    let t0 = Instant::now();
    while !ended(third) {
        assert!(
            t0.elapsed() < Duration::from_secs(5),
            "the tender outlived the stop"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}

/// A stop waits for nothing (§9): with its tender stopped (SIGSTOP), so
/// that SIGTERM cannot end it, the daemon still stops at once. The tender
/// ends when it runs again. A SIGSTOP takes hold only once the tender is
/// next scheduled, so the stop waits until it reads as stopped: before
/// that, the stop's SIGTERM ends a tender still running (theseus-ux8g).
#[test]
fn a_stop_does_not_wait_for_the_tender() {
    tender_bin();
    let mut s = Served::start(|_| {}, index_on);
    let h = until(&s, "the tender", |h| running(h).is_some());
    let pid = running(&h).unwrap();
    let _seen = Reap(vec![pid]);
    signal(pid, libc::SIGSTOP);
    let t = Instant::now();
    while state(pid) != Some('T') {
        assert!(
            t.elapsed() < Duration::from_secs(20),
            "the tender never stopped: {:?}",
            state(pid)
        );
        std::thread::sleep(Duration::from_millis(1));
    }
    let t0 = Instant::now();
    s.call("shutdown", Value::Null).unwrap();
    while s.daemon.try_wait().is_none() {
        assert!(
            t0.elapsed() < Duration::from_secs(2),
            "the daemon waited for its stopped tender:\n{}",
            s.log()
        );
        std::thread::sleep(Duration::from_millis(5));
    }
    let took = t0.elapsed();
    assert!(took < Duration::from_secs(1), "{took:?}");
    assert_eq!(state(pid), Some('T'), "the tender is still stopped");
    signal(pid, libc::SIGCONT);
    let t1 = Instant::now();
    while !ended(pid) {
        assert!(
            t1.elapsed() < Duration::from_secs(5),
            "the tender outlived its daemon"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}

/// The config note the restart test's daemon reads from its fake vault.
const NOTE_REF: &str = "op://Test/theseus-config/notesPlain";

/// A stand-in `op` in `dir/bin`: `read` answers `dir/note.toml` once
/// `dir/go` exists, and `inject` fills each reference with a test value.
fn fake_vault(dir: &Path) {
    use std::os::unix::fs::PermissionsExt;
    let path = |p: &str| dir.join(p);
    std::fs::create_dir_all(path("bin")).unwrap();
    let op = path("bin/op");
    std::fs::write(
        &op,
        format!(
            "#!/bin/sh\n\
             case \"$1\" in\n\
             \x20 read) while [ ! -e '{go}' ] && [ -d '{dir}' ]; do sleep 0.05; done; cat '{note}' ;;\n\
             \x20 inject) sed -e 's/{{{{ [^}}]* }}}}/test-secret-value-0000/g' ;;\n\
             \x20 *) exit 1 ;;\n\
             esac\n",
            go = path("go").display(),
            dir = dir.display(),
            note = path("note.toml").display()
        ),
    )
    .unwrap();
    std::fs::set_permissions(&op, std::fs::Permissions::from_mode(0o755)).unwrap();
}

/// `theseusd` on `NOTE_REF`, through the fake vault in `dir/bin`, with its
/// state, socket, and log in `dir`, and none of this environment's settings.
fn on_the_vault(theseusd: &Path, dir: &Path) -> Daemon {
    let path = |p: &str| dir.join(p);
    let log = std::fs::File::create(path("theseusd.log")).unwrap();
    Daemon::spawn(
        Command::new(theseusd)
            .args(["--config", NOTE_REF, "--state-dir"])
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
            .env_remove("THESEUS_RESTARTED_ONTO_VAULT")
            .env_remove("THESEUS_CONFIG_VAULT_FIRST")
            .env_remove("THESEUS_OPERATOR_UMASK")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(log),
    )
}

/// One call on the socket at `sock`, and its result.
fn call_on(sock: &Path, method: &str) -> Result<Value, String> {
    let s = UnixStream::connect(sock).map_err(|e| e.to_string())?;
    s.set_read_timeout(Some(Duration::from_secs(30))).unwrap();
    let req = json!({"jsonrpc": "2.0", "id": 1, "method": method, "params": {"n": 1000}});
    (&s).write_all(format!("{req}\n").as_bytes())
        .map_err(|e| e.to_string())?;
    for line in BufReader::new(&s).lines() {
        let v: Value =
            serde_json::from_str(&line.map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
        if v["id"] == 1 {
            return Ok(v["result"].clone());
        }
    }
    Err("the connection closed".into())
}

/// The events of the `index.tender` rows, once there are `n`, or as many as
/// there are after 5 s. Each row is written off the runtime's workers, a
/// moment after its fact.
fn tender_events(sock: &Path, n: usize) -> Vec<Value> {
    let t0 = Instant::now();
    loop {
        let rows = call_on(sock, "ledger.tail").unwrap();
        let events: Vec<Value> = rows["rows"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|r| r["kind"] == "index.tender")
            .map(|r| r["data"]["event"].clone())
            .collect();
        if events.len() >= n || t0.elapsed() > Duration::from_secs(5) {
            return events;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

/// A restart in place onto the vault's changed note (theseus-2fo) keeps the
/// daemon's pid and children: the new image takes the running tender over,
/// and no second starts. The fake vault answers the note only once the
/// tender runs, so the old image has started it.
#[test]
fn a_restart_in_place_takes_the_running_tender_over() {
    tender_bin();
    let theseusd = PathBuf::from(env!("CARGO_BIN_EXE_theseusd"));
    let dir = tempfile::tempdir().unwrap();
    let path = |p: &str| dir.path().join(p);
    std::fs::create_dir_all(path("projects")).unwrap();
    fake_vault(dir.path());
    let note = |spend: f64| {
        let mut t: toml::Table = common::safe_note(&theseusd, &path("projects"), spend)
            .parse()
            .unwrap();
        index_on(&mut t);
        toml::to_string(&t).unwrap()
    };
    std::fs::write(path("note.toml"), note(42.5)).unwrap();
    config_copy::write(
        &config_copy::path(Some(&path("state"))),
        NOTE_REF,
        &note(100.0),
    )
    .unwrap();
    let mut daemon = on_the_vault(&theseusd, dir.path());
    let daemon_pid = daemon.id();
    let logs = || std::fs::read_to_string(path("theseusd.log")).unwrap_or_default();
    let sock = path("sock");
    let wait = |what: &str, daemon: &mut Daemon, ok: &dyn Fn(&Value) -> bool| -> Value {
        let deadline = Instant::now() + Duration::from_secs(20);
        loop {
            if let Ok(h) = call_on(&sock, "health") {
                if ok(&h) {
                    return h;
                }
            }
            if let Some(st) = daemon.try_wait() {
                panic!("theseusd exited ({st}) before {what}:\n{}", logs());
            }
            assert!(Instant::now() < deadline, "no {what} in 20 s:\n{}", logs());
            std::thread::sleep(Duration::from_millis(10));
        }
    };
    let h = wait("the tender, before the vault answers", &mut daemon, &|h| {
        running(h).is_some()
    });
    assert_eq!(h["config"]["state"], "confirming", "{}", h["config"]);
    let pid = running(&h).unwrap();
    let _seen = Reap(vec![pid]);
    // The vault answers a changed note: the daemon restarts in place.
    std::fs::write(path("go"), "").unwrap();
    let h = wait("the restart", &mut daemon, &|h| {
        !h["config"]["restarted"].is_null() && running(h).is_some()
    });
    assert!(daemon.try_wait().is_none(), "the same process");
    assert_eq!(running(&h), Some(pid), "the same tender: {}", tender(&h));
    assert_eq!(tender(&h)["adopted"], true);
    assert_eq!(tender(&h)["restarts"], 0);
    assert_eq!(tenders_of(daemon_pid), [pid], "one tender");
    assert!(logs().contains("took over the tender"), "{}", logs());
    // Its rows: the old image started it, the new one took it over.
    let events = tender_events(&sock, 2);
    assert_eq!(events, ["started", "adopted"], "{}", logs());
    let _ = call_on(&sock, "shutdown");
    let t0 = Instant::now();
    while !ended(pid) {
        assert!(
            t0.elapsed() < Duration::from_secs(5),
            "the tender outlived the stop"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}
