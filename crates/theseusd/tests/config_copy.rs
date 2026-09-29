//! The config copy with the real `theseusd` (theseus-2fo). A first start
//! reads the vault's note and keeps the copy; a later start serves from the
//! copy while the vault is read; a changed note restarts the daemon onto the
//! vault's version through `exec`, in the same process; a comment-only change
//! confirms; a vault that does not answer holds. The vault is a fake `op`,
//! first on the daemon's PATH, that answers after `OP_MS`.

use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

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

    fn spawn(&self) -> (Child, Instant) {
        let log = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(self.path("theseusd.log"))
            .unwrap();
        let t0 = Instant::now();
        let child = self
            .command()
            .stdout(Stdio::null())
            .stderr(log)
            .spawn()
            .unwrap();
        (child, t0)
    }

    /// Spawn to the first `health` answer.
    fn start(&self) -> (Child, Value, Duration) {
        let (mut child, t0) = self.spawn();
        let h = self.until(&mut child, "a first answer", |_| true);
        (child, h, t0.elapsed())
    }

    /// Ask `health` until `ok` says so, at most 15 s; a restart in between
    /// closes the socket, and the next ask finds the new one.
    fn until(&self, child: &mut Child, what: &str, ok: impl Fn(&Value) -> bool) -> Value {
        let deadline = Instant::now() + Duration::from_secs(15);
        loop {
            if let Ok(h) = self.call("health", Value::Null) {
                if ok(&h) {
                    return h;
                }
            }
            if let Some(status) = child.try_wait().unwrap() {
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

    fn stop(&self, mut child: Child) {
        let _ = self.call("shutdown", Value::Null);
        let deadline = Instant::now() + Duration::from_secs(10);
        while child.try_wait().unwrap().is_none() {
            if Instant::now() > deadline {
                let _ = child.kill();
                panic!("theseusd did not stop:\n{}", self.log());
            }
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

/// The template, made safe to serve here: the web UI and Discord off, no
/// GitHub token, every secret on the fake vault, and the model endpoints on
/// a port nothing answers.
fn test_note(r: &Rig, spend_limit_usd: f64) -> String {
    let out = Command::new(&r.theseusd)
        .arg("example-config")
        .output()
        .unwrap();
    let mut t: toml::Table = String::from_utf8(out.stdout).unwrap().parse().unwrap();
    fn table<'a>(t: &'a mut toml::Table, key: &str) -> &'a mut toml::Table {
        t.entry(key)
            .or_insert_with(|| toml::Value::Table(Default::default()))
            .as_table_mut()
            .unwrap()
    }
    table(&mut t, "model").insert("api_base".into(), "http://127.0.0.1:9".into());
    for (_, p) in table(&mut t, "providers").iter_mut() {
        p.as_table_mut()
            .unwrap()
            .insert("api_base".into(), "http://127.0.0.1:9".into());
    }
    let secrets = table(&mut t, "secrets");
    let names: Vec<String> = secrets
        .keys()
        .filter(|k| k.as_str() != "github_token")
        .cloned()
        .collect();
    secrets.clear();
    for n in names {
        secrets.insert(n.clone(), format!("op://Test/{n}/credential").into());
    }
    table(&mut t, "discord").insert("enabled".into(), false.into());
    table(&mut t, "web").insert("enabled".into(), false.into());
    table(&mut t, "tools").insert(
        "projects_dir".into(),
        r.path("projects").display().to_string().into(),
    );
    table(&mut t, "kernel").insert("spend_limit_usd".into(), spend_limit_usd.into());
    toml::to_string(&t).unwrap()
}

#[test]
fn the_copy_serves_the_next_start_and_a_changed_note_restarts_the_daemon_in_place() {
    let r = Rig::new();
    let note = test_note(&r, 100.0);
    std::fs::write(r.path("note.toml"), &note).unwrap();
    let copy = config_copy::path(Some(&r.path("state")));

    // A first start: no copy, so it reads the vault before serving, and it
    // keeps the copy once serving.
    let (mut child, h, took) = r.start();
    assert_eq!(h["config"]["started_from"], "vault", "{}", h["config"]);
    assert_eq!(h["config"]["state"], "confirmed");
    assert!(
        took >= Duration::from_millis(OP_MS),
        "it read the vault first: {took:?}"
    );
    r.until(&mut child, "the copy kept", |h| {
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
    r.stop(child);

    // From the copy: health answers before the vault could, says
    // confirming, and then confirmed.
    let (mut child, h, took) = r.start();
    assert_eq!(h["config"]["state"], "confirming", "{}", h["config"]);
    assert_eq!(h["config"]["started_from"], "copy");
    assert!(
        took < Duration::from_millis(OP_MS),
        "served from the copy: {took:?}"
    );
    let h = r.until(&mut child, "confirmed", |h| {
        h["config"]["state"] == "confirmed"
    });
    assert_eq!(h["config"]["detail"], "the same text as the copy");
    let phases = h["startup"].as_array().unwrap();
    assert!(phases
        .iter()
        .any(|p| p["name"] == "config.vault" && p["background"] == true));
    r.stop(child);
    let shown = r.command().arg("config").output().unwrap();
    let shown = String::from_utf8(shown.stdout).unwrap();
    assert!(
        shown.contains(&format!(
            "# copy: {} matches the vault's note",
            copy.display()
        )),
        "{shown}"
    );

    // The note changes: the daemon serves from the old copy, finds the
    // change, and restarts itself onto the vault's version, in place.
    let changed = test_note(&r, 42.5);
    std::fs::write(r.path("note.toml"), &changed).unwrap();
    let (mut child, h, _) = r.start();
    assert_eq!(h["config"]["state"], "confirming");
    let pid = child.id();
    let h = r.until(&mut child, "the restart confirmed", |h| {
        h["config"]["state"] == "confirmed" && !h["config"]["restarted"].is_null()
    });
    assert!(
        child.try_wait().unwrap().is_none(),
        "the same process: pid {pid}"
    );
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
    r.stop(child);

    // A comment-only change confirms, with no restart.
    let commented = format!("# pasted again\n{changed}");
    std::fs::write(r.path("note.toml"), &commented).unwrap();
    let (mut child, _, _) = r.start();
    let h = r.until(&mut child, "confirmed", |h| {
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
    r.stop(child);

    // A vault that does not answer: held, answering reads, and saying why;
    // and shutdown works.
    std::fs::write(r.path("down"), "").unwrap();
    let (mut child, _, _) = r.start();
    let h = r.until(&mut child, "held", |h| h["config"]["state"] == "held");
    let why = h["config"]["detail"].as_str().unwrap();
    assert!(why.starts_with("the vault did not answer: "), "{why}");
    assert!(why.contains("network down"), "{why}");
    r.stop(child);
}
