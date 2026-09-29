//! M3 integration tests: scripted tool loops through the whole core — nodes,
//! compilation, the gate, confirm and decline, background jobs with late results,
//! continuation turns, and memory across a restart.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use serde_json::{json, Value};
use theseus_protocol::{NarrativeLine, SessionKind, TurnSubmitResult};

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
    fake: Arc<FakeProvider>,
    root: PathBuf,
    _dir: tempfile::TempDir,
}

fn config(root: &Path, state: &Path) -> Config {
    let mut cfg = Config::example();
    cfg.server.state_dir = state.to_string_lossy().into_owned();
    cfg.tools.projects_dir = Some(root.to_string_lossy().into_owned());
    cfg.tools.roots = vec![];
    cfg.tools.approve_paths = vec![root.join("secret").to_string_lossy().into_owned()];
    cfg.tools.proc_sync_secs = 10;
    // The template is a deployment (enforcement = notify); these scenarios
    // test the gate's stops, so what the template leaves to enforcement (the
    // writers and proc.run) waits, and its read-only tools stay open.
    cfg.policy.enforcement = Posture::Approve;
    cfg
}

fn rig_with(script: Vec<Scripted>, tweak: impl FnOnce(&mut Config)) -> Rig {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("work");
    std::fs::create_dir_all(&root).unwrap();
    let root = root.canonicalize().unwrap();
    let mut cfg = config(&root, dir.path());
    tweak(&mut cfg);
    let store = Store::open(&dir.path().join("store"), theseus_store::Engine::Redb).unwrap();
    let fake = Arc::new(FakeProvider::scripted(script));
    let core = Core::with_provider(cfg, fake.clone(), store, vec![]).unwrap();
    Rig {
        core,
        fake,
        root,
        _dir: dir,
    }
}

fn rig(script: Vec<Scripted>) -> Rig {
    rig_with(script, |_| {})
}

async fn turn(core: &Arc<Core>, session: Option<&str>, input: &str) -> TurnSubmitResult {
    let rec = match session {
        Some(id) => core
            .store
            .get_session::<SessionRecord>(id)
            .unwrap()
            .unwrap(),
        None => {
            let r = SessionRecord::new(SessionKind::Conversation, None);
            core.store.put_session(&r.session_id, &r).unwrap();
            r
        }
    };
    let (live, _) = core.live_profile();
    let target = core.runner.resolve_target(&live, None, None, None).unwrap();
    let sink = EventSink::new(core.bus.clone(), &rec.session_id, None);
    core.runner
        .run(TurnRequest {
            session: rec,
            input: Some(input.into()),
            target,
            sink,
            author: "test".into(),
            recompile: None,
        })
        .await
        .unwrap()
}

fn kinds(core: &Core, sid: &str) -> Vec<String> {
    core.store
        .session_nodes(sid)
        .unwrap()
        .iter()
        .map(|(_, n)| match &n.body {
            Body::ToolResult { status, late, .. } => format!(
                "tool_result:{}{}",
                status.as_str(),
                if *late { ":late" } else { "" }
            ),
            _ => n.kind_str().to_string(),
        })
        .collect()
}

fn results(core: &Core, sid: &str) -> Vec<(ResultStatus, String)> {
    core.store
        .session_nodes(sid)
        .unwrap()
        .into_iter()
        .filter_map(|(_, n)| match n.body {
            Body::ToolResult {
                status, content, ..
            } => Some((status, content)),
            _ => None,
        })
        .collect()
}

#[tokio::test]
async fn a_tool_loop_reads_a_file_and_the_prefix_never_changes() {
    let r = rig(vec![
        Scripted::tools(
            "Reading it.",
            &[("t1", "fs_read", json!({"path": "hello.txt"}))],
        ),
        Scripted::text("It says hi."),
    ]);
    std::fs::write(r.root.join("hello.txt"), "hi there\n").unwrap();
    let res = turn(&r.core, None, "what does hello.txt say?").await;
    assert_eq!(res.loops, 2, "{res:?}");
    assert_eq!(res.tool_calls, 1);
    assert!(res.output.contains("It says hi."));
    assert_eq!(
        kinds(&r.core, &res.session_id),
        vec![
            "user_message",
            "assistant_message",
            "tool_call",
            "tool_result:ok",
            "assistant_message"
        ]
    );
    let rs = results(&r.core, &res.session_id);
    assert!(rs[0].1.contains("hi there"), "{rs:?}");
    // The second request replays the first byte for byte and appends the result.
    let reqs = r.fake.requests();
    assert_eq!(reqs.len(), 2);
    assert_eq!(&reqs[1].messages[..1], &reqs[0].messages[..]);
    assert_eq!(reqs[1].system, reqs[0].system);
    assert_eq!(reqs[1].tools, reqs[0].tools);
    assert_eq!(reqs[1].messages[2]["content"][0]["type"], "tool_result");
    assert!(reqs[0]
        .tools
        .iter()
        .any(|t| t["name"] == "fs_read" && t["eager_input_streaming"] == true));
    assert!(
        reqs[0].system[0]["text"]
            .as_str()
            .unwrap()
            .contains(r.root.to_str().unwrap()),
        "roots are in the system prompt"
    );
    // One compilation (the new session), appended to thereafter.
    assert_eq!(
        r.core
            .store
            .session_compilations(&res.session_id)
            .unwrap()
            .len(),
        1
    );
    // A second turn keeps the whole first exchange as its prefix.
    r.fake
        .script
        .lock()
        .unwrap()
        .push_back(Scripted::text("Anything else?"));
    let res2 = turn(&r.core, Some(&res.session_id), "thanks").await;
    let reqs = r.fake.requests();
    assert_eq!(&reqs[2].messages[..3], &reqs[1].messages[..]);
    assert_eq!(reqs[2].messages.len(), 5);
    assert_eq!(res2.loops, 1);
    assert!(
        res2.cost_usd.is_some(),
        "the default model is in the catalog"
    );
}

#[tokio::test]
async fn a_write_waits_for_confirmation_then_the_driver_resumes_it() {
    let r = rig(vec![
        Scripted::tools(
            "",
            &[(
                "t1",
                "fs_write",
                json!({"path": "out.txt", "content": "made by theseus\n"}),
            )],
        ),
        Scripted::text("Written."),
    ]);
    let res = turn(&r.core, None, "write out.txt").await;
    let corr = res
        .awaiting_confirm
        .clone()
        .expect("the write must wait for the operator");
    assert_eq!(res.stop_reason, "awaiting_confirm");
    assert!(
        !r.root.join("out.txt").exists(),
        "nothing written before the confirm"
    );
    let pending = r.core.pending_confirms(&res.session_id).unwrap();
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].tool, "fs.write");
    assert!(
        pending[0].reason.contains("out.txt"),
        "{}",
        pending[0].reason
    );
    let exec = res.execution_id.clone().unwrap();
    assert_eq!(
        r.core
            .kernel
            .execution(&exec)
            .unwrap()
            .unwrap()
            .state
            .as_str(),
        "waiting"
    );

    r.core.confirm_action(&corr, true, None, "test").unwrap();
    let e = r.core.kernel.execution(&exec).unwrap().unwrap();
    assert_eq!(e.state.as_str(), "queued");
    assert!(e.resume_pending, "the driver takes the next turn");
    let cont = r.core.continue_execution(&exec).await.unwrap().unwrap();
    assert!(cont.continuation);
    assert_eq!(cont.output, "Written.");
    assert_eq!(
        std::fs::read_to_string(r.root.join("out.txt")).unwrap(),
        "made by theseus\n"
    );
    assert_eq!(results(&r.core, &res.session_id)[0].0, ResultStatus::Ok);
    assert!(r.core.pending_confirms(&res.session_id).unwrap().is_empty());
    assert_eq!(
        r.core
            .kernel
            .execution(&exec)
            .unwrap()
            .unwrap()
            .state
            .as_str(),
        "waiting"
    );
}

#[tokio::test]
async fn a_declined_write_and_a_superseded_one_never_run() {
    let r = rig(vec![
        Scripted::tools(
            "",
            &[("t1", "fs_write", json!({"path": "a.txt", "content": "x"}))],
        ),
        Scripted::text("Understood, not writing."),
        Scripted::tools(
            "",
            &[("t2", "fs_write", json!({"path": "b.txt", "content": "y"}))],
        ),
        Scripted::text("Okay, never mind then."),
    ]);
    let res = turn(&r.core, None, "write a").await;
    let corr = res.awaiting_confirm.clone().unwrap();
    r.core
        .confirm_action(&corr, false, Some("not now"), "test")
        .unwrap();
    let cont = r
        .core
        .continue_execution(res.execution_id.as_deref().unwrap())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(cont.output, "Understood, not writing.");
    assert!(!r.root.join("a.txt").exists());
    let rs = results(&r.core, &res.session_id);
    assert_eq!(rs[0].0, ResultStatus::Declined);
    assert!(
        rs[0].1.contains("declined") && rs[0].1.contains("not now") && !rs[0].1.contains("denied"),
        "{rs:?}"
    );

    let res2 = turn(&r.core, Some(&res.session_id), "write b").await;
    assert!(res2.awaiting_confirm.is_some());
    // The operator types instead of confirming: the pending write is superseded, never run.
    let res3 = turn(&r.core, Some(&res.session_id), "actually, don't").await;
    assert_eq!(res3.output, "Okay, never mind then.");
    assert!(!r.root.join("b.txt").exists());
    let rs = results(&r.core, &res.session_id);
    assert_eq!(rs[1].0, ResultStatus::Declined);
    assert!(rs[1].1.contains("new message"), "{rs:?}");
    // The request that followed has the decline before the new input, in one user message.
    let last = r.fake.requests().pop().unwrap();
    let final_user = last.messages.last().unwrap();
    assert_eq!(final_user["content"][0]["type"], "tool_result");
    assert_eq!(final_user["content"][1]["text"], "actually, don't");
}

/// A daemon from before theseus-8az recorded a decline as `denied by <who>:
/// <note>`. Resumed by this binary, such an action still reaches the model as
/// a decline with the operator's note. New declines use the new names.
#[tokio::test]
async fn a_decline_recorded_under_the_old_names_still_reads_as_a_decline() {
    use theseus_store::{kinds, NewRecord};
    let r = rig(vec![
        Scripted::tools(
            "",
            &[("t1", "fs_write", json!({"path": "a.txt", "content": "x"}))],
        ),
        Scripted::text("Understood, not writing."),
    ]);
    let res = turn(&r.core, None, "write a").await;
    let corr = res.awaiting_confirm.clone().unwrap();
    r.core
        .confirm_action(&corr, false, Some("not now"), "test")
        .unwrap();
    let mut a = r.core.kernel.action(&corr).unwrap().unwrap();
    assert_eq!(
        a.resolution.as_deref(),
        Some("declined by operator: not now")
    );
    assert_eq!(ledgered(&r, "action.declined").len(), 1);
    assert!(ledgered(&r, "action.denied").is_empty());
    // Store the action again as the previous binary wrote it.
    a.resolution = Some("denied by operator: not now".into());
    let rec = NewRecord::json(kinds::ACTION, Some(&corr), &a)
        .unwrap()
        .scoped(&a.session_id);
    r.core.store.append(vec![rec]).unwrap();
    let cont = r
        .core
        .continue_execution(res.execution_id.as_deref().unwrap())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(cont.output, "Understood, not writing.");
    assert!(!r.root.join("a.txt").exists());
    let rs = results(&r.core, &res.session_id);
    assert_eq!(rs[0].0, ResultStatus::Declined);
    assert_eq!(
        rs[0].1,
        "Not run: the operator declined this call (not now)."
    );
}

#[tokio::test]
async fn outside_the_roots_and_approve_paths_wait_with_a_clear_reason() {
    let outside = tempfile::tempdir().unwrap();
    let file = outside.path().canonicalize().unwrap().join("notes.txt");
    std::fs::write(&file, "outside the workspace\n").unwrap();
    let r = rig(vec![
        Scripted::tools(
            "",
            &[("t1", "fs_read", json!({"path": file.to_string_lossy()}))],
        ),
        Scripted::text("Read it."),
        Scripted::tools("", &[("t2", "fs_read", json!({"path": "secret/key.pem"}))]),
        Scripted::text("Skipped it."),
        Scripted::tools(
            "",
            &[
                ("t3", "fs_read", json!({"path": 42})),
                ("t4", "no_such_tool", json!({})),
            ],
        ),
        Scripted::text("Those were errors."),
    ]);
    // Outside the roots: the call waits, and runs once approved.
    let res = turn(&r.core, None, "read the notes").await;
    let sid = res.session_id.clone();
    let corr = res
        .awaiting_confirm
        .clone()
        .expect("outside the roots waits");
    let pending = r.core.pending_confirms(&sid).unwrap();
    assert!(!pending[0].floor);
    assert!(
        pending[0].reason.ends_with(&format!(
            "fs.read — approve ({} is outside the workspace roots: {})",
            file.display(),
            r.root.display()
        )),
        "{}",
        pending[0].reason
    );
    assert!(
        results(&r.core, &sid).is_empty(),
        "nothing runs before the answer"
    );
    r.core.confirm_action(&corr, true, None, "test").unwrap();
    let cont = r
        .core
        .continue_execution(res.execution_id.as_deref().unwrap())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(cont.output, "Read it.");
    let rs = results(&r.core, &sid);
    assert_eq!(rs[0].0, ResultStatus::Ok, "{rs:?}");
    assert!(rs[0].1.contains("outside the workspace"), "{}", rs[0].1);

    // On approve_paths, inside the roots: the call waits; a decline means it never runs.
    std::fs::create_dir_all(r.root.join("secret")).unwrap();
    std::fs::write(r.root.join("secret/key.pem"), "not for the model\n").unwrap();
    let res = turn(&r.core, Some(&sid), "read the key").await;
    let corr = res
        .awaiting_confirm
        .clone()
        .expect("an approve_paths hit waits");
    let pending = r.core.pending_confirms(&sid).unwrap();
    assert!(!pending[0].floor);
    assert!(
        pending[0].reason.ends_with(&format!(
            "fs.read — approve ({0}/secret/key.pem is protected: {0}/secret is on the approve list)",
            r.root.display()
        )),
        "{}",
        pending[0].reason
    );
    r.core.confirm_action(&corr, false, None, "test").unwrap();
    let cont = r
        .core
        .continue_execution(res.execution_id.as_deref().unwrap())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(cont.output, "Skipped it.");
    let rs = results(&r.core, &sid);
    assert_eq!(
        rs[1].0,
        ResultStatus::Declined,
        "declined, never run: {rs:?}"
    );
    assert!(!rs[1].1.contains("not for the model"), "{}", rs[1].1);

    // Bad input and unknown tools are errors, as before.
    let res = turn(&r.core, Some(&sid), "try these").await;
    assert_eq!(res.output, "Those were errors.");
    let rs = results(&r.core, &sid);
    assert_eq!(rs[2].0, ResultStatus::Error);
    assert!(rs[2].1.starts_with("Invalid input"), "{}", rs[2].1);
    assert_eq!(rs[3].0, ResultStatus::Error);
    assert!(rs[3].1.contains("Unknown tool"));
    assert!(
        ledgered(&r, "tool.denied").is_empty(),
        "the gate refused nothing"
    );
    assert_eq!(ledgered(&r, "tool.confirm_requested").len(), 2);
    assert_eq!(ledgered(&r, "tool.invalid_input").len(), 1);
}

#[tokio::test]
async fn the_token_file_the_daemon_was_given_is_on_the_floor() {
    let elsewhere = tempfile::tempdir().unwrap();
    let token = elsewhere.path().canonicalize().unwrap().join("op-token");
    std::fs::write(&token, "a decoy, not a token\n").unwrap();
    let script = || {
        vec![
            Scripted::tools(
                "",
                &[("t1", "fs_read", json!({"path": token.to_string_lossy()}))],
            ),
            Scripted::text("Waiting on you."),
        ]
    };
    // Named by `--op-token-file` or THESEUS_OP_TOKEN_FILE, it is the floor.
    let given = token.clone();
    let r = rig_with(script(), move |cfg| {
        cfg.policy.enforcement = Posture::Open;
        cfg.op_token_file = Some(given);
    });
    let res = turn(&r.core, None, "read the token").await;
    let corr = res
        .awaiting_confirm
        .clone()
        .expect("the floor waits, even under open");
    let pending = r.core.pending_confirms(&res.session_id).unwrap();
    assert!(pending[0].floor, "{}", pending[0].reason);
    assert!(
        pending[0].reason.ends_with(&format!(
            "fs.read — approve (floor: {} is Theseus's own state or the 1Password token)",
            token.display()
        )),
        "{}",
        pending[0].reason
    );
    r.core.confirm_action(&corr, false, None, "test").unwrap();
    let cont = r
        .core
        .continue_execution(res.execution_id.as_deref().unwrap())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(cont.output, "Waiting on you.");
    assert_eq!(
        results(&r.core, &res.session_id)[0].0,
        ResultStatus::Declined
    );
    // The same file, not named as the token file, is only outside the roots.
    let r = rig_with(script(), |cfg| cfg.policy.enforcement = Posture::Open);
    let res = turn(&r.core, None, "read the token").await;
    assert!(res.awaiting_confirm.is_some());
    let pending = r.core.pending_confirms(&res.session_id).unwrap();
    assert!(!pending[0].floor, "{}", pending[0].reason);
    assert!(
        pending[0].reason.contains("is outside the workspace roots"),
        "{}",
        pending[0].reason
    );
}

#[tokio::test]
async fn proc_run_returns_in_turn_and_a_slow_one_comes_back_later_as_a_late_result() {
    let r = rig_with(
        vec![
            Scripted::tools(
                "",
                &[(
                    "t1",
                    "proc_run",
                    json!({"argv": ["echo", "hello from a job"]}),
                )],
            ),
            Scripted::text("It printed hello."),
            Scripted::tools(
                "",
                &[(
                    "t2",
                    "proc_run",
                    json!({"argv": ["bash", "-c", "sleep 1.5; echo slow done"]}),
                )],
            ),
            Scripted::text("Started; I'll report back."),
            Scripted::text("The slow job finished: slow done."),
        ],
        |cfg| {
            cfg.policy.tools.insert("proc.run".into(), Posture::Open);
            cfg.tools.proc_sync_secs = 1;
        },
    );
    let res = turn(&r.core, None, "echo something").await;
    assert_eq!(res.output, "It printed hello.");
    let rs = results(&r.core, &res.session_id);
    assert_eq!(rs[0].0, ResultStatus::Ok);
    assert!(
        rs[0].1.contains("hello from a job") && rs[0].1.contains("[exit code 0]"),
        "{}",
        rs[0].1
    );

    let res2 = turn(&r.core, Some(&res.session_id), "run the slow build").await;
    assert_eq!(res2.output, "Started; I'll report back.");
    let rs = results(&r.core, &res.session_id);
    assert_eq!(rs[1].0, ResultStatus::Background, "{rs:?}");
    let exec = res2.execution_id.clone().unwrap();
    let e = r.core.kernel.execution(&exec).unwrap().unwrap();
    assert_eq!(e.state.as_str(), "waiting");
    assert!(
        matches!(e.wake, Some(theseus_kernel::Wake::Actions { .. })),
        "{:?}",
        e.wake
    );

    // The job finishes while no turn runs; the spool has it; the heartbeat drains it.
    for _ in 0..100 {
        tokio::time::sleep(Duration::from_millis(50)).await;
        r.core.heartbeat("test");
        if r.core
            .kernel
            .execution(&exec)
            .unwrap()
            .unwrap()
            .state
            .as_str()
            == "queued"
        {
            break;
        }
    }
    let e = r.core.kernel.execution(&exec).unwrap().unwrap();
    assert_eq!(
        e.state.as_str(),
        "queued",
        "the completion woke the execution"
    );
    let cont = r.core.continue_execution(&exec).await.unwrap().unwrap();
    assert!(cont.continuation);
    assert_eq!(cont.output, "The slow job finished: slow done.");
    let ks = kinds(&r.core, &res.session_id);
    assert!(ks.contains(&"tool_result:ok:late".to_string()), "{ks:?}");
    let last = r.fake.requests().pop().unwrap();
    let late = last.messages.last().unwrap()["content"][0]["text"]
        .as_str()
        .unwrap()
        .to_string();
    assert!(
        late.contains("Background result") && late.contains("slow done"),
        "{late}"
    );
}

#[tokio::test]
async fn a_session_keeps_its_memory_across_a_restart() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("w");
    std::fs::create_dir_all(&root).unwrap();
    let cfg = config(&root.canonicalize().unwrap(), dir.path());
    let sid = {
        let store = Store::open(&dir.path().join("store"), theseus_store::Engine::Redb).unwrap();
        let fake = Arc::new(FakeProvider::scripted(vec![Scripted::text(
            "My name is Theseus.",
        )]));
        let core = Core::with_provider(cfg.clone(), fake, store, vec![]).unwrap();
        turn(&core, None, "who are you?").await.session_id
    };
    let store = Store::open(&dir.path().join("store"), theseus_store::Engine::Redb).unwrap();
    let fake = Arc::new(FakeProvider::scripted(vec![Scripted::text(
        "You asked who I am.",
    )]));
    let core = Core::with_provider(cfg, fake.clone(), store, vec![]).unwrap();
    let res = turn(&core, Some(&sid), "what did I just ask?").await;
    assert_eq!(res.output, "You asked who I am.");
    let req = fake.requests().pop().unwrap();
    assert_eq!(req.messages.len(), 3, "{:?}", req.messages);
    assert_eq!(req.messages[0]["content"][0]["text"], "who are you?");
    assert_eq!(req.messages[1]["content"][0]["text"], "My name is Theseus.");
    // Still one compilation: the restart did not recompile anything.
    assert_eq!(core.store.session_compilations(&sid).unwrap().len(), 1);
}

#[tokio::test]
async fn a_fresh_recompile_starts_over_and_is_recorded() {
    let r = rig(vec![Scripted::text("one"), Scripted::text("two")]);
    let res = turn(&r.core, None, "first").await;
    let mut rec: SessionRecord = r.core.store.get_session(&res.session_id).unwrap().unwrap();
    rec.pending_recompile = Some(crate::compiler::Recompile::Fresh);
    r.core.store.put_session(&rec.session_id, &rec).unwrap();
    turn(&r.core, Some(&res.session_id), "second").await;
    let req = r.fake.requests().pop().unwrap();
    assert_eq!(req.messages.len(), 1, "fresh: only the new input");
    let comps = r.core.store.session_compilations(&res.session_id).unwrap();
    assert_eq!(comps.len(), 2);
    assert_eq!(comps[1].trigger, "manual_fresh");
    assert_eq!(comps[1].derived_from.as_deref(), Some(comps[0].id.as_str()));
    let rec: SessionRecord = r.core.store.get_session(&res.session_id).unwrap().unwrap();
    assert!(rec.pending_recompile.is_none(), "applied once");
    let _: Value = serde_json::to_value(&rec).unwrap();
}

/// Run one turn in a watched session; return it and the notices it posted.
async fn watched_turn(r: &Rig, input: &str) -> (TurnSubmitResult, Vec<Value>) {
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    let rec = SessionRecord::new(SessionKind::Conversation, None);
    r.core.store.put_session(&rec.session_id, &rec).unwrap();
    r.core.bus.watch(&rec.session_id, "watcher", tx);
    let res = turn(&r.core, Some(&rec.session_id), input).await;
    let mut notices = vec![];
    while let Ok(m) = rx.try_recv() {
        if let theseus_protocol::Message::Notification(n) = m {
            if n.method == theseus_protocol::notify::POLICY_NOTIFIED {
                notices.push(n.params);
            }
        }
    }
    (res, notices)
}

fn ledgered(r: &Rig, kind: &str) -> Vec<Value> {
    r.core
        .store
        .ledger_tail::<crate::ledger::LedgerRow>(300)
        .unwrap()
        .into_iter()
        .filter(|(_, row)| row.kind == kind)
        .map(|(_, row)| row.data)
        .collect()
}

#[tokio::test]
async fn under_approve_a_command_waits_and_runs_once_approved() {
    let r = rig_with(
        vec![
            Scripted::tools(
                "",
                &[("t1", "proc_run", json!({"argv": ["echo", "approved run"]}))],
            ),
            Scripted::text("It printed."),
        ],
        |cfg| cfg.policy.enforcement = Posture::Approve,
    );
    let res = turn(&r.core, None, "echo something").await;
    let corr = res.awaiting_confirm.clone().expect("approve waits");
    let pending = r.core.pending_confirms(&res.session_id).unwrap();
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].tool, "proc.run");
    assert!(!pending[0].floor);
    assert!(
        pending[0]
            .reason
            .ends_with("proc.run — approve (enforcement = approve)"),
        "{}",
        pending[0].reason
    );
    assert!(
        results(&r.core, &res.session_id).is_empty(),
        "nothing runs before the approval"
    );
    r.core.confirm_action(&corr, true, None, "test").unwrap();
    let cont = r
        .core
        .continue_execution(res.execution_id.as_deref().unwrap())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(cont.output, "It printed.");
    let rs = results(&r.core, &res.session_id);
    assert_eq!(rs[0].0, ResultStatus::Ok, "{rs:?}");
    assert!(rs[0].1.contains("approved run"), "{}", rs[0].1);
}

#[tokio::test]
async fn under_notify_a_command_runs_with_a_notice_and_a_read_stays_quiet() {
    let r = rig_with(
        vec![
            Scripted::tools(
                "",
                &[
                    ("t1", "fs_read", json!({"path": "a.txt"})),
                    ("t2", "proc_run", json!({"argv": ["echo", "noticed"]})),
                ],
            ),
            Scripted::text("Done, and you were told."),
        ],
        |cfg| cfg.policy.enforcement = Posture::Notify,
    );
    std::fs::write(r.root.join("a.txt"), "read me\n").unwrap();
    let (res, notices) = watched_turn(&r, "read, then echo").await;
    assert!(res.awaiting_confirm.is_none(), "notify never parks");
    assert_eq!(res.output, "Done, and you were told.");
    let rs = results(&r.core, &res.session_id);
    assert_eq!(rs[0].0, ResultStatus::Ok, "{rs:?}");
    assert_eq!(rs[1].0, ResultStatus::Ok, "{rs:?}");
    assert!(rs[1].1.contains("noticed"), "{}", rs[1].1);
    // The template's `"fs.read" = "open"` keeps the read quiet; proc.run
    // inherits enforcement and posts the notice.
    assert_eq!(notices.len(), 1, "{notices:?}");
    assert_eq!(notices[0]["tool"], "proc.run");
    assert_eq!(notices[0]["kind"], "notify");
    assert_eq!(notices[0]["setting"], "enforcement = notify");
    assert_eq!(
        notices[0]["rule"],
        "proc.run — notify (enforcement = notify)"
    );
    let rows = ledgered(&r, "tool.notified");
    assert_eq!(rows.len(), 1, "{rows:?}");
    assert_eq!(rows[0]["tool"], "proc.run");
}

#[tokio::test]
async fn a_per_tool_override_makes_the_same_command_wait_under_notify() {
    let r = rig_with(
        vec![
            Scripted::tools(
                "",
                &[("t1", "proc_run", json!({"argv": ["echo", "pinned"]}))],
            ),
            Scripted::text("Waiting."),
        ],
        |cfg| {
            cfg.policy.enforcement = Posture::Notify;
            cfg.policy.tools.insert("proc.run".into(), Posture::Approve);
        },
    );
    let (res, notices) = watched_turn(&r, "echo").await;
    assert!(res.awaiting_confirm.is_some(), "the override waits");
    assert!(notices.is_empty(), "{notices:?}");
    let pending = r.core.pending_confirms(&res.session_id).unwrap();
    assert!(
        pending[0]
            .reason
            .ends_with("proc.run — approve ([policy.tools] \"proc.run\" = approve)"),
        "{}",
        pending[0].reason
    );
}

#[tokio::test]
async fn under_open_a_floor_path_still_waits_and_is_never_refused() {
    // The rig's store is `<state>/store`, beside the workspace `<state>/work`.
    let r = rig_with(
        vec![
            Scripted::tools(
                "",
                &[("t1", "proc_run", json!({"argv": ["cat", "../store/any"]}))],
            ),
            Scripted::text("Understood, I will not read it."),
        ],
        |cfg| cfg.policy.enforcement = Posture::Open,
    );
    let (res, notices) = watched_turn(&r, "cat the store").await;
    let corr = res
        .awaiting_confirm
        .clone()
        .expect("the floor waits, even under open");
    assert!(notices.is_empty(), "{notices:?}");
    let pending = r.core.pending_confirms(&res.session_id).unwrap();
    assert!(pending[0].floor, "the card is marked as the floor");
    assert!(
        pending[0].reason.contains(
            "proc.run — approve (floor: argument `../store/any` is Theseus's own state or the 1Password token)"
        ),
        "{}",
        pending[0].reason
    );
    assert!(results(&r.core, &res.session_id).is_empty(), "not refused");
    r.core.confirm_action(&corr, false, None, "test").unwrap();
    let cont = r
        .core
        .continue_execution(res.execution_id.as_deref().unwrap())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(cont.output, "Understood, I will not read it.");
    let rs = results(&r.core, &res.session_id);
    assert_eq!(
        rs[0].0,
        ResultStatus::Declined,
        "declined, never run: {rs:?}"
    );
}

/// `session.list` counts each session's waiting calls from one scan of the
/// open actions, not one scan per session (theseus-hco).
#[tokio::test]
async fn session_list_counts_each_sessions_waiting_calls() {
    let r = rig(vec![
        Scripted::tools(
            "",
            &[("t1", "fs_write", json!({"path": "a.txt", "content": "a"}))],
        ),
        Scripted::tools(
            "",
            &[("t2", "fs_write", json!({"path": "b.txt", "content": "b"}))],
        ),
        Scripted::text("Hello."),
    ]);
    let a = turn(&r.core, None, "write a.txt").await;
    let b = turn(&r.core, None, "write b.txt").await;
    let plain = turn(&r.core, None, "hi").await;
    assert!(a.awaiting_confirm.is_some() && b.awaiting_confirm.is_some());
    let list = r.core.session_list().unwrap();
    let waiting = |sid: &str| {
        list.iter()
            .find(|s| s.session_id == sid)
            .unwrap()
            .pending_confirms
    };
    assert_eq!(list.len(), 3);
    assert_eq!(waiting(&a.session_id), 1);
    assert_eq!(waiting(&b.session_id), 1);
    assert_eq!(waiting(&plain.session_id), 0);

    r.core
        .confirm_action(a.awaiting_confirm.as_deref().unwrap(), false, None, "test")
        .unwrap();
    let list = r.core.session_list().unwrap();
    let waiting = |sid: &str| {
        list.iter()
            .find(|s| s.session_id == sid)
            .unwrap()
            .pending_confirms
    };
    assert_eq!(waiting(&a.session_id), 0, "a declined call waits no longer");
    assert_eq!(waiting(&b.session_id), 1);
}

/// Every WAL frame is its own fsync, so each frame on the turn path costs
/// every turn. A plain one-loop turn writes 17 (27 before theseus-hco
/// removed the hook rows); a change that adds one raises this on purpose.
/// The template turns the narrative on, and a subscriber is watching: the
/// narrative is never stored, so it adds no frame (theseus-5fy).
#[tokio::test]
async fn a_plain_turn_stays_within_its_frame_budget() {
    let r = rig(vec![Scripted::text("first"), Scripted::text("hello")]);
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    assert!(
        r.core.narrator.watch("watcher", tx).is_some(),
        "narration is on"
    );
    let first = turn(&r.core, None, "warm up").await;
    let before = r.core.store.stats().unwrap().frames_appended;
    let res = turn(&r.core, Some(&first.session_id), "hi").await;
    assert_eq!(res.loops, 1);
    let frames = r.core.store.stats().unwrap().frames_appended - before;
    assert!(frames <= 17, "a plain turn wrote {frames} frames");
    let sent = std::iter::from_fn(|| rx.try_recv().ok()).count();
    let lines = narrated(&r, &res.session_id);
    assert!(
        lines
            .iter()
            .any(|l| l.turn_id.as_deref() == Some(res.turn_id.as_str())),
        "the turn was narrated"
    );
    assert_eq!(sent, lines.len(), "each line went to the watcher");
}

// ---------------------------------------------------------------- narrative (theseus-5fy)

/// The lines narrated about one session, oldest first.
fn narrated(r: &Rig, sid: &str) -> Vec<NarrativeLine> {
    r.core
        .narrator
        .tail()
        .into_iter()
        .filter(|l| l.session_id.as_deref() == Some(sid))
        .collect()
}

/// The parts in order, a run of one part counted once.
fn parts(lines: &[NarrativeLine]) -> Vec<&'static str> {
    let mut v: Vec<&'static str> = lines.iter().map(|l| l.part.as_str()).collect();
    v.dedup();
    v
}

/// Whether a line of `part` says `needle`.
fn said(lines: &[NarrativeLine], part: &str, needle: &str) -> bool {
    lines
        .iter()
        .any(|l| l.part.as_str() == part && l.text.contains(needle))
}

fn dump(lines: &[NarrativeLine]) -> String {
    lines
        .iter()
        .map(|l| format!("{:>8} | {}", l.part.as_str(), l.text))
        .collect::<Vec<_>>()
        .join("\n")
}

fn read_hello() -> Vec<Scripted> {
    vec![
        Scripted::tools(
            "Reading it.",
            &[("t1", "fs_read", json!({"path": "hello.txt"}))],
        ),
        Scripted::text("It says hi."),
    ]
}

/// Off means off: a tool turn makes no line, and the health flag is false.
/// (`narrative.watch` refusing is `rpc::tests::narrative_watch_streams_a_turn_and_refuses_when_off`.)
#[tokio::test]
async fn with_narration_off_a_tool_turn_makes_no_line() {
    let r = rig_with(read_hello(), |cfg| cfg.narrative = false);
    std::fs::write(r.root.join("hello.txt"), "hi there\n").unwrap();
    let res = turn(&r.core, None, "what does hello.txt say?").await;
    assert_eq!((res.loops, res.tool_calls), (2, 1));
    assert!(
        r.core.narrator.tail().is_empty(),
        "{}",
        dump(&r.core.narrator.tail())
    );
    assert!(!r.core.health().narrative);
    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
    assert!(r.core.narrator.watch("w", tx).is_none());
}

/// On: a two-loop tool turn narrates each architectural part in order, from
/// the values the structure holds, and never the input or the file's text.
#[tokio::test]
async fn a_two_loop_tool_turn_is_narrated_part_by_part() {
    let r = rig(read_hello());
    std::fs::write(r.root.join("hello.txt"), "hi there\n").unwrap();
    let res = turn(&r.core, None, "what does hello.txt say?").await;
    assert_eq!((res.loops, res.tool_calls), (2, 1));
    assert!(r.core.health().narrative);
    let lines = narrated(&r, &res.session_id);
    assert_eq!(
        parts(&lines),
        [
            "session", "turn", "loop", "context", "model", "tool", "loop", "context", "model",
            "loop", "turn", "session"
        ],
        "\n{}",
        dump(&lines)
    );
    for (part, needle) in [
        ("session", "Woken by new input from test"),
        ("turn", "started by test"),
        ("turn", "24 characters of input"),
        ("loop", "Loop 1 of up to 40"),
        ("context", "new compilation"),
        ("context", "because the session is new"),
        ("model", "Calling claude-sonnet-5-5 on anthropic: reserving"),
        ("model", "answered in"),
        ("model", "it stopped to call 1 tool"),
        ("tool", "fs.read hello.txt: posture open"),
        ("tool", "fs.read done in"),
        ("tool", "1 line"),
        ("loop", "Loop 1: continuing"),
        ("loop", "Loop 2 of up to 40"),
        ("context", "appending to compilation"),
        ("model", "it ended its turn"),
        ("loop", "Stopping: the model ended its turn"),
        ("turn", "ended after 2 loops"),
        ("turn", "1 tool call"),
        ("session", "Parked until the next input"),
    ] {
        assert!(
            said(&lines, part, needle),
            "no {part} line says {needle:?}:\n{}",
            dump(&lines)
        );
    }
    for l in &lines {
        assert!(
            !l.text.contains("hi there") && !l.text.contains("what does"),
            "a line carries content: {}",
            l.text
        );
        if l.part.as_str() != "session" {
            assert_eq!(
                l.turn_id.as_deref(),
                Some(res.turn_id.as_str()),
                "{}",
                l.text
            );
        }
    }
    let seqs: Vec<u64> = lines.iter().map(|l| l.seq).collect();
    assert!(seqs.windows(2).all(|w| w[0] < w[1]), "{seqs:?}");
}

/// A turn that fails in its second loop says so, with its class and what
/// the finished loop spent.
#[tokio::test]
async fn a_failed_turn_narrates_its_class_and_what_the_finished_loops_spent() {
    use crate::provider::ProviderError;
    let r = rig(vec![
        Scripted::tools("", &[("t1", "text_diff", json!({"a": "x\n", "b": "y\n"}))]),
        Scripted::Fail(ProviderError::Overloaded {
            message: "busy".into(),
        }),
    ]);
    let rec = SessionRecord::new(SessionKind::Conversation, None);
    r.core.store.put_session(&rec.session_id, &rec).unwrap();
    let (live, _) = r.core.live_profile();
    let target = r
        .core
        .runner
        .resolve_target(&live, None, None, None)
        .unwrap();
    let err = r
        .core
        .runner
        .run(TurnRequest {
            session: rec.clone(),
            input: Some("diff these".into()),
            target,
            sink: EventSink::new(r.core.bus.clone(), &rec.session_id, None),
            author: "test".into(),
            recompile: None,
        })
        .await
        .expect_err("the second call fails");
    let te = err.downcast_ref::<crate::turn::TurnError>().unwrap();
    let lines = narrated(&r, &rec.session_id);
    let spent = crate::narrative::money(te.cost_usd);
    assert!(te.cost_usd.unwrap_or(0.0) > 0.0, "{spent}");
    for (part, needle) in [
        ("model", "failed after"),
        ("model", "overloaded"),
        ("turn", "failed (overloaded) in loop 2"),
        ("turn", "1 finished loop"),
        ("turn", spent.as_str()),
        ("turn", "1 tool call"),
        ("session", "Parked until the next input"),
    ] {
        assert!(
            said(&lines, part, needle),
            "no {part} line says {needle:?}:\n{}",
            dump(&lines)
        );
    }
    assert!(!said(&lines, "turn", "ended after"), "{}", dump(&lines));
}

/// Approval and jobs: a write waits and is approved, and the driver resumes
/// it; a slow command goes to the background, and its completion comes back
/// from the spool as a late result. The command's script is never shown.
#[tokio::test]
async fn the_narrative_follows_an_approval_and_a_background_job() {
    let r = rig_with(
        vec![
            Scripted::tools(
                "",
                &[(
                    "t1",
                    "fs_write",
                    json!({"path": "out.txt", "content": "x\n"}),
                )],
            ),
            Scripted::text("Written."),
            Scripted::tools(
                "",
                &[(
                    "t2",
                    "proc_run",
                    json!({"argv": ["bash", "-c", "sleep 1.5; echo slow done"]}),
                )],
            ),
            Scripted::text("Started; I'll report back."),
            Scripted::text("The slow job finished."),
        ],
        |cfg| {
            cfg.policy.tools.insert("proc.run".into(), Posture::Open);
            cfg.tools.proc_sync_secs = 1;
        },
    );
    let res = turn(&r.core, None, "write out.txt").await;
    let corr = res.awaiting_confirm.clone().unwrap();
    let exec = res.execution_id.clone().unwrap();
    r.core.confirm_action(&corr, true, None, "test").unwrap();
    r.core.continue_execution(&exec).await.unwrap().unwrap();
    turn(&r.core, Some(&res.session_id), "run the slow one").await;
    for _ in 0..100 {
        tokio::time::sleep(Duration::from_millis(50)).await;
        r.core.heartbeat("test");
        let e = r.core.kernel.execution(&exec).unwrap().unwrap();
        if e.state.as_str() == "queued" {
            break;
        }
    }
    let cont = r.core.continue_execution(&exec).await.unwrap().unwrap();
    assert_eq!(cont.output, "The slow job finished.");
    let lines = narrated(&r, &res.session_id);
    for (part, needle) in [
        ("tool", "fs.write out.txt: posture approve"),
        ("tool", "waiting for approval"),
        ("session", "Parked until the operator answers the approval"),
        ("approval", "fs.write approved by test"),
        ("session", "Woken by the approval"),
        ("session", "The driver resumes execution"),
        ("turn", "Continuation turn"),
        ("approval", "fs.write: approved; running it now"),
        ("tool", "fs.write done in"),
        ("tool", "proc.run bash: posture open"),
        ("tool", "started as job"),
        ("job", "continues in the background"),
        ("session", "Parked until 1 background job"),
        ("job", "from the spool"),
        ("job", "late result"),
    ] {
        assert!(
            said(&lines, part, needle),
            "no {part} line says {needle:?}:\n{}",
            dump(&lines)
        );
    }
    for l in &lines {
        assert!(
            !l.text.contains("sleep") && !l.text.contains("slow done"),
            "a line carries the command: {}",
            l.text
        );
    }
}

// ---------------------------------------------------------------- budgets in dollars (theseus-0sg)

/// The template's live profile is Sonnet 5.5: a call reserves its
/// 128,000-token output cap at $10 per million ($1.28) plus its input
/// estimate at $2. A $1.40 limit fits the first call, not the second once
/// the first's 30,000 words out (about $0.30) are spent, and fits the second
/// again after a reset.
fn over_budget_rig(then: Vec<Scripted>) -> Rig {
    let mut script = vec![Scripted::tools(
        &"word ".repeat(30_000),
        &[("t1", "text_diff", json!({"a": "x\n", "b": "y\n"}))],
    )];
    script.extend(then);
    rig_with(script, |c| c.kernel.spend_limit_usd = 1.40)
}

/// A new session watched from before its first turn.
fn watched_session(
    r: &Rig,
) -> (
    String,
    tokio::sync::mpsc::UnboundedReceiver<theseus_protocol::Message>,
) {
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
    let rec = SessionRecord::new(SessionKind::Conversation, None);
    r.core.store.put_session(&rec.session_id, &rec).unwrap();
    r.core.bus.watch(&rec.session_id, "watcher", tx);
    (rec.session_id, rx)
}

fn sent(
    rx: &mut tokio::sync::mpsc::UnboundedReceiver<theseus_protocol::Message>,
    method: &str,
) -> Vec<Value> {
    let mut out = vec![];
    while let Ok(m) = rx.try_recv() {
        if let theseus_protocol::Message::Notification(n) = m {
            if n.method == method {
                out.push(n.params);
            }
        }
    }
    out
}

/// A scripted session reaches its limit: the turn does not fail; it parks,
/// and the execution waits with the reason `budget`. The question goes where
/// an approval goes (a `confirm.requested`, the history's pending confirms,
/// the session list's count). An approval resets the spend to $0, writes a
/// `budget.reset` row, and leaves the lifetime cost where it was; the driver
/// then makes the call that did not fit, and the new call is the new spend.
#[tokio::test]
async fn a_session_at_its_limit_asks_and_an_approved_reset_makes_the_waiting_call() {
    use theseus_kernel::{micros_to_usd, ExecState};
    let r = over_budget_rig(vec![Scripted::text("The diff is one line.")]);
    let (sid, mut rx) = watched_session(&r);
    let res = turn(&r.core, Some(&sid), "diff these").await;
    assert_eq!(res.stop_reason, "budget", "{res:?}");
    assert_eq!(res.loops, 2, "loop 0 ran; loop 1's call did not fit");
    assert_eq!(r.fake.requests().len(), 1, "nothing ran over the limit");
    let q = res
        .awaiting_confirm
        .clone()
        .expect("the turn parks on the question");
    let exec = res.execution_id.clone().unwrap();
    let e = r.core.kernel.execution(&exec).unwrap().unwrap();
    assert_eq!(e.state, ExecState::Waiting);
    assert_eq!(
        serde_json::to_value(&e.wake).unwrap(),
        json!({"on": "budget", "correlation_id": q})
    );
    let spent = e.budget.spent_micros;
    assert!(
        (250_000..400_000).contains(&spent),
        "loop 0's 30,000 words: {spent}"
    );
    let asked = sent(&mut rx, theseus_protocol::notify::CONFIRM_REQUESTED);
    assert_eq!(asked.len(), 1, "{asked:?}");
    let req: theseus_protocol::ConfirmRequest = serde_json::from_value(asked[0].clone()).unwrap();
    assert_eq!(
        (req.tool.as_str(), req.correlation_id.as_str()),
        ("budget.reset", q.as_str())
    );
    assert_eq!(
        req.reason,
        format!(
            "This session has spent {} of its $1.40 limit. Reset its spend to $0 and continue?",
            crate::narrative::dollars(spent)
        )
    );
    let b = req.budget.clone().unwrap();
    assert_eq!((b.spent_usd, b.limit_usd), (micros_to_usd(spent), 1.4));
    assert!(b.needed_usd > 1.28, "{b:?}");
    let pending = r.core.pending_confirms(&sid).unwrap();
    assert_eq!(pending.len(), 1);
    assert_eq!(
        (
            pending[0].correlation_id.as_str(),
            pending[0].reason.as_str()
        ),
        (q.as_str(), req.reason.as_str())
    );
    let listed = r.core.session_list().unwrap();
    assert_eq!(
        listed
            .iter()
            .find(|s| s.session_id == sid)
            .unwrap()
            .pending_confirms,
        1
    );
    assert_eq!(ledgered(&r, "budget.asked").len(), 1);
    let lifetime = r
        .core
        .store
        .get_session::<SessionRecord>(&sid)
        .unwrap()
        .unwrap()
        .cost_usd;
    assert!(
        (lifetime - micros_to_usd(spent)).abs() < 1e-5,
        "{lifetime} vs {spent}"
    );

    let ans = r
        .core
        .confirm_action(&q, true, None, "discord:eddie")
        .unwrap();
    assert!(ans.approved && ans.resumes);
    let e = r.core.kernel.execution(&exec).unwrap().unwrap();
    assert_eq!(
        (e.state, e.budget.spent_micros, e.budget.resets),
        (ExecState::Queued, 0, 1)
    );
    assert!(e.resume_pending, "the driver takes the next turn");
    let s = r
        .core
        .store
        .get_session::<SessionRecord>(&sid)
        .unwrap()
        .unwrap();
    assert_eq!(
        s.cost_usd, lifetime,
        "a reset never lowers the lifetime cost"
    );
    assert_eq!(r.core.health().cost_usd_total, lifetime);
    let reset = ledgered(&r, "budget.reset");
    assert_eq!(reset.len(), 1);
    assert_eq!(reset[0]["by"], "discord:eddie");
    assert_eq!(reset[0]["spent_before_usd"], json!(micros_to_usd(spent)));
    assert_eq!(reset[0]["limit_usd"], json!(1.4));
    assert_eq!(reset[0]["correlation_id"], json!(q));
    assert!(r.core.pending_confirms(&sid).unwrap().is_empty());

    let cont = r.core.continue_execution(&exec).await.unwrap().unwrap();
    assert!(cont.continuation);
    assert_eq!(cont.output, "The diff is one line.");
    assert_eq!(r.fake.requests().len(), 2, "the waiting call ran");
    let e = r.core.kernel.execution(&exec).unwrap().unwrap();
    assert_eq!(e.state, ExecState::Waiting);
    assert!(
        e.budget.spent_micros > 0 && e.budget.spent_micros < spent,
        "the call after the reset is the new spend: {}",
        e.budget.spent_micros
    );
    let s = r
        .core
        .store
        .get_session::<SessionRecord>(&sid)
        .unwrap()
        .unwrap();
    let now = lifetime + cont.cost_usd.unwrap();
    assert!((s.cost_usd - now).abs() < 1e-9, "{} vs {now}", s.cost_usd);
    assert!((r.core.health().cost_usd_total - now).abs() < 1e-9);
}

/// A decline, or no answer, is not a hard no: the session keeps waiting on
/// its budget, and nothing resumes. Its next message asks again; a message
/// that comes while a question is open replaces it. Cancel still ends it,
/// and closes the question.
#[tokio::test]
async fn a_declined_reset_keeps_waiting_and_the_next_message_asks_again() {
    use theseus_kernel::{ActionState, ExecState, Wake};
    let r = over_budget_rig(vec![]);
    let res = turn(&r.core, None, "diff these").await;
    let (sid, exec) = (res.session_id.clone(), res.execution_id.clone().unwrap());
    let q1 = res.awaiting_confirm.clone().unwrap();
    let spent = r
        .core
        .kernel
        .execution(&exec)
        .unwrap()
        .unwrap()
        .budget
        .spent_micros;
    let ans = r
        .core
        .confirm_action(&q1, false, None, "discord:eddie")
        .unwrap();
    assert!(!ans.approved && !ans.resumes, "a decline resumes nothing");
    let e = r.core.kernel.execution(&exec).unwrap().unwrap();
    assert_eq!(
        (
            e.state,
            e.wake.clone(),
            e.budget.spent_micros,
            e.budget.resets
        ),
        (
            ExecState::Waiting,
            Some(Wake::Budget {
                correlation_id: q1.clone()
            }),
            spent,
            0
        )
    );
    assert!(!e.resume_pending);
    assert!(r.core.pending_confirms(&sid).unwrap().is_empty());

    let res2 = turn(&r.core, Some(&sid), "go on").await;
    assert_eq!(res2.stop_reason, "budget");
    let q2 = res2.awaiting_confirm.clone().unwrap();
    assert_ne!(q2, q1, "a new question");
    assert_eq!(r.fake.requests().len(), 1, "no call ran over the limit");
    let res3 = turn(&r.core, Some(&sid), "still there?").await;
    let q3 = res3.awaiting_confirm.clone().unwrap();
    let a2 = r.core.kernel.action(&q2).unwrap().unwrap();
    assert_eq!(a2.state, ActionState::Cancelled);
    assert!(
        a2.resolution.as_deref().unwrap().contains("superseded"),
        "{:?}",
        a2.resolution
    );
    let pending = r.core.pending_confirms(&sid).unwrap();
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].correlation_id, q3);
    assert_eq!(ledgered(&r, "budget.asked").len(), 3);
    assert!(ledgered(&r, "budget.reset").is_empty());
    assert!(
        r.core.confirm_action(&q2, true, None, "late").is_err(),
        "a replaced question is closed"
    );

    r.core.cancel_execution(&exec, "discord:eddie").unwrap();
    assert_eq!(
        r.core.kernel.action(&q3).unwrap().unwrap().state,
        ActionState::Cancelled
    );
    assert!(r.core.pending_confirms(&sid).unwrap().is_empty());
}

/// One call's reservation and settlement in dollars, by hand from the
/// catalog (Sonnet 5.5: $2 in, $10 out, $0.20 cache read, $2.50 cache write
/// per million tokens). The call reserves 128,000 output tokens at $10 plus
/// the compiler's input estimate at $2, and settles at 1,200 in, 900 out,
/// 40,000 cache reads, and 3,000 cache writes: $0.0024 + $0.009 + $0.008 +
/// $0.0075 = $0.0269, in the budget, the session, and the ledger alike.
#[tokio::test]
async fn one_calls_reservation_and_settlement_match_the_catalog_by_hand() {
    let usage = theseus_protocol::Usage {
        input_tokens: 1_200,
        output_tokens: 900,
        cache_read_input_tokens: 40_000,
        cache_creation_input_tokens: 3_000,
    };
    let r = rig(vec![Scripted::Billed {
        usage,
        then: Box::new(Scripted::text("Priced.")),
    }]);
    let res = turn(&r.core, None, "price one call").await;
    assert_eq!(res.output, "Priced.");
    let est = ledgered(&r, "context.compiled")[0]["est_tokens"]
        .as_u64()
        .unwrap();
    let planned = ledgered(&r, "action.planned");
    let call = planned
        .iter()
        .find(|p| p["tool"] == "provider.messages")
        .unwrap();
    let reserved = 128_000 * 10 + est * 2;
    assert_eq!(
        call["reserved_usd"],
        json!(theseus_kernel::micros_to_usd(reserved)),
        "{call}"
    );
    let e = r
        .core
        .kernel
        .execution(res.execution_id.as_deref().unwrap())
        .unwrap()
        .unwrap();
    assert_eq!(e.budget.spent_micros, 2_400 + 9_000 + 8_000 + 7_500);
    assert_eq!(e.budget.reserved_micros, 0);
    let s = r
        .core
        .store
        .get_session::<SessionRecord>(&res.session_id)
        .unwrap()
        .unwrap();
    assert!((s.cost_usd - 0.0269).abs() < 1e-12, "{}", s.cost_usd);
    let row = &ledgered(&r, "provider.call")[0];
    assert!(
        (row["cost_usd"].as_f64().unwrap() - 0.0269).abs() < 1e-12,
        "{row}"
    );
    assert!((res.cost_usd.unwrap() - 0.0269).abs() < 1e-12);
}

/// A model with no price never runs (budgets are dollars): the turn fails
/// with the class `unpriced` and says what to add to the config.
#[tokio::test]
async fn a_model_with_no_price_is_not_called() {
    let r = rig(vec![Scripted::text("never")]);
    let rec = SessionRecord::new(SessionKind::Conversation, None);
    r.core.store.put_session(&rec.session_id, &rec).unwrap();
    let (live, _) = r.core.live_profile();
    let target = r
        .core
        .runner
        .resolve_target(&live, None, None, Some("mystery-model"))
        .unwrap();
    let err = r
        .core
        .runner
        .run(TurnRequest {
            session: rec,
            input: Some("hi".into()),
            target,
            sink: EventSink::new(r.core.bus.clone(), "x", None),
            author: "test".into(),
            recompile: None,
        })
        .await
        .unwrap_err();
    let te = err.downcast_ref::<crate::turn::TurnError>().unwrap();
    assert_eq!(te.class, "unpriced");
    assert!(format!("{:#}", te.source).contains("[catalog.\"mystery-model\"]"));
    assert!(r.fake.requests().is_empty());
}
