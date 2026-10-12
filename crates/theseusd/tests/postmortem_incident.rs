//! The incident `theseus postmortem` is proved on (theseus-m2tiu), scripted on
//! a real `theseusd` with a stand-in for the Messages API, and its report
//! asked of the real `theseus` beside it:
//! - a task that runs a long command (33 s, over the report's 30 s line);
//! - a tool that fails (a read of a file that is not there);
//! - two questions to the owner: one answered from the CLI, one left to
//!   expire;
//! - a `kill -9` of the daemon in the middle of turns, and a restart that
//!   resumes them;
//! - a wake that comes due while the daemon is down, and runs late.
//!
//! It takes about a minute, so it runs when `THESEUS_RECORD_POSTMORTEM` names
//! a directory: it writes the incident's ledger rows there, scrubbed, as
//! `incident.jsonl`, the fixture `crates/theseus/tests/fixtures/postmortem/`
//! holds (`tests/postmortem.rs` renders it). Without it the test says so and
//! passes, as `tests/lsp.rs` does where there is no server to run.
//!
//! To record another incident from a real daemon:
//!   `theseus postmortem --since 6h --rows-out incident.jsonl`
//! keeps the rows the report was made from, already scrubbed of secrets by
//! the daemon; this test's `fixture` function is the rest of the step (paths
//! made invented, no name left). Read the file before it is committed.

mod common;

use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use common::model::FakeModel;
use common::Daemon;
use serde_json::{json, Value};

/// A fake `op`: every reference's value is `tv-<its item>`.
const FAKE_OP: &str = "#!/bin/sh\n\
    case \"$1\" in\n\
    \x20 inject) sed -E 's#\\{\\{ op://Test/([^/]+)/[^}]* \\}\\}#tv-\\1#g' ;;\n\
    \x20 read) for a; do ref=\"$a\"; done; printf 'tv-%s' \"$(echo \"$ref\" | cut -d/ -f4)\" ;;\n\
    \x20 *) exit 1 ;;\n\
    esac\n";

type Script = Arc<Mutex<Vec<(String, Vec<(&'static str, Value)>)>>>;

struct Rig {
    dir: tempfile::TempDir,
    _model: FakeModel,
}

fn table<'a>(t: &'a mut toml::Table, key: &str) -> &'a mut toml::Table {
    t.entry(key)
        .or_insert_with(|| toml::Value::Table(Default::default()))
        .as_table_mut()
        .unwrap()
}

impl Rig {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let path = |p: &str| dir.path().join(p);
        let notes = path("projects/notes.txt").display().to_string();
        let missing = path("projects/missing.txt").display().to_string();
        // The first phrase a prompt holds gives its calls; the task's own
        // prompt quotes its parent's, so its phrase comes first.
        let script: Script = Arc::new(Mutex::new(vec![
            (
                "Run the long job".into(),
                vec![(
                    "proc_run",
                    json!({"argv": ["sh", "-c", "touch long-started; sleep 33; echo long job done"], "timeout_secs": 120}),
                )],
            ),
            (
                "Start the long job as a task".into(),
                vec![(
                    "task_create",
                    json!({"brief": "Run the long job and report.", "arrangement":
                        {"pieces": [{"quote": "Start the long job as a task", "role": "objective"}]}}),
                )],
            ),
            (
                "Run the slow step".into(),
                vec![(
                    "proc_run",
                    json!({"argv": ["sh", "-c", "touch slow-started; sleep 22; echo slow step done"], "timeout_secs": 120}),
                )],
            ),
            (
                "Read the missing file".into(),
                vec![("fs_read", json!({"path": missing}))],
            ),
            (
                "Look at the state once".into(),
                vec![("fs_write", json!({"path": notes, "content": "first\n"}))],
            ),
            (
                "Look at the state twice".into(),
                vec![("fs_write", json!({"path": notes, "content": "second\n"}))],
            ),
            (
                "Wake me after a while".into(),
                vec![(
                    "wake_at",
                    json!({"after": "10s", "note": "check the long job"}),
                )],
            ),
        ]));
        let model = FakeModel::start(move |prompt| {
            script
                .lock()
                .unwrap()
                .iter()
                .find(|(p, _)| prompt.contains(p.as_str()))
                .map(|(_, calls)| calls.clone())
                .unwrap_or_default()
        });
        let theseusd = PathBuf::from(env!("CARGO_BIN_EXE_theseusd"));
        for d in ["bin", "projects", "state", "xdg-state"] {
            std::fs::create_dir_all(path(d)).unwrap();
        }
        use std::os::unix::fs::PermissionsExt;
        std::fs::write(path("bin/op"), FAKE_OP).unwrap();
        std::fs::set_permissions(path("bin/op"), std::fs::Permissions::from_mode(0o755)).unwrap();
        let mut t: toml::Table = common::safe_note(&theseusd, &path("projects"), 100.0)
            .parse()
            .unwrap();
        table(&mut t, "model").insert("api_base".into(), model.base.clone().into());
        for (_, p) in table(&mut t, "providers").iter_mut() {
            p.as_table_mut()
                .unwrap()
                .insert("api_base".into(), model.base.clone().into());
        }
        table(&mut t, "policy").insert("enforcement".into(), "notify".into());
        table(&mut t, "policy")
            .entry("tools")
            .or_insert_with(|| toml::Value::Table(Default::default()))
            .as_table_mut()
            .unwrap()
            .insert("fs.write".into(), "approve".into());
        table(&mut t, "tools").insert("proc_sync_secs".into(), 60.into());
        // A question left alone expires within the incident.
        table(&mut t, "kernel").insert("confirm_ttl_secs".into(), 25.into());
        std::fs::write(path("config.toml"), toml::to_string(&t).unwrap()).unwrap();
        Self { dir, _model: model }
    }

    fn path(&self, p: &str) -> PathBuf {
        self.dir.path().join(p)
    }

    fn spawn(&self) -> Daemon {
        let log = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(self.path("theseusd.log"))
            .unwrap();
        let d = Daemon::spawn(
            Command::new(env!("CARGO_BIN_EXE_theseusd"))
                .arg("--config")
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
                .stderr(log),
        );
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
                t0.elapsed() < Duration::from_secs(90),
                "no {what} in 90 s; the daemon's log ends:\n{}",
                tail(&self.log(), 30)
            );
            std::thread::sleep(Duration::from_millis(50));
        }
    }

    fn call(&self, method: &str, params: Value) -> Result<Value, Value> {
        call(&self.path("sock"), method, params)
    }

    fn rows(&self, kind: &str) -> Vec<Value> {
        self.call("ledger.tail", json!({"n": 500, "kind": kind}))
            .unwrap()["rows"]
            .as_array()
            .cloned()
            .unwrap_or_default()
    }

    /// The operator's own `theseus`, from outside every job.
    fn cli(&self, args: &[&str]) -> std::process::Output {
        let theseusd = PathBuf::from(env!("CARGO_BIN_EXE_theseusd"));
        Command::new(theseusd.with_file_name("theseus"))
            .arg("--socket")
            .arg(self.path("sock"))
            .args(args)
            .env("XDG_STATE_HOME", self.path("xdg-state"))
            .env("TZ", "UTC")
            .env_remove("THESEUS_SESSION")
            .stdin(Stdio::null())
            .output()
            .unwrap()
    }
}

fn call(sock: &Path, method: &str, params: Value) -> Result<Value, Value> {
    let s = UnixStream::connect(sock).map_err(|e| json!(e.to_string()))?;
    s.set_read_timeout(Some(Duration::from_secs(120))).unwrap();
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

fn tail(s: &str, n: usize) -> String {
    let lines: Vec<&str> = s.lines().collect();
    lines[lines.len().saturating_sub(n)..].join("\n")
}

/// A turn on a new session, asked in a thread of its own: it returns when the
/// turn does, or with an error when the daemon is killed under it.
fn ask(sock: PathBuf, prompt: &'static str) -> std::thread::JoinHandle<Result<Value, Value>> {
    std::thread::spawn(move || {
        call(
            &sock,
            "turn.submit",
            json!({"input": prompt, "author": "owner", "attachments": []}),
        )
    })
}

#[test]
fn an_incident_is_recorded_and_its_report_names_every_fact() {
    let Some(out) = std::env::var_os("THESEUS_RECORD_POSTMORTEM") else {
        eprintln!("skipped: THESEUS_RECORD_POSTMORTEM names the directory the fixture goes to");
        return;
    };
    let r = Rig::new();
    let sock = r.path("sock");
    let first = r.spawn();

    // Quiet turns, a failing tool, and the two questions.
    for p in ["Say hello", "Say hello again", "Say hello once more"] {
        ask(sock.clone(), p).join().unwrap().unwrap();
    }
    ask(sock.clone(), "Read the missing file")
        .join()
        .unwrap()
        .unwrap();
    let once = ask(sock.clone(), "Look at the state once")
        .join()
        .unwrap()
        .unwrap();
    assert!(once["awaiting_confirm"].is_string(), "{once}");
    ask(sock.clone(), "Look at the state twice")
        .join()
        .unwrap()
        .unwrap();
    let questions = r.wait("two questions", || {
        let q = r.call("confirm.list", Value::Null).ok()?;
        let q = q["confirms"].as_array()?.clone();
        (q.len() == 2).then_some(q)
    });
    let first_q = questions[0]["correlation_id"].as_str().unwrap().to_string();
    // The owner answers the first from their own shell, a while after it was asked.
    std::thread::sleep(Duration::from_secs(4));
    let said = r.cli(&["confirm", &first_q, "--approve", "--no-wait"]);
    assert!(
        said.status.success(),
        "{}",
        String::from_utf8_lossy(&said.stderr)
    );

    // The task with the long command, a turn with a slow step, and a wake due soon.
    let task = ask(sock.clone(), "Start the long job as a task");
    let slow = ask(sock.clone(), "Run the slow step");
    r.wait("the task's job", || {
        r.path("projects/long-started").exists().then_some(())
    });
    r.wait("the slow step", || {
        r.path("projects/slow-started").exists().then_some(())
    });
    ask(sock, "Wake me after a while").join().unwrap().unwrap();
    task.join().unwrap().unwrap();
    std::thread::sleep(Duration::from_secs(2));

    // The daemon dies in the middle of turns, and stays down while the wake comes due.
    drop(first);
    assert!(
        slow.join().unwrap().is_err(),
        "the connection closes with the daemon"
    );
    std::thread::sleep(Duration::from_secs(12));
    let _second = r.spawn();
    r.wait("the wake to run", || {
        (!r.rows("wake.fired").is_empty()).then_some(())
    });
    r.wait("the task to end", || {
        (!r.rows("task.ended").is_empty()).then_some(())
    });
    r.wait("the question to expire", || {
        (!r.rows("action.expired").is_empty()).then_some(())
    });
    r.wait("the turns to end", || {
        let ended = r.rows("turn.ended").len() + r.rows("turn.failed").len();
        let started = r.rows("turn.started").len();
        let cut = r.rows("execution.interrupted").len();
        (started > 0 && ended + cut >= started).then_some(())
    });
    std::thread::sleep(Duration::from_secs(2));

    // The report, from the real CLI.
    let rep = r.cli(&["postmortem", "--since", "30m"]);
    let md = String::from_utf8_lossy(&rep.stdout).to_string();
    assert!(
        rep.status.success(),
        "{}\n{}",
        md,
        String::from_utf8_lossy(&rep.stderr)
    );
    std::fs::create_dir_all(&out).unwrap();
    std::fs::write(Path::new(&out).join("incident.md"), &md).unwrap();
    std::fs::write(Path::new(&out).join("incident.jsonl"), fixture(&r)).unwrap();

    for fact in [
        "## Restarts",
        "ran 3",
        "failed (inproc:fs.read)",
        "approved by",
        "expired unanswered",
        "wake",
        "interrupted",
    ] {
        assert!(md.contains(fact), "the report names `{fact}`:\n{md}");
    }
}

/// The incident's ledger rows as JSON Lines, in order, with the rig's paths
/// made invented ones. No person's name is in them: the stand-in model and
/// the author `owner` are all that spoke.
fn fixture(r: &Rig) -> String {
    let mut rows = Vec::new();
    let mut after = 0u64;
    loop {
        let w = r
            .call(
                "ledger.window",
                json!({"since_ms": 0, "after": after, "limit": 1000}),
            )
            .unwrap();
        rows.extend(w["rows"].as_array().cloned().unwrap_or_default());
        match w["next"].as_u64() {
            Some(n) => after = n,
            None => break,
        }
    }
    let dir = r.dir.path().display().to_string();
    let mut out = String::new();
    for row in rows {
        let line = serde_json::to_string(&row).unwrap();
        out.push_str(&line.replace(&dir, "/srv/theseus"));
        out.push('\n');
    }
    out
}
