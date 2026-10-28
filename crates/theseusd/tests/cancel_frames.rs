//! A cancel's frames, with the real `theseusd` and a real job (theseus-dwoj).
//! A frame costs one `fdatasync`, so on a busy disk a cancel's round trip is
//! mostly its frames. A turn starts a `proc.run` job that outlives the turn,
//! and `execution.cancel` ends it; the frames written from the request to its
//! answer are read from the WAL, record by record, as the turn bench reads a
//! turn's (theseus-sim's `walcount`). Two: the cancel's own (the execution
//! ended, the job's action `requested`, written before anything is asked of
//! the job), then, once the wrapper has answered, the job's acknowledgement,
//! its verdict, the verdict's fact, and the call's answer in the transcript.
//! This is the daemon's cancel path (`cancel_execution_judged`); the core's
//! test rig runs its jobs on a thread and stops them through the task path.

mod common;

use common::{table, Fixture, FAKE_OP};

use std::os::unix::fs::FileExt;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::{Duration, Instant};

use common::model::FakeModel;
use common::Daemon;
use serde_json::{json, Value};
use theseus_store::wal::{list_segments, read_frame, segment_path, FrameRead};
use theseus_store::{kinds, Record};

/// The frames a cancel of one running job writes, each as its records'
/// kinds in order (a ledger row by its own kind). In the second, the
/// acknowledgement's row (`action.cancel`, `acknowledged`) comes first; its
/// action record is the verdict's, since a transaction keeps one record of
/// a key, its last.
const CANCEL_FRAMES: &[&[&str]] = &[
    &["execution", "action", "ledger:execution.cancelled"],
    &[
        "ledger:action.cancel",
        "execution",
        "action",
        "ledger:action.cancel",
        "ledger:action.cancel_verified",
        "node",
    ],
];

struct Rig {
    dir: tempfile::TempDir,
    _model: FakeModel,
    daemon: Daemon,
}

impl Fixture for Rig {
    fn dir(&self) -> &std::path::Path {
        self.dir.path()
    }
}

impl Rig {
    fn start() -> Self {
        let model = FakeModel::start(|prompt| {
            let argv = if prompt.contains("Start the long job") {
                json!(["sleep", "60"])
            } else {
                return vec![];
            };
            vec![("proc_run", json!({"argv": argv, "timeout_secs": 120}))]
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
        table(&mut t, "tools").insert("proc_sync_secs".into(), 1.into());
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
        let r = Self {
            dir,
            _model: model,
            daemon,
        };
        r.wait("the secrets", || {
            r.call("health", Value::Null)
                .ok()
                .filter(|h| h["secrets"]["state"] == "ready")
        });
        r
    }

    fn call(&self, method: &str, params: Value) -> Result<Value, String> {
        common::rpc::call_text(&self.sock(), method, params)
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
                std::fs::read_to_string(self.path("theseusd.log")).unwrap_or_default()
            );
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    /// A turn in a new session that leaves its job running, and the WAL's
    /// frames once the log is still: the execution and how many frames
    /// were written before the cancel.
    fn start_job(&self, prompt: &str) -> (String, usize) {
        let s = self.call("session.open", json!({"label": prompt})).unwrap();
        let res = self
            .call(
                "turn.submit",
                json!({"session_id": s["session_id"], "input": prompt, "author": "test", "attachments": []}),
            )
            .unwrap();
        let exec = res["execution_id"].as_str().unwrap().to_string();
        self.wait("the job dispatched", || {
            (self.dispatched() == 1).then_some(())
        });
        // What a turn's end leaves to write (its deferred rows) is written
        // before the cancel: wait for the log to be still.
        let wal = self.path("state/store/wal");
        let mut before = frames(&wal).len();
        self.wait("the log still", || {
            std::thread::sleep(Duration::from_millis(200));
            let now = frames(&wal).len();
            let still = now == before;
            before = now;
            still.then_some(())
        });
        (exec, before)
    }

    fn dispatched(&self) -> u64 {
        self.call("health", Value::Null).unwrap()["kernel"]["actions_by_state"]["dispatched"]
            .as_u64()
            .unwrap_or(0)
    }
}

impl Drop for Rig {
    fn drop(&mut self) {
        let _ = self.call("shutdown", Value::Null);
        let t0 = Instant::now();
        while self.daemon.try_wait().is_none() && t0.elapsed() < Duration::from_secs(5) {
            std::thread::sleep(Duration::from_millis(10));
        }
    }
}

/// Every whole frame in the WAL at `dir`, each as its records' kinds.
fn frames(dir: &Path) -> Vec<Vec<String>> {
    let mut out = Vec::new();
    let mut position = 1;
    for seg in list_segments(dir).unwrap() {
        let file = std::fs::File::open(segment_path(dir, seg)).unwrap();
        let mut bytes = vec![0u8; file.metadata().unwrap().len() as usize];
        file.read_exact_at(&mut bytes, 0).unwrap();
        let mut i = 0;
        while i < bytes.len() {
            match read_frame(&bytes, i, seg, 0, position) {
                FrameRead::Whole { end, records, .. } => {
                    if let Some((last, _)) = records.last() {
                        position = last.position + 1;
                    }
                    out.push(records.iter().map(|(r, _)| label(r)).collect());
                    i = end;
                }
                _ => break,
            }
        }
    }
    out
}

fn label(r: &Record) -> String {
    let name = kinds::name(r.kind);
    if r.kind == kinds::LEDGER {
        if let Some(k) = serde_json::from_slice::<Value>(&r.payload)
            .ok()
            .and_then(|v| v["kind"].as_str().map(str::to_string))
        {
            return format!("{name}:{k}");
        }
    }
    name.to_string()
}

/// The frames as text, one a line.
fn lines(frames: &[Vec<String>]) -> String {
    frames
        .iter()
        .map(|f| format!("[{}]", f.join(", ")))
        .collect::<Vec<_>>()
        .join("\n")
}

/// A cancel of a running job writes two frames: its own, before the job is
/// asked, and one with the job's last steps, its fact and its answer.
#[test]
fn a_cancel_of_a_running_job_writes_two_frames() {
    let r = Rig::start();
    let (exec, before) = r.start_job("Start the long job.");
    let c = r
        .call("execution.cancel", json!({"execution_id": exec}))
        .unwrap();
    assert_eq!(
        c["verdicts"][0]["state"], "termination_verified",
        "the job's tree was stopped: {c}"
    );
    let written: Vec<Vec<String>> = frames(&r.path("state/store/wal"))[before..].to_vec();
    let want: Vec<Vec<String>> = CANCEL_FRAMES
        .iter()
        .map(|f| f.iter().map(|s| s.to_string()).collect())
        .collect();
    assert_eq!(
        written,
        want,
        "the cancel's frames, record by record:\n{}",
        lines(&written)
    );
}
