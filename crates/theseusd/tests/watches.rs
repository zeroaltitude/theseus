//! Watches with the real `theseusd` and the real `theseus` CLI (theseus-f2r2b):
//! `theseus watch-for add` makes a check that runs with no model until its
//! condition holds. A file that already says MERGED is met by the first look
//! (the driver's tick, within a second or two): the owner is told, the watch
//! is gone, the ledger holds `watch.checked`-free `watch.met`, and no turn or
//! model call happened. A watch the owner ends with `watch-for rm` is not
//! checked again, and a job's shell is refused before anything is sent.
//! (The maintainer's live check, `watch-for add --every 1m` and a file written
//! three minutes later, is this scenario with the interval waited out.)

mod common;

use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use common::Daemon;
use serde_json::{json, Value};

struct Rig {
    dir: tempfile::TempDir,
    _daemon: Daemon,
}

impl Rig {
    fn start() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let path = |p: &str| dir.path().join(p);
        let theseusd = PathBuf::from(env!("CARGO_BIN_EXE_theseusd"));
        std::fs::create_dir_all(path("projects")).unwrap();
        let cli = theseusd.with_file_name("theseus");
        assert!(
            cli.exists(),
            "no {} to run: build the CLI (cargo build -p theseus)",
            cli.display()
        );
        std::fs::copy(&cli, path("projects/theseus")).unwrap();
        std::fs::write(
            path("config.toml"),
            common::safe_note(&theseusd, &path("projects"), 100.0),
        )
        .unwrap();
        let log = std::fs::File::create(path("theseusd.log")).unwrap();
        let daemon = Daemon::spawn(
            Command::new(&theseusd)
                .arg("--config")
                .arg(path("config.toml"))
                .arg("--state-dir")
                .arg(path("state"))
                .arg("--socket")
                .arg(path("projects/sock"))
                .env("OP_SERVICE_ACCOUNT_TOKEN", "test-not-a-token")
                .env_remove("THESEUS_OP_TOKEN_FILE")
                .env_remove("THESEUS_CONFIG")
                .env_remove("THESEUS_STATE_DIR")
                .env_remove("THESEUS_SOCKET")
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(log),
        );
        let r = Self {
            dir,
            _daemon: daemon,
        };
        r.wait("the daemon", || r.call("health", Value::Null).is_ok());
        r
    }

    fn path(&self, p: &str) -> PathBuf {
        self.dir.path().join(p)
    }

    fn wait(&self, what: &str, f: impl Fn() -> bool) {
        let t0 = Instant::now();
        while !f() {
            assert!(
                t0.elapsed() < Duration::from_secs(30),
                "no {what} in 30 s:\n{}",
                self.log()
            );
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    fn call(&self, method: &str, params: Value) -> Result<Value, Value> {
        let s =
            UnixStream::connect(self.path("projects/sock")).map_err(|e| json!(e.to_string()))?;
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

    /// The operator's own `theseus`, or a job's shell when `job` names one.
    fn cli(&self, job: Option<&str>, args: &[&str]) -> (bool, String) {
        let mut c = Command::new(self.path("projects/theseus"));
        c.arg("--socket")
            .arg(self.path("projects/sock"))
            .args(args)
            .env("XDG_STATE_HOME", self.path("xdg-state"))
            .env_remove("THESEUS_SESSION")
            .stdin(Stdio::null());
        if let Some(j) = job {
            c.env("THESEUS_SESSION", j);
        }
        let o = c.output().unwrap();
        (
            o.status.success(),
            format!(
                "{}{}",
                String::from_utf8_lossy(&o.stdout),
                String::from_utf8_lossy(&o.stderr)
            ),
        )
    }

    fn ledger(&self) -> Vec<Value> {
        self.call("ledger.tail", json!({"n": 2000})).unwrap()["rows"]
            .as_array()
            .unwrap()
            .clone()
    }

    fn kind(&self, kind: &str) -> Vec<Value> {
        self.ledger()
            .into_iter()
            .filter(|r| r["kind"] == kind)
            .map(|r| r["data"].clone())
            .collect()
    }

    fn log(&self) -> String {
        let s = std::fs::read_to_string(self.path("theseusd.log")).unwrap_or_default();
        let lines: Vec<&str> = s.lines().collect();
        lines[lines.len().saturating_sub(30)..].join("\n")
    }
}

#[test]
fn a_file_that_says_merged_is_met_by_the_first_look_with_no_model_and_a_job_may_not_set_one() {
    let r = Rig::start();
    let file = r.path("projects/pr-123");
    // A job's shell is refused before anything is sent.
    let (ok, out) = r.cli(
        Some("ses_job"),
        &[
            "watch-for",
            "add",
            "PR 123 merged?",
            "--file",
            file.to_str().unwrap(),
            "--until",
            "equals MERGED",
        ],
    );
    assert!(!ok && out.contains("theseus watch-for add"), "{out}");
    let (_, listed) = r.cli(None, &["watch-for"]);
    assert!(listed.contains("no open watches"), "{listed}");
    // The owner's own: the file is not there, and it is only watched.
    let args = |until: &'static str| {
        vec![
            "watch-for",
            "add",
            "PR 123 merged?",
            "--file",
            file.to_str().unwrap(),
            "--until",
            until,
            "--every",
            "1m",
        ]
    };
    let (ok, out) = r.cli(None, &args("equals MERGED"));
    assert!(ok && out.contains("Watching \"PR 123 merged?\""), "{out}");
    r.wait("its first look", || {
        r.call("watch.list", json!({})).unwrap()["watches"][0]["checks"]
            .as_u64()
            .unwrap_or(0)
            >= 1
    });
    let (_, listed) = r.cli(None, &["watch-for"]);
    assert!(
        listed.contains("PR 123 merged?") && listed.contains("equals MERGED"),
        "{listed}"
    );
    // Ended by the owner: not checked again, and gone from the list.
    let id = r.call("watch.list", json!({})).unwrap()["watches"][0]["short"]
        .as_str()
        .unwrap()
        .to_string();
    let (ok, out) = r.cli(None, &["watch-for", "rm", &id]);
    assert!(ok, "{out}");
    let (_, listed) = r.cli(None, &["watch-for"]);
    assert!(listed.contains("no open watches"), "{listed}");
    // A file that already says MERGED: the first look meets it.
    std::fs::write(&file, "MERGED\n").unwrap();
    let (ok, out) = r.cli(None, &args("equals MERGED"));
    assert!(ok, "{out}");
    r.wait("the watch met", || !r.kind("watch.met").is_empty());
    let met = r.kind("watch.met");
    assert_eq!(met.len(), 1, "{met:?}");
    assert_eq!(met[0]["value"], "MERGED");
    assert!(
        met[0]["text"]
            .as_str()
            .unwrap()
            .starts_with("👁 PR 123 merged → MERGED (after "),
        "{met:?}"
    );
    let (_, listed) = r.cli(None, &["watch-for"]);
    assert!(listed.contains("no open watches"), "{listed}");
    // No model, no turn: nothing in the ledger says either happened.
    let ran: Vec<String> = r
        .ledger()
        .iter()
        .map(|row| row["kind"].as_str().unwrap_or("").to_string())
        .filter(|k| k.starts_with("provider.") || k.starts_with("turn.") || k.starts_with("loop."))
        .collect();
    assert!(ran.is_empty(), "a watch calls no model: {ran:?}");
}
