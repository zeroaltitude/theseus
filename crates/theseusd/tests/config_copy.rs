//! The config copy with the real `theseusd` (theseus-2fo). A first start
//! reads the vault's note and keeps the copy; a later start serves from the
//! copy while the vault is read; a changed note restarts the daemon onto the
//! vault's version through `exec`, in the same process; a comment-only change
//! confirms; a vault that does not answer holds. The vault is a fake `op`,
//! first on the daemon's PATH, that answers after `OP_MS`. Each daemon is
//! held by a guard that kills and reaps it, so an assertion that fails
//! before its stop leaves none running.

mod common;

use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use common::Daemon;
use serde_json::{json, Value};
use theseus_core::config_copy;

const NOTE_REF: &str = "op://Test/theseus-config/notesPlain";
/// How long the fake vault takes to answer. A first answer that says
/// `confirming` came before it could have.
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
        std::fs::write(&op, fake_op(&r.path("note.toml"), &r.path("down"))).unwrap();
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
/// gives every secret one value, each after `OP_MS`. While `down` exists it
/// answers nothing.
fn fake_op(note: &Path, down: &Path) -> String {
    format!(
        "#!/bin/sh\n\
         sleep {}\n\
         if [ -e '{}' ]; then echo '[ERROR] 2026/09/29 12:00:00 network down' >&2; exit 1; fi\n\
         case \"$1\" in\n\
         \x20 read) cat '{}' ;;\n\
         \x20 inject) sed -e 's/{{{{ [^}}]* }}}}/test-secret-value-0000/g' ;;\n\
         \x20 *) exit 1 ;;\n\
         esac\n",
        OP_MS as f64 / 1000.0,
        down.display(),
        note.display()
    )
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

    // From the copy: health answers before the vault could, says
    // confirming, and then confirmed.
    let (mut daemon, h, took) = r.start();
    assert_eq!(h["config"]["state"], "confirming", "{}", h["config"]);
    assert_eq!(h["config"]["started_from"], "copy");
    assert!(
        took < Duration::from_millis(OP_MS),
        "served from the copy: {took:?}"
    );
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

    // A store that still holds a unit budget (from before theseus-0sg) is
    // migrated only under the vault's own config: the start from the copy
    // refuses it before any write, and execs itself, in place, to read the
    // vault first.
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
    let pid = daemon.id();
    assert_eq!(h["config"]["started_from"], "vault", "{}", h["config"]);
    assert!(
        h["config"]["detail"]
            .as_str()
            .is_some_and(|d| d.starts_with("the store still holds unit budgets")),
        "{}",
        h["config"]
    );
    assert_eq!(
        r.ledger("budget.migrated").len(),
        1,
        "migrated under the vault's config"
    );
    assert!(daemon.try_wait().is_none(), "the same process: pid {pid}");
    r.until(&mut daemon, "the copy kept", |h| {
        h["config"]["detail"]
            .as_str()
            .is_some_and(|d| d.contains("the copy is kept"))
    });
    r.stop(daemon);

    // The note changes: the daemon serves from the old copy, finds the
    // change, and restarts itself onto the vault's version, in place.
    let changed = test_note(&r, 42.5);
    std::fs::write(r.path("note.toml"), &changed).unwrap();
    let (mut daemon, h, _) = r.start();
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

    // A vault that does not answer: held, answering reads, and saying why;
    // and shutdown works.
    std::fs::write(r.path("down"), "").unwrap();
    let (mut daemon, _, _) = r.start();
    let h = r.until(&mut daemon, "held", |h| h["config"]["state"] == "held");
    let why = h["config"]["detail"].as_str().unwrap();
    assert!(why.starts_with("the vault did not answer: "), "{why}");
    assert!(why.contains("network down"), "{why}");
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
    config_copy::write(
        &config_copy::path(Some(&r.path("state"))),
        NOTE_REF,
        &test_note(&r, 100.0),
    )
    .unwrap();
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
