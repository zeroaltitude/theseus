//! A continuation runs on the profile its session's last turn ran on, never
//! on the live one (theseus-kol), with the real `theseusd` and a stand-in for
//! the Messages API. The template's live profile is `sonnet`
//! (claude-sonnet-5-5); every turn here is asked on `glm` (glm-5.3-flash,
//! provider `zai`), whose endpoint is the same stand-in, which keeps every
//! request, so each continuation's model is read from what it was asked.
//! - a job that outlives `proc_sync_secs`: its late result's turn;
//! - a SIGKILL while a job runs inside its turn: the restart's continuation,
//!   then the late result's;
//! - a wake the turn set: the wake's turn.

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

const GLM: &str = "glm-5.3-flash";

struct Rig {
    dir: tempfile::TempDir,
    model: FakeModel,
}

impl Rig {
    /// `Run the job` gets a `proc.run` that marks its start in the projects
    /// dir and sleeps `job_secs`; `Wake me soon` gets a wake two seconds
    /// ahead; anything else gets text.
    fn new(proc_sync_secs: u64, job_secs: u64) -> Self {
        let model = FakeModel::start(move |prompt| {
            if prompt.contains("Run the job") {
                let script = format!("touch started; sleep {job_secs}; echo the job is done");
                vec![("proc_run", json!({"argv": ["sh", "-c", script]}))]
            } else if prompt.contains("Wake me soon") {
                vec![("wake_at", json!({"after": "2s", "note": "check the job"}))]
            } else {
                vec![]
            }
        });
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
        table(&mut t, "policy").insert("enforcement".into(), "notify".into());
        table(&mut t, "tools").insert("proc_sync_secs".into(), (proc_sync_secs as i64).into());
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

    /// Wait until the stand-in has been asked about the job's late result,
    /// and every turn started has ended but the `cut` ones a kill ended.
    fn late_result_answered(&self, cut: usize) {
        self.wait("the late result's turn", || {
            let asked = self.model.requests().iter().any(late_text);
            let started = self.rows("turn.started").len();
            (asked && started > cut && self.rows("turn.ended").len() + cut == started).then_some(())
        });
    }

    /// Every model call went to `glm`, and every turn ran on it, the
    /// continuations included (`n_continuations` of them, at least).
    fn all_on_glm(&self, n_continuations: usize) {
        let asked: Vec<String> = self
            .model
            .requests()
            .iter()
            .map(|r| r["model"].as_str().unwrap_or("").to_string())
            .collect();
        assert!(!asked.is_empty());
        assert!(
            asked.iter().all(|m| m == GLM),
            "every call goes to {GLM}, never the live profile's model: {asked:?}"
        );
        let started = self.rows("turn.started");
        let continuations = started
            .iter()
            .filter(|r| r["data"]["continuation"] == true)
            .count();
        assert!(
            continuations >= n_continuations,
            "{continuations} continuations: {started:?}"
        );
        for r in &started {
            assert_eq!(r["data"]["profile"], "glm", "{r}");
            assert_eq!(r["data"]["model"], GLM, "{r}");
        }
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

/// A turn asked on `glm`, on a new session.
fn ask_glm(sock: &Path, prompt: &str) -> Result<Value, Value> {
    call(
        sock,
        "turn.submit",
        json!({"input": prompt, "profile": "glm", "author": "test", "attachments": []}),
    )
}

/// Whether a request's last message is a job's late result.
fn late_text(req: &Value) -> bool {
    req["messages"]
        .as_array()
        .and_then(|m| m.last())
        .and_then(|m| m["content"].as_array())
        .is_some_and(|c| {
            c.iter().any(|b| {
                b["text"]
                    .as_str()
                    .is_some_and(|t| t.contains("Background result"))
            })
        })
}

fn tail(s: &str, n: usize) -> String {
    let lines: Vec<&str> = s.lines().collect();
    lines[lines.len().saturating_sub(n)..].join("\n")
}

/// A job that outlives `proc_sync_secs` is answered `background`, and its
/// late result's turn runs on the profile the turn that started it ran on.
#[test]
fn a_late_results_turn_runs_on_the_profile_that_started_the_job() {
    let r = Rig::new(1, 3);
    let _daemon = r.spawn();
    let first = ask_glm(&r.path("sock"), "Run the job").unwrap();
    assert_eq!(first["profile"], "glm", "{first}");
    // A socket daemon's result names nothing left for later: its late
    // results come back on their own (theseus-mqxk).
    assert!(first.get("later").is_none(), "{first}");
    r.late_result_answered(0);
    r.all_on_glm(1);
}

/// A SIGKILL while the job runs inside its turn: the restarted daemon's
/// continuation (the restart's placeholder for the call) and the late
/// result's turn both run on `glm`, and the late result is answered.
#[test]
fn a_restarts_continuation_runs_on_the_profile_of_the_turn_it_cut() {
    let r = Rig::new(30, 4);
    let daemon = r.spawn();
    let sock = r.path("sock");
    let asking = std::thread::spawn(move || ask_glm(&sock, "Run the job"));
    r.wait("the job's start", || {
        r.path("projects/started").exists().then_some(())
    });
    drop(daemon);
    // The connection closes with the daemon, unanswered.
    assert!(asking.join().unwrap().is_err());
    let _daemon = r.spawn();
    r.late_result_answered(1);
    r.all_on_glm(2);
    let placeholder = r.model.requests().iter().any(|q| {
        q["messages"]
            .to_string()
            .contains("the harness restarted meanwhile")
    });
    assert!(placeholder, "the restart's placeholder was read");
    assert_eq!(r.rows("execution.interrupted").len(), 1);
}

/// A wake the turn set: its turn runs on the profile the session's last turn
/// ran on, not the live one.
#[test]
fn a_wakes_turn_runs_on_the_profile_of_the_turn_that_set_it() {
    let r = Rig::new(1, 1);
    let _daemon = r.spawn();
    let first = ask_glm(&r.path("sock"), "Wake me soon").unwrap();
    assert_eq!(first["output"], "Done.", "{first}");
    r.wait("the wake's turn", || {
        let fired = r.rows("wake.fired").len();
        let started = r.rows("turn.started").len();
        (fired == 1 && started == 2 && r.rows("turn.ended").len() == 2).then_some(())
    });
    r.all_on_glm(1);
    // The socket daemon's words for the wake are as they were, and its
    // result names nothing left (theseus-mqxk).
    assert!(first.get("later").is_none(), "{first}");
    let said = r.model.requests().iter().any(|q| {
        let m = q["messages"].to_string();
        m.contains("This conversation gets a turn then") && !m.contains("will not fire")
    });
    assert!(said, "the wake's answer changed");
}
