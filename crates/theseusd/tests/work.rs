//! The work board against a real daemon (theseus-hkx30): `work.watch` over real
//! turns from the stand-in model, and the real `theseus` binary beside it. A
//! conversation with a job and a task that holds one, another that asks a
//! question and is answered, one whose task ends and whose wake fires: the
//! tree the board holds, `theseus work`'s lines, and `theseus status`'s counts
//! say the same. And a connection that closes stops being a watcher.

mod common;

use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::{Arc, Mutex};
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

/// What the stand-in model calls for a prompt that contains a phrase.
fn script(prompt: &str) -> Vec<(&'static str, Value)> {
    if prompt.contains("Run the slow job.") {
        return vec![("proc_run", json!({"argv": ["sleep", "30"]}))];
    }
    if prompt.starts_with("[Task ") && prompt.contains("Count to three.") {
        return vec![];
    }
    let calls: [(&str, Vec<(&'static str, Value)>); 5] = [
        (
            "hold a long job",
            vec![("proc_run", json!({"argv": ["sleep", "30"]}))],
        ),
        (
            "write the note",
            vec![("fs_write", json!({"path": "note.txt", "content": "kept\n"}))],
        ),
        (
            "start a small task",
            vec![(
                "task_create",
                json!({"brief": "Count to three.", "arrangement":
                       {"pieces": [{"quote": "Please start a small task.", "role": "objective"}]}}),
            )],
        ),
        (
            "set a timer",
            vec![("wake_at", json!({"after": "1s", "note": "tick-tock"}))],
        ),
        (
            "start a slow task",
            vec![(
                "task_create",
                json!({"brief": "Run the slow job.", "arrangement":
                       {"pieces": [{"quote": "Please start a slow task.", "role": "objective"}]}}),
            )],
        ),
    ];
    calls
        .into_iter()
        .find(|(p, _)| prompt.contains(p))
        .map(|(_, c)| c)
        .unwrap_or_default()
}

struct Rig {
    dir: tempfile::TempDir,
    _model: FakeModel,
}

impl Rig {
    fn new() -> Self {
        let model = FakeModel::start(script);
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
        table(&mut t, "policy").insert("enforcement".into(), "open".into());
        table(&mut t, "policy")
            .entry("tools")
            .or_insert_with(|| toml::Value::Table(Default::default()))
            .as_table_mut()
            .unwrap()
            .insert("fs.write".into(), "approve".into());
        table(&mut t, "tools").insert("proc_sync_secs".into(), 1.into());
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
            assert!(
                t0.elapsed() < Duration::from_secs(40),
                "no {what} in 40 s; the daemon's log ends:\n{}",
                tail(&self.log(), 30)
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

    fn ask(&self, sid: &str, prompt: &str) -> Value {
        self.call(
            "turn.submit",
            json!({"session_id": sid, "input": prompt, "author": "test", "attachments": []}),
        )
        .unwrap()
    }

    fn open(&self) -> String {
        self.call("session.open", json!({})).unwrap()["session_id"]
            .as_str()
            .unwrap()
            .to_string()
    }

    fn execution(&self, sid: &str) -> Value {
        self.call("execution.list", Value::Null).unwrap()["executions"]
            .as_array()
            .unwrap()
            .iter()
            .find(|e| e["session_id"] == sid)
            .cloned()
            .unwrap()
    }

    fn until_ready(&self, sid: &str, turns: u64) {
        self.wait(&format!("{sid} ready after {turns} turns"), || {
            let e = self.execution(sid);
            (e["state"] == "waiting"
                && e["wake"]["on"] == "input"
                && e["turns"].as_u64() >= Some(turns))
            .then_some(())
        })
    }

    /// The real `theseus` binary beside the daemon's, on this daemon.
    fn cli(&self, args: &[&str]) -> Option<std::process::Output> {
        let bin = PathBuf::from(env!("CARGO_BIN_EXE_theseusd")).with_file_name("theseus");
        bin.exists().then(|| {
            std::process::Command::new(bin)
                .arg("--socket")
                .arg(self.path("sock"))
                .args(args)
                .env_remove("THESEUS_SESSION")
                .output()
                .expect("running theseus")
        })
    }
}

fn tail(s: &str, n: usize) -> String {
    let lines: Vec<&str> = s.lines().collect();
    lines[lines.len().saturating_sub(n)..].join("\n")
}

/// A connection with `work.watch` on it: every line the daemon sends it, kept
/// by a reader thread.
struct Watcher {
    _sock: UnixStream,
    lines: Arc<Mutex<Vec<Value>>>,
}

impl Watcher {
    fn start(rig: &Rig) -> Self {
        let sock = UnixStream::connect(rig.path("sock")).unwrap();
        let req = json!({"jsonrpc": "2.0", "id": 1, "method": "work.watch",
                         "params": {"interactive": true, "limit": 100}});
        (&sock).write_all(format!("{req}\n").as_bytes()).unwrap();
        let lines: Arc<Mutex<Vec<Value>>> = Arc::default();
        let into = lines.clone();
        let read = sock.try_clone().unwrap();
        std::thread::spawn(move || {
            for line in BufReader::new(read).lines() {
                let Ok(line) = line else { break };
                if let Ok(v) = serde_json::from_str::<Value>(&line) {
                    into.lock().unwrap().push(v);
                }
            }
        });
        let w = Self { _sock: sock, lines };
        rig.wait("the snapshot", || w.snapshot());
        w
    }

    fn snapshot(&self) -> Option<Value> {
        self.lines
            .lock()
            .unwrap()
            .iter()
            .find(|v| v["id"] == 1)
            .map(|v| v["result"].clone())
    }

    fn notes(&self, method: &str) -> Vec<Value> {
        self.lines
            .lock()
            .unwrap()
            .iter()
            .filter(|v| v["method"] == method)
            .map(|v| v["params"].clone())
            .collect()
    }

    /// What a client holds by the position rule: each view with the greatest
    /// position, from the snapshot or an event, less what left.
    fn held(&self) -> std::collections::BTreeMap<String, Value> {
        let mut out: std::collections::BTreeMap<String, Value> = Default::default();
        fn apply(out: &mut std::collections::BTreeMap<String, Value>, v: &Value) {
            let id = v["id"].as_str().unwrap().to_string();
            if out
                .get(&id)
                .is_none_or(|o| v["position"].as_u64() > o["position"].as_u64())
            {
                out.insert(id, v.clone());
            }
        }
        for v in self.snapshot().unwrap()["views"].as_array().unwrap() {
            apply(&mut out, v);
        }
        for c in self.notes("work.changed") {
            for v in c["views"].as_array().unwrap() {
                apply(&mut out, v);
            }
            for g in c["gone"].as_array().unwrap_or(&vec![]) {
                out.remove(g.as_str().unwrap());
            }
        }
        out
    }
}

/// The board against a real daemon and the real CLI.
#[test]
#[expect(clippy::too_many_lines, reason = "shape budget: split it")]
fn the_work_board_and_the_cli_say_one_thing() {
    let r = Rig::new();
    let _d = r.spawn();
    let health = r.call("health", Value::Null).unwrap();
    assert_eq!(health["push"]["work_watchers"], json!(0));
    let w = Watcher::start(&r);
    assert!(w.snapshot().unwrap()["views"]
        .as_array()
        .unwrap()
        .is_empty());
    let health = r.call("health", Value::Null).unwrap();
    assert_eq!(
        (
            health["push"]["work_watchers"].as_u64(),
            health["push"]["interactive"].as_u64()
        ),
        (Some(1), Some(1)),
        "{}",
        health["push"]
    );

    // A conversation with a job that goes on, and a slow task that holds one.
    let c = r.open();
    r.ask(&c, "Please hold a long job.");
    r.ask(&c, "Please start a slow task.");
    // A conversation whose task ends, and whose wake fires.
    let b = r.open();
    r.ask(&b, "Please start a small task.");
    r.ask(&b, "Please set a timer.");
    r.until_ready(&b, 3);
    // A conversation with a question.
    let a = r.open();
    let asked = r.ask(&a, "Now write the note.");
    let corr = asked["awaiting_confirm"].as_str().unwrap().to_string();

    // The board: three roots, a job under each of C's executions, a question.
    r.wait("the board holds it all", || {
        let held = w.held();
        let of = |k: &str| held.values().filter(|v| v["kind"] == k).count();
        (of("conversation") == 3
            && of("task") == 2
            && of("job") >= 2
            && of("question") == 1
            && of("wake") == 1)
            .then_some(())
    });
    let held = w.held();
    for v in held.values().filter(|v| v["kind"] != "conversation") {
        assert!(
            held.contains_key(v["parent"].as_str().unwrap()),
            "{} hangs from a view the board holds",
            v["title"]
        );
    }
    let question = held.values().find(|v| v["kind"] == "question").unwrap();
    assert_eq!(
        question["id"],
        json!(corr),
        "a question's id is its confirm's"
    );
    let titles: Vec<&str> = held.values().filter_map(|v| v["title"].as_str()).collect();
    assert!(
        titles.contains(&"sleep 30"),
        "a job is titled by its command: {titles:?}"
    );

    // `theseus status` counts what the board shows needs you and works.
    if let Some(out) = r.cli(&["--json", "status"]) {
        let status: Value = serde_json::from_slice(&out.stdout).unwrap();
        let count = |level: &str| {
            held.values()
                .filter(|v| matches!(v["kind"].as_str(), Some("conversation" | "task")))
                .filter(|v| v["attention"]["level"] == level)
                .count() as u64
        };
        assert_eq!(
            status["summary"]["needs_you"].as_u64(),
            Some(count("needs_you")),
            "{status}"
        );
        assert_eq!(
            status["summary"]["working"].as_u64(),
            Some(count("working")),
            "{status}"
        );
        // And `theseus work` draws the tree.
        let tree = r.cli(&["work"]).unwrap();
        let text = String::from_utf8_lossy(&tree.stdout).into_owned();
        assert!(text.contains("sleep 30"), "{text}");
        assert!(text.contains("confirm fs.write"), "{text}");
        assert!(text.contains("(1 need you"), "the rollup is drawn: {text}");
        eprintln!("theseus work:\n{text}");
    } else {
        eprintln!("no theseus binary beside theseusd: the CLI half is skipped");
    }

    // The answer ends the question; the events and the snapshot agree.
    r.call(
        "action.confirm",
        json!({"correlation_id": corr, "approve": true, "author": "test"}),
    )
    .unwrap();
    r.wait("the question ends", || {
        w.held()
            .get(&corr)
            .is_some_and(|v| v["state"] == "done" || v["state"] == "cancelled")
            .then_some(())
    });
    let retracted = w
        .notes("work.notice")
        .iter()
        .any(|n| n["why"] == "answered" && n["retracts"] == json!(corr));
    assert!(
        retracted,
        "the answer retracts the ping: {:?}",
        w.notes("work.notice")
    );

    // A fresh client's snapshot is what this one holds, by the position rule.
    let again = r.call("work.watch", json!({"limit": 100})).unwrap();
    let fresh: std::collections::BTreeMap<String, u64> = again["views"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| {
            (
                v["id"].as_str().unwrap().to_string(),
                v["position"].as_u64().unwrap(),
            )
        })
        .collect();
    let held = w.held();
    for (id, position) in &fresh {
        assert!(
            held.get(id)
                .is_some_and(|v| v["position"].as_u64() >= Some(*position)),
            "{id} at {position} is in what the watcher holds"
        );
    }

    // The watcher goes: the count goes with it.
    // The reader thread holds a copy of the socket: shut it down.
    w._sock.shutdown(std::net::Shutdown::Both).unwrap();
    drop(w);
    r.wait("no watcher", || {
        let h = r.call("health", Value::Null).ok()?;
        (h["push"]["work_watchers"] == json!(0)).then_some(())
    });
}
