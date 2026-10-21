//! A job a restart found alive has its short id (theseus-53ujg.8), with the
//! real `theseusd` and a stand-in for the Messages API: a blocking `proc_run`
//! cut by a SIGKILL is answered by the restarted daemon's continuation with
//! the job's handle in its words and meta, so `job_read` lists it, `job_wait`
//! takes it, and a second restart reads the same handle back.

mod common;

use common::rpc::call;
use common::{table, Fixture, FAKE_OP};

use std::path::PathBuf;

use common::model::FakeModel;
use serde_json::{json, Value};

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
    /// `Run the job` gets a blocking `proc.run` that marks its start in the
    /// projects dir and waits there for a `release` file; `Read the jobs` and
    /// `Wait for it` get the job tools; anything else gets text.
    fn new(proc_sync_secs: i64) -> Self {
        let model = FakeModel::start(move |prompt| match prompt {
            "Run the job" => {
                let script = "touch started; i=0; while [ ! -e release ] && [ $i -lt 600 ]; do sleep 0.1; i=$((i+1)); done; echo the job is done";
                vec![("proc_run", json!({"argv": ["sh", "-c", script]}))]
            }
            "Read the jobs" => vec![("job_read", json!({}))],
            "Wait for it" => vec![("job_wait", json!({"job": "j1"}))],
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
        table(&mut t, "tools").insert("proc_sync_secs".into(), proc_sync_secs.into());
        std::fs::write(path("config.toml"), toml::to_string(&t).unwrap()).unwrap();
        Self { dir, model }
    }

    fn rows_ended(&self) -> usize {
        self.rows("turn.ended").len()
    }

    fn rows(&self, kind: &str) -> Vec<Value> {
        self.call("ledger.tail", json!({"n": 500, "kind": kind}))
            .unwrap()["rows"]
            .as_array()
            .cloned()
            .unwrap_or_default()
    }

    fn turn(&self, session: Option<&str>, input: &str) -> Value {
        let mut p = json!({"input": input, "author": "test", "attachments": []});
        if let Some(s) = session {
            p["session_id"] = json!(s);
        }
        self.call("turn.submit", p).unwrap()
    }

    /// The text of the last tool result the stand-in was sent.
    fn last_result(&self) -> String {
        let requests = self.model.requests();
        let last = requests.iter().rev().find_map(|r| {
            let m = r["messages"].as_array()?.last()?;
            m["content"]
                .as_array()?
                .iter()
                .find(|b| b["type"] == "tool_result")
                .cloned()
        });
        match &last.expect("a tool result was sent")["content"] {
            Value::String(s) => s.clone(),
            Value::Array(parts) => parts.iter().filter_map(|p| p["text"].as_str()).collect(),
            other => other.to_string(),
        }
    }

    /// The meta of the `proc.run` result the continuation wrote, once it has.
    fn placeholder_meta(&self, session: &str) -> Value {
        self.wait("the restart's placeholder", || {
            let nodes = self
                .call(
                    "node.list",
                    json!({"session_id": session, "kind": "tool_result"}),
                )
                .ok()?;
            nodes["nodes"].as_array()?.iter().find_map(|n| {
                n["text"]
                    .as_str()?
                    .contains("the harness restarted meanwhile")
                    .then(|| n["detail"]["meta"].clone())
            })
        })
    }
}

/// A turn on a blocking job, a SIGKILL while it blocks, a new daemon: its
/// session, and the job's file `release` for the test to let it end.
fn cut_job(r: &Rig) -> (String, common::Daemon) {
    let daemon = r.spawn();
    let sock = r.path("sock");
    let asking = std::thread::spawn(move || {
        call(
            &sock,
            "turn.submit",
            json!({"input": "Run the job", "author": "test", "attachments": []}),
        )
    });
    r.wait("the job's start", || {
        r.path("projects/started").exists().then_some(())
    });
    let sid = r.wait("its session", || {
        r.call("session.list", json!({})).ok()?["sessions"][0]["session_id"]
            .as_str()
            .map(String::from)
    });
    drop(daemon);
    assert!(asking.join().unwrap().is_err());
    (sid, r.spawn())
}

/// The key test: the restarted daemon found the job alive, and says which job
/// it is.
#[test]
fn a_job_found_alive_after_a_restart_has_its_handle() {
    let r = Rig::new(600);
    let (sid, _daemon) = cut_job(&r);
    let meta = r.placeholder_meta(&sid);
    assert_eq!(meta["job"], "j1", "{meta}");
    assert!(meta["pid"].as_u64().is_some_and(|p| p > 1), "{meta}");
    r.wait("the model's next request", || {
        r.model
            .requests()
            .iter()
            .any(|q| {
                let m = q["messages"].to_string();
                m.contains("the harness restarted meanwhile") && m.contains("job j1")
            })
            .then_some(())
    });
    r.wait("the continuation's end", || {
        (r.rows_ended() >= 1).then_some(())
    });
    r.turn(Some(&sid), "Read the jobs");
    let read = r.last_result();
    assert!(read.contains("j1"), "job_read with no job lists it: {read}");
    let sock = r.path("sock");
    let waiting = std::thread::spawn(move || {
        call(
            &sock,
            "turn.submit",
            json!({"input": "Wait for it", "session_id": sid, "author": "test", "attachments": []}),
        )
    });
    std::thread::sleep(std::time::Duration::from_millis(500));
    std::fs::write(r.path("projects/release"), "").unwrap();
    waiting.join().unwrap().unwrap();
    assert!(
        r.last_result().contains("the job is done"),
        "{}",
        r.last_result()
    );
}

/// The handle is read back from the placeholder's meta after another restart.
#[test]
fn an_adopted_jobs_handle_survives_a_second_restart() {
    let r = Rig::new(600);
    let (sid, daemon) = cut_job(&r);
    assert_eq!(r.placeholder_meta(&sid)["job"], "j1");
    r.wait("the continuation's end", || {
        (r.rows_ended() >= 1).then_some(())
    });
    drop(daemon);
    let _again = r.spawn();
    r.turn(Some(&sid), "Read the jobs");
    assert!(r.last_result().contains("j1"), "{}", r.last_result());
    std::fs::write(r.path("projects/release"), "").unwrap();
}
