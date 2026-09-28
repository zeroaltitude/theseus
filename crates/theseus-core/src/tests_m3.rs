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
use crate::policy::Mode;
use crate::provider::{FakeProvider, Scripted};
use crate::session::SessionRecord;
use crate::store::Store;
use crate::turn::TurnRequest;
use crate::{Config, Core};

pub(crate) struct Rig {
    pub(crate) core: Arc<Core>,
    pub(crate) fake: Arc<FakeProvider>,
    pub(crate) root: PathBuf,
    pub(crate) _dir: tempfile::TempDir,
}

fn config(root: &Path, state: &Path) -> Config {
    let mut cfg = Config::example();
    cfg.server.state_dir = state.to_string_lossy().into_owned();
    cfg.tools.projects_dir = Some(root.to_string_lossy().into_owned());
    cfg.tools.roots = vec![];
    cfg.tools.deny_paths = vec![root.join("secret").to_string_lossy().into_owned()];
    cfg.tools.proc_sync_secs = 10;
    // The template is a deployment (enforcement = notify); these scenarios
    // test the gate's own bands, so they run at the built-in level.
    cfg.policy.enforcement = crate::policy::Enforcement::Strict;
    cfg
}

pub(crate) fn rig_with(script: Vec<Scripted>, tweak: impl FnOnce(&mut Config)) -> Rig {
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

pub(crate) async fn turn(core: &Arc<Core>, session: Option<&str>, input: &str) -> TurnSubmitResult {
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

pub(crate) fn results(core: &Core, sid: &str) -> Vec<(ResultStatus, String)> {
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
            cfg.policy.run = Mode::Allow;
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

#[tokio::test]
async fn enforcement_open_runs_what_would_ask_or_be_refused_and_says_so_but_the_floor_holds() {
    let outside = tempfile::tempdir().unwrap();
    std::fs::write(outside.path().join("note.txt"), "outside the workspace\n").unwrap();
    let outside_file = outside
        .path()
        .join("note.txt")
        .to_string_lossy()
        .into_owned();
    let r = rig_with(
        vec![
            Scripted::tools(
                "",
                &[
                    (
                        "t1",
                        "fs_write",
                        json!({"path": "out.txt", "content": "no card\n"}),
                    ),
                    ("t2", "fs_read", json!({"path": outside_file})),
                    ("t3", "proc_run", json!({"argv": ["theseusd", "config"]})),
                ],
            ),
            Scripted::text("Done, and you were told."),
        ],
        |cfg| cfg.policy.enforcement = crate::policy::Enforcement::Open,
    );
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    let res = {
        let rec = SessionRecord::new(SessionKind::Conversation, None);
        r.core.store.put_session(&rec.session_id, &rec).unwrap();
        r.core.bus.watch(&rec.session_id, "watcher", tx);
        turn(
            &r.core,
            Some(&rec.session_id),
            "write, read outside, and run theseusd",
        )
        .await
    };
    assert!(
        res.awaiting_confirm.is_none(),
        "enforcement = open never parks"
    );
    assert_eq!(
        std::fs::read_to_string(r.root.join("out.txt")).unwrap(),
        "no card\n",
        "the write ran without a card"
    );
    let rs = results(&r.core, &res.session_id);
    assert_eq!(rs[0].0, ResultStatus::Ok);
    assert_eq!(
        rs[1].0,
        ResultStatus::Ok,
        "off_policy = notify ran the read outside the roots"
    );
    assert!(rs[1].1.contains("outside the workspace"), "{}", rs[1].1);
    assert_eq!(
        rs[2].0,
        ResultStatus::Denied,
        "the floor holds: theseusd never runs"
    );

    let mut notices = vec![];
    while let Ok(m) = rx.try_recv() {
        if let theseus_protocol::Message::Notification(n) = m {
            if n.method == theseus_protocol::notify::POLICY_NOTIFIED {
                notices.push(n.params);
            }
        }
    }
    let kinds: Vec<&str> = notices
        .iter()
        .map(|n| n["kind"].as_str().unwrap())
        .collect();
    assert_eq!(kinds, vec!["approval_skipped", "off_policy"], "{notices:?}");
    assert_eq!(notices[0]["tool"], "fs.write");
    assert_eq!(notices[0]["setting"], "enforcement = open");
    assert!(notices[1]["rule"]
        .as_str()
        .unwrap()
        .contains("outside the workspace roots"));
    let ledgered = r
        .core
        .store
        .ledger_tail::<crate::ledger::LedgerRow>(300)
        .unwrap()
        .into_iter()
        .filter(|(_, row)| row.kind == "tool.notified")
        .count();
    assert_eq!(ledgered, 2);
}

#[tokio::test]
async fn enforcement_ask_turns_a_refusal_into_a_marked_confirm() {
    let r = rig_with(
        vec![
            Scripted::tools("", &[("t1", "fs_read", json!({"path": "/etc/hostname"}))]),
            Scripted::text("Read it."),
        ],
        |cfg| cfg.policy.enforcement = crate::policy::Enforcement::Ask,
    );
    let res = turn(&r.core, None, "read /etc/hostname").await;
    assert!(
        res.awaiting_confirm.is_some(),
        "ask parks instead of refusing"
    );
    let pending = r.core.pending_confirms(&res.session_id).unwrap();
    assert_eq!(pending.len(), 1);
    assert!(
        pending[0].against_policy,
        "the card says it is against policy"
    );
    assert!(
        pending[0].reason.starts_with("against policy"),
        "{}",
        pending[0].reason
    );
}
