//! A stop's jobs, with the real `theseusd` (review 2's S2, theseus-bzq):
//! `execution.stop` ends a session's jobs together, every SIGTERM first, one
//! shared grace, then SIGKILL for the stragglers, and waits on the runtime's
//! timer, so no worker is held while it waits. The daemon runs on one worker
//! thread (`TOKIO_WORKER_THREADS=1`): a stop that held its worker would hold
//! every other request with it.

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

/// A job that outlives SIGTERM: its shell traps it and carries on (a
/// `sleep` the signal ends is followed by the next), so only SIGKILL ends
/// it. The trap takes the signal's time in the shell itself, with no fork
/// (bash's `$EPOCHREALTIME`, in microseconds), ignores SIGTERM from then
/// on, and writes the time to `job-<i>.term.tmp`, renamed `job-<i>.term`
/// once whole (theseus-y0lm). Its wrapper's stop SIGTERMs every process it
/// meets in the tree through the grace, one born after the first signal
/// too: the trap's `date` once was, and its file was left empty. The `mv`
/// now inherits the ignored signal. It writes its pid to `job-<i>.pid`,
/// and ends by itself within 30 s should a failing test leave it.
fn stubborn(i: usize) -> Value {
    json!({
        "argv": ["bash", "-c", format!(
            "trap 't=$EPOCHREALTIME; trap \"\" TERM; printf \"%s\\n\" \"${{t/[.,]/}}\" > job-{i}.term.tmp && mv job-{i}.term.tmp job-{i}.term' TERM; echo $$ > job-{i}.pid; n=0; while [ $n -lt 600 ]; do sleep 0.05; n=$((n+1)); done"
        )],
        "timeout_secs": 60,
    })
}

struct Rig {
    dir: tempfile::TempDir,
    _model: FakeModel,
}

impl Rig {
    fn new() -> Self {
        let model = FakeModel::start(|prompt| {
            if prompt.contains("Start the three builds") {
                (0..3).map(|i| ("proc_run", stubborn(i))).collect()
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
        table(&mut t, "tools").insert("proc_sync_secs".into(), 1.into());
        std::fs::write(path("config.toml"), toml::to_string(&t).unwrap()).unwrap();
        Self { dir, _model: model }
    }

    fn path(&self, p: &str) -> PathBuf {
        self.dir.path().join(p)
    }

    /// The daemon, on one runtime worker.
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
                .env("TOKIO_WORKER_THREADS", "1")
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

    /// What a stop left, for a job whose SIGTERM is not on record: each
    /// job's files, the jobs' rows in `action.list` (`cancel`, the verdict),
    /// the stop's answer and how long it took, and the daemon's log lines
    /// for the stop.
    fn stop_seen(&self, eid: &str, took: Duration, answer: &Value) -> String {
        let mut out = vec![format!("the stop took {took:?} and answered {answer}")];
        let mut files: Vec<_> = std::fs::read_dir(self.path("projects"))
            .unwrap()
            .flatten()
            .map(|e| e.path())
            .filter(|p| {
                p.file_name()
                    .is_some_and(|n| n.to_string_lossy().starts_with("job-"))
            })
            .collect();
        files.sort();
        for p in files {
            let text = std::fs::read_to_string(&p).unwrap_or_default();
            out.push(format!("{}: {:?}", p.display(), text));
        }
        match self.call("action.list", json!({"execution_id": eid})) {
            Ok(a) => {
                for a in a["actions"].as_array().into_iter().flatten() {
                    out.push(format!("action: {a}"));
                }
            }
            Err(e) => out.push(format!("action.list failed: {e}")),
        }
        let log = self.log();
        let stop = log.lines().filter(|l| {
            ["stop", "cancel", "job", "wrapper", "verdict"]
                .iter()
                .any(|w| l.contains(w))
        });
        out.extend(stop.map(|l| format!("log: {l}")));
        out.join("\n")
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

/// Review 2's S2 (theseus-bzq): three jobs that ignore SIGTERM, then a stop.
/// It settles within one grace (2 s) and a margin, not three, every job's
/// command is gone after it, and while it waits, `health` answers at once on
/// the daemon's one worker.
#[test]
fn a_stop_of_three_jobs_that_ignore_sigterm_takes_one_grace_and_holds_no_worker() {
    let r = Rig::new();
    let _daemon = r.spawn();
    let res = r
        .call(
            "turn.submit",
            json!({"input": "Start the three builds", "author": "test", "attachments": []}),
        )
        .unwrap();
    let eid = res["execution_id"].as_str().unwrap().to_string();
    let jobs: Vec<u32> = r.wait("the three jobs running", || {
        let pids: Vec<u32> = (0..3)
            .filter_map(|i| r.pid(&format!("job-{i}.pid")))
            .collect();
        let e = r.call("execution.list", Value::Null).ok()?;
        let e = e["executions"]
            .as_array()?
            .iter()
            .find(|e| e["execution_id"] == eid.as_str())?
            .clone();
        (pids.len() == 3 && e["state"] == "waiting" && e["outstanding"] == 3).then_some(pids)
    });

    let stop = {
        let sock = r.path("sock");
        let eid = eid.clone();
        std::thread::spawn(move || {
            let t0 = Instant::now();
            let s = UnixStream::connect(sock).unwrap();
            s.set_read_timeout(Some(Duration::from_secs(60))).unwrap();
            let req = json!({"jsonrpc": "2.0", "id": 1, "method": "execution.stop",
                "params": {"execution_id": eid, "author": "test"}});
            (&s).write_all(format!("{req}\n").as_bytes()).unwrap();
            let line = BufReader::new(&s).lines().next().unwrap().unwrap();
            (t0.elapsed(), serde_json::from_str::<Value>(&line).unwrap())
        })
    };
    // While the stop waits out its grace, health answers at once.
    std::thread::sleep(Duration::from_millis(200));
    let mut answers = vec![];
    while !stop.is_finished() {
        let t0 = Instant::now();
        r.call("health", Value::Null).unwrap();
        answers.push(t0.elapsed());
        std::thread::sleep(Duration::from_millis(100));
    }
    let (took, answer) = stop.join().unwrap();
    let result = &answer["result"];
    assert_eq!(result["stopped"], true, "{answer}");
    assert_eq!(result["stopped_actions"].as_array().unwrap().len(), 3);
    assert!(
        took >= Duration::from_secs(2),
        "the grace was waited out: {took:?}"
    );
    // One grace, not three, by order rather than by a stopwatch
    // (theseus-3dsz): every job had its SIGTERM before any job's grace was
    // out. Stopped one after another, each next job would be signalled only
    // after the last one's grace (2 s), however fast the machine; stopped
    // together, they are signalled at once, however slow it is. Each time
    // is the one its trap took as it began, with no fork, so a trap slow to
    // write still proves the order; a job with no time on record is the
    // failure, printed with what tells its cause apart (theseus-y0lm).
    let terms: Vec<u128> = (0..3)
        .map(|i| {
            let t = std::fs::read_to_string(r.path("projects").join(format!("job-{i}.term")))
                .unwrap_or_default();
            t.lines()
                .next()
                .and_then(|l| l.trim().parse().ok())
                .unwrap_or_else(|| {
                    panic!(
                        "job {i} had no SIGTERM\n{}",
                        r.stop_seen(&eid, took, &answer)
                    )
                })
        })
        .collect();
    let spread =
        Duration::from_micros((terms.iter().max().unwrap() - terms.iter().min().unwrap()) as u64);
    assert!(
        spread < Duration::from_secs(2),
        "one grace, not three: the jobs' SIGTERMs came {spread:?} apart"
    );
    assert!(
        answers.len() >= 5,
        "health was asked during the stop: {answers:?}"
    );
    // A health that waited on the stop would answer only once the grace
    // was out: its first answer, asked 200 ms in, at about 1.8 s, and the
    // stop finished by then, so one answer in all. The bound is well under
    // that and well over a scheduler's stall under load (theseus-3dsz: one
    // answer took 880 ms at a load of 25 on 16 cores).
    let slowest = answers.iter().max().unwrap();
    assert!(
        *slowest < Duration::from_millis(1500),
        "health waited on the stop: {answers:?}"
    );
    for pid in &jobs {
        assert!(!alive(*pid), "job {pid} is gone");
    }
    let actions = r.call("action.list", json!({"execution_id": eid})).unwrap();
    let stopped: Vec<&Value> = actions["actions"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|a| a["tool"] == "proc.run")
        .collect();
    assert_eq!(stopped.len(), 3, "{actions}");
    for a in stopped {
        assert_eq!(a["state"], "cancelled", "{a}");
        assert_eq!(a["cancel"], "termination_verified", "{a}");
    }
}
