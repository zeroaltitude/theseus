//! A job's handle with the real `theseusd` and real job wrappers
//! (theseus-n8gk): a job `proc_run` started in the background is read with
//! `job_read` and waited for with `job_wait`, whose result carries what it
//! printed; and a run past its window, named by the handle its answer gave,
//! is stopped with `job_stop`, which leaves no process of its tree. The
//! stand-in model ends each turn once it has a tool's result, so each step is
//! a turn of its own.

mod common;

use common::{table, Fixture, FAKE_OP};

use std::path::PathBuf;
use std::process::Stdio;

use common::model::FakeModel;
use common::Daemon;
use serde_json::{json, Value};

/// A tree: a shell that writes its pid and its child's, a `sleep` it waits
/// on, gone by itself within `secs`.
fn tree(secs: u32) -> Vec<String> {
    vec![
        "bash".into(),
        "-c".into(),
        format!("echo $$ > tree.pid; sleep {secs} & echo $! > child.pid; wait"),
    ]
}

struct Rig {
    dir: tempfile::TempDir,
    model: FakeModel,
}

impl Fixture for Rig {
    fn dir(&self) -> &std::path::Path {
        self.dir.path()
    }
}

impl Rig {
    fn new(sync_secs: i64) -> Self {
        let model = FakeModel::start(|prompt| match prompt {
            "Start it" => vec![(
                "proc_run",
                json!({"argv": ["sh", "-c", "sleep 20; printf 'do%s\\n' ne"], "background": true}),
            )],
            "Read j1" => vec![("job_read", json!({"job": "j1"}))],
            "Wait for j1" => vec![("job_wait", json!({"job": "j1"}))],
            "Run the long one" => vec![("proc_run", json!({"argv": tree(90)}))],
            "Stop j1" => vec![("job_stop", json!({"job": "j1"}))],
            _ => vec![],
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
        table(&mut t, "model").insert("api_base".into(), model.base.clone().into());
        for (_, p) in table(&mut t, "providers").iter_mut() {
            p.as_table_mut()
                .unwrap()
                .insert("api_base".into(), model.base.clone().into());
        }
        table(&mut t, "policy").insert("enforcement".into(), "notify".into());
        table(&mut t, "tools").insert("proc_sync_secs".into(), sync_secs.into());
        std::fs::write(path("config.toml"), toml::to_string(&t).unwrap()).unwrap();
        Self { dir, model }
    }

    fn spawn(&self) -> Daemon {
        let log = std::fs::File::create(self.path("theseusd.log")).unwrap();
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

    fn call(&self, method: &str, params: Value) -> Result<Value, Value> {
        common::rpc::call_within(&self.sock(), 120, method, params)
    }

    /// A turn with `input`, in `session` (a new one when none): its session.
    fn turn(&self, session: Option<&str>, input: &str) -> String {
        let mut p = json!({"input": input, "author": "test", "attachments": []});
        if let Some(s) = session {
            p["session_id"] = json!(s);
        }
        let res = self.call("turn.submit", p).unwrap();
        res["session_id"].as_str().unwrap().to_string()
    }

    /// The text of the last tool result the stand-in was sent.
    fn last_result(&self) -> String {
        let requests = self.model.requests();
        for r in requests.iter().rev() {
            let Some(last) = r["messages"].as_array().and_then(|m| m.last()) else {
                continue;
            };
            for b in last["content"].as_array().into_iter().flatten() {
                if b["type"] == "tool_result" {
                    return match &b["content"] {
                        Value::String(s) => s.clone(),
                        Value::Array(parts) => parts
                            .iter()
                            .filter_map(|p| p["text"].as_str())
                            .collect::<Vec<_>>()
                            .join(""),
                        other => other.to_string(),
                    };
                }
            }
        }
        panic!("no tool result was sent")
    }

    /// A pid the job wrote to `file` in the projects dir.
    fn pid(&self, file: &str) -> Option<u32> {
        std::fs::read_to_string(self.path("projects").join(file))
            .ok()?
            .trim()
            .parse()
            .ok()
    }
}

/// Running or sleeping: neither gone nor a zombie.
fn alive(pid: u32) -> bool {
    std::fs::read_to_string(format!("/proc/{pid}/stat"))
        .ok()
        .and_then(|s| s[s.rfind(')')? + 2..].chars().next())
        .is_some_and(|c| c != 'Z' && c != 'X')
}

/// Every process whose command line holds `marker`.
fn holding(marker: &str) -> Vec<(u32, String)> {
    let mut out = Vec::new();
    for e in std::fs::read_dir("/proc").unwrap().flatten() {
        let Some(pid) = e.file_name().to_str().and_then(|n| n.parse::<u32>().ok()) else {
            continue;
        };
        let cmd = std::fs::read(format!("/proc/{pid}/cmdline")).unwrap_or_default();
        let cmd = String::from_utf8_lossy(&cmd).replace('\0', " ");
        if cmd.contains(marker) && alive(pid) {
            out.push((pid, cmd));
        }
    }
    out
}

/// `proc_run {background: true}` answers at once (the job still runs at the
/// next turn's read); `job_read` shows the job running; `job_wait` returns
/// its result, which the request after it carries.
#[test]
fn a_background_job_is_read_and_waited_for() {
    let r = Rig::new(60);
    let _daemon = r.spawn();
    let sid = r.turn(None, "Start it");
    let started = r.last_result();
    assert!(started.starts_with("Started job j1 ("), "{started}");
    r.turn(Some(&sid), "Read j1");
    let read = r.last_result();
    assert!(read.starts_with("Job j1 (sh -c sleep 20; "), "{read}");
    assert!(read.contains("), running "), "{read}");
    r.turn(Some(&sid), "Wait for j1");
    let waited = r.last_result();
    // Its first line names where it ran (theseus-9dt2).
    assert_eq!(
        waited
            .split_once("]\n")
            .map(|(head, rest)| (head.starts_with("[exit code 0 · in /"), rest)),
        Some((true, "done\n")),
        "{waited}\n{}",
        r.log()
    );
}

/// A run past its window answers with its handle; `job_stop` by that handle
/// stops its whole tree, verified, and nothing of it is left. A negative
/// assertion: when it fails it prints each process left.
#[test]
fn a_run_past_its_window_is_stopped_by_its_handle_and_nothing_is_left() {
    let r = Rig::new(1);
    let _daemon = r.spawn();
    let sid = r.turn(None, "Run the long one");
    let still = r.last_result();
    assert!(
        still.starts_with("Still running as job j1 after 1 s (timeout 600 s)."),
        "{still}"
    );
    let (shell, child) = r.wait("the tree's pids", || {
        Some((r.pid("tree.pid")?, r.pid("child.pid")?))
    });
    assert!(alive(shell) && alive(child));
    r.turn(Some(&sid), "Stop j1");
    let stopped = r.last_result();
    let marker = "echo $$ > tree.pid; sleep 90";
    let left = holding(marker);
    assert!(
        stopped.starts_with("Stopped job j1 (bash -c echo $$ > tree.pid; sleep 90")
            && stopped.contains("): nothing of it is left.\n"),
        "{stopped}\nleft: {left:?}"
    );
    assert!(
        stopped.contains("[cancelled: stopped by a job_stop call"),
        "{stopped}"
    );
    assert!(
        !alive(shell) && !alive(child) && left.is_empty(),
        "the job's tree is gone: shell {shell} {}, sleep {child} {}, left {left:?}\n{stopped}",
        alive(shell),
        alive(child)
    );
}
