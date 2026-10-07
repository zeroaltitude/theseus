//! The config copy with the real `theseusd` (theseus-2fo, theseus-zmgb). A
//! first start reads the vault's note and keeps the copy, its digest in the
//! store; a later start serves from the copy, and acts on it, while the vault
//! is read; an edited copy is not used, and that start reads the vault first;
//! a changed note restarts the daemon onto the vault's version through
//! `exec`, in the same process; a comment-only change confirms; a vault that
//! does not answer is said in health while the copy serves. The vault is a fake `op`,
//! first on the daemon's PATH, that answers after `OP_MS`, and not while
//! the test holds it (`hold`). Each daemon is
//! held by a guard that kills and reaps it, so an assertion that fails
//! before its stop leaves none running.

mod common;

use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use common::stdio::StdioClient;
use common::Daemon;
use serde_json::{json, Value};
use theseus_core::config_copy;

const NOTE_REF: &str = "op://Test/theseus-config/notesPlain";
/// How long the fake vault takes to answer. A start from the copy is proved
/// to serve first by order, not by this: the vault is held until health has
/// answered (theseus-a2ec).
const OP_MS: u64 = 500;

struct Rig {
    theseusd: PathBuf,
    dir: tempfile::TempDir,
}

impl Rig {
    fn new() -> Self {
        let r = Self {
            theseusd: PathBuf::from(env!("CARGO_BIN_EXE_theseusd")),
            dir: tempfile::tempdir().unwrap(),
        };
        let op = r.path("bin").join("op");
        std::fs::create_dir_all(op.parent().unwrap()).unwrap();
        std::fs::write(
            &op,
            fake_op(
                &r.path("note.toml"),
                &r.path("down"),
                &r.path("hold"),
                &r.path("hold-read"),
            ),
        )
        .unwrap();
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&op, std::fs::Permissions::from_mode(0o755)).unwrap();
        std::fs::create_dir_all(r.path("projects")).unwrap();
        r
    }

    fn path(&self, p: &str) -> PathBuf {
        self.dir.path().join(p)
    }

    fn command(&self) -> Command {
        let mut c = Command::new(&self.theseusd);
        c.arg("--config")
            .arg(NOTE_REF)
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
            .env_remove("THESEUS_RESTARTED_ONTO_VAULT")
            .env_remove("THESEUS_CONFIG_VAULT_FIRST")
            .stdin(Stdio::null());
        c
    }

    fn spawn(&self) -> (Daemon, Instant) {
        let log = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(self.path("theseusd.log"))
            .unwrap();
        let t0 = Instant::now();
        let daemon = Daemon::spawn(self.command().stdout(Stdio::null()).stderr(log));
        (daemon, t0)
    }

    /// Spawn to the first `health` answer.
    fn start(&self) -> (Daemon, Value, Duration) {
        let (mut daemon, t0) = self.spawn();
        let h = self.until(&mut daemon, "a first answer", |_| true);
        (daemon, h, t0.elapsed())
    }

    /// Ask `health` until `ok` says so, at most 15 s; a restart in between
    /// closes the socket, and the next ask finds the new one.
    fn until(&self, daemon: &mut Daemon, what: &str, ok: impl Fn(&Value) -> bool) -> Value {
        let deadline = Instant::now() + Duration::from_secs(15);
        loop {
            if let Ok(h) = self.call("health", Value::Null) {
                if ok(&h) {
                    return h;
                }
            }
            if let Some(status) = daemon.try_wait() {
                panic!("theseusd exited ({status}) before {what}:\n{}", self.log());
            }
            assert!(
                Instant::now() < deadline,
                "no {what} in 15 s:\n{}",
                self.log()
            );
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    fn call(&self, method: &str, params: Value) -> Result<Value, String> {
        let s = UnixStream::connect(self.path("sock")).map_err(|e| e.to_string())?;
        s.set_read_timeout(Some(Duration::from_secs(60))).unwrap();
        let req = json!({"jsonrpc": "2.0", "id": 1, "method": method, "params": params});
        (&s).write_all(format!("{req}\n").as_bytes())
            .map_err(|e| e.to_string())?;
        let mut lines = BufReader::new(&s).lines();
        loop {
            let line = lines
                .next()
                .ok_or("the connection closed")?
                .map_err(|e| e.to_string())?;
            let v: Value = serde_json::from_str(&line).map_err(|e| e.to_string())?;
            if v["id"] == 1 {
                return match v.get("error") {
                    Some(e) if !e.is_null() => Err(e.to_string()),
                    _ => Ok(v["result"].clone()),
                };
            }
        }
    }

    /// The protocol's `shutdown`, to the process's exit: a clean stop. The
    /// guard kills one that does not stop.
    fn stop(&self, mut daemon: Daemon) {
        let _ = self.call("shutdown", Value::Null);
        let deadline = Instant::now() + Duration::from_secs(10);
        while daemon.try_wait().is_none() {
            assert!(
                Instant::now() < deadline,
                "theseusd did not stop:\n{}",
                self.log()
            );
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    fn log(&self) -> String {
        let s = std::fs::read_to_string(self.path("theseusd.log")).unwrap_or_default();
        let lines: Vec<&str> = s.lines().collect();
        lines[lines.len().saturating_sub(30)..].join("\n")
    }

    /// The copy as the daemon keeps one, its digest in the store
    /// (theseus-zmgb), so the next start serves from it.
    fn keep_copy(&self, text: &str) {
        self.keep_copy_in("store", text);
    }

    /// `keep_copy`, its digest in the store named `store` (a `--stdio`
    /// daemon's is `store-stdio`).
    fn keep_copy_in(&self, store: &str, text: &str) {
        let store = theseus_core::store::Store::open(&self.path("state").join(store)).unwrap();
        config_copy::keep(
            &store,
            &config_copy::path(Some(&self.path("state"))),
            NOTE_REF,
            text,
        )
        .unwrap();
    }

    fn ledger(&self, kind: &str) -> Vec<Value> {
        let t = self.call("ledger.tail", json!({"n": 1000})).unwrap();
        t["rows"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|r| r["kind"] == kind)
            .map(|r| r["data"].clone())
            .collect()
    }
}

/// The fake vault: `read` answers the note from its file, and `inject`
/// gives every secret one value, each after `OP_MS`, and not while `hold`
/// exists; `read` not while `hold_read` exists either, so the secrets
/// resolve and the daemon settles while the note waits. While `down`
/// exists it answers nothing.
fn fake_op(note: &Path, down: &Path, hold: &Path, hold_read: &Path) -> String {
    format!(
        "#!/bin/sh\n\
         sleep {}\n\
         while [ -e '{}' ]; do sleep 0.01; done\n\
         if [ \"$1\" = read ]; then while [ -e '{}' ]; do sleep 0.01; done; fi\n\
         if [ -e '{}' ]; then echo '[ERROR] 2026/09/29 12:00:00 network down' >&2; exit 1; fi\n\
         case \"$1\" in\n\
         \x20 read) cat '{}' ;;\n\
         \x20 inject) sed -e 's/{{{{ [^}}]* }}}}/test-secret-value-0000/g' ;;\n\
         \x20 *) exit 1 ;;\n\
         esac\n",
        OP_MS as f64 / 1000.0,
        hold.display(),
        hold_read.display(),
        down.display(),
        note.display()
    )
}

/// Whether a daemon, asked through `ask`, has settled after serving: its
/// history check has ended, and since its start the driver has started and
/// the secrets have settled, so none of its start's work is left to write.
fn settled(ask: &mut dyn FnMut(&str, Value) -> Option<Value>) -> bool {
    let verified = ask("health", Value::Null).is_some_and(|h| {
        h["startup"].as_array().is_some_and(|ps| {
            ps.iter()
                .any(|p| p["name"] == "store.verify" && !p["end_us"].is_null())
        })
    });
    let Some(t) = ask("ledger.tail", json!({"n": 200})) else {
        return false;
    };
    let rows = t["rows"].as_array().cloned().unwrap_or_default();
    let started = rows
        .iter()
        .rposition(|r| r["kind"] == "server.started")
        .unwrap_or(0);
    let has = |k: &str| rows[started..].iter().any(|r| r["kind"] == k);
    verified && has("driver.started") && (has("secrets.resolved") || has("secrets.failed"))
}

/// The template, made safe to serve here, with every secret on the fake vault.
fn test_note(r: &Rig, spend_limit_usd: f64) -> String {
    common::safe_note(&r.theseusd, &r.path("projects"), spend_limit_usd)
}

#[test]
#[expect(clippy::cognitive_complexity, reason = "shape budget: split it")]
#[expect(clippy::too_many_lines, reason = "shape budget: split it")]
fn the_copy_serves_the_next_start_and_a_changed_note_restarts_the_daemon_in_place() {
    let r = Rig::new();
    let note = test_note(&r, 100.0);
    std::fs::write(r.path("note.toml"), &note).unwrap();
    let copy = config_copy::path(Some(&r.path("state")));

    // A first start: no copy, so it reads the vault before serving, and it
    // keeps the copy once serving.
    let (mut daemon, h, took) = r.start();
    assert_eq!(h["config"]["started_from"], "vault", "{}", h["config"]);
    assert_eq!(h["config"]["state"], "confirmed");
    assert!(
        took >= Duration::from_millis(OP_MS),
        "it read the vault first: {took:?}"
    );
    r.until(&mut daemon, "the copy kept", |h| {
        h["config"]["detail"]
            .as_str()
            .is_some_and(|d| d.contains("the copy is kept"))
    });
    use std::os::unix::fs::PermissionsExt;
    let mode = std::fs::metadata(&copy).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode, 0o600);
    assert_eq!(
        config_copy::read(&copy, NOTE_REF).unwrap().unwrap().text,
        note
    );
    r.stop(daemon);

    // From the copy: health answers while the vault is held, so before it
    // could answer, by order and not by a stopwatch (theseus-a2ec); it says
    // confirming, and once the vault answers, confirmed. A start that waited
    // for the vault would not answer at all until the hold is lifted.
    std::fs::write(r.path("hold"), "").unwrap();
    let (mut daemon, h, _) = r.start();
    std::fs::remove_file(r.path("hold")).unwrap();
    assert_eq!(h["config"]["state"], "confirming", "{}", h["config"]);
    assert_eq!(h["config"]["started_from"], "copy");
    let h = r.until(&mut daemon, "confirmed", |h| {
        h["config"]["state"] == "confirmed"
    });
    assert_eq!(h["config"]["detail"], "the same text as the copy");
    let phases = h["startup"].as_array().unwrap();
    assert!(phases
        .iter()
        .any(|p| p["name"] == "config.vault" && p["background"] == true));
    r.stop(daemon);
    let shown = r.command().arg("config").output().unwrap();
    let shown = String::from_utf8(shown.stdout).unwrap();
    assert!(
        shown.contains(&format!(
            "# copy: {} matches the vault's note",
            copy.display()
        )),
        "{shown}"
    );

    // A copy edited by hand (or by a job running as the operator's user) is
    // not the one the daemon wrote, whatever the edit: the start execs
    // itself, in place, to read the vault first, and keeps the copy again.
    {
        let raw = std::fs::read_to_string(&copy).unwrap();
        std::fs::write(&copy, format!("{raw}# edited\n")).unwrap();
    }
    let (mut daemon, h, _) = r.start();
    let pid = daemon.id();
    assert_eq!(h["config"]["started_from"], "vault", "{}", h["config"]);
    assert!(
        h["config"]["detail"]
            .as_str()
            .is_some_and(|d| d.starts_with("the copy was not used: its sha256 is not the one")),
        "{}",
        h["config"]
    );
    r.until(&mut daemon, "the copy kept", |h| {
        h["config"]["detail"]
            .as_str()
            .is_some_and(|d| d.contains("the copy is kept"))
    });
    assert!(daemon.try_wait().is_none(), "the same process: pid {pid}");
    assert_eq!(
        config_copy::read(&copy, NOTE_REF).unwrap().unwrap().text,
        note
    );
    r.stop(daemon);

    // A store that still holds a unit budget (from before theseus-0sg) is
    // migrated by the start from the copy, which acts at once.
    {
        use theseus_store::{kinds, NewRecord};
        let store = theseus_core::store::Store::open(&r.path("state").join("store")).unwrap();
        let legacy = json!({"id": "exe_old", "schema": 1, "session_id": "ses_old", "kind": "conversation",
            "authority": {"principal": "operator", "ceilings": {}}, "outstanding": [], "queued_results": [],
            "turns": 3, "interrupted": 0, "resume_pending": false, "created_at_ms": 1_790_000_000_000u64,
            "updated_at_ms": 1_790_000_500_000u64, "state": "waiting", "wake": {"on": "input"},
            "budget": {"limit": 20_000_000, "spent": 154_321, "reserved": 0, "held_unknown": 0,
                "control_reserve": 10_000, "reservations": {}}});
        store
            .append(
                &[NewRecord::json(kinds::EXECUTION, Some("exe_old"), &legacy)
                    .unwrap()
                    .scoped("ses_old")],
            )
            .unwrap();
    }
    let (mut daemon, h, _) = r.start();
    assert_eq!(h["config"]["started_from"], "copy", "{}", h["config"]);
    assert_eq!(
        r.ledger("budget.migrated").len(),
        1,
        "migrated under the copy's config"
    );
    r.until(&mut daemon, "confirmed", |h| {
        h["config"]["state"] == "confirmed"
    });
    r.stop(daemon);

    // The note changes: the daemon serves from the old copy, finds the
    // change, and restarts itself onto the vault's version, in place.
    // The first answer comes while the vault is held, so before it could
    // say the note changed, whatever the load (theseus-a2ec).
    let changed = test_note(&r, 42.5);
    std::fs::write(r.path("note.toml"), &changed).unwrap();
    std::fs::write(r.path("hold"), "").unwrap();
    let (mut daemon, h, _) = r.start();
    std::fs::remove_file(r.path("hold")).unwrap();
    assert_eq!(h["config"]["state"], "confirming");
    let pid = daemon.id();
    let h = r.until(&mut daemon, "the restart confirmed", |h| {
        h["config"]["state"] == "confirmed" && !h["config"]["restarted"].is_null()
    });
    assert!(daemon.try_wait().is_none(), "the same process: pid {pid}");
    assert_eq!(h["config"]["restarted"]["tables"], json!(["kernel"]));
    assert_eq!(
        h["kernel"]["spend_limit_usd"], 42.5,
        "the vault's version governs"
    );
    let cmdline = std::fs::read(format!("/proc/{pid}/cmdline")).unwrap();
    assert!(
        String::from_utf8_lossy(&cmdline).contains(NOTE_REF),
        "the same arguments"
    );
    // Exec'd as /proc/self/exe, it keeps its own name for ps and pgrep.
    let comm = std::fs::read_to_string(format!("/proc/{pid}/comm")).unwrap();
    assert_eq!(comm.trim_end(), "theseusd");
    let c = r.ledger("config.changed");
    assert_eq!(c.len(), 1);
    assert_eq!(c[0]["tables"], json!(["kernel"]));
    assert_eq!(c[0]["vault_sha256"], config_copy::sha256(&changed));
    assert_eq!(c[0]["copy_sha256"], config_copy::sha256(&note));
    assert_eq!(
        config_copy::read(&copy, NOTE_REF).unwrap().unwrap().text,
        changed
    );
    r.stop(daemon);

    // A comment-only change confirms, with no restart.
    let commented = format!("# pasted again\n{changed}");
    std::fs::write(r.path("note.toml"), &commented).unwrap();
    let (mut daemon, _, _) = r.start();
    let h = r.until(&mut daemon, "confirmed", |h| {
        h["config"]["state"] == "confirmed"
    });
    assert!(h["config"]["restarted"].is_null());
    assert_eq!(
        h["config"]["detail"],
        "only comments or formatting differed, and the copy was rewritten"
    );
    assert_eq!(
        config_copy::read(&copy, NOTE_REF).unwrap().unwrap().text,
        commented
    );
    r.stop(daemon);

    // A vault that does not answer: held, saying why, and serving the copy,
    // which acts; and shutdown works.
    std::fs::write(r.path("down"), "").unwrap();
    let (mut daemon, _, _) = r.start();
    let h = r.until(&mut daemon, "held", |h| h["config"]["state"] == "held");
    let why = h["config"]["detail"].as_str().unwrap();
    assert!(why.starts_with("the vault did not answer: "), "{why}");
    assert!(why.contains("network down"), "{why}");
    let opened = r.call("session.open", json!({})).unwrap();
    assert!(opened["session_id"].is_string(), "{opened}");
    r.stop(daemon);
}

/// A restart onto a changed note is an exec in the same process, which keeps
/// every child (theseus-z4b). The daemon is a child subreaper again after it,
/// and it reaps what the old image left. Here the vault answers the note at
/// once, and takes 2 s over the secrets, so an `op inject` is still running
/// when the daemon restarts: the old image's runtime kills that `op` as it
/// stops, and the `sleep` the fake `op` was running comes to the daemon. The
/// new image learns it as an orphan and reaps it, while its own `op` runs.
#[test]
fn after_a_restart_in_place_the_daemon_adopts_and_reaps_what_the_old_image_left() {
    let r = Rig::new();
    let op = r.path("bin").join("op");
    std::fs::write(
        &op,
        format!(
            "#!/bin/sh\n\
             case \"$1\" in\n\
             \x20 read) cat '{}' ;;\n\
             \x20 inject) sleep 2; sed -e 's/{{{{ [^}}]* }}}}/test-secret-value-0000/g' ;;\n\
             \x20 *) exit 1 ;;\n\
             esac\n",
            r.path("note.toml").display()
        ),
    )
    .unwrap();
    let changed = test_note(&r, 42.5);
    std::fs::write(r.path("note.toml"), &changed).unwrap();
    r.keep_copy(&test_note(&r, 100.0));
    let (mut daemon, _, _) = r.start();
    let pid = daemon.id();
    let h = r.until(&mut daemon, "the restart", |h| {
        !h["config"]["restarted"].is_null()
    });
    assert!(daemon.try_wait().is_none(), "the same process: pid {pid}");
    assert_eq!(h["children"]["subreaper"], true, "{}", h["children"]);
    let h = r.until(&mut daemon, "what the old image left, reaped", |h| {
        let c = &h["children"];
        c["reaped_orphans"].as_u64() >= Some(1) && c["orphans"] == 0 && c["zombies"] == 0
    });
    assert_eq!(h["children"]["subreaper"], true);
    let log = std::fs::read_to_string(r.path("theseusd.log")).unwrap();
    assert!(
        log.contains("children kept across the restart"),
        "the new image learned its children:\n{}",
        r.log()
    );
    // The new image's own `op` got its status: the secrets resolved by it.
    let h = r.until(&mut daemon, "the secrets", |h| {
        h["secrets"]["state"] == "ready"
    });
    assert_eq!(h["secrets"]["method"], "inject", "{}", h["secrets"]);
    assert_eq!(h["children"]["zombies"], 0);
    r.stop(daemon);
}

impl Rig {
    /// A daemon on a copy of the note at a $100 limit, the vault's note at
    /// $42.5: once the vault answers, it restarts in place. `stdio` keeps
    /// the copy's digest in the `--stdio` daemon's store.
    fn restart_ahead(&self, stdio: bool) {
        std::fs::write(self.path("note.toml"), test_note(self, 42.5)).unwrap();
        let store = if stdio { "store-stdio" } else { "store" };
        self.keep_copy_in(store, &test_note(self, 100.0));
    }

    /// A `--stdio` daemon, `env` set, and its client; not yet asked
    /// anything.
    fn spawn_stdio(&self, env: &[(&str, &str)]) -> (Daemon, StdioClient) {
        let log = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(self.path("theseusd.log"))
            .unwrap();
        let mut d = Daemon::spawn(
            self.command()
                .arg("--stdio")
                .envs(env.iter().copied())
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(log),
        );
        let c = StdioClient::new(&mut d);
        (d, c)
    }

    /// Until `settled` says so of `daemon`, asked through `ask`, at most
    /// 40 s.
    fn until_settled(
        &self,
        daemon: &mut Daemon,
        ask: &mut dyn FnMut(&str, Value) -> Option<Value>,
    ) {
        let t0 = Instant::now();
        while !settled(ask) {
            if let Some(status) = daemon.try_wait() {
                panic!(
                    "theseusd exited ({status}) before it settled:\n{}",
                    self.log()
                );
            }
            assert!(
                t0.elapsed() < Duration::from_secs(40),
                "not settled in 40 s:\n{}",
                self.log()
            );
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    /// Until the daemon's whole log holds `what` `n` times, at most 40 s.
    fn logged(&self, what: &str, n: usize) {
        let t0 = Instant::now();
        loop {
            let s = std::fs::read_to_string(self.path("theseusd.log")).unwrap_or_default();
            if s.matches(what).count() >= n {
                return;
            }
            assert!(
                t0.elapsed() < Duration::from_secs(40),
                "no {what:?} ×{n} in 40 s:\n{}",
                self.log()
            );
            std::thread::sleep(Duration::from_millis(1));
        }
    }
}

/// A start's `store` phase: what its open replayed and repaired.
fn store_phase(h: &Value) -> Value {
    h["startup"]
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["name"] == "store")
        .map(|p| p["detail"].clone())
        .unwrap()
}

/// Whether the daemon's stdout relay (its thread `stdio-out`) is blocked in a
/// write to stdout, holding what it has not written: its syscall, as
/// `/proc` says.
fn relay_blocked_on_stdout(pid: u32) -> bool {
    let Ok(tasks) = std::fs::read_dir(format!("/proc/{pid}/task")) else {
        return false;
    };
    tasks.flatten().any(|t| {
        let read = |f: &str| std::fs::read_to_string(t.path().join(f)).unwrap_or_default();
        let call = read("syscall");
        let mut words = call.split_whitespace();
        read("comm").trim_end() == "stdio-out"
            && words.next() == Some(&libc::SYS_write.to_string())
            && words.next() == Some("0x1")
    })
}

/// A restart in place ends as a stop does (theseus-jo7f): the old image's
/// runtime is dropped, which waits for its tasks, so the store closes before
/// the exec. A task on the blocking pool that holds the core 1.5 s past the
/// stop's start (the debug build's plant, as a slow warm build can on a
/// loaded machine) kept the store open into the exec under the runtime's
/// 500 ms shutdown bound: redb never closed, the stop's last checkpoint was
/// lost, and the next image replayed the run. A record written after the
/// stop's last checkpoint (the second plant, as a writer the stop never
/// waited for writes one) was replayed by the next image too; the store's
/// close now checkpoints it (theseus-fts6). Now the next image's open
/// replays nothing and repairs nothing, on the socket and on `--stdio`. The
/// vault's note is held until the daemon has settled, so the restart never
/// meets the start's own work, as it did on a loaded machine, 48 ms after
/// serving.
#[test]
fn a_restart_in_place_closes_the_store_before_the_exec() {
    const HOLD: (&str, &str) = ("THESEUS_TEST_HOLD_CORE_MS", "1500");
    const LATE: (&str, &str) = ("THESEUS_TEST_LATE_WRITE", "1");
    let r = Rig::new();
    r.restart_ahead(false);
    std::fs::write(r.path("hold-read"), "").unwrap();
    let mut d = Daemon::spawn(
        r.command().envs([HOLD, LATE]).stdout(Stdio::null()).stderr(
            std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(r.path("theseusd.log"))
                .unwrap(),
        ),
    );
    r.until_settled(&mut d, &mut |m, p| r.call(m, p).ok());
    std::fs::remove_file(r.path("hold-read")).unwrap();
    let h = r.until(&mut d, "the restart", |h| {
        !h["config"]["restarted"].is_null()
    });
    let s = store_phase(&h);
    assert_eq!(
        (&s["replayed_into_index"], &s["index_repaired"]),
        (&json!(0), &json!(false)),
        "the socket daemon's exec left its store open: {s}\n{}",
        r.log()
    );
    r.stop(d);

    let r = Rig::new();
    r.restart_ahead(true);
    std::fs::write(r.path("hold-read"), "").unwrap();
    let (mut d, mut c) = r.spawn_stdio(&[HOLD, LATE]);
    r.until_settled(&mut d, &mut |m, p| c.call(m, p).ok());
    std::fs::remove_file(r.path("hold-read")).unwrap();
    r.logged("serving protocol on stdio", 2);
    let h = c.call("health", Value::Null).unwrap();
    assert!(!h["config"]["restarted"].is_null(), "{}", h["config"]);
    let s = store_phase(&h);
    assert_eq!(
        (&s["replayed_into_index"], &s["index_repaired"]),
        (&json!(0), &json!(false)),
        "the stdio daemon's exec left its store open: {s}\n{}",
        r.log()
    );
    assert_eq!(c.call("shutdown", Value::Null), Ok(json!({"ok": true})));
    let t0 = Instant::now();
    while d.try_wait().is_none() {
        assert!(t0.elapsed() < Duration::from_secs(20), "{}", r.log());
        std::thread::sleep(Duration::from_millis(5));
    }
}

/// A `--stdio` daemon's restart in place leaves every line whole
/// (theseus-jo7f): what the core wrote before the exec reaches stdout
/// before it, as at a stop (`stdio::flush`). The client holds its reads, so
/// over 64 KB of answers back up in the pipe and the relay is blocked
/// mid-copy as the old image ends; it reads again 100 ms after the old
/// image's runtime is dropped, well inside the flush's 500 ms, and long
/// after an exec with no flush would have come. An exec with no
/// flush ended the relay mid-copy: the pipe's last line was cut, and glued
/// to the new image's first. A request sent once the new image serves is
/// answered; one sent during the restart may be read by the old image's
/// stdin thread, and dies with it, unanswered, as a socket client's
/// request in flight at a restart does.
#[test]
fn a_stdio_restart_in_place_keeps_every_line_whole() {
    let r = Rig::new();
    r.restart_ahead(true);
    // The vault is held until the backlog is in place.
    std::fs::write(r.path("hold"), "").unwrap();
    let (mut d, mut c) = r.spawn_stdio(&[("THESEUS_LOG", "info,theseus_core::startup=debug")]);
    let one = c.call("health", Value::Null).unwrap().to_string().len();
    c.hold();
    // About 150 KB of answers: past the pipe's 64 KB, so the relay blocks
    // holding the rest, and short of what the pipe, the relay's buffer and
    // the socket pair hold together, so the core never blocks mid-line.
    let n = 150_000 / (one + 64);
    let ids: Vec<u64> = (0..n).map(|_| c.send("health", Value::Null)).collect();
    let t0 = Instant::now();
    while !relay_blocked_on_stdout(d.id()) {
        assert!(
            t0.elapsed() < Duration::from_secs(40),
            "the relay never blocked: {} bytes in the pipe\n{}",
            c.pending(),
            r.log()
        );
        std::thread::sleep(Duration::from_millis(5));
    }
    std::fs::remove_file(r.path("hold")).unwrap();
    r.logged("runtime dropped", 1);
    // Long after an exec with no flush would have come, and well inside the
    // flush's bound.
    std::thread::sleep(Duration::from_millis(100));
    c.release();
    r.logged("serving protocol on stdio", 2);
    let after = c.send("health", Value::Null);
    let mut answered = Vec::new();
    let h = loop {
        let l = match c.line(Duration::from_secs(60)) {
            Some(Ok(l)) => l,
            other => panic!("no answer to {after}: {other:?}\n{}", r.log()),
        };
        let v: Value = serde_json::from_str(&l)
            .unwrap_or_else(|e| panic!("a line cut at the restart ({e}): {l:?}\n{}", r.log()));
        assert!(l.ends_with('\n'), "{l:?}");
        let id = v["id"].as_u64().unwrap_or(0);
        if id == after {
            break v["result"].clone();
        }
        if id != 0 {
            answered.push(id);
        }
    };
    assert!(!h["config"]["restarted"].is_null(), "{}", h["config"]);
    assert!(
        answered.len() * (one + 64) > 65_536,
        "the backlog came through: {} of {n} answers",
        answered.len()
    );
    let mut sorted = answered.clone();
    sorted.dedup();
    assert_eq!(sorted.len(), answered.len(), "each once: {answered:?}");
    assert!(answered.iter().all(|i| ids.contains(i)), "{answered:?}");
    assert_eq!(c.call("shutdown", Value::Null), Ok(json!({"ok": true})));
    let t0 = Instant::now();
    while d.try_wait().is_none() {
        assert!(t0.elapsed() < Duration::from_secs(20), "{}", r.log());
        std::thread::sleep(Duration::from_millis(5));
    }
}
