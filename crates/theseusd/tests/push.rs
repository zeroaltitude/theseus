//! The push (theseus-in3, 9b): `executions.watch` against a real daemon, over
//! real turns from the stand-in model. The prove: a turn, a job, a confirm, a
//! task, a wake, a stop, and a cancel, with an all-session watcher. Every
//! `execution.*` ledger row that names a state has an `execution.changed` at
//! its position or later with that state (unless a later row of the same
//! execution came first, in the same frame), and every event has its row. And
//! a question parked before the daemon started is in the seed.

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

/// What the stand-in model calls for a prompt that contains a phrase. A
/// prompt that contains none is answered with text.
fn script(prompt: &str) -> Vec<(&'static str, Value)> {
    let calls: [(&str, Vec<(&'static str, Value)>); 6] = [
        (
            "run the quick job",
            vec![("proc_run", json!({"argv": ["sleep", "2"]}))],
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
            "hold a long job",
            vec![("proc_run", json!({"argv": ["sleep", "30"]}))],
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
    if prompt.contains("Run the slow job.") {
        return vec![("proc_run", json!({"argv": ["sleep", "30"]}))];
    }
    // A task's prompt quotes the message that started it (M5 27): its
    // brief answers with text.
    if prompt.starts_with("[Task ") && prompt.contains("Count to three.") {
        return vec![];
    }
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
        let tools = t
            .get_mut("policy")
            .and_then(|p| p.as_table_mut())
            .unwrap()
            .entry("tools")
            .or_insert_with(|| toml::Value::Table(Default::default()))
            .as_table_mut()
            .unwrap();
        tools.insert("fs.write".into(), "approve".into());
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

    fn execution(&self, sid: &str) -> Value {
        self.call("execution.list", Value::Null).unwrap()["executions"]
            .as_array()
            .unwrap()
            .iter()
            .find(|e| e["session_id"] == sid)
            .cloned()
            .unwrap()
    }

    /// Until `sid`'s execution waits on input, with at least `turns` turns.
    fn until_ready(&self, sid: &str, turns: u64) {
        self.wait(&format!("{sid} ready after {turns} turns"), || {
            let e = self.execution(sid);
            (e["state"] == "waiting"
                && e["wake"]["on"] == "input"
                && e["turns"].as_u64() >= Some(turns))
            .then_some(())
        })
    }

    /// Until a task of `parent` is in `state`, and its id.
    fn task_in(&self, parent: &str, state: &str) -> String {
        self.wait(&format!("a task of {parent} {state}"), || {
            let tasks = self.call("task.list", json!({"session_id": parent})).ok()?;
            tasks["tasks"]
                .as_array()?
                .iter()
                .find(|t| t["state"] == state)
                .and_then(|t| t["task_id"].as_str().map(str::to_string))
        })
    }

    fn open(&self) -> String {
        self.call("session.open", json!({})).unwrap()["session_id"]
            .as_str()
            .unwrap()
            .to_string()
    }
}

/// Each record's frame, as the position of the frame's last record, read
/// from the store's WAL segments while the daemon runs (read-only; a frame
/// still being written at the end is left out).
fn frame_ends(state: &std::path::Path) -> std::collections::BTreeMap<u64, u64> {
    use theseus_store::wal::{list_segments, read_frame, segment_path, FrameRead};
    let dir = state.join("store").join("wal");
    let mut out = std::collections::BTreeMap::new();
    let mut next = 1;
    for seg in list_segments(&dir).unwrap() {
        let bytes = std::fs::read(segment_path(&dir, seg)).unwrap();
        let mut off = 0;
        while let FrameRead::Whole { end, records, .. } = read_frame(&bytes, off, seg, 0, next) {
            let last = records.last().map_or(next - 1, |(r, _)| r.position);
            for (r, _) in &records {
                out.insert(r.position, last);
            }
            next = last + 1;
            off = end;
        }
    }
    out
}

fn tail(s: &str, n: usize) -> String {
    let lines: Vec<&str> = s.lines().collect();
    lines[lines.len().saturating_sub(n)..].join("\n")
}

/// A connection with `executions.watch` on it: every line the daemon sends it,
/// kept by a reader thread.
struct Watcher {
    _sock: UnixStream,
    lines: Arc<Mutex<Vec<Value>>>,
}

impl Watcher {
    fn start(rig: &Rig) -> Self {
        let sock = UnixStream::connect(rig.path("sock")).unwrap();
        let req = json!({"jsonrpc": "2.0", "id": 1, "method": "executions.watch", "params": {}});
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
}

/// The state a ledger row says its execution is in, for the rows that name
/// one: `execution.opened` starts it waiting on input, and a completion's row
/// (`action.succeeded`, …) says the state its frame left the execution in,
/// since a job's result queues a waiting execution with no `execution.queued`
/// row of its own.
fn row_state(row: &Value) -> Option<String> {
    let kind = row["kind"].as_str()?;
    let state = match kind {
        "execution.opened" => "waiting",
        "execution.queued" => "queued",
        "execution.running" => "running",
        "execution.waiting" => "waiting",
        "execution.blocked" => "blocked",
        "execution.failed" => "failed",
        "execution.complete" => "complete",
        "execution.cancelled" => "cancelled",
        k if k.starts_with("action.") => {
            return row["data"]["execution_state"].as_str().map(str::to_string)
        }
        _ => return None,
    };
    Some(state.to_string())
}

#[test]
#[expect(clippy::cognitive_complexity, reason = "shape budget: split it")]
#[expect(clippy::too_many_lines, reason = "shape budget: split it")]
fn every_execution_row_has_its_event_and_every_event_its_row() {
    let r = Rig::new();
    let _d = r.spawn();
    let health = r.call("health", Value::Null).unwrap();
    assert_eq!(
        health["push"]["seeded"], false,
        "no observer until someone watches: {}",
        health["push"]
    );
    let w = Watcher::start(&r);
    let snap = w.snapshot().unwrap();
    assert!(snap["executions"].as_array().unwrap().is_empty());
    let health = r.call("health", Value::Null).unwrap();
    assert_eq!(health["push"]["seeded"], true);
    assert_eq!(health["push"]["watchers"], 1);

    // A turn.
    let a = r.open();
    r.ask(&a, "Say hello.");
    r.until_ready(&a, 1);
    // A job: it outlives proc_sync_secs, so the turn waits on it, and its
    // result starts a continuation.
    r.ask(&a, "Please run the quick job.");
    r.until_ready(&a, 3);
    // A confirm, and its answer.
    let asked = r.ask(&a, "Now write the note.");
    let corr = asked["awaiting_confirm"].as_str().unwrap().to_string();
    r.call(
        "action.confirm",
        json!({"correlation_id": corr, "approve": true, "author": "test"}),
    )
    .unwrap();
    r.until_ready(&a, 5);
    // A task that completes.
    let b = r.open();
    r.ask(&b, "Please start a small task.");
    r.task_in(&b, "complete");
    // A wake.
    r.ask(&b, "Please set a timer.");
    r.wait("the wake fired", || {
        let rows = r
            .call("ledger.tail", json!({"n": 50, "kind": "wake.fired"}))
            .unwrap();
        (!rows["rows"].as_array().unwrap().is_empty()).then_some(())
    });
    r.until_ready(&b, 3);
    // A stop of a running job.
    let c = r.open();
    r.ask(&c, "Please hold a long job.");
    let stopped = r.execution(&c);
    assert_eq!(stopped["wake"]["on"], "actions", "{stopped}");
    r.call(
        "execution.stop",
        json!({"execution_id": stopped["execution_id"], "author": "test"}),
    )
    .unwrap();
    r.until_ready(&c, 1);
    // A cancel of a task whose job runs.
    r.ask(&c, "Please start a slow task.");
    let slow = r.wait("the slow task's job", || {
        let tasks = r.call("task.list", json!({"session_id": c})).ok()?;
        tasks["tasks"]
            .as_array()?
            .iter()
            .find(|t| t["waiting_on"] == "a job")
            .and_then(|t| t["task_id"].as_str().map(str::to_string))
    });
    r.call("task.cancel", json!({"task": slow, "author": "test"}))
        .unwrap();
    r.task_in(&c, "cancelled");

    // Everything has settled once the last row's event has come.
    let mut rows: Vec<(u64, String, String)> = r.call("ledger.tail", json!({"n": 1000})).unwrap()
        ["rows"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|row| {
            let state = row_state(row)?;
            let exec = row["data"]["execution_id"].as_str()?.to_string();
            Some((row["position"].as_u64()?, exec, state))
        })
        .collect();
    rows.sort();
    let last = rows.iter().map(|(p, _, _)| *p).max().unwrap();
    r.wait("the last row's event", || {
        w.notes("execution.changed")
            .iter()
            .any(|v| v["position"].as_u64() >= Some(last))
            .then_some(())
    });
    let events: Vec<(u64, String, String)> = w
        .notes("execution.changed")
        .iter()
        .map(|v| {
            (
                v["position"].as_u64().unwrap(),
                v["execution_id"].as_str().unwrap().to_string(),
                v["state"].as_str().unwrap().to_string(),
            )
        })
        .collect();
    let execs: std::collections::BTreeSet<&str> = rows.iter().map(|(_, e, _)| e.as_str()).collect();
    assert!(
        execs.len() >= 5,
        "three conversations and two tasks: {execs:?}"
    );

    eprintln!(
        "push prove: {} state rows, {} execution.changed, {} executions, {} confirm events",
        rows.len(),
        events.len(),
        execs.len(),
        w.notes("confirm.requested").len() + w.notes("confirm.resolved").len()
    );
    // Every event has its row: the latest row of its execution at or before
    // its position says its state.
    for (p, exec, state) in &events {
        let row = rows
            .iter()
            .filter(|(q, e, _)| e == exec && q <= p)
            .max_by_key(|(q, _, _)| *q);
        assert_eq!(
            row.map(|(_, _, s)| s.as_str()),
            Some(state.as_str()),
            "the event at {p} for {exec} says {state}; the rows say {row:?}"
        );
    }
    // Every row has its event, from its own frame: the event's position is
    // the last of the row's frame, and it says the row's state, unless a
    // later row of that execution in the same frame took its place (two
    // transitions in one frame give one event, with the last state).
    let frames = frame_ends(&r.path("state"));
    for (i, (q, exec, state)) in rows.iter().enumerate() {
        // A row that names the state its execution was in already (a
        // provider call's completion while the turn runs) is no transition.
        let before = rows[..i].iter().rev().find(|(_, e, _)| e == exec);
        if before.is_some_and(|(_, _, s)| s == state) {
            continue;
        }
        let end = frames[q];
        if rows
            .iter()
            .any(|(q2, e, _)| e == exec && q2 > q && frames[q2] == end)
        {
            continue;
        }
        let event = events
            .iter()
            .find(|(p, e, _)| e == exec && *p == end)
            .unwrap_or_else(|| {
                let mine: Vec<_> = events.iter().filter(|(_, e, _)| e == exec).collect();
                let near: Vec<_> = frames.range(q.saturating_sub(4)..end + 4).collect();
                panic!(
                    "no event from the frame ending at {end} for the row at {q} ({exec} {state}); \
                     its events {mine:?}; frames near {near:?}"
                )
            });
        assert_eq!(
            &event.2, state,
            "the row at {q} says {exec} is {state}; its frame's event says {}",
            event.2
        );
    }
    // And every event is a frame's own: its position ends a frame.
    for (p, exec, _) in &events {
        assert_eq!(
            frames.get(p),
            Some(p),
            "the event at {p} for {exec} ends no frame"
        );
    }
    // Positions only grow per execution.
    for exec in &execs {
        let ps: Vec<u64> = events
            .iter()
            .filter(|(_, e, _)| e == exec)
            .map(|(p, _, _)| *p)
            .collect();
        assert!(ps.windows(2).all(|w| w[0] < w[1]), "{exec}: {ps:?}");
    }
    // No event shows a question but the one asked: a call run at once is
    // planned, authorized, and dispatched in one frame, and is never one.
    for v in w.notes("execution.changed") {
        for p in v["pending"].as_array().unwrap() {
            assert_eq!(
                p["correlation_id"], corr,
                "a question that was not asked: {v}"
            );
        }
        if v["attention"]["level"] == "needs_you" {
            assert_eq!(v["session_id"], a, "only the confirm needs you: {v}");
        }
    }
    // The questions came on the all-session watch too.
    let requested = w.notes("confirm.requested");
    let resolved = w.notes("confirm.resolved");
    assert!(requested.iter().any(|c| c["correlation_id"] == corr));
    assert!(resolved
        .iter()
        .any(|c| c["correlation_id"] == corr && c["approved"] == true));
    // The task's view names its parent session.
    let task_views: Vec<Value> = w
        .notes("execution.changed")
        .into_iter()
        .filter(|v| v["kind"] == "task")
        .collect();
    assert!(task_views.iter().any(|v| v["parent_session_id"] == b));
    let health = r.call("health", Value::Null).unwrap();
    assert!(
        health["push"]["events"].as_u64().unwrap() >= events.len() as u64,
        "{}",
        health["push"]
    );
}

/// A question parked before the daemon started is in the seed: the first
/// `executions.watch` after a restart shows its execution needing you, with
/// its tool and the gate's reason, and lists the question.
#[test]
fn a_question_parked_before_the_start_is_in_the_seed() {
    let r = Rig::new();
    let d = r.spawn();
    let a = r.open();
    let asked = r.ask(&a, "Now write the note.");
    let corr = asked["awaiting_confirm"].as_str().unwrap().to_string();
    r.call("shutdown", Value::Null).unwrap();
    drop(d);
    let _d = r.spawn();
    let w = Watcher::start(&r);
    let snap = w.snapshot().unwrap();
    let view = snap["executions"]
        .as_array()
        .unwrap()
        .iter()
        .find(|v| v["session_id"] == a)
        .cloned()
        .unwrap_or_else(|| panic!("{a} not in the seed: {snap}"));
    assert_eq!(view["attention"]["level"], "needs_you", "{view}");
    let label = view["attention"]["label"].as_str().unwrap();
    assert!(label.starts_with("confirm fs.write: "), "{label}");
    assert_eq!(view["pending"][0]["correlation_id"], corr);
    assert_eq!(snap["confirms"][0]["correlation_id"], corr);
    assert!(view["position"].as_u64().unwrap() > 0);
    assert!(snap["position"].as_u64() >= view["position"].as_u64());
}
