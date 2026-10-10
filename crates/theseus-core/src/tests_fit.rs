//! Calls that fit the tools (theseus-9dt2) through the whole core: the names
//! and shapes a model reaches for run as the call it meant, the gate judges
//! `command` as the argv it runs, an input that did not parse says what shape
//! to send, and a `proc.run` result says where it ran.

use std::path::PathBuf;
use std::sync::Arc;

use serde_json::{json, Value};
use theseus_protocol::{SessionKind, TurnSubmitResult};

use crate::bus::EventSink;
use crate::node::{Body, ResultStatus};
use crate::policy::Posture;
use crate::provider::{FakeProvider, Scripted};
use crate::session::SessionRecord;
use crate::store::Store;
use crate::turn::TurnRequest;
use crate::{Config, Core};

struct Rig {
    core: Arc<Core>,
    root: PathBuf,
    _dir: tempfile::TempDir,
}

fn rig(script: Vec<Scripted>, tweak: impl FnOnce(&mut Config)) -> Rig {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("work");
    std::fs::create_dir_all(root.join("sub")).unwrap();
    let root = root.canonicalize().unwrap();
    let mut cfg = Config::example();
    cfg.server.state_dir = dir.path().join("state").to_string_lossy().into_owned();
    cfg.tools.projects_dir = Some(root.to_string_lossy().into_owned());
    cfg.tools.roots = vec![];
    cfg.tools.proc_sync_secs = 10;
    cfg.policy.enforcement = Posture::Notify;
    cfg.policy.tools.remove("proc.run");
    tweak(&mut cfg);
    let store = Store::open(&dir.path().join("store")).unwrap();
    let fake = Arc::new(FakeProvider::scripted(script));
    let mut p = crate::rpc::Parts::for_tests(cfg, fake, store);
    p.launcher = Arc::new(crate::toolrun::InlineLauncher);
    Rig {
        core: Core::build(p).unwrap(),
        root,
        _dir: dir,
    }
}

fn call(name: &str, input: Value) -> Scripted {
    Scripted::tools("", &[("t1", name, input)])
}

impl Rig {
    async fn turn(&self) -> TurnSubmitResult {
        let rec = SessionRecord::new(SessionKind::Conversation, None);
        self.core.store.put_session(&rec.session_id, &rec).unwrap();
        let (live, _) = self.core.live_profile();
        let target = self
            .core
            .runner
            .resolve_target(&live, None, None, None)
            .unwrap();
        let sink = EventSink::new(self.core.bus.clone(), &rec.session_id, None);
        self.core
            .runner
            .run(TurnRequest {
                prompt: None,
                session: rec,
                input: Some("go".into()),
                target,
                sink,
                author: "test".into(),
                recompile: None,
                attachments: vec![],
                arrived: None,
                reply_to: None,
            })
            .await
            .unwrap()
    }

    fn results(&self, sid: &str) -> Vec<(ResultStatus, String)> {
        self.core
            .store
            .session_nodes(sid)
            .unwrap()
            .into_iter()
            .filter_map(|(_, n)| match &n.body {
                Body::ToolResult {
                    status, content, ..
                } => Some((*status, content.clone())),
                _ => None,
            })
            .collect()
    }

    /// The stored call: its tool, and its input as the gate saw it.
    fn stored_call(&self, sid: &str) -> (String, Value) {
        self.core
            .store
            .session_nodes(sid)
            .unwrap()
            .into_iter()
            .find_map(|(_, n)| match &n.body {
                Body::ToolCall { tool, input, .. } => Some((tool.clone(), input.clone())),
                _ => None,
            })
            .expect("a tool call")
    }

    /// The call's gate: its posture, whether the floor asked, and its reason.
    fn gate(&self, sid: &str) -> (String, bool, String) {
        self.core
            .store
            .session_nodes(sid)
            .unwrap()
            .into_iter()
            .find_map(|(_, n)| match &n.body {
                Body::ToolCall { gate: Some(g), .. } => {
                    let d = g.decision.clone().unwrap_or_default();
                    Some((d.posture.unwrap_or_default(), d.floor, d.reason))
                }
                _ => None,
            })
            .expect("a tool call with its gate")
    }

    fn dir(&self) -> String {
        self.core.tools.ctx.cwd.display().to_string()
    }
}

/// An unknown `bash` or `shell` with a `command` is `proc_run {command}`: its
/// result says so first, then where it ran, then its output; the call the
/// gate and the record hold is the `["bash", "-c", …]` it ran as.
#[tokio::test]
async fn a_bash_or_shell_call_runs_as_proc_run_and_its_result_says_so_and_where() {
    for name in ["bash", "shell"] {
        let r = rig(
            vec![
                call(name, json!({"command": "pwd && ls"})),
                Scripted::text("ok"),
            ],
            |_| {},
        );
        let res = r.turn().await;
        let rs = r.results(&res.session_id);
        assert_eq!(rs.len(), 1, "{rs:?}");
        assert_eq!(rs[0].0, ResultStatus::Ok, "{}", rs[0].1);
        let head = format!(
            "[ran as proc_run {{command}}: there is no {name} tool]\n[exit code 0 · in {}]\n",
            r.dir()
        );
        assert!(rs[0].1.starts_with(&head), "{}", rs[0].1);
        assert!(
            rs[0].1.contains("sub"),
            "ls lists the directory: {}",
            rs[0].1
        );
        let (tool, input) = r.stored_call(&res.session_id);
        assert_eq!(tool, "proc.run");
        assert_eq!(input, json!({"argv": ["bash", "-c", "pwd && ls"]}));
    }
}

/// `proc_run {command}` runs under bash in the directory, and says where; a
/// `cwd` moves it, and a batch says where its last step ran.
#[tokio::test]
async fn proc_runs_command_runs_under_bash_and_the_first_line_names_the_directory() {
    let r = rig(
        vec![
            call("proc_run", json!({"command": "echo $((1+1))"})),
            Scripted::text("ok"),
        ],
        |_| {},
    );
    let res = r.turn().await;
    let rs = r.results(&res.session_id);
    assert_eq!(
        rs[0].1,
        format!("[exit code 0 · in {}]\n2\n", r.dir()),
        "bash ran it"
    );
    let r = rig(
        vec![
            call("proc_run", json!({"command": "pwd", "cwd": "sub"})),
            Scripted::text("ok"),
        ],
        |_| {},
    );
    let res = r.turn().await;
    let rs = r.results(&res.session_id);
    let sub = r.root.join("sub");
    assert!(
        rs[0]
            .1
            .starts_with(&format!("[exit code 0 · in {}]\n", sub.display())),
        "{}",
        rs[0].1
    );
    let r = rig(
        vec![
            call(
                "proc_run",
                json!({"steps": [{"argv": ["true"]}, {"argv": ["pwd"], "cwd": "sub"}]}),
            ),
            Scripted::text("ok"),
        ],
        |_| {},
    );
    let res = r.turn().await;
    let text = &r.results(&res.session_id)[0].1;
    let sub = r.root.join("sub");
    assert!(
        text.contains(&format!("[exit code 0 · in {}]", sub.display())),
        "the last step's directory: {text}"
    );
}

/// A call gives exactly one of `argv`, `steps` and `command`: two, or none,
/// is an input error naming the three.
#[tokio::test]
async fn two_of_argv_steps_and_command_or_none_is_an_error_naming_the_three() {
    for input in [
        json!({"command": "ls", "argv": ["ls"]}),
        json!({"command": "ls", "steps": [{"argv": ["ls"]}]}),
        json!({"cwd": "sub"}),
    ] {
        let r = rig(vec![call("proc_run", input), Scripted::text("ok")], |_| {});
        let res = r.turn().await;
        let rs = r.results(&res.session_id);
        assert_eq!(rs[0].0, ResultStatus::Error, "{}", rs[0].1);
        for field in ["argv", "steps", "command"] {
            assert!(rs[0].1.contains(field), "{}", rs[0].1);
        }
    }
}

/// The gate judges `{command: X}` as it judges `["bash", "-c", X]`: the same
/// decision, reason and stored call, refused (asked) and run alike; and
/// `bash {command}` as well.
#[tokio::test]
async fn the_gate_judges_command_as_the_bash_c_argv_it_runs_as() {
    let ask = |c: &mut Config| {
        c.policy.enforcement = Posture::Open;
        c.policy.approve_argv = vec![vec!["bash".into(), "-c".into()]];
    };
    let line = "echo gated";
    let mut seen = Vec::new();
    for input in [
        ("proc_run", json!({"argv": ["bash", "-c", line]})),
        ("proc_run", json!({"command": line})),
        ("bash", json!({"command": line})),
    ] {
        let r = rig(vec![call(input.0, input.1), Scripted::text("ok")], ask);
        let res = r.turn().await;
        assert_eq!(res.stop_reason, "awaiting_confirm", "{res:?}");
        let (posture, _, _) = r.gate(&res.session_id);
        assert_eq!(posture, "approve");
        // Each rig has a directory of its own, which its reason names.
        let (gate, call) = (r.gate(&res.session_id), r.stored_call(&res.session_id));
        let gate = (gate.0, gate.1, gate.2.replace(&r.dir(), "DIR"));
        seen.push((gate, call));
    }
    assert_eq!(seen[0], seen[1], "command is judged as its argv");
    assert_eq!(seen[0], seen[2], "bash {{command}} too");
    // Not on the list, the same line runs, as the argv does.
    let r = rig(
        vec![
            call("proc_run", json!({"command": line})),
            Scripted::text("ok"),
        ],
        |c| c.policy.enforcement = Posture::Open,
    );
    let res = r.turn().await;
    assert_eq!(r.results(&res.session_id)[0].0, ResultStatus::Ok);
}

/// A string where `argv`'s array goes runs as a shell line, and the result's
/// first line says so.
#[tokio::test]
async fn a_string_for_argv_runs_as_a_shell_line_and_says_so() {
    let r = rig(
        vec![
            call("proc_run", json!({"argv": "echo one; echo two"})),
            Scripted::text("ok"),
        ],
        |_| {},
    );
    let res = r.turn().await;
    let rs = r.results(&res.session_id);
    assert!(
        rs[0]
            .1
            .starts_with("[ran as proc_run {command}: argv takes an array"),
        "{}",
        rs[0].1
    );
    assert!(rs[0].1.trim_end().ends_with("\none\ntwo"), "{}", rs[0].1);
}

/// An input that is not JSON but `{"argv": ` and a bare line runs as the line
/// when that is the only reading; otherwise the error names the field and
/// shows the shape, and the count of such inputs by tool is `tool.list`'s.
#[tokio::test]
async fn an_unparsed_proc_run_input_is_recovered_or_named_never_echoed() {
    let r = rig(
        vec![
            call("proc_run", json!(r#"{"argv": echo recovered}"#)),
            Scripted::text("ok"),
        ],
        |_| {},
    );
    let res = r.turn().await;
    let rs = r.results(&res.session_id);
    assert_eq!(rs[0].0, ResultStatus::Ok, "{}", rs[0].1);
    assert!(
        rs[0].1.contains("not valid JSON; its shell line ran"),
        "{}",
        rs[0].1
    );
    assert!(rs[0].1.trim_end().ends_with("\nrecovered"), "{}", rs[0].1);
    let listed = |r: &Rig| {
        r.core
            .tool_list()
            .tools
            .into_iter()
            .map(|t| (t.name, t.invalid_json))
            .collect::<Vec<_>>()
    };
    assert!(
        listed(&r).iter().all(|(_, n)| *n == 0),
        "recovered is not invalid"
    );

    let broken = r#"{"argv": ["bash", "-c", "echo "hi""]}"#;
    let r = rig(
        vec![call("proc_run", json!(broken)), Scripted::text("ok")],
        |_| {},
    );
    let res = r.turn().await;
    let rs = r.results(&res.session_id);
    assert_eq!(rs[0].0, ResultStatus::Error);
    assert!(rs[0].1.contains(r#"{"command": "…"}"#), "{}", rs[0].1);
    assert!(
        rs[0].1.contains(r#"{"argv": ["bash", "-c", "…"]}"#),
        "{}",
        rs[0].1
    );
    assert!(
        !rs[0].1.contains("INVALID_JSON") && !rs[0].1.contains("echo \"hi"),
        "{}",
        rs[0].1
    );
    let counts: Vec<_> = listed(&r).into_iter().filter(|(_, n)| *n > 0).collect();
    assert_eq!(counts, [("proc.run".to_string(), 1)], "by tool");
}

/// An unknown tool is answered with the one or two real tools nearest its
/// name, never the whole list; a field a sibling takes names the sibling.
#[tokio::test]
async fn an_unknown_tool_names_its_nearest_and_a_misplaced_field_names_its_owner() {
    let r = rig(
        vec![
            Scripted::tools(
                "",
                &[
                    ("t1", "fs_reed", json!({"path": "a"})),
                    ("t2", "fs_read", json!({"path": "a", "max_results": 3})),
                ],
            ),
            Scripted::text("ok"),
        ],
        |_| {},
    );
    let res = r.turn().await;
    let rs = r.results(&res.session_id);
    let said = |needle: &str| {
        rs.iter()
            .find(|(_, t)| t.contains(needle))
            .unwrap_or_else(|| panic!("no result says {needle}: {rs:?}"))
            .1
            .clone()
    };
    let unknown = said("Unknown tool");
    assert!(
        unknown.starts_with("Unknown tool `fs_reed`. Nearest: `fs_read`"),
        "{unknown}"
    );
    assert!(
        unknown.matches('`').count() <= 6,
        "one or two tools, never the list: {unknown}"
    );
    let owner = said("max_results");
    assert!(
        owner.contains("`max_results` is fs_grep's; fs_read takes limit, offset, pages, path"),
        "{owner}"
    );
}

/// `fs_write` takes `file` and `file_text` as `path` and `content`.
#[tokio::test]
async fn fs_write_accepts_the_names_a_model_reaches_for() {
    let r = rig(
        vec![
            call("fs_write", json!({"file": "a.txt", "file_text": "x"})),
            Scripted::text("ok"),
        ],
        |_| {},
    );
    let res = r.turn().await;
    let rs = r.results(&res.session_id);
    assert_eq!(rs[0].0, ResultStatus::Ok, "{}", rs[0].1);
    assert_eq!(std::fs::read_to_string(r.root.join("a.txt")).unwrap(), "x");
}

/// A `bash {command}` call that waits for the operator runs as `proc_run`
/// once approved, after the continuation takes it again from the model's
/// own words (the stored reply says `bash`).
#[tokio::test]
async fn an_approved_bash_call_runs_as_proc_run_after_the_continuation() {
    let r = rig(
        vec![
            call("bash", json!({"command": "echo approved > out.txt"})),
            Scripted::text("Ran."),
        ],
        |c| c.policy.enforcement = Posture::Approve,
    );
    let res = r.turn().await;
    assert_eq!(res.stop_reason, "awaiting_confirm", "{res:?}");
    let pending = r.core.pending_confirms(&res.session_id).unwrap();
    assert_eq!(pending.len(), 1);
    r.core
        .confirm_action(&pending[0].correlation_id, true, None, "test")
        .unwrap();
    let exec = res.execution_id.clone().unwrap();
    let cont = r.core.continue_execution(&exec).await.unwrap().unwrap();
    assert_eq!(cont.output, "Ran.");
    assert_eq!(
        std::fs::read_to_string(r.root.join("out.txt")).unwrap(),
        "approved\n"
    );
    let rs = r.results(&res.session_id);
    assert_eq!(rs[0].0, ResultStatus::Ok, "{}", rs[0].1);
    assert!(rs[0].1.contains("[exit code 0 · in "), "{}", rs[0].1);
}

/// A field a sibling tool takes says which tool takes it, and lists the
/// fields this one takes, read from the schemas.
#[tokio::test]
async fn term_open_given_term_reads_field_names_the_owner_and_its_own_fields() {
    let r = rig(
        vec![
            call("term_open", json!({"argv": ["sh"], "timeout_ms": 500})),
            Scripted::text("ok"),
        ],
        |_| {},
    );
    let res = r.turn().await;
    let rs = r.results(&res.session_id);
    assert_eq!(rs[0].0, ResultStatus::Error, "{}", rs[0].1);
    assert!(
        rs[0].1.contains(
            "`timeout_ms` is term_read's and term_send's; term_open takes argv, cols, cwd, quiet_ms, rows"
        ),
        "{}",
        rs[0].1
    );
}
