//! `owner_ask` against a real daemon (theseus-j7cje): a task started by the
//! stand-in model asks the owner which plan to take, the question waits on the
//! board and in `confirm.list`, the real `theseus answer <id> 2` answers it by
//! number, and the task resumes with the option it names, reads it back in its
//! result, and ends. The task was started with `notify_done`.

mod common;

use common::{table, Fixture, FAKE_OP};

use std::path::PathBuf;

use common::model::FakeModel;
use serde_json::{json, Value};

/// What the stand-in model calls for a prompt: a conversation that starts a
/// task, and the task, which asks the owner which plan to take.
fn script(prompt: &str) -> Vec<(&'static str, Value)> {
    if prompt.starts_with("[Task ") && prompt.contains("Decide the plan.") {
        return vec![(
            "owner_ask",
            json!({"question": "Which plan for the harbour survey?",
                   "options": [{"label": "Careful"}, {"label": "Ship it behind the flag"}],
                   "default": 1}),
        )];
    }
    if prompt.contains("start a deciding task") {
        return vec![(
            "task_create",
            json!({"brief": "Decide the plan.", "notify_done": true, "arrangement":
                   {"pieces": [{"quote": "Please start a deciding task.", "role": "objective"}]}}),
        )];
    }
    vec![]
}

struct Rig {
    dir: tempfile::TempDir,
    _model: FakeModel,
}

impl Fixture for Rig {
    fn dir(&self) -> &std::path::Path {
        self.dir.path()
    }
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

/// The task's question reaches `confirm.list` as an `owner_ask` with its
/// options, `theseus answer <id> 2` answers it, and the task ends having read
/// "The owner chose 2".
#[test]
fn a_task_asks_and_the_cli_answers_by_number() {
    let r = Rig::new();
    let _d = r.spawn();
    let parent = r.open();
    r.ask(&parent, "Please start a deciding task.");
    let q = r.wait("the task's question", || {
        let l = r.call("confirm.list", Value::Null).ok()?;
        l["confirms"].as_array()?.first().cloned()
    });
    assert_eq!(q["tool"], "owner_ask", "{q}");
    let ids: Vec<&str> = q["options"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|o| o["id"].as_str())
        .collect();
    assert_eq!(ids, ["choice:0", "choice:1", "choice:other"], "{q}");
    let corr = q["correlation_id"].as_str().unwrap().to_string();
    let task = q["session_id"].as_str().unwrap().to_string();
    match r.cli(&["answer", &corr, "2"]) {
        Some(out) => assert!(
            out.status.success(),
            "theseus answer: {}",
            String::from_utf8_lossy(&out.stderr)
        ),
        None => {
            eprintln!("no theseus binary beside theseusd: answering over the socket");
            r.call(
                "work.answer",
                json!({"question_id": corr, "option": "2", "author": "test"}),
            )
            .unwrap();
        }
    }
    r.wait("the task ends", || {
        let e = r.execution(&task);
        (e["state"] == "complete").then_some(())
    });
    let history = r
        .call("session.history", json!({"session_id": task}))
        .unwrap();
    let read = history["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|n| n["kind"] == "tool_result")
        .map(|n| n["text"].clone());
    assert_eq!(
        read,
        Some(json!("The owner chose 2, \"Ship it behind the flag\".")),
        "the task read the owner's choice: {history}"
    );
}
