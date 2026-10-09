//! A terminal's background job at its session's end, through the real daemon
//! (theseus-ggqf): a task's terminal starts a job in the background and the
//! task reports; its session's end closes the terminal, which ends the
//! terminal's program and leaves the job running, as `proc.run` leaves one.
//! Health lists the job while it runs, its parent is the daemon (a child
//! subreaper), and once it exits the daemon reaps it: no zombie is left.

mod common;

use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::process::Stdio;
use std::time::{Duration, Instant};

use common::model::FakeModel;
use common::Daemon;
use serde_json::{json, Value};

/// A fake `op` that answers every reference at once.
const QUICK_OP: &str = "#!/bin/sh\n\
     case \"$1\" in\n\
     \x20 inject) sed -e 's/{{ [^}]* }}/test-secret-value-0000/g' ;;\n\
     \x20 read) printf '%s' test-secret-value-0000 ;;\n\
     \x20 *) exit 1 ;;\n\
     esac\n";

const ASK: &str = "Please start the job task.";
const BRIEF: &str = "Leave a job running.";

struct Rig {
    dir: tempfile::TempDir,
    daemon: Daemon,
    _model: FakeModel,
}

impl Rig {
    fn start(job: String) -> Self {
        let open = json!({"argv": ["bash", "-c", job], "quiet_ms": 200});
        let model = FakeModel::start(move |prompt| {
            if prompt.starts_with("[Task ") && prompt.contains(BRIEF) {
                return vec![("term_open", open.clone())];
            }
            match prompt.contains(ASK) {
                true => vec![(
                    "task_create",
                    json!({"brief": BRIEF, "arrangement":
                           {"pieces": [{"quote": ASK, "role": "objective"}]}}),
                )],
                false => vec![],
            }
        });
        let dir = tempfile::tempdir().unwrap();
        let path = |p: &str| dir.path().join(p);
        let theseusd = PathBuf::from(env!("CARGO_BIN_EXE_theseusd"));
        for d in ["bin", "projects"] {
            std::fs::create_dir_all(path(d)).unwrap();
        }
        use std::os::unix::fs::PermissionsExt;
        std::fs::write(path("bin/op"), QUICK_OP).unwrap();
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
        std::fs::write(path("config.toml"), toml::to_string(&t).unwrap()).unwrap();
        let log = std::fs::File::create(path("theseusd.log")).unwrap();
        let daemon = Daemon::spawn(
            std::process::Command::new(&theseusd)
                .arg("--config")
                .arg(path("config.toml"))
                .arg("--state-dir")
                .arg(path("state"))
                .arg("--socket")
                .arg(path("sock"))
                .env(
                    "PATH",
                    format!(
                        "{}:{}",
                        path("bin").display(),
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
        let mut r = Self {
            dir,
            daemon,
            _model: model,
        };
        r.until("the first round of secrets", |h| {
            h["secrets"]["state"]
                .as_str()
                .is_some_and(|s| s != "resolving")
        });
        r
    }

    fn log(&self) -> String {
        std::fs::read_to_string(self.dir.path().join("theseusd.log")).unwrap_or_default()
    }

    fn call(&self, method: &str, params: Value) -> Result<Value, Value> {
        let s =
            UnixStream::connect(self.dir.path().join("sock")).map_err(|e| json!(e.to_string()))?;
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

    /// Ask `health` until `ok` says so.
    fn until(&mut self, what: &str, ok: impl Fn(&Value) -> bool) -> Value {
        let t0 = Instant::now();
        loop {
            if let Ok(h) = self.call("health", Value::Null) {
                if ok(&h) {
                    return h;
                }
            }
            if let Some(status) = self.daemon.try_wait() {
                panic!("theseusd exited ({status}) before {what}:\n{}", self.log());
            }
            assert!(
                t0.elapsed() < Duration::from_secs(30),
                "no {what} in 30 s:\n{}",
                self.log()
            );
            std::thread::sleep(Duration::from_millis(10));
        }
    }
}

/// `/proc/<pid>/stat`: the state letter and the parent's pid.
fn stat(pid: u32) -> Option<(char, u32)> {
    let s = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    let mut f = s[s.rfind(')')? + 2..].split(' ');
    Some((f.next()?.chars().next()?, f.next()?.parse().ok()?))
}

/// A task's terminal leaves its `&` job at the task's end: health lists it
/// (pid, program, why, its terminal), the terminal's own program is gone,
/// the job is the daemon's child, and when it exits the daemon reaps it:
/// health's list empties, and no zombie of the daemon's is left.
#[test]
fn a_tasks_terminal_job_is_left_listed_and_reaped() {
    let run = format!("{:07}", std::process::id());
    let (bg, fg) = (format!("4793.{run}"), format!("4794.{run}"));
    let mut r = Rig::start(format!("set -m; sleep {bg} & exec sleep {fg}"));
    let daemon = r.daemon.id();
    let s = r
        .call("session.open", json!({"label": "terminals"}))
        .unwrap();
    r.call(
        "turn.submit",
        json!({"session_id": s["session_id"], "input": ASK, "author": "test", "attachments": []}),
    )
    .unwrap();
    let h = r.until("the job listed", |h| {
        h["terminals_left"]
            .as_array()
            .is_some_and(|l| !l.is_empty())
    });
    let left = &h["terminals_left"][0];
    assert_eq!(left["program"], "sleep", "{left}");
    assert_eq!(left["why"], "a background job", "{left}");
    assert_eq!(left["terminal_program"], "bash", "{left}");
    let pid = left["pid"].as_u64().unwrap() as u32;
    let cmdline = std::fs::read(format!("/proc/{pid}/cmdline")).unwrap();
    assert!(
        String::from_utf8_lossy(&cmdline).contains(&bg),
        "the listed pid is not the job"
    );
    assert!(
        r.log()
            .contains("a terminal's close left processes running"),
        "{}",
        r.log()
    );
    // The terminal's own program ended with the session; the job is the
    // daemon's now.
    let t0 = Instant::now();
    while stat(pid).map(|(_, p)| p) != Some(daemon) {
        assert!(t0.elapsed() < Duration::from_secs(10), "{:?}", stat(pid));
        std::thread::sleep(Duration::from_millis(10));
    }
    let fronts = std::fs::read_dir("/proc")
        .unwrap()
        .flatten()
        .filter(|e| {
            std::fs::read(e.path().join("cmdline"))
                .is_ok_and(|c| String::from_utf8_lossy(&c).contains(&fg))
        })
        .count();
    assert_eq!(fronts, 0, "the terminal's program outlived its session");
    let before = h["children"]["reaped_orphans"].as_u64().unwrap_or(0);
    // It exits: the daemon reaps it, and health forgets it.
    unsafe { libc::kill(pid as i32, libc::SIGKILL) };
    let h = r.until("the job reaped and forgotten", |h| {
        h["terminals_left"].as_array().is_none_or(|l| l.is_empty())
            && h["children"]["reaped_orphans"].as_u64().unwrap_or(0) > before
    });
    assert_eq!(h["children"]["zombies"], 0, "{}", h["children"]);
    assert_eq!(stat(pid), None, "the job was not reaped");
    r.call("shutdown", Value::Null).ok();
}
