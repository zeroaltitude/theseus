//! Jobs whose wrappers went while no daemon ran (theseus-vej5), with the real
//! `theseusd` and real wrappers. A daemon starts five `proc.run` jobs and is
//! killed with SIGKILL, as a unit stop with `KillMode=control-group` kills
//! it; then, with no daemon running, two wrappers are killed with their
//! jobs, a third's pid file is pointed at a live process that is no wrapper
//! (its pid reused), a quick job finishes and spools its completion (it
//! waits for the test's word), and the
//! last wrapper lives on. The next daemon on the same state settles the
//! three gone jobs `outcome_unknown` at its first beat after serving, each
//! with a `job.wrapper_gone` row, instead of at their hour-long deadlines;
//! the quick job from its completion; and leaves the live one dispatched,
//! its wrapper untouched.

mod common;

use std::collections::BTreeMap;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
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
    _model: FakeModel,
    daemon: Option<Daemon>,
    /// What the test started beside the daemon, killed when dropped.
    others: Vec<Child>,
    /// The wrappers left running, whose groups are killed when dropped.
    groups: Vec<u32>,
}

impl Rig {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let go = dir.path().join("go");
        let model = FakeModel::start(move |prompt| {
            let argv = if prompt.contains("Start the long job") {
                json!(["sleep", "300"])
            } else if prompt.contains("Start the quick job") {
                // It ends once the test says so, with no daemon running, and
                // after a minute at most, should the test be gone.
                json!(["sh", "-c", "i=0; while [ ! -e \"$1\" ] && [ $i -lt 600 ]; do sleep 0.1; i=$((i+1)); done; echo done", "quick", go])
            } else {
                return vec![];
            };
            vec![("proc_run", json!({"argv": argv, "timeout_secs": 3600}))]
        });
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
        table(&mut t, "tools").insert("proc_sync_secs".into(), 1.into());
        std::fs::write(path("config.toml"), toml::to_string(&t).unwrap()).unwrap();
        Self {
            dir,
            _model: model,
            daemon: None,
            others: vec![],
            groups: vec![],
        }
    }

    /// Start a daemon on the rig's state, its log `theseusd-<n>.log`, and
    /// wait for its secrets.
    fn start(&mut self, n: u32) {
        let path = |p: &str| self.dir.path().join(p);
        let log = std::fs::File::create(path(&format!("theseusd-{n}.log"))).unwrap();
        self.daemon = Some(Daemon::spawn(
            Command::new(env!("CARGO_BIN_EXE_theseusd"))
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
        ));
        self.wait("the secrets", || {
            self.call("health", Value::Null)
                .ok()
                .filter(|h| h["secrets"]["state"] == "ready")
        });
    }

    /// SIGKILL the daemon, and reap it: its wrappers live on, as each is
    /// the leader of a session of its own.
    fn kill(&mut self) {
        drop(self.daemon.take());
    }

    fn path(&self, p: &str) -> PathBuf {
        self.dir.path().join(p)
    }

    fn logs(&self) -> String {
        (1..=2)
            .map(|n| {
                std::fs::read_to_string(self.path(&format!("theseusd-{n}.log"))).unwrap_or_default()
            })
            .collect::<Vec<_>>()
            .join("\n---\n")
    }

    fn call(&self, method: &str, params: Value) -> Result<Value, String> {
        let s = UnixStream::connect(self.path("sock")).map_err(|e| e.to_string())?;
        s.set_read_timeout(Some(Duration::from_secs(60))).unwrap();
        let req = json!({"jsonrpc": "2.0", "id": 1, "method": method, "params": params});
        (&s).write_all(format!("{req}\n").as_bytes())
            .map_err(|e| e.to_string())?;
        for line in BufReader::new(&s).lines() {
            let v: Value = serde_json::from_str(&line.map_err(|e| e.to_string())?)
                .map_err(|e| e.to_string())?;
            if v["id"] == 1 {
                return match v.get("error") {
                    Some(e) if !e.is_null() => Err(e.to_string()),
                    _ => Ok(v["result"].clone()),
                };
            }
        }
        Err("the connection closed".into())
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
                self.logs()
            );
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    fn actions(&self) -> Value {
        self.call("health", Value::Null).unwrap()["kernel"]["actions_by_state"].clone()
    }

    fn count(&self, state: &str) -> u64 {
        self.actions()[state].as_u64().unwrap_or(0)
    }

    /// A turn in a new session whose job runs on after the turn.
    fn start_job(&self, prompt: &str) {
        let s = self.call("session.open", json!({"label": prompt})).unwrap();
        self.call(
            "turn.submit",
            json!({"session_id": s["session_id"], "input": prompt, "author": "test", "attachments": []}),
        )
        .unwrap();
    }

    /// The spool's pid files: each job's wrapper pid.
    fn pids(&self) -> BTreeMap<String, u32> {
        std::fs::read_dir(self.path("state/spool/pids"))
            .unwrap()
            .flatten()
            .map(|e| {
                let pid = std::fs::read_to_string(e.path())
                    .unwrap()
                    .trim()
                    .parse()
                    .unwrap();
                (e.file_name().to_string_lossy().into_owned(), pid)
            })
            .collect()
    }

    fn ledger(&self, kind: &str) -> Vec<Value> {
        let t = self.call("ledger.tail", json!({"n": 2000})).unwrap();
        t["rows"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|r| r["kind"] == kind)
            .map(|r| r["data"].clone())
            .collect()
    }
}

impl Drop for Rig {
    fn drop(&mut self) {
        if self.daemon.is_some() {
            let _ = self.call("shutdown", Value::Null);
            let t0 = Instant::now();
            while self.daemon.as_mut().is_some_and(|d| d.try_wait().is_none())
                && t0.elapsed() < Duration::from_secs(5)
            {
                std::thread::sleep(Duration::from_millis(10));
            }
        }
        for g in &self.groups {
            kill_group(*g);
        }
        for c in &mut self.others {
            let _ = c.kill();
            let _ = c.wait();
        }
    }
}

/// SIGKILL to a wrapper's process group: the wrapper and its job.
fn kill_group(pid: u32) {
    // SAFETY: a signal to a process group this test's daemon started.
    unsafe {
        libc::kill(-(pid as i32), libc::SIGKILL);
    }
}

/// The argv after a wrapper's `--`: its job's command.
fn job_of(pid: u32) -> String {
    let c = std::fs::read(format!("/proc/{pid}/cmdline")).unwrap_or_default();
    let words: Vec<String> = c
        .split(|&b| b == 0)
        .map(|w| String::from_utf8_lossy(w).into_owned())
        .collect();
    let at = words
        .iter()
        .position(|w| w == "--")
        .map_or(words.len(), |i| i + 1);
    words[at..].join(" ")
}

fn gone(pid: u32) -> bool {
    !std::path::Path::new(&format!("/proc/{pid}")).exists()
        || std::fs::read(format!("/proc/{pid}/cmdline")).is_ok_and(|c| c.is_empty())
}

#[test]
fn a_restart_settles_the_jobs_whose_wrappers_went_while_no_daemon_ran() {
    let mut r = Rig::new();
    r.start(1);
    for _ in 0..4 {
        r.start_job("Start the long job.");
    }
    r.start_job("Start the quick job.");
    r.wait("five jobs dispatched", || {
        (r.count("dispatched") == 5).then_some(())
    });
    let pids = r.wait("five wrappers", || {
        let p = r.pids();
        (p.len() == 5 && p.values().all(|&pid| !job_of(pid).is_empty())).then_some(p)
    });
    let (quick, long): (Vec<_>, Vec<_>) = pids
        .iter()
        .map(|(id, &pid)| (id.clone(), pid))
        .partition(|(_, pid)| job_of(*pid).starts_with("sh -c"));
    assert_eq!((quick.len(), long.len()), (1, 4), "{pids:?}");
    let (killed, reused, alive) = (&long[..2], &long[2], &long[3]);

    // Every turn has ended, so no turn of the next daemon resumes a job's
    // call (`toolrun/resume.rs` settles one whose wrapper is gone as
    // `interrupted_by_restart`): the jobs are the probe's alone.
    let before = r.wait("the turns ended", || {
        let h = r.call("health", Value::Null).ok()?;
        let e = h["kernel"]["executions_by_state"].clone();
        (e["running"].as_u64().unwrap_or(0) == 0).then_some(e)
    });
    r.kill();
    // With no daemon running: two wrappers die with their jobs, one's pid
    // file names a live process that is no wrapper, the quick job reports.
    for (_, pid) in killed {
        kill_group(*pid);
    }
    kill_group(reused.1);
    let other = Command::new("sleep").arg("300").spawn().unwrap();
    std::fs::write(
        r.path(&format!("state/spool/pids/{}", reused.0)),
        other.id().to_string(),
    )
    .unwrap();
    r.others.push(other);
    r.groups.push(alive.1);
    r.wait("the wrappers gone", || {
        killed
            .iter()
            .chain([reused])
            .all(|(_, pid)| gone(*pid))
            .then_some(())
    });
    std::fs::write(r.path("go"), "").unwrap();
    r.wait("the quick job's completion", || {
        r.path(&format!("state/spool/{}.json", quick[0].0))
            .exists()
            .then_some(())
    });

    let t0 = Instant::now();
    r.start(2);
    // The probe has run once its phase is in health: it settles in that
    // beat, before the phase is recorded.
    let phase = r.wait("the probe", || {
        let h = r.call("health", Value::Null).ok()?;
        h["startup"]
            .as_array()?
            .iter()
            .find(|p| p["name"] == "jobs_at_start")
            .cloned()
    });
    let settled_in = t0.elapsed();
    // A late result the start delivered wakes its turn, whose model call
    // is dispatched for a moment: `actions_by_state` counts it too.
    r.wait("the live wrapper's job alone dispatched", || {
        (r.count("dispatched") == 1).then_some(())
    });
    let a = r.actions();
    assert_eq!(a["dispatched"], 1, "the live wrapper's job runs on: {a}");
    assert!(
        !r.path(&format!("state/spool/{}.json", quick[0].0)).exists(),
        "the quick job settled from its completion, which the start took: {a}"
    );
    assert!(
        settled_in < Duration::from_secs(10),
        "settled at the start, not at the deadline: {settled_in:?}"
    );
    assert!(!gone(alive.1), "the live wrapper is untouched");
    assert!(job_of(alive.1).starts_with("sleep"));

    let want: Vec<(String, u64)> = killed
        .iter()
        .map(|(id, pid)| (id.clone(), u64::from(*pid)))
        .chain([(reused.0.clone(), u64::from(r.others[0].id()))])
        .collect();
    rows_name_the_gone(&r, want, &before);
    assert_eq!(
        (
            &phase["detail"]["probed"],
            &phase["detail"]["gone"],
            &phase["detail"]["alive"]
        ),
        (&json!(4), &json!(3), &json!(1)),
        "{phase}"
    );
}

/// Each gone job has its `job.wrapper_gone` row, with the pid its spool
/// named, and its unknown mark the probe's; none is called lost.
fn rows_name_the_gone(r: &Rig, mut want: Vec<(String, u64)>, before: &Value) {
    let rows = r.ledger("job.wrapper_gone");
    let mut named: Vec<(String, u64)> = rows
        .iter()
        .map(|d| {
            (
                d["correlation_id"].as_str().unwrap().to_string(),
                d["pid"].as_u64().unwrap(),
            )
        })
        .collect();
    named.sort();
    want.sort();
    assert_eq!(
        named,
        want,
        "a row for each gone job, with the pid its spool named; the unknown: {:?}; executions before: {:?}",
        r.ledger("action.outcome_unknown"),
        before
    );
    assert!(
        r.ledger("job.wrapper_lost").is_empty(),
        "nothing was reaped: no wrapper is called lost"
    );
    // Under load the kill can also catch a turn's provider call in flight,
    // which the earlier process's rule marks unknown at the driver's first
    // tick (theseus-m9iy): only the jobs are this probe's.
    let unknown = r.ledger("action.outcome_unknown");
    let mut probed: Vec<&str> = unknown
        .iter()
        .filter(|u| u["producer"] == "reconciler:wrapper_gone_at_start")
        .map(|u| u["correlation_id"].as_str().unwrap())
        .collect();
    probed.sort_unstable();
    let gone_ids: Vec<&str> = want.iter().map(|(id, _)| id.as_str()).collect();
    assert_eq!(probed, gone_ids, "{unknown:?}");
    assert!(
        unknown
            .iter()
            .all(|u| u["producer"] == "reconciler:wrapper_gone_at_start"
                || u["producer"] == "reconciler:in_process_before_restart"),
        "{unknown:?}"
    );
}
