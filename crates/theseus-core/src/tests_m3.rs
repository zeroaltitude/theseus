//! M3 integration tests: scripted tool loops through the whole core — nodes,
//! compilation, the gate, confirm and deny, background jobs with late results,
//! continuation turns, and memory across a restart.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

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
    fake: Arc<FakeProvider>,
    root: PathBuf,
    _dir: tempfile::TempDir,
}

fn config(root: &Path, state: &Path) -> Config {
    let mut cfg = Config::example();
    cfg.server.state_dir = state.to_string_lossy().into_owned();
    cfg.tools.projects_dir = Some(root.to_string_lossy().into_owned());
    cfg.tools.roots = vec![];
    cfg.tools.deny_paths = vec![root.join("secret").to_string_lossy().into_owned()];
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
    assert_eq!(rs[0].0, ResultStatus::Denied);
    assert!(
        rs[0].1.contains("declined") && rs[0].1.contains("not now"),
        "{rs:?}"
    );

    let res2 = turn(&r.core, Some(&res.session_id), "write b").await;
    assert!(res2.awaiting_confirm.is_some());
    // The operator types instead of confirming: the pending write is denied as superseded.
    let res3 = turn(&r.core, Some(&res.session_id), "actually, don't").await;
    assert_eq!(res3.output, "Okay, never mind then.");
    assert!(!r.root.join("b.txt").exists());
    let rs = results(&r.core, &res.session_id);
    assert_eq!(rs[1].0, ResultStatus::Denied);
    assert!(rs[1].1.contains("new message"), "{rs:?}");
    // The request that followed has the denial before the new input, in one user message.
    let last = r.fake.requests().pop().unwrap();
    let final_user = last.messages.last().unwrap();
    assert_eq!(final_user["content"][0]["type"], "tool_result");
    assert_eq!(final_user["content"][1]["text"], "actually, don't");
}

#[tokio::test]
async fn outside_the_roots_and_protected_paths_are_denied_with_a_clear_reason() {
    let r = rig(vec![
        Scripted::tools(
            "",
            &[
                ("t1", "fs_read", json!({"path": "/etc/hostname"})),
                ("t2", "fs_read", json!({"path": "secret/key.pem"})),
                ("t3", "fs_read", json!({"path": 42})),
                ("t4", "no_such_tool", json!({})),
            ],
        ),
        Scripted::text("Those were refused."),
    ]);
    let res = turn(&r.core, None, "read things").await;
    assert_eq!(res.loops, 2);
    let rs = results(&r.core, &res.session_id);
    assert_eq!(rs.len(), 4);
    assert_eq!(rs[0].0, ResultStatus::Denied);
    assert!(
        rs[0].1.contains("outside the workspace roots"),
        "{}",
        rs[0].1
    );
    assert_eq!(rs[1].0, ResultStatus::Denied);
    assert!(rs[1].1.contains("protected"), "{}", rs[1].1);
    assert_eq!(rs[2].0, ResultStatus::Error);
    assert!(rs[2].1.starts_with("Invalid input"), "{}", rs[2].1);
    assert_eq!(rs[3].0, ResultStatus::Error);
    assert!(rs[3].1.contains("Unknown tool"));
    let denied = r
        .core
        .store
        .ledger_tail::<crate::ledger::LedgerRow>(200)
        .unwrap()
        .into_iter()
        .filter(|(_, row)| row.kind == "tool.denied")
        .count();
    assert_eq!(denied, 2);
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
    assert_eq!(rs[0].0, ResultStatus::Denied, "declined, never run: {rs:?}");
}
