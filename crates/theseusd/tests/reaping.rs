//! The daemon reaps its own children (theseus-z4b), with the real `theseusd`
//! and its real job wrappers, driven by a stand-in for the Messages API:
//! - 200 short jobs leave no zombie child behind. Before, each job left one
//!   until the daemon exited;
//! - an `op` run that lasts through a burst of jobs still gets its own exit
//!   status: the reaper never takes a child that tokio waits for.

mod common;

use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use common::model::FakeModel;
use common::Daemon;
use serde_json::{json, Value};

type Script = Arc<Mutex<Vec<(String, Vec<(&'static str, Value)>)>>>;

/// Where the jobs, and the fake `op`, leave their marks: inside the
/// workspace root, so every `proc.run` runs under `notify`.
const OUT: &str = "projects/out";

/// A fake `op` that answers every reference at once.
fn quick_op(_: &Path) -> String {
    "#!/bin/sh\n\
     case \"$1\" in\n\
     \x20 inject) sed -e 's/{{ [^}]* }}/test-secret-value-0000/g' ;;\n\
     \x20 read) printf '%s' test-secret-value-0000 ;;\n\
     \x20 *) exit 1 ;;\n\
     esac\n"
        .into()
}

struct Rig {
    dir: tempfile::TempDir,
    daemon: Daemon,
    script: Script,
    _model: FakeModel,
}

impl Rig {
    /// A daemon on a file config, under `notify`, whose secrets come from a
    /// fake `op` (`op` writes its script, given the rig's directory), with
    /// `extra` added to `[secrets]`. It returns once the first round of
    /// secrets has settled.
    fn start(op: impl FnOnce(&Path) -> String, extra: &[&str]) -> Self {
        let script = Script::default();
        let asks = script.clone();
        let model = FakeModel::start(move |prompt| {
            asks.lock()
                .unwrap()
                .iter()
                .find(|(p, _)| prompt.starts_with(p.as_str()))
                .map(|(_, calls)| calls.clone())
                .unwrap_or_default()
        });
        let dir = tempfile::tempdir().unwrap();
        let path = |p: &str| dir.path().join(p);
        let theseusd = PathBuf::from(env!("CARGO_BIN_EXE_theseusd"));
        for d in ["bin", OUT] {
            std::fs::create_dir_all(path(d)).unwrap();
        }
        let fake_op = path("bin/op");
        std::fs::write(&fake_op, op(dir.path())).unwrap();
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&fake_op, std::fs::Permissions::from_mode(0o755)).unwrap();
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
        table(&mut t, "tools").insert("proc_sync_secs".into(), 1.into());
        for name in extra {
            table(&mut t, "secrets").insert(
                (*name).into(),
                format!("op://Test/{name}/credential").into(),
            );
        }
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
            script,
            _model: model,
        };
        r.until("the first round of secrets", |h| {
            h["secrets"]["state"]
                .as_str()
                .is_some_and(|s| s != "resolving")
        });
        r
    }

    fn path(&self, p: &str) -> PathBuf {
        self.dir.path().join(p)
    }

    /// The model answers a prompt that starts with `prompt` with `calls`.
    fn asks(&self, prompt: &str, calls: Vec<(&'static str, Value)>) {
        self.script.lock().unwrap().push((prompt.into(), calls));
    }

    /// `proc.run` of `sh -c script`, whose `$1` is the directory for marks.
    fn job(&self, script: &str) -> (&'static str, Value) {
        let argv = [
            "sh".to_string(),
            "-c".into(),
            script.into(),
            "job".into(),
            self.path(OUT).display().to_string(),
        ];
        ("proc_run", json!({"argv": argv, "timeout_secs": 60}))
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

    fn wait<T>(&self, what: &str, mut f: impl FnMut() -> Option<T>) -> T {
        let t0 = Instant::now();
        loop {
            if let Some(v) = f() {
                return v;
            }
            assert!(
                t0.elapsed() < Duration::from_secs(30),
                "no {what} in 30 s:\n{}",
                self.log()
            );
            std::thread::sleep(Duration::from_millis(10));
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

    /// A turn in a new session: its result, once it parks or ends.
    fn turn(&self, prompt: &str) -> Value {
        let s = self.call("session.open", json!({"label": prompt})).unwrap();
        self.call(
            "turn.submit",
            json!({"session_id": s["session_id"], "input": prompt, "author": "test", "attachments": []}),
        )
        .unwrap()
    }

    /// The states of an execution's `proc.run` actions.
    fn job_states(&self, execution: &Value) -> Vec<String> {
        let a = self
            .call("action.list", json!({"execution_id": execution}))
            .unwrap();
        a["actions"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|a| a["tool"] == "proc.run")
            .map(|a| a["state"].as_str().unwrap_or("").to_string())
            .collect()
    }

    fn log(&self) -> String {
        std::fs::read_to_string(self.path("theseusd.log")).unwrap_or_default()
    }

    fn log_tail(&self) -> String {
        let s = self.log();
        let lines: Vec<&str> = s.lines().collect();
        lines[lines.len().saturating_sub(30)..].join("\n")
    }
}

/// `/proc/<pid>/stat`: the state letter and the parent's pid.
fn stat(pid: u32) -> Option<(char, u32)> {
    let s = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    let mut f = s[s.rfind(')')? + 2..].split(' ');
    Some((f.next()?.chars().next()?, f.next()?.parse().ok()?))
}

/// The zombies whose parent is `pid`: children that exited, not reaped.
fn zombie_children(pid: u32) -> Vec<u32> {
    std::fs::read_dir("/proc")
        .unwrap()
        .flatten()
        .filter_map(|e| e.file_name().to_str()?.parse::<u32>().ok())
        .filter(|&p| stat(p) == Some(('Z', pid)))
        .collect()
}

/// 200 short jobs, 50 in each of four sessions at once, leave no zombie child
/// of the daemon: each wrapper is reaped as it exits, and health counts them.
#[test]
fn two_hundred_jobs_leave_no_zombie_behind() {
    let mut r = Rig::start(quick_op, &[]);
    let pid = r.daemon.id();
    let calls: Vec<_> = (0..50).map(|_| r.job("exit 0")).collect();
    for s in 0..4 {
        r.asks(&format!("fifty jobs, batch {s}"), calls.clone());
    }
    let executions: Vec<Value> = std::thread::scope(|sc| {
        let turns: Vec<_> = (0..4)
            .map(|s| {
                let r = &r;
                sc.spawn(move || r.turn(&format!("fifty jobs, batch {s}"))["execution_id"].clone())
            })
            .collect();
        turns.into_iter().map(|t| t.join().unwrap()).collect()
    });
    for e in &executions {
        let states = r.job_states(e);
        assert_eq!(states.len(), 50, "{e}: {states:?}");
        assert!(states.iter().all(|s| s == "succeeded"), "{e}: {states:?}");
    }
    // Each wrapper had written its completion before it exited; give the last
    // ones the moment they take to exit and be reaped.
    let t0 = Instant::now();
    loop {
        let z = zombie_children(pid);
        if z.is_empty() {
            break;
        }
        assert!(
            t0.elapsed() < Duration::from_secs(10),
            "{} zombie children of theseusd remain after 200 jobs",
            z.len()
        );
        std::thread::sleep(Duration::from_millis(20));
    }
    let h = r.until("every wrapper counted", |h| {
        h["children"]["reaped_wrappers"] == 200
    });
    let c = &h["children"];
    assert_eq!(
        (
            &c["zombies"],
            &c["wrappers_running"],
            &c["orphans"],
            &c["subreaper"]
        ),
        (&json!(0), &json!(0), &json!(0), &json!(true)),
        "{c}"
    );
}

/// A fake `op` whose first injection fails, so that the one reference that
/// `read` cannot answer (`slow`) is fetched again 5 s later. That second
/// injection says it has begun (`op.retrying`), waits for the test's word
/// (`op.go`), takes 1 s, then says it has answered (`op.answered`) and does.
fn slow_retry_op(dir: &Path) -> String {
    let out = dir.join(OUT);
    let o = out.display();
    format!(
        "#!/bin/sh\n\
         O='{o}'\n\
         case \"$1\" in\n\
         \x20 inject)\n\
         \x20   if [ ! -e \"$O/op.failed-once\" ]; then touch \"$O/op.failed-once\"; echo '[ERROR] not yet' >&2; exit 1; fi\n\
         \x20   touch \"$O/op.retrying\"\n\
         \x20   while [ ! -e \"$O/op.go\" ] && [ -d \"$O\" ]; do sleep 0.01; done\n\
         \x20   sleep 1\n\
         \x20   touch \"$O/op.answered\"\n\
         \x20   sed -e 's/{{{{ [^}}]* }}}}/test-secret-value-0000/g' ;;\n\
         \x20 read)\n\
         \x20   case \"$3\" in *slow*) echo '[ERROR] not yet' >&2; exit 1 ;; esac\n\
         \x20   printf '%s' test-secret-value-0000 ;;\n\
         \x20 *) exit 1 ;;\n\
         esac\n"
    )
}

/// An `op` run that lasts 1 s, through a burst of 50 jobs whose wrappers exit
/// and are reaped before, while, and after it runs: tokio's wait for it still
/// gets its status, so the secret resolves by that injection, and no `op`
/// call fails. The job in the middle of the burst waits for `op` to have
/// answered, so the burst goes on after it.
#[test]
fn an_op_run_through_a_burst_of_jobs_keeps_its_exit_status() {
    let mut r = Rig::start(slow_retry_op, &["slow"]);
    let h = r.call("health", Value::Null).unwrap();
    assert_eq!(h["secrets"]["state"], "failed", "{}", h["secrets"]);
    let out = r.path(OUT);
    r.wait("the retry's op to begin", || {
        out.join("op.retrying").exists().then_some(())
    });
    let calls: Vec<_> = (0..50)
        .map(|i| match i {
            25 => r.job("while [ ! -e \"$1/op.answered\" ] && [ -d \"$1\" ]; do sleep 0.01; done"),
            _ => r.job("exit 0"),
        })
        .collect();
    r.asks("a burst of jobs", calls);
    let (reaped_at_go, execution) = std::thread::scope(|sc| {
        let rr = &r;
        let turn = sc.spawn(move || rr.turn("a burst of jobs")["execution_id"].clone());
        let reaped = r.wait("ten wrappers reaped", || {
            let h = r.call("health", Value::Null).ok()?;
            let n = h["children"]["reaped_wrappers"].as_u64()?;
            (n >= 10).then_some(n)
        });
        std::fs::write(out.join("op.go"), "").unwrap();
        (reaped, turn.join().unwrap())
    });
    assert!(
        reaped_at_go < 26,
        "the op ran while the burst did: {reaped_at_go}"
    );
    let states = r.job_states(&execution);
    assert_eq!(states.len(), 50, "{states:?}");
    let h = r.until("the slow secret", |h| h["secrets"]["state"] == "ready");
    assert!(
        h["secrets"]["ready"]
            .as_array()
            .unwrap()
            .iter()
            .any(|n| n == "slow"),
        "{}",
        h["secrets"]
    );
    let log = r.log();
    assert_eq!(
        log.matches("op inject failed").count(),
        1,
        "only the first injection failed, as the fake op made it:\n{}",
        r.log_tail()
    );
    for bad in ["No child processes", "running op inject", "running op read"] {
        assert!(!log.contains(bad), "{bad}:\n{}", r.log_tail());
    }
    let h = r.until("every wrapper reaped", |h| {
        h["children"]["reaped_wrappers"] == 50 && h["children"]["zombies"] == 0
    });
    assert_eq!(h["children"]["owned"], 0, "{}", h["children"]);
    assert!(zombie_children(r.daemon.id()).is_empty());
}
