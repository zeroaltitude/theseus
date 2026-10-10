//! A job's handle with the real `theseusd` and real job wrappers
//! (theseus-n8gk): a job `proc_run` started in the background is read with
//! `job_read` and waited for with `job_wait`, whose result carries what it
//! printed; and a run past its window, named by the handle its answer gave,
//! is stopped with `job_stop`, which leaves no process of its tree. The
//! stand-in model ends each turn once it has a tool's result, so each step is
//! a turn of its own.

mod common;

use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::process::Stdio;
use std::time::{Duration, Instant};

use common::model::FakeModel;
use common::Daemon;
use serde_json::{json, Value};

/// A fake `op`, as `stops.rs`'s: every reference's value is `tv-<its item>`.
const FAKE_OP: &str = "#!/bin/sh\n\
    case \"$1\" in\n\
    \x20 inject) sed -E 's#\\{\\{ op://Test/([^/]+)/[^}]* \\}\\}#tv-\\1#g' ;;\n\
    \x20 read) for a; do ref=\"$a\"; done; printf 'tv-%s' \"$(echo \"$ref\" | cut -d/ -f4)\" ;;\n\
    \x20 *) exit 1 ;;\n\
    esac\n";

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
        table(&mut t, "tools").insert("proc_sync_secs".into(), sync_secs.into());
        std::fs::write(path("config.toml"), toml::to_string(&t).unwrap()).unwrap();
        Self { dir, model }
    }

    fn path(&self, p: &str) -> PathBuf {
        self.dir.path().join(p)
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

    fn log(&self) -> String {
        std::fs::read_to_string(self.path("theseusd.log")).unwrap_or_default()
    }

    fn wait<T>(&self, what: &str, mut f: impl FnMut() -> Option<T>) -> T {
        let t0 = Instant::now();
        loop {
            if let Some(v) = f() {
                return v;
            }
            let log = self.log();
            let lines: Vec<&str> = log.lines().collect();
            assert!(
                t0.elapsed() < Duration::from_secs(40),
                "no {what} in 40 s; the daemon's log ends:\n{}",
                lines[lines.len().saturating_sub(30)..].join("\n")
            );
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    fn call(&self, method: &str, params: Value) -> Result<Value, Value> {
        let s = UnixStream::connect(self.path("sock")).map_err(|e| json!(e.to_string()))?;
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
    assert_eq!(waited, "[exit code 0]\ndone\n", "{}", r.log());
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
