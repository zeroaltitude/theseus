//! `proc.run`'s `steps` with the real `theseusd` and its real job wrapper
//! (theseus-7gir.3): a `/stop` during a step reaches that step's process and
//! starts no more; a `kill -9` of the daemon while a step runs leaves the
//! call to settle at the restart as one job's does, and the steps after it
//! never start.

mod common;

use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
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

/// The batch: a first step, a second that writes its pid and runs for
/// `secs`, and a third whose file must never appear.
fn batch(secs: u32) -> Value {
    json!({"steps": [
        {"argv": ["touch", "first"]},
        {"argv": ["sh", "-c", format!("echo $$ > second.pid; exec sleep {secs}")]},
        {"argv": ["touch", "never"]},
    ]})
}

struct Rig {
    dir: tempfile::TempDir,
    _model: FakeModel,
}

impl Rig {
    fn new(secs: u32) -> Self {
        let model = FakeModel::start(move |prompt| {
            if prompt.contains("Run the batch") {
                vec![("proc_run", batch(secs))]
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
        table(&mut t, "tools").insert("proc_sync_secs".into(), 30.into());
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
        call(&self.path("sock"), method, params)
    }

    /// The pid the second step wrote.
    fn second(&self) -> Option<u32> {
        std::fs::read_to_string(self.path("projects/second.pid"))
            .ok()?
            .trim()
            .parse()
            .ok()
    }

    /// The batch's execution and its one `proc.run` action, once it runs.
    fn running(&self) -> (String, Value) {
        self.wait("the batch's action", || {
            let e = self.call("execution.list", json!({})).ok()?;
            e["executions"].as_array()?.iter().find_map(|e| {
                let id = e["execution_id"].as_str()?.to_string();
                let a = self.action(&id)?;
                Some((id, a))
            })
        })
    }

    fn action(&self, execution: &str) -> Option<Value> {
        let a = self
            .call("action.list", json!({"execution_id": execution}))
            .ok()?;
        a["actions"]
            .as_array()?
            .iter()
            .find(|a| a["tool"] == "proc.run")
            .cloned()
    }

    /// `turn.submit` on a thread of its own, since the batch holds it.
    fn submit(&self) -> std::thread::JoinHandle<Result<Value, Value>> {
        let sock = self.path("sock");
        std::thread::spawn(move || {
            call(
                &sock,
                "turn.submit",
                json!({"input": "Run the batch", "author": "test", "attachments": []}),
            )
        })
    }
}

fn call(sock: &std::path::Path, method: &str, params: Value) -> Result<Value, Value> {
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

/// Running or sleeping: neither gone nor a zombie.
fn alive(pid: u32) -> bool {
    std::fs::read_to_string(format!("/proc/{pid}/stat"))
        .ok()
        .and_then(|s| s[s.rfind(')')? + 2..].chars().next())
        .is_some_and(|c| c != 'Z' && c != 'X')
}

/// A `/stop` while the second step runs reaches its process, by the call's
/// one correlation id, and the third never starts.
#[test]
fn a_stop_during_the_second_step_kills_it_and_starts_no_third() {
    let r = Rig::new(30);
    let _daemon = r.spawn();
    let turn = r.submit();
    let pid = r.wait("the second step", || r.second());
    let (eid, _) = r.running();
    let stop = r
        .call(
            "execution.stop",
            json!({"execution_id": eid, "author": "test"}),
        )
        .unwrap();
    assert_eq!(stop["stopped"], true, "{stop}");
    r.wait("the second step's end", || (!alive(pid)).then_some(()));
    let _ = turn.join().unwrap();
    std::thread::sleep(Duration::from_millis(500));
    assert!(r.path("projects/first").exists());
    assert!(
        !r.path("projects/never").exists(),
        "a step after the stop ran"
    );
    let a = r.action(&eid).unwrap();
    assert_eq!(a["state"], "cancelled", "{a}");
}

/// A `kill -9` of the daemon while the second step runs: the step's wrapper
/// runs on and reports, the restart settles the call from that, as one
/// job's, and the third never starts.
#[test]
fn a_restart_with_a_step_running_settles_the_call() {
    let r = Rig::new(2);
    let daemon = r.spawn();
    let _turn = r.submit();
    let pid = r.wait("the second step", || r.second());
    let (eid, a) = r.running();
    assert_eq!(a["state"], "dispatched", "{a}");
    drop(daemon);
    let _daemon = r.spawn();
    r.wait("the second step's end", || (!alive(pid)).then_some(()));
    let settled = r.wait("the call settled", || {
        let a = r.action(&eid)?;
        (a["state"] != "dispatched").then_some(a)
    });
    // From the second step's own report: it exited 0.
    assert_eq!(settled["state"], "succeeded", "{settled}");
    std::thread::sleep(Duration::from_millis(500));
    assert!(
        !r.path("projects/never").exists(),
        "a step after the restart ran"
    );
}
