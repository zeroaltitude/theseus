//! Store versions with the real `theseusd` (theseus-qa0 F4a, theseus-8ni),
//! over a copy of a store an older binary wrote (460a35b; the fixture's
//! README says how it was made):
//! - it serves at once, and every session, execution, node, and ledger row
//!   in it reads; its manifest stays format 2 until this build writes a
//!   record newer than the older one could, and then is marked, once;
//! - a store marked with a schema newer than this build knows is refused,
//!   with a message that names the kind, both schemas, and what to do, and
//!   nothing in it is written;
//! - the WAL's history, which the open no longer checks, is checked after
//!   serving, and a corrupt frame in it is loud: health's `store.verify`
//!   phase, a `store.corrupt` ledger row, and reads from it refused.

mod common;

use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::{Duration, Instant};

use common::Daemon;
use serde_json::{json, Value};

/// A fake `op`: every reference's value is `tv-<its item>`.
const FAKE_OP: &str = "#!/bin/sh\n\
    case \"$1\" in\n\
    \x20 inject) sed -E 's#\\{\\{ op://Test/([^/]+)/[^}]* \\}\\}#tv-\\1#g' ;;\n\
    \x20 read) for a; do ref=\"$a\"; done; printf 'tv-%s' \"$(echo \"$ref\" | cut -d/ -f4)\" ;;\n\
    \x20 *) exit 1 ;;\n\
    esac\n";

/// The fixture's last position: 121 records.
const LAST: u64 = 121;

struct Rig {
    dir: tempfile::TempDir,
}

impl Rig {
    /// A scratch dir with a fake `op`, a safe config, and the fixture's
    /// store copied in as `state/store`.
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let path = |p: &str| dir.path().join(p);
        let theseusd = PathBuf::from(env!("CARGO_BIN_EXE_theseusd"));
        for d in ["bin", "projects", "state/store/wal"] {
            std::fs::create_dir_all(path(d)).unwrap();
        }
        use std::os::unix::fs::PermissionsExt;
        std::fs::write(path("bin/op"), FAKE_OP).unwrap();
        std::fs::set_permissions(path("bin/op"), std::fs::Permissions::from_mode(0o755)).unwrap();
        // The fixture's executions follow a $100 limit: the same keeps the
        // start from rewriting them.
        std::fs::write(
            path("config.toml"),
            common::safe_note(&theseusd, &path("projects"), 100.0),
        )
        .unwrap();
        let fixture = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../theseus-core/tests/fixtures/store-460a35b");
        for f in ["MANIFEST.json", "index.redb", "wal/000000001.seg"] {
            std::fs::copy(fixture.join(f), path("state/store").join(f)).unwrap();
        }
        Self { dir }
    }

    fn path(&self, p: &str) -> PathBuf {
        self.dir.path().join(p)
    }

    fn command(&self) -> std::process::Command {
        let log = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(self.path("theseusd.log"))
            .unwrap();
        let mut c = std::process::Command::new(env!("CARGO_BIN_EXE_theseusd"));
        c.arg("--config")
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
            .env_remove("THESEUS_OP_TOKEN_FILE")
            .env_remove("THESEUS_CONFIG")
            .env_remove("THESEUS_STATE_DIR")
            .env_remove("THESEUS_SOCKET")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(log);
        c
    }

    fn spawn(&self) -> Daemon {
        let d = Daemon::spawn(&mut self.command());
        self.wait("the socket", || self.call("health", Value::Null).ok());
        d
    }

    fn log(&self) -> String {
        std::fs::read_to_string(self.path("theseusd.log")).unwrap_or_default()
    }

    fn wait<T>(&self, what: &str, mut f: impl FnMut() -> Option<T>) -> T {
        let t0 = Instant::now();
        loop {
            if let Some(v) = f() {
                return v;
            }
            assert!(
                t0.elapsed() < Duration::from_secs(40),
                "no {what} in 40 s; the daemon's log ends:\n{}",
                tail(&self.log(), 30)
            );
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    fn call(&self, method: &str, params: Value) -> Result<Value, Value> {
        let s = UnixStream::connect(self.path("sock")).map_err(|e| json!(e.to_string()))?;
        s.set_read_timeout(Some(Duration::from_secs(60))).unwrap();
        let req = json!({"jsonrpc": "2.0", "id": 1, "method": method, "params": params});
        (&s).write_all(format!("{req}\n").as_bytes())
            .map_err(|e| json!(e.to_string()))?;
        for line in BufReader::new(&s).lines() {
            let v: Value = serde_json::from_str(&line.map_err(|e| json!(e.to_string()))?)
                .map_err(|e| json!(e.to_string()))?;
            if v["id"] == 1 {
                return match v.get("error") {
                    Some(e) if !e.is_null() => Err(e.clone()),
                    _ => Ok(v["result"].clone()),
                };
            }
        }
        Err(json!("the connection closed"))
    }

    /// Health's `store.verify` phase, once it has ended.
    fn verified(&self) -> Value {
        self.phase_end("store.verify")
    }

    /// A background startup phase's detail, once it has ended.
    fn phase_end(&self, name: &str) -> Value {
        self.wait(&format!("{name}'s end"), || {
            let h = self.call("health", Value::Null).ok()?;
            h["startup"]
                .as_array()?
                .iter()
                .find(|p| p["name"] == name && !p["end_us"].is_null())
                .map(|p| p["detail"].clone())
        })
    }

    fn manifest(&self) -> Value {
        serde_json::from_slice(&std::fs::read(self.path("state/store/MANIFEST.json")).unwrap())
            .unwrap()
    }

    /// (kind, schema) of every record after `after`, from a copy of the WAL
    /// (read-only: the daemon holds the store).
    fn written_after(&self, after: u64) -> Vec<(String, u16)> {
        let copy = tempfile::tempdir().unwrap();
        std::fs::copy(
            self.path("state/store/wal/000000001.seg"),
            copy.path().join("000000001.seg"),
        )
        .unwrap();
        let wal =
            theseus_store::Wal::open(copy.path(), theseus_store::WalConfig::default()).unwrap();
        wal.replay_from(after)
            .unwrap()
            .into_iter()
            .map(|(r, _)| {
                let label = match r.kind {
                    theseus_store::kinds::LEDGER => {
                        let row: Value = serde_json::from_slice(&r.payload).unwrap();
                        format!("ledger {}", row["kind"].as_str().unwrap_or("?"))
                    }
                    k => theseus_store::kinds::name(k).to_string(),
                };
                (label, r.schema)
            })
            .collect()
    }
}

fn tail(s: &str, n: usize) -> String {
    let lines: Vec<&str> = s.lines().collect();
    lines[lines.len().saturating_sub(n)..].join("\n")
}

/// Every file under `dir` with its bytes: what "nothing was written"
/// compares.
fn snapshot(dir: &Path) -> Vec<(PathBuf, Vec<u8>)> {
    let mut out = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        for e in std::fs::read_dir(&d).unwrap() {
            let p = e.unwrap().path();
            if p.is_dir() {
                stack.push(p);
            } else {
                out.push((p.clone(), std::fs::read(&p).unwrap()));
            }
        }
    }
    out.sort();
    out
}

#[test]
fn an_older_binarys_store_serves_at_once_and_is_marked_at_its_first_newer_record() {
    let rig = Rig::new();
    let _d = rig.spawn();
    let h = rig.call("health", Value::Null).unwrap();
    assert_eq!(h["sessions"], 4, "{h}");
    let store = h["startup"]
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["name"] == "store")
        .cloned()
        .unwrap();
    assert_eq!(store["detail"]["last_position"], LAST);
    assert_eq!(
        store["detail"]["history_bytes"], 59_867,
        "the old index's checkpoint spares the open the whole WAL: {store}"
    );

    // Every session, its nodes, and every execution read.
    let sessions = rig.call("session.list", json!({})).unwrap()["sessions"]
        .as_array()
        .cloned()
        .unwrap();
    assert_eq!(sessions.len(), 4);
    let mut nodes = 0;
    for s in &sessions {
        let hist = rig
            .call("session.history", json!({"session_id": s["session_id"]}))
            .unwrap();
        nodes += hist["nodes"].as_array().unwrap().len();
    }
    assert_eq!(nodes, 5, "the turn's messages");
    let execs = rig.call("execution.list", json!({})).unwrap();
    let states: Vec<&str> = execs["executions"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["state"].as_str().unwrap())
        .collect();
    assert_eq!(states.len(), 4, "{execs}");
    assert_eq!(
        states.iter().filter(|s| **s == "waiting").count(),
        3,
        "{states:?}"
    );
    assert!(states.contains(&"cancelled"), "{states:?}");
    let rows = rig
        .call("ledger.tail", json!({"n": 200, "kind": "tool.job_started"}))
        .unwrap();
    assert_eq!(rows["rows"].as_array().unwrap().len(), 1, "{rows}");

    // The whole history checks after serving.
    let v = rig.verified();
    assert_eq!(v["outcome"], "ok", "{v}");
    assert_eq!(v["records"], LAST, "{v}");

    // The older binary kept no terms in its index (theseus-lv2): the start
    // left them to after serving, and the kernel counted by a full read
    // until they were whole; its counts are the same after.
    assert_eq!(store["detail"]["terms_pending"], true, "{store}");
    let terms = rig.phase_end("store.terms");
    assert_eq!(terms["outcome"], "whole", "{terms}");
    let h = rig.call("health", Value::Null).unwrap();
    let by_state = &h["kernel"]["executions_by_state"];
    assert_eq!(by_state, &json!({"waiting": 3, "cancelled": 1}), "{h}");
    // Health's session totals, now from the projection, say what the full
    // read said.
    assert_eq!(h["sessions"], 4, "{h}");

    // The start wrote only what the older binary writes too: the manifest
    // is still format 2, and a rollback would still open the store.
    let m = rig.manifest();
    assert_eq!(
        m["format"],
        2,
        "the start marked the store; it wrote {:?}",
        rig.written_after(LAST)
    );
    // A new session is a session record at schema 6 (T1's hold, then
    // theseus-ljr's run of failures, theseus-0s4's images not shown,
    // theseus-qiy's search query in the hold, and theseus-ev1's 1-hour cache
    // writes in its usage): the manifest is marked first.
    rig.call("session.open", json!({"label": "after the upgrade"}))
        .unwrap();
    let m = rig.manifest();
    assert_eq!(m["format"], 3, "{m}");
    let session = m["kinds"]
        .as_array()
        .unwrap()
        .iter()
        .find(|k| k["name"] == "session")
        .cloned()
        .unwrap();
    assert_eq!(session["schema"], 6, "{m}");
    assert!(rig
        .written_after(LAST)
        .contains(&("session".to_string(), 6)));
}

#[test]
fn a_store_marked_newer_is_refused_with_the_message_and_left_as_it_was() {
    let rig = Rig::new();
    std::fs::write(
        rig.path("state/store/MANIFEST.json"),
        r#"{"format": 3, "engine": "redb", "kinds": [{"kind": 1, "name": "session", "schema": 7}]}"#,
    )
    .unwrap();
    let before = snapshot(&rig.path("state/store"));
    let mut d = Daemon::spawn(&mut rig.command());
    let status = rig.wait("the refusal", || d.try_wait());
    assert!(!status.success(), "it served");
    let log = rig.log();
    for says in [
        "session records (kind 1) at schema 7",
        "this build reads session records up to schema 6",
        "install the newer theseusd",
    ] {
        assert!(
            log.contains(says),
            "{says:?} missing from:\n{}",
            tail(&log, 10)
        );
    }
    assert_eq!(
        snapshot(&rig.path("state/store")),
        before,
        "the refused store was written to"
    );
    assert!(!rig.path("sock").exists(), "it never served");
}

/// The store phase's detail in health: what the open found.
fn store_phase(rig: &Rig) -> Value {
    let h = rig.call("health", Value::Null).unwrap();
    h["startup"]
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["name"] == "store")
        .map(|p| p["detail"].clone())
        .unwrap()
}

/// A clean stop drops the store, so the index is closed and the next start
/// repairs nothing: a static that owned the core once kept it open, and every
/// start paid redb's repair, 4 syncs more than a clean open (theseus-8ni).
/// After a SIGKILL the next start does repair it, which shows the signal.
#[test]
fn a_clean_stop_closes_the_index_and_the_next_start_repairs_nothing() {
    let rig = Rig::new();
    let mut d = rig.spawn();
    rig.verified();
    rig.call("shutdown", Value::Null).unwrap();
    rig.wait("the stop", || d.try_wait());
    let mut d = rig.spawn();
    let s = store_phase(&rig);
    assert_eq!(
        s["index_repaired"], false,
        "a clean stop left the index open: {s}"
    );
    let pid = d.id().to_string();
    let killed = std::process::Command::new("kill")
        .args(["-9", &pid])
        .status()
        .unwrap();
    assert!(killed.success());
    rig.wait("the kill", || d.try_wait());
    let _d = rig.spawn();
    let s = store_phase(&rig);
    assert_eq!(s["index_repaired"], true, "a SIGKILL needs a repair: {s}");
}

/// SIGTERM, which is systemd's stop and `kill`'s default, takes the clean
/// path, as SIGINT does (theseus-bv5); it used to kill the daemon outright.
/// Each exits 0, its socket gone, its `server.stopping` row naming the
/// signal, and the next start replays nothing and repairs nothing.
#[test]
fn a_sigterm_or_a_sigint_stops_cleanly_and_the_next_start_replays_nothing() {
    let rig = Rig::new();
    let mut d = rig.spawn();
    for signal in ["SIGTERM", "SIGINT"] {
        // Everything a start writes is written: the history check's end,
        // and this start's driver.
        rig.verified();
        rig.wait("the driver", || {
            let rows = rig.call("ledger.tail", json!({"n": 200})).ok()?;
            let rows = rows["rows"].as_array()?;
            let started = rows.iter().rposition(|r| r["kind"] == "server.started")?;
            rows[started..]
                .iter()
                .any(|r| r["kind"] == "driver.started")
                .then_some(())
        });
        let pid = d.id().to_string();
        let sent = std::process::Command::new("kill")
            .args([format!("-{}", &signal[3..]), pid])
            .status()
            .unwrap();
        assert!(sent.success());
        let status = rig.wait("the stop", || d.try_wait());
        assert!(
            status.success(),
            "{signal}: {status}; the log ends:\n{}",
            tail(&rig.log(), 20)
        );
        assert!(!rig.path("sock").exists(), "{signal} left the socket");
        d = rig.spawn();
        let s = store_phase(&rig);
        assert_eq!(s["replayed_into_index"], 0, "{signal}: {s}");
        assert_eq!(s["index_repaired"], false, "{signal}: {s}");
        let rows = rig.call("ledger.tail", json!({"n": 200})).unwrap();
        let stopping: Vec<&Value> = rows["rows"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|r| r["kind"] == "server.stopping")
            .collect();
        assert_eq!(
            stopping.last().map(|r| r["data"]["signal"].clone()),
            Some(json!(signal)),
            "{stopping:?}"
        );
    }
}

/// A start at once after a stop (theseus-qa0 F4b). `shutdown` answers before
/// the daemon removes its socket and closes its store, so a start that
/// followed at once could find the store still held and exit with "Database
/// already open". It now waits for the lock. Five stops, each followed at
/// once by a start on the same store: each start serves, and each stopped
/// daemon exits cleanly.
#[test]
fn a_start_at_once_after_a_stop_waits_for_the_store() {
    let rig = Rig::new();
    let mut d = rig.spawn();
    let mut waited = Vec::new();
    for i in 0..5 {
        rig.call("shutdown", Value::Null).unwrap();
        let mut next = Daemon::spawn(&mut rig.command());
        let status = rig.wait("the stop", || d.try_wait());
        assert!(status.success(), "stop {i}: {status}");
        rig.wait("the next start's answer", || {
            if let Some(s) = next.try_wait() {
                panic!(
                    "start {i} at once after a stop exited ({s}); the log ends:\n{}",
                    tail(&rig.log(), 20)
                );
            }
            rig.call("health", Value::Null).ok()
        });
        waited.push(store_phase(&rig)["lock_wait_ms"].as_f64().unwrap());
        d = next;
    }
    assert!(
        !rig.log().contains("Database already open"),
        "{}",
        tail(&rig.log(), 20)
    );
    eprintln!("each start's wait for the stopped daemon's store, ms: {waited:?}");
}

#[test]
fn the_history_check_after_serving_finds_a_corrupt_frame_and_says_so() {
    let rig = Rig::new();
    // The last byte of the first frame's body (the end of its last
    // record's payload): the records still decode, and only the crc knows.
    let seg = rig.path("state/store/wal/000000001.seg");
    let mut b = std::fs::read(&seg).unwrap();
    let body_len = u32::from_le_bytes(b[4..8].try_into().unwrap()) as usize;
    b[12 + body_len - 1] ^= 0x01;
    std::fs::write(&seg, &b).unwrap();
    let _d = rig.spawn();
    let h = rig.call("health", Value::Null).unwrap();
    assert_eq!(
        h["sessions"], 4,
        "the open did not read the history: it serves"
    );
    let v = rig.verified();
    assert_eq!(v["outcome"], "corrupt", "{v}");
    let e = v["error"].as_str().unwrap();
    assert!(
        e.contains("segment 1 at offset 0") && e.contains("crc mismatch"),
        "{e}"
    );
    // The newest rows (the corrupt frame holds the oldest: the first
    // start's steps, whose reads are now refused).
    let rows = rig
        .call("ledger.tail", json!({"n": 1, "kind": "store.corrupt"}))
        .unwrap();
    assert_eq!(rows["rows"].as_array().unwrap().len(), 1, "{rows}");
    assert!(rig.log().contains("the WAL's history does not check"));
    let refused = rig
        .call("ledger.tail", json!({"n": 1000}))
        .expect_err("a read of the corrupt frame's rows is refused");
    assert!(refused.to_string().contains("corrupt"), "{refused}");
}
