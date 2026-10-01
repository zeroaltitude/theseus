//! A failure that will not pass is not retried forever (theseus-ljr), with the
//! real `theseusd` and its driver, and a stand-in for the Messages API that
//! refuses with the status a test gives it:
//! - a 400 on every call: the input turn, one retry by the driver, and then
//!   nothing, however long the daemon runs;
//! - a 529 twice, then an answer: the driver's retries keep their backoff,
//!   and the turn answers once the stand-in recovers.

mod common;

use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::Stdio;
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

struct Rig {
    dir: tempfile::TempDir,
    model: FakeModel,
}

impl Rig {
    fn new() -> Self {
        let model = FakeModel::start(|_| vec![]);
        let dir = tempfile::tempdir().unwrap();
        let path = |p: &str| dir.path().join(p);
        let theseusd = PathBuf::from(env!("CARGO_BIN_EXE_theseusd"));
        for d in ["bin", "projects", "state"] {
            std::fs::create_dir_all(path(d)).unwrap();
        }
        use std::os::unix::fs::PermissionsExt;
        std::fs::write(path("bin/op"), FAKE_OP).unwrap();
        std::fs::set_permissions(path("bin/op"), std::fs::Permissions::from_mode(0o755)).unwrap();
        let mut t: toml::Table = common::safe_note(&theseusd, &path("projects"), 100.0)
            .parse()
            .unwrap();
        fn table<'a>(t: &'a mut toml::Table, key: &str) -> &'a mut toml::Table {
            t.entry(key)
                .or_insert_with(|| toml::Value::Table(Default::default()))
                .as_table_mut()
                .unwrap()
        }
        table(&mut t, "model").insert("api_base".into(), model.base.clone().into());
        for (_, p) in table(&mut t, "providers").iter_mut() {
            p.as_table_mut()
                .unwrap()
                .insert("api_base".into(), model.base.clone().into());
        }
        std::fs::write(path("config.toml"), toml::to_string(&t).unwrap()).unwrap();
        Self { dir, model }
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
            std::process::Command::new(env!("CARGO_BIN_EXE_theseusd"))
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
                t0.elapsed() < Duration::from_secs(40),
                "no {what} in 40 s; the daemon's log ends:\n{}",
                tail(&self.log(), 30)
            );
            std::thread::sleep(Duration::from_millis(20));
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

    /// The `turn.next` rows' `then` and `notice`, oldest first.
    fn nexts(&self) -> Vec<(String, bool)> {
        self.rows("turn.next")
            .iter()
            .map(|r| {
                (
                    r["data"]["then"].as_str().unwrap_or("").to_string(),
                    r["data"]["notice"].as_bool().unwrap_or(false),
                )
            })
            .collect()
    }

    fn execution_state(&self) -> String {
        let execs = self.call("execution.list", json!({})).unwrap();
        let list = execs["executions"].as_array().cloned().unwrap_or_default();
        assert_eq!(list.len(), 1, "{execs}");
        list[0]["state"].as_str().unwrap_or("").to_string()
    }
}

fn call(sock: &Path, method: &str, params: Value) -> Result<Value, Value> {
    let s = UnixStream::connect(sock).map_err(|e| json!(e.to_string()))?;
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

fn ask(sock: &Path, prompt: &str) -> Result<Value, Value> {
    call(
        sock,
        "turn.submit",
        json!({"input": prompt, "author": "test", "attachments": []}),
    )
}

fn tail(s: &str, n: usize) -> String {
    let lines: Vec<&str> = s.lines().collect();
    lines[lines.len().saturating_sub(n)..].join("\n")
}

/// A 400 on every call: the input turn fails, the driver retries it once,
/// and then the execution waits on input. Four seconds later (the old
/// driver's next two retries would have come at 2 s and 4 s) nothing more
/// has been asked.
#[test]
fn a_400_every_time_is_retried_once_then_never_again() {
    let r = Rig::new();
    let _daemon = r.spawn();
    r.model.fail_always(400);
    let err = ask(&r.path("sock"), "chart the shoals").expect_err("the stand-in refuses it");
    assert_eq!(err["data"]["class"], "invalid_request", "{err}");
    r.wait("the retry's park", || {
        (r.rows("turn.failed").len() == 2 && r.nexts().len() == 2).then_some(())
    });
    assert_eq!(
        r.nexts(),
        [("retry".to_string(), false), ("park".to_string(), true)]
    );
    assert_eq!(r.execution_state(), "waiting");
    std::thread::sleep(Duration::from_secs(4));
    assert_eq!(r.model.requests().len(), 2, "no third call");
    assert_eq!(r.rows("turn.failed").len(), 2, "no third turn");
    assert_eq!(r.execution_state(), "waiting");
}

/// A 529 twice, then an answer. The input turn fails, the driver's first
/// retry fails at once, and its second waits out the backoff (2 s) before it
/// answers. Only the run's first failure would post a notice.
#[test]
fn a_529_keeps_the_backoff_and_answers_once_the_stand_in_recovers() {
    let r = Rig::new();
    let _daemon = r.spawn();
    r.model.fail_next(&[529, 529]);
    let err = ask(&r.path("sock"), "chart the shoals").expect_err("the first call is refused");
    assert_eq!(err["data"]["class"], "overloaded", "{err}");
    r.wait("the answer", || {
        r.rows("turn.ended")
            .iter()
            .any(|t| t["data"]["continuation"] == true)
            .then_some(())
    });
    assert_eq!(r.model.requests().len(), 3);
    let at = r.model.arrivals();
    let backoff = at[2] - at[1];
    assert!(
        backoff >= Duration::from_millis(1_900),
        "the second retry waited only {backoff:?}"
    );
    assert_eq!(
        r.nexts(),
        [
            ("backoff".to_string(), true),
            ("backoff".to_string(), false)
        ]
    );
    assert_eq!(r.execution_state(), "waiting");
}
