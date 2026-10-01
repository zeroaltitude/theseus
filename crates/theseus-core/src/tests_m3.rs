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
    rig_full(script, tweak, vec![])
}

/// A rig whose tool runtime also has `toollets`, registered after the
/// built-ins, so one may stand in for a built-in (theseus-a60).
fn rig_full(
    script: Vec<Scripted>,
    tweak: impl FnOnce(&mut Config),
    toollets: Vec<Arc<dyn theseus_tools::Tool>>,
) -> Rig {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("work");
    std::fs::create_dir_all(&root).unwrap();
    let root = root.canonicalize().unwrap();
    let mut cfg = config(&root, dir.path());
    tweak(&mut cfg);
    let store = Store::open(&dir.path().join("store")).unwrap();
    let fake = Arc::new(FakeProvider::scripted(script));
    let core = Core::build(crate::rpc::Parts {
        toollets,
        ..crate::rpc::Parts::for_tests(cfg, fake.clone(), store)
    })
    .unwrap();
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
            attachments: vec![],
            arrived: None,
            config_wait_us: 0,
            reply_to: None,
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
    r.core.store.append(&[rec]).unwrap();
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

/// Store an action again as a binary from before theseus-0g4 wrote it, with
/// no `proposal` on it.
fn stored_without_its_proposal(r: &Rig, corr: &str) {
    use theseus_store::{kinds, NewRecord};
    let a = r.core.kernel.action(corr).unwrap().unwrap();
    let mut v = serde_json::to_value(&a).unwrap();
    assert!(v.as_object_mut().unwrap().remove("proposal").is_some());
    let rec = NewRecord::json(kinds::ACTION, Some(corr), &v)
        .unwrap()
        .scoped(&a.session_id);
    r.core.store.append(&[rec]).unwrap();
    assert!(r
        .core
        .kernel
        .action(corr)
        .unwrap()
        .unwrap()
        .proposal
        .is_none());
}

/// A call that waits keeps its proposal on the action (theseus-0g4). One
/// stored before that, with only its node's gate record to go by, is still
/// found by every reader (the history, the session list, `confirm.list`) and
/// answered: the confirm binds the node's proposal, and the driver runs it.
#[tokio::test]
async fn a_confirm_pending_from_before_theseus_0g4_is_found_and_answered() {
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
    let corr = res.awaiting_confirm.clone().unwrap();
    let kept = r.core.kernel.action(&corr).unwrap().unwrap().proposal;
    assert_eq!(kept.map(|p| p.tool), Some("fs.write".to_string()));
    stored_without_its_proposal(&r, &corr);

    let pending = r.core.pending_confirms(&res.session_id).unwrap();
    assert_eq!(pending.len(), 1);
    assert_eq!(
        (pending[0].correlation_id.as_str(), pending[0].tool.as_str()),
        (corr.as_str(), "fs.write")
    );
    assert!(
        pending[0].reason.contains("out.txt"),
        "{}",
        pending[0].reason
    );
    let all = r.core.confirm_list().unwrap();
    assert_eq!(all.len(), 1);
    assert_eq!(all[0].correlation_id, corr);
    let listed = r.core.session_list().unwrap();
    assert_eq!(listed[0].pending_confirms, 1);

    r.core.confirm_action(&corr, true, None, "test").unwrap();
    assert!(r.core.confirm_list().unwrap().is_empty());
    let cont = r
        .core
        .continue_execution(res.execution_id.as_deref().unwrap())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(cont.output, "Written.");
    assert_eq!(
        std::fs::read_to_string(r.root.join("out.txt")).unwrap(),
        "made by theseus\n"
    );
}

/// Approved under the old binary and resumed by the new one: the resumed turn
/// authorizes with the proposal on the node's gate record.
#[tokio::test]
async fn a_call_approved_before_theseus_0g4_resumes_from_its_node() {
    let r = rig(vec![
        Scripted::tools(
            "",
            &[("t1", "fs_write", json!({"path": "b.txt", "content": "b\n"}))],
        ),
        Scripted::text("Written."),
    ]);
    let res = turn(&r.core, None, "write b.txt").await;
    let corr = res.awaiting_confirm.clone().unwrap();
    r.core.confirm_action(&corr, true, None, "test").unwrap();
    stored_without_its_proposal(&r, &corr);
    let cont = r
        .core
        .continue_execution(res.execution_id.as_deref().unwrap())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(cont.output, "Written.");
    assert_eq!(results(&r.core, &res.session_id)[0].0, ResultStatus::Ok);
    assert_eq!(
        std::fs::read_to_string(r.root.join("b.txt")).unwrap(),
        "b\n"
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

/// Review 2's H3 (theseus-wz2): a job's raw output, which the scrubber never
/// saw, is 0600 while it waits and is deleted once its result is written: an
/// in-turn job's at once, a background job's when its late result is
/// absorbed. No node names the file, and no client is sent its path.
#[tokio::test]
async fn a_jobs_raw_output_is_deleted_once_its_result_is_written() {
    use std::os::unix::fs::PermissionsExt;
    let r = rig_with(
        vec![
            Scripted::tools(
                "",
                &[("t1", "proc_run", json!({"argv": ["echo", "in the turn"]}))],
            ),
            Scripted::text("Done."),
            Scripted::tools(
                "",
                &[(
                    "t2",
                    "proc_run",
                    json!({"argv": ["bash", "-c", "sleep 1.5; echo later"]}),
                )],
            ),
            Scripted::text("Started."),
            Scripted::text("It finished."),
        ],
        |cfg| {
            cfg.policy.tools.insert("proc.run".into(), Posture::Open);
            cfg.tools.proc_sync_secs = 1;
        },
    );
    let outputs = || -> Vec<PathBuf> {
        std::fs::read_dir(r.core.spool.dir().join("results"))
            .unwrap()
            .map(|e| e.unwrap().path())
            .collect()
    };
    let res = turn(&r.core, None, "echo something").await;
    assert_eq!(res.output, "Done.");
    assert!(results(&r.core, &res.session_id)[0]
        .1
        .contains("in the turn"));
    assert_eq!(outputs(), Vec::<PathBuf>::new(), "the in-turn job's output");

    let res2 = turn(&r.core, Some(&res.session_id), "run the slow one").await;
    assert_eq!(res2.output, "Started.");
    let exec = res2.execution_id.clone().unwrap();
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
    let waiting = outputs();
    assert_eq!(waiting.len(), 1, "the background job's output waits");
    let mode = std::fs::metadata(&waiting[0]).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode, 0o600);
    let cont = r.core.continue_execution(&exec).await.unwrap().unwrap();
    assert_eq!(cont.output, "It finished.");
    assert!(results(&r.core, &res.session_id)[2].1.contains("later"));
    assert_eq!(outputs(), Vec::<PathBuf>::new(), "absorbed, then gone");

    let nodes = r.core.store.transcript(&res.session_id).unwrap();
    let mut jobs = 0;
    for (_, n) in &nodes {
        if let Body::ToolResult {
            full_ref,
            correlation_id: Some(corr),
            ..
        } = &n.body
        {
            jobs += 1;
            assert_eq!(full_ref, &None);
            let a = r.core.kernel.action(corr).unwrap().unwrap();
            assert!(
                a.result_ref.as_deref().is_some_and(|p| p.starts_with('/')),
                "a job's"
            );
            let info = Core::action_info(&a);
            assert_eq!(info.result_ref, None, "no client gets the spool's path");
        }
    }
    assert!(jobs >= 2, "{nodes:?}");
}

/// Review 2's R3 (theseus-102): a job that prints past its output cap runs
/// to its end, and its result says what it printed, what was dropped, and
/// how to get the rest (proc.run's own words, theseus-46v), then which bytes
/// of what was kept follow. The completion's detail carries the counts.
#[tokio::test]
async fn a_job_past_its_output_cap_says_what_was_dropped_and_how_to_get_the_rest() {
    // 20,000 lines of 21 bytes: 420,000 bytes against a cap of 65,536.
    let r = rig_with(
        vec![
            Scripted::tools(
                "",
                &[(
                    "t1",
                    "proc_run",
                    json!({"argv": ["bash", "-c", "yes 'a line of the roster' | head -n 20000; : > ran"]}),
                )],
            ),
            Scripted::text("Done."),
        ],
        |cfg| {
            cfg.policy.tools.insert("proc.run".into(), Posture::Open);
            cfg.tools.job_output_max_bytes = 65_536;
        },
    );
    let res = turn(&r.core, None, "print the roster").await;
    assert_eq!(res.output, "Done.");
    assert!(r.root.join("ran").exists(), "the job ran to its end");
    let (status, text) = results(&r.core, &res.session_id).remove(0);
    assert_eq!(status, ResultStatus::Ok);
    assert!(
        text.starts_with(
            "[exit code 0]\n[truncated: it printed 420,000 bytes, and the 354,464 bytes past its \
             output cap of 65,536 bytes were dropped; its output is not kept: run it again \
             printing less, or with its output sent to a file that fs_read then reads in ranges]\n\
             a line of the roster\n"
        ),
        "{text}"
    );
    let node = r
        .core
        .store
        .transcript(&res.session_id)
        .unwrap()
        .into_iter()
        .find_map(|(_, n)| match &n.body {
            Body::ToolResult { meta, .. } => Some(meta.clone()),
            _ => None,
        })
        .unwrap();
    assert_eq!(node["detail"]["dropped"], 354_464, "{node}");
    assert_eq!(node["detail"]["bytes"], 65_536, "{node}");
}

/// Review 2's R3 (theseus-102): below `[server] disk_floor_mb` a job is not
/// started. Its result gives the reason, to the model and every surface, a
/// `job.refused` row records the numbers, and health says the disk is below
/// the floor. The space comes from a stand-in for `statvfs`: no test fills a
/// disk.
#[tokio::test]
async fn a_job_below_the_disk_floor_is_refused_with_its_reason() {
    let r = rig_with(
        vec![
            Scripted::tools("", &[("t1", "proc_run", json!({"argv": ["touch", "ran"]}))]),
            Scripted::text("Understood."),
        ],
        |cfg| {
            cfg.policy.tools.insert("proc.run".into(), Posture::Open);
            cfg.server.disk_floor_mb = 1024;
            cfg.server.disk_warn_mb = 5120;
        },
    );
    let space = crate::disk::FixedSpace::new(812, 100_000);
    r.core.tools.disk.set_probe(space.clone());
    let res = turn(&r.core, None, "touch a file").await;
    assert_eq!(res.output, "Understood.");
    assert!(!r.root.join("ran").exists(), "no job started");
    let refusal = "the disk under the state dir has 812 MB free, below the floor of 1,024 MB, so \
                   the job was not started";
    assert_eq!(
        results(&r.core, &res.session_id),
        vec![(ResultStatus::Error, refusal.to_string())]
    );
    let rows = ledgered(&r, "job.refused");
    assert_eq!(rows.len(), 1, "{rows:?}");
    assert_eq!(rows[0]["free_mb"], 812);
    assert_eq!(rows[0]["floor_mb"], 1024);
    assert_eq!(rows[0]["tool"], "proc.run");
    let corr = rows[0]["correlation_id"].as_str().unwrap();
    let a = r.core.kernel.action(corr).unwrap().unwrap();
    assert_eq!(a.state, theseus_kernel::ActionState::Failed);
    let disk = r.core.health().disk;
    assert_eq!((disk.state.as_str(), disk.free_mb), ("below_floor", 812));
    // Above the floor, the next job starts.
    space.set_free_mb(4000);
    assert_eq!(r.core.health().disk.state, "low");
}

/// theseus-2ij: the spool's sweep removes the raw output no result will
/// absorb, by what the store says, and keeps what a turn may still read:
/// - absorbed: an in-turn job's result was written, and its file came back
///   (as a crash between the frame and the delete would leave it);
/// - ended: a background job finished, and its execution was cancelled
///   before a turn read its late result;
/// - unknown: a file from before H3, 0644, three days old, no job in the
///   store;
/// - kept, running: a job still running, and one whose execution was
///   cancelled (its action too) while its wrapper still lives: here a
///   stand-in, a process whose command line names the job, with its pid in
///   the spool, since the in-process launcher has no wrapper to keep alive;
/// - kept, pending: a background job that finished in a session that goes on;
/// - kept, young: a file the store does not know, written just now.
///
/// One `spool.swept` row says so with counts and bytes, and health shows the
/// sweep.
#[tokio::test]
async fn the_sweep_removes_raw_output_no_result_will_absorb_and_keeps_the_rest() {
    use std::os::unix::fs::PermissionsExt;
    let r = rig_with(
        vec![
            Scripted::tools(
                "",
                &[("t1", "proc_run", json!({"argv": ["echo", "in the turn"]}))],
            ),
            Scripted::text("Done."),
            Scripted::tools(
                "",
                &[(
                    "t2",
                    "proc_run",
                    json!({"argv": ["bash", "-c", "sleep 1.2; echo ended"]}),
                )],
            ),
            Scripted::text("Started."),
            Scripted::tools(
                "",
                &[(
                    "t3",
                    "proc_run",
                    json!({"argv": ["bash", "-c", "sleep 1.2; echo pending"]}),
                )],
            ),
            Scripted::text("Started."),
            Scripted::tools(
                "",
                &[(
                    "t4",
                    "proc_run",
                    json!({"argv": ["bash", "-c", "echo running; while [ -e running.marker ]; do sleep 0.05; done"]}),
                )],
            ),
            Scripted::text("Started."),
            Scripted::tools(
                "",
                &[(
                    "t5",
                    "proc_run",
                    json!({"argv": ["bash", "-c", "echo running on; while [ -e running.marker ]; do sleep 0.05; done"]}),
                )],
            ),
            Scripted::text("Started."),
        ],
        |cfg| {
            cfg.policy.tools.insert("proc.run".into(), Posture::Open);
            cfg.tools.proc_sync_secs = 1;
        },
    );
    std::fs::write(r.root.join("running.marker"), "").unwrap();
    let results_dir = r.core.spool.dir().join("results");
    let out = |corr: &str| results_dir.join(format!("{corr}.out"));
    let job_of = |sid: &str| -> String {
        r.core
            .kernel
            .actions()
            .unwrap()
            .into_iter()
            .find(|a| a.session_id == sid && a.tool == "proc.run")
            .unwrap()
            .correlation_id
    };

    // Absorbed, then the file back, as a crash before the delete leaves it.
    let a = turn(&r.core, None, "echo something").await;
    let absorbed = job_of(&a.session_id);
    assert!(
        !out(&absorbed).exists(),
        "deleted once its result was written"
    );
    std::fs::write(out(&absorbed), "in the turn\n").unwrap();

    // Two background jobs that finish; the first's execution is cancelled.
    let b = turn(&r.core, None, "start the first").await;
    let ended = job_of(&b.session_id);
    let c = turn(&r.core, None, "start the second").await;
    let pending = job_of(&c.session_id);
    for _ in 0..100 {
        tokio::time::sleep(Duration::from_millis(50)).await;
        r.core.heartbeat("test");
        let settled = |corr: &str| {
            r.core
                .kernel
                .action(corr)
                .unwrap()
                .unwrap()
                .state
                .is_settled()
        };
        if settled(&ended) && settled(&pending) {
            break;
        }
    }
    r.core
        .cancel_execution(b.execution_id.as_deref().unwrap(), "test")
        .await
        .unwrap();

    // A job that still runs, and one that runs on in a cancelled execution.
    let d = turn(&r.core, None, "start the third").await;
    let running = job_of(&d.session_id);
    let e = turn(&r.core, None, "start the fourth").await;
    let running_on = job_of(&e.session_id);
    r.core
        .cancel_execution(e.execution_id.as_deref().unwrap(), "test")
        .await
        .unwrap();
    let a = r.core.kernel.action(&running_on).unwrap().unwrap();
    assert_eq!(a.state, theseus_kernel::ActionState::Cancelled);
    std::fs::write(
        r.root.join("job-wrapper"),
        "while [ -e running.marker ]; do sleep 0.05; done\n",
    )
    .unwrap();
    let mut wrapper = std::process::Command::new("sh")
        .args(["job-wrapper", "--correlation-id", &running_on])
        .current_dir(&r.root)
        .spawn()
        .unwrap();
    r.core.spool.write_pid(&running_on, wrapper.id()).unwrap();
    assert!(theseus_kernel::job::wrapper_alive(
        wrapper.id(),
        &running_on
    ));

    // Two files no job in the store owns: one from before H3, one just made.
    let pre_h3 = results_dir.join("act_invented_pre_h3.out");
    std::fs::write(&pre_h3, "stale\n").unwrap();
    std::fs::set_permissions(&pre_h3, std::fs::Permissions::from_mode(0o644)).unwrap();
    std::fs::File::options()
        .write(true)
        .open(&pre_h3)
        .unwrap()
        .set_modified(std::time::SystemTime::now() - Duration::from_secs(3 * 24 * 3600))
        .unwrap();
    let young = results_dir.join("act_invented_young.out");
    std::fs::write(&young, "new\n").unwrap();

    for (what, path) in [
        ("absorbed", out(&absorbed)),
        ("ended", out(&ended)),
        ("pending", out(&pending)),
        ("running", out(&running)),
        ("running on", out(&running_on)),
    ] {
        assert!(path.exists(), "{what}'s output is there before the sweep");
    }
    let s = r.core.sweep_spool(true);
    assert!(!out(&absorbed).exists(), "absorbed");
    assert!(!out(&ended).exists(), "ended");
    assert!(!pre_h3.exists(), "pre-H3");
    assert!(out(&pending).exists(), "pending is kept");
    assert!(out(&running).exists(), "running is kept");
    assert!(
        out(&running_on).exists(),
        "running on in a cancelled execution is kept"
    );
    assert!(young.exists(), "young is kept");
    let by = |m: &std::collections::BTreeMap<String, u64>| -> Vec<(String, u64)> {
        m.iter().map(|(k, v)| (k.clone(), *v)).collect()
    };
    assert_eq!(
        by(&s.removed_by),
        [
            ("absorbed".into(), 1),
            ("ended".into(), 1),
            ("unknown".into(), 1)
        ]
    );
    assert_eq!(
        by(&s.kept_by),
        [
            ("pending".into(), 1),
            ("running".into(), 2),
            ("young".into(), 1)
        ]
    );
    assert_eq!((s.removed, s.kept), (3, 4));
    assert_eq!(
        s.removed_bytes,
        ("in the turn\n".len() + "ended\n".len() + "stale\n".len()) as u64
    );
    let rows = ledgered(&r, "spool.swept");
    assert_eq!(rows.len(), 1, "{rows:?}");
    assert_eq!(rows[0]["removed"], 3);
    assert_eq!(rows[0]["removed_by"]["unknown"], 1);
    assert_eq!(r.core.health().spool.last_sweep, Some(s));

    // A later sweep that removes nothing writes no row; health still shows it.
    let again = r.core.sweep_spool(false);
    assert_eq!((again.removed, again.kept), (0, 4));
    assert_eq!(ledgered(&r, "spool.swept").len(), 1);
    assert_eq!(r.core.health().spool.last_sweep, Some(again));
    std::fs::remove_file(r.root.join("running.marker")).unwrap();
    let _ = wrapper.kill();
    let _ = wrapper.wait();
}

#[tokio::test]
async fn a_session_keeps_its_memory_across_a_restart() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("w");
    std::fs::create_dir_all(&root).unwrap();
    let cfg = config(&root.canonicalize().unwrap(), dir.path());
    let sid = {
        let store = Store::open(&dir.path().join("store")).unwrap();
        let fake = Arc::new(FakeProvider::scripted(vec![Scripted::text(
            "My name is Theseus.",
        )]));
        let core = Core::build(crate::rpc::Parts::for_tests(cfg.clone(), fake, store)).unwrap();
        turn(&core, None, "who are you?").await.session_id
    };
    let store = Store::open(&dir.path().join("store")).unwrap();
    let fake = Arc::new(FakeProvider::scripted(vec![Scripted::text(
        "You asked who I am.",
    )]));
    let core = Core::build(crate::rpc::Parts::for_tests(cfg, fake.clone(), store)).unwrap();
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

/// A provider whose calls, while `hold` is set, say they have begun and then
/// wait at a gate: a test's way to act while a turn is inside its call.
struct Gated {
    fake: FakeProvider,
    hold: std::sync::atomic::AtomicBool,
    entered: tokio::sync::mpsc::UnboundedSender<()>,
    gate: tokio::sync::Semaphore,
}

impl crate::provider::Provider for Gated {
    fn name(&self) -> &str {
        "fake"
    }
    fn stream_message<'a>(
        &'a self,
        req: &'a crate::provider::ProviderRequest,
        on_delta: crate::provider::DeltaSink<'a>,
    ) -> crate::provider::ProviderFuture<'a> {
        Box::pin(async move {
            if self.hold.load(std::sync::atomic::Ordering::SeqCst) {
                let _ = self.entered.send(());
                self.gate.acquire().await.unwrap().forget();
            }
            self.fake.stream_message(req, on_delta).await
        })
    }
}

/// theseus-xeo, as filed: a `session.recompile` asked while a turn runs was
/// lost, because the turn wrote back the record it had read when it began.
/// Here the request lands while the turn is inside its provider call, between
/// its read and its write: it survives the turn, and the next turn applies it,
/// once.
#[tokio::test]
async fn a_recompile_asked_during_a_turn_is_kept_for_the_next() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("w");
    std::fs::create_dir_all(&root).unwrap();
    let cfg = config(&root.canonicalize().unwrap(), dir.path());
    let store = Store::open(&dir.path().join("store")).unwrap();
    let (entered, mut began) = tokio::sync::mpsc::unbounded_channel();
    let model = Arc::new(Gated {
        fake: FakeProvider::scripted(vec![
            Scripted::text("one"),
            Scripted::text("two"),
            Scripted::text("three"),
        ]),
        hold: false.into(),
        entered,
        gate: tokio::sync::Semaphore::new(0),
    });
    let core = Core::build(crate::rpc::Parts::for_tests(cfg, model.clone(), store)).unwrap();
    let sid = turn(&core, None, "first").await.session_id;
    model.hold.store(true, std::sync::atomic::Ordering::SeqCst);
    let running = {
        let (core, sid) = (core.clone(), sid.clone());
        tokio::spawn(async move { turn(&core, Some(&sid), "second").await })
    };
    began.recv().await.unwrap();
    assert!(core
        .request_recompile(&sid, crate::compiler::Recompile::Fresh, "test")
        .unwrap());
    model.hold.store(false, std::sync::atomic::Ordering::SeqCst);
    model.gate.add_permits(1);
    running.await.unwrap();
    let rec: SessionRecord = core.store.get_session(&sid).unwrap().unwrap();
    assert_eq!(
        rec.pending_recompile,
        Some(crate::compiler::Recompile::Fresh),
        "the turn's end wrote the request over"
    );
    assert_eq!(rec.turns, 2, "and the turn's books are there too");
    turn(&core, Some(&sid), "third").await;
    let comps = core.store.session_compilations(&sid).unwrap();
    assert_eq!(comps.last().unwrap().trigger, "manual_fresh");
    assert_eq!(comps.len(), 2, "applied by the third turn, once");
    let rec: SessionRecord = core.store.get_session(&sid).unwrap().unwrap();
    assert!(rec.pending_recompile.is_none());
}

/// One writer at a time per session record (theseus-xeo): a recompile's
/// write waits while another writer holds the record between its read and
/// its write, then reads again, so both changes stand.
#[tokio::test]
async fn a_session_records_writers_never_lose_each_others_change() {
    let r = rig(vec![Scripted::text("one")]);
    let sid = turn(&r.core, None, "first").await.session_id;
    let held = r.core.store.lock_session(&sid);
    // The first writer has read the record, and changes its copy.
    let mut copy: SessionRecord = r.core.store.get_session(&sid).unwrap().unwrap();
    copy.turns += 1;
    let second = {
        let (core, sid) = (r.core.clone(), sid.clone());
        std::thread::spawn(move || {
            core.request_recompile(&sid, crate::compiler::Recompile::Transcript, "test")
                .unwrap()
        })
    };
    let t0 = std::time::Instant::now();
    let waited = loop {
        if r.core.store.session_writers_waiting() > 0 {
            break true;
        }
        if second.is_finished() {
            break false;
        }
        assert!(
            t0.elapsed() < Duration::from_secs(10),
            "the second writer neither waited nor wrote"
        );
        std::thread::yield_now();
    };
    r.core.store.put_session(&sid, &copy).unwrap();
    drop(held);
    assert!(second.join().unwrap());
    let rec: SessionRecord = r.core.store.get_session(&sid).unwrap().unwrap();
    assert_eq!(rec.turns, 2, "the first writer's change");
    assert_eq!(
        rec.pending_recompile,
        Some(crate::compiler::Recompile::Transcript),
        "the second's"
    );
    assert!(waited, "the second writer waited for the first's write");
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
        .ledger_tail::<crate::ledger::LedgerRow>(10_000)
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

/// A cancel ends a call that waits for the operator (theseus-w98). The call
/// settles cancelled, its resolution "the execution was cancelled by
/// operator"; the session list counts nothing waiting; the history lists no
/// question, and shows the call's result as the session's next turn would
/// have written it, "Not run: the execution was cancelled by operator.";
/// and an answer finds nothing waiting. Nothing ran.
#[tokio::test]
async fn a_cancel_ends_a_call_waiting_for_approval_and_nothing_counts_it_waiting() {
    let r = rig_with(
        vec![Scripted::tools(
            "",
            &[("t1", "proc_run", json!({"argv": ["echo", "never run"]}))],
        )],
        |cfg| cfg.policy.enforcement = Posture::Approve,
    );
    let res = turn(&r.core, None, "echo something").await;
    let corr = res.awaiting_confirm.clone().expect("approve waits");
    let exec = res.execution_id.clone().unwrap();
    let waiting = |core: &Core| {
        core.session_list()
            .unwrap()
            .into_iter()
            .find(|s| s.session_id == res.session_id)
            .unwrap()
            .pending_confirms
    };
    assert_eq!(waiting(&r.core), 1);

    let (e, to_kill) = r.core.cancel_execution(&exec, "operator").await.unwrap();
    assert_eq!(e.state, theseus_kernel::ExecState::Cancelled);
    assert!(to_kill.is_empty(), "nothing was dispatched");
    let a = r.core.kernel.action(&corr).unwrap().unwrap();
    assert_eq!(a.state, theseus_kernel::ActionState::Cancelled);
    assert_eq!(
        a.resolution.as_deref(),
        Some("the execution was cancelled by operator")
    );
    assert_eq!(
        waiting(&r.core),
        0,
        "the session list counts nothing waiting"
    );
    assert!(r.core.pending_confirms(&res.session_id).unwrap().is_empty());
    assert!(r.core.confirm_list().unwrap().is_empty());
    // The history: its nodes, as `session.history` gives them, end with the
    // call's result, and no question follows them.
    let nodes = r.core.store.session_nodes(&res.session_id).unwrap();
    let (pos, last) = nodes.last().unwrap();
    let shown = Core::node_info(*pos, last);
    assert_eq!(shown.kind, "tool_result");
    assert_eq!(shown.detail["status"], "cancelled");
    assert_eq!(shown.detail["correlation_id"], json!(corr));
    assert_eq!(
        shown.text,
        "Not run: the execution was cancelled by operator."
    );
    assert_eq!(
        results(&r.core, &res.session_id),
        vec![(
            ResultStatus::Cancelled,
            "Not run: the execution was cancelled by operator.".to_string()
        )]
    );
    let e = r
        .core
        .confirm_action(&corr, true, None, "test")
        .expect_err("nothing waits");
    assert!(format!("{e:#}").contains("not waiting"), "{e:#}");
    // A second cancel writes nothing more.
    r.core.cancel_execution(&exec, "operator").await.unwrap();
    assert_eq!(results(&r.core, &res.session_id).len(), 1);
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

/// Thirty notified commands in one turn (theseus-w4f): all run, each posts a
/// notice and a `tool.notified` row, and each `tool.proposed` carries the
/// setting that Discord's tool line names.
#[tokio::test]
async fn thirty_notified_commands_give_thirty_notices_and_ledger_rows() {
    let ids: Vec<String> = (0..30).map(|i| format!("t{i}")).collect();
    let calls: Vec<(&str, &str, Value)> = ids
        .iter()
        .map(|id| (id.as_str(), "proc_run", json!({"argv": ["echo", id]})))
        .collect();
    let r = rig_with(
        vec![
            Scripted::tools("", &calls),
            Scripted::text("All thirty ran."),
        ],
        |cfg| cfg.policy.enforcement = Posture::Notify,
    );
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    let rec = SessionRecord::new(SessionKind::Conversation, None);
    r.core.store.put_session(&rec.session_id, &rec).unwrap();
    r.core.bus.watch(&rec.session_id, "watcher", tx);
    let res = turn(&r.core, Some(&rec.session_id), "echo thirty times").await;
    assert_eq!(res.output, "All thirty ran.");
    let rs = results(&r.core, &res.session_id);
    assert_eq!(rs.len(), 30);
    assert!(rs.iter().all(|(s, _)| *s == ResultStatus::Ok), "{rs:?}");
    let (mut notices, mut settings) = (0, vec![]);
    while let Ok(m) = rx.try_recv() {
        if let theseus_protocol::Message::Notification(n) = m {
            match n.method.as_str() {
                theseus_protocol::notify::POLICY_NOTIFIED => notices += 1,
                theseus_protocol::notify::TOOL_PROPOSED => {
                    settings.push(n.params["gate"]["decision"]["notify"]["setting"].clone())
                }
                _ => {}
            }
        }
    }
    assert_eq!(notices, 30);
    assert_eq!(settings, vec![json!("enforcement = notify"); 30]);
    let rows = ledgered(&r, "tool.notified");
    let mut ledgered_ids: Vec<&str> = rows
        .iter()
        .map(|row| row["tool_use_id"].as_str().unwrap())
        .collect();
    ledgered_ids.sort_unstable();
    let mut want: Vec<&str> = ids.iter().map(String::as_str).collect();
    want.sort_unstable();
    assert_eq!(ledgered_ids, want);
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

/// The ledger kinds written after WAL position `after`, in order.
fn ledger_after(core: &Core, after: u64) -> Vec<String> {
    core.store
        .ledger_tail::<crate::ledger::LedgerRow>(1000)
        .unwrap()
        .into_iter()
        .filter(|(p, _)| *p > after)
        .map(|(_, row)| row.kind)
        .collect()
}

/// Every WAL frame is its own fsync, so each frame on the turn path costs
/// every turn. A plain one-loop turn writes 5: 27 before theseus-hco removed
/// the hook rows, 17 before theseus-qa0 let the rows that are no state
/// transition ride in the next frame and planned, authorized, and dispatched
/// the provider call in one, and 8 before theseus-l6y. That step woke and
/// admitted the input's turn in one frame, stopped queueing the provider
/// call's result (the turn reads it itself, so nothing is left to consume at
/// its end), and put the session write in `end_turn`'s frame. A change that
/// adds one raises this on purpose. The rows keep the order they had when
/// each was its own frame. The template turns the narrative on, and a
/// subscriber is watching: the narrative is never stored, so it adds no
/// frame (theseus-5fy).
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
    let from = r.core.store.last_position();
    let res = turn(&r.core, Some(&first.session_id), "hi").await;
    assert_eq!(res.loops, 1);
    let frames = r.core.store.stats().unwrap().frames_appended - before;
    assert!(frames <= 5, "a plain turn wrote {frames} frames");
    assert_eq!(
        ledger_after(&r.core, from),
        [
            "execution.queued",
            "execution.running",
            "turn.started",
            "context.compiled",
            "loop.started",
            "action.planned",
            "action.authorized",
            "action.dispatched",
            "action.succeeded",
            "provider.call",
            "loop.ended",
            "turn.ended",
            "turn.trace",
            "execution.waiting"
        ]
    );
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

/// A loop with one in-process tool call costs four frames (theseus-qa0): the
/// provider call's plan and its completion, and the tool call's plan (with
/// its node) and its completion (with its result). Before, it cost twelve.
#[tokio::test]
async fn a_loop_with_one_tool_call_costs_four_frames() {
    let r = rig(vec![
        Scripted::text("first"),
        Scripted::text("hello"),
        Scripted::tools(
            "Reading it.",
            &[("t1", "fs_read", json!({"path": "hello.txt"}))],
        ),
        Scripted::text("It says hi."),
    ]);
    std::fs::write(r.root.join("hello.txt"), "hi\n").unwrap();
    let sid = turn(&r.core, None, "warm up").await.session_id;
    let frames = || r.core.store.stats().unwrap().frames_appended;
    let f0 = frames();
    turn(&r.core, Some(&sid), "hi").await;
    let plain = frames() - f0;
    let (f1, from) = (frames(), r.core.store.last_position());
    let res = turn(&r.core, Some(&sid), "read hello.txt").await;
    assert_eq!((res.loops, res.tool_calls), (2, 1));
    let two_loops = frames() - f1;
    assert!(plain <= 8, "a plain turn wrote {plain} frames");
    assert!(
        two_loops <= plain + 4,
        "the second loop and its tool call wrote {} frames",
        two_loops - plain
    );
    let rows = ledger_after(&r.core, from);
    let planned = rows.iter().filter(|k| *k == "action.planned").count();
    assert_eq!(planned, 3, "{rows:?}");
}

/// A turn that faults with a call unanswered still resumes it, after a
/// restart too (theseus-l6y). The provider call's result is the turn's own
/// and is no longer queued, and its queue entry was what requeued a turn
/// that faulted; `run` wakes the execution instead (`execution.queued`, why
/// `fault`). The fault: the frame that plans the model's `fs_read` fails, as
/// a full disk would, while the call is gated.
#[tokio::test]
async fn a_turn_that_faults_with_a_call_unanswered_resumes_it_after_a_restart() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("w");
    std::fs::create_dir_all(&root).unwrap();
    let root = root.canonicalize().unwrap();
    std::fs::write(root.join("hello.txt"), "hi\n").unwrap();
    let cfg = config(&root, dir.path());
    let (sid, exec) = {
        let store = Store::open(&dir.path().join("store")).unwrap();
        let fake = Arc::new(FakeProvider::scripted(vec![Scripted::tools(
            "Reading it.",
            &[("t1", "fs_read", json!({"path": "hello.txt"}))],
        )]));
        let core = Core::build(crate::rpc::Parts::for_tests(cfg.clone(), fake, store)).unwrap();
        core.store.fail_turn_frame(|records| {
            records.iter().any(|r| {
                r.kind == theseus_store::kinds::ACTION
                    && serde_json::from_slice::<Value>(&r.payload)
                        .is_ok_and(|a| a["tool"].as_str().is_some_and(|t| t.starts_with("fs")))
            })
        });
        let rec = SessionRecord::new(SessionKind::Conversation, None);
        core.store.put_session(&rec.session_id, &rec).unwrap();
        let sid = rec.session_id.clone();
        let (live, _) = core.live_profile();
        let target = core.runner.resolve_target(&live, None, None, None).unwrap();
        let sink = EventSink::new(core.bus.clone(), &sid, None);
        let err = core
            .runner
            .run(TurnRequest {
                session: rec,
                input: Some("What does hello.txt say?".into()),
                target,
                sink,
                author: "test".into(),
                recompile: None,
                attachments: vec![],
                arrived: None,
                config_wait_us: 0,
                reply_to: None,
            })
            .await
            .expect_err("the turn faults");
        assert!(format!("{err:#}").contains("an injected fault"), "{err:#}");
        assert_eq!(
            kinds(&core, &sid),
            ["user_message", "assistant_message"],
            "the call is unanswered"
        );
        let rows: Vec<(u64, crate::ledger::LedgerRow)> = core.store.ledger_tail(200).unwrap();
        let woken = rows
            .iter()
            .find(|(_, r)| r.kind == "execution.queued" && r.data["why"] == "fault")
            .map(|(_, r)| r.data["execution_id"].as_str().unwrap().to_string())
            .expect("the faulted turn's execution was woken");
        let e = core.kernel.execution(&woken).unwrap().unwrap();
        assert_eq!(e.state, theseus_kernel::ExecState::Queued);
        assert!(e.resume_pending && e.queued_results.is_empty(), "{e:?}");
        (sid, woken)
    };
    // A restart: a new core on the same store, whose model reads the result.
    let store = Store::open(&dir.path().join("store")).unwrap();
    let fake = Arc::new(FakeProvider::scripted(vec![Scripted::text("It says hi.")]));
    let core = Core::build(crate::rpc::Parts::for_tests(cfg, fake.clone(), store)).unwrap();
    let res = core
        .continue_execution(&exec)
        .await
        .unwrap()
        .expect("the continuation runs");
    assert_eq!(res.output, "It says hi.");
    let k = kinds(&core, &sid);
    assert!(k.contains(&"tool_result:ok".to_string()), "{k:?}");
    let req = fake.requests().pop().unwrap();
    assert!(
        serde_json::to_string(&req.messages)
            .unwrap()
            .contains("tool_result"),
        "the model read the call's result: {:?}",
        req.messages
    );
}

/// A turn reads its session's transcript once, however many loops it runs
/// (theseus-qa0): resume, absorb, the "anything new?" check, and each loop's
/// compile share it, and every node the turn writes joins it, so each loop's
/// request still carries every result so far. It read it 5 times before.
#[tokio::test]
async fn a_turn_reads_its_transcript_once() {
    let read =
        |id: &str, path: &str| Scripted::tools("", &[(id, "fs_read", json!({ "path": path }))]);
    let r = rig(vec![
        Scripted::text("first"),
        read("t1", "a.txt"),
        read("t2", "b.txt"),
        Scripted::text("Both read."),
    ]);
    std::fs::write(r.root.join("a.txt"), "alpha\n").unwrap();
    std::fs::write(r.root.join("b.txt"), "beta\n").unwrap();
    let sid = turn(&r.core, None, "warm up").await.session_id;
    let reads = r.core.store.transcript_reads();
    let res = turn(&r.core, Some(&sid), "read a.txt, then b.txt").await;
    assert_eq!((res.loops, res.tool_calls), (3, 2));
    assert_eq!(r.core.store.transcript_reads() - reads, 1);
    let last = r.fake.requests().pop().unwrap();
    let text = serde_json::to_string(&last.messages).unwrap();
    assert!(text.contains("alpha") && text.contains("beta"), "{text}");
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
            attachments: vec![],
            arrived: None,
            config_wait_us: 0,
            reply_to: None,
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
    let q3 = res3.awaiting_confirm.unwrap();
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

    r.core
        .cancel_execution(&exec, "discord:eddie")
        .await
        .unwrap();
    assert_eq!(
        r.core.kernel.action(&q3).unwrap().unwrap().state,
        ActionState::Cancelled
    );
    assert!(r.core.pending_confirms(&sid).unwrap().is_empty());
}

// ------------------------------------------- one call over the whole limit (theseus-kks)

/// The template's live profile reserves $1.28 for its output cap alone, so
/// under a $1 limit its very first call cannot fit, whatever the spend.
fn over_limit_rig() -> Rig {
    rig_with(vec![Scripted::text("never sent")], |c| {
        c.kernel.spend_limit_usd = 1.0;
    })
}

/// A call whose reservation alone is bigger than the whole limit asks once,
/// and its question says so: both figures, and the two remedies (a higher
/// `spend_limit_usd`, a lower `max_output_tokens` on the profile it runs on).
/// Every surface that shows the question again (`confirm.list`, the card)
/// says the same, and the `budget.asked` row marks it `exceeds_limit`.
#[tokio::test]
async fn a_call_over_the_whole_limit_asks_once_and_names_the_remedies() {
    let r = over_limit_rig();
    let (sid, mut rx) = watched_session(&r);
    let res = turn(&r.core, Some(&sid), "hello").await;
    assert_eq!(res.stop_reason, "budget", "{res:?}");
    assert!(r.fake.requests().is_empty(), "nothing ran over the limit");
    let exec = res.execution_id.unwrap();
    let e = r.core.kernel.execution(&exec).unwrap().unwrap();
    let (needed, limit) = (e.budget.question_needs_micros, e.budget.limit_micros);
    assert!(needed > limit, "{needed} vs {limit}");
    let asked = sent(&mut rx, theseus_protocol::notify::CONFIRM_REQUESTED);
    assert_eq!(asked.len(), 1, "asked once: {asked:?}");
    let req: theseus_protocol::ConfirmRequest = serde_json::from_value(asked[0].clone()).unwrap();
    let target = r
        .core
        .runner
        .resolve_target(&r.core.live_profile().0, None, None, None)
        .unwrap();
    for part in [
        format!(
            "alone reserves {}: more than its whole {} limit",
            crate::narrative::dollars(needed),
            crate::narrative::dollars(limit)
        ),
        "cannot make it fit".to_string(),
        format!(
            "Raise `[kernel] spend_limit_usd` above {}",
            crate::narrative::dollars(needed)
        ),
        format!(
            "lower `max_output_tokens` under `[profiles.{}]` (now {})",
            target.profile,
            crate::narrative::thousands(u64::from(target.max_tokens))
        ),
        "does not ask again".to_string(),
    ] {
        assert!(req.reason.contains(&part), "{part:?} in {:?}", req.reason);
    }
    let pending = r.core.pending_confirms(&sid).unwrap();
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].reason, req.reason, "the card says the same");
    let rows = ledgered(&r, "budget.asked");
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["exceeds_limit"], json!(true));
}

/// An approved reset cannot make such a call fit, so it does not bring the
/// same question back: the retry tries the call once, and when it still does
/// not fit the turn fails (`over_limit`) with the figures and the remedies,
/// asks nothing, and the session waits on its next input.
#[tokio::test]
async fn an_approved_reset_of_a_call_over_the_whole_limit_does_not_ask_again() {
    use theseus_kernel::{ExecState, Wake};
    let r = over_limit_rig();
    let (sid, mut rx) = watched_session(&r);
    let res = turn(&r.core, Some(&sid), "hello").await;
    let (exec, q) = (
        res.execution_id.clone().unwrap(),
        res.awaiting_confirm.clone().unwrap(),
    );
    let _ = sent(&mut rx, theseus_protocol::notify::CONFIRM_REQUESTED);
    let ans = r
        .core
        .confirm_action(&q, true, None, "discord:eddie")
        .unwrap();
    assert!(ans.approved && ans.resumes);
    let err = r
        .core
        .continue_execution(&exec)
        .await
        .expect_err("the retry fails instead of asking");
    let te = err
        .downcast_ref::<crate::turn::TurnError>()
        .unwrap_or_else(|| panic!("a turn failure: {err:#}"));
    assert_eq!(te.class, "over_limit");
    let why = format!("{:#}", te.source);
    assert!(
        why.contains("spend_limit_usd") && why.contains("max_output_tokens"),
        "{why}"
    );
    assert!(r.fake.requests().is_empty(), "nothing ran over the limit");
    assert!(
        sent(&mut rx, theseus_protocol::notify::CONFIRM_REQUESTED).is_empty(),
        "no second question"
    );
    assert_eq!(ledgered(&r, "budget.asked").len(), 1);
    assert_eq!(ledgered(&r, "budget.over_limit").len(), 1);
    assert!(r.core.pending_confirms(&sid).unwrap().is_empty());
    let e = r.core.kernel.execution(&exec).unwrap().unwrap();
    assert_eq!(
        (e.state, e.wake.clone(), e.budget.question.clone()),
        (ExecState::Waiting, Some(Wake::Input), None)
    );
    // Nothing more for the driver: the session waits on its operator.
    assert!(!e.resume_pending);
    // That is the rule for any failure (theseus-ljr): this one settled no
    // call of its own, so nothing would retry it but the next message.
    let next = ledgered(&r, "turn.next");
    assert_eq!(next.len(), 1, "{next:?}");
    assert_eq!(
        (&next[0]["then"], &next[0]["class"], &next[0]["settled"]),
        (&json!("park"), &json!("over_limit"), &json!(false))
    );
}

/// The ordinary path is as it was: a call that fits the limit but not what
/// is left of it asks the old question, the reset's retry makes the call,
/// and nothing says the call is over the whole limit.
#[tokio::test]
async fn an_ordinary_over_budget_call_still_asks_resets_and_goes_ahead() {
    let r = over_budget_rig(vec![Scripted::text("The diff is one line.")]);
    let (sid, mut rx) = watched_session(&r);
    let res = turn(&r.core, Some(&sid), "diff these").await;
    assert_eq!(res.stop_reason, "budget", "{res:?}");
    let (exec, q) = (
        res.execution_id.clone().unwrap(),
        res.awaiting_confirm.clone().unwrap(),
    );
    let asked = sent(&mut rx, theseus_protocol::notify::CONFIRM_REQUESTED);
    let req: theseus_protocol::ConfirmRequest = serde_json::from_value(asked[0].clone()).unwrap();
    assert!(
        req.reason
            .ends_with("limit. Reset its spend to $0 and continue?")
            && !req.reason.contains("spend_limit_usd"),
        "{}",
        req.reason
    );
    assert_eq!(
        ledgered(&r, "budget.asked")[0]["exceeds_limit"],
        json!(false)
    );
    r.core
        .confirm_action(&q, true, None, "discord:eddie")
        .unwrap();
    let cont = r.core.continue_execution(&exec).await.unwrap().unwrap();
    assert_eq!(cont.output, "The diff is one line.");
    assert_eq!(r.fake.requests().len(), 2, "the waiting call ran");
    assert!(ledgered(&r, "budget.over_limit").is_empty());
    assert_eq!(ledgered(&r, "budget.asked").len(), 1);
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
        ..Default::default()
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

/// A 1-hour cache write is priced at 2 × input (theseus-ev1), in the budget,
/// the session, the ledger, and health alike. On Sonnet 5.5: 1,000 in at $2,
/// 100 out at $10, and 50,000 cache writes, 30,000 of them 1-hour ones, so
/// 20,000 at $2.50 and 30,000 at $4.00: 2,000 + 1,000 + 50,000 + 120,000 µ$
/// = $0.173. Priced all as 5-minute writes, as before, it was $0.128.
#[tokio::test]
async fn a_one_hour_cache_write_costs_twice_the_input_price_everywhere() {
    let usage = theseus_protocol::Usage {
        input_tokens: 1_000,
        output_tokens: 100,
        cache_creation_input_tokens: 50_000,
        cache_creation_1h_input_tokens: 30_000,
        ..Default::default()
    };
    let r = rig(vec![Scripted::Billed {
        usage: usage.clone(),
        then: Box::new(Scripted::text("An hour.")),
    }]);
    let res = turn(&r.core, None, "write for an hour").await;
    assert_eq!(res.output, "An hour.");
    let e = r
        .core
        .kernel
        .execution(res.execution_id.as_deref().unwrap())
        .unwrap()
        .unwrap();
    assert_eq!(e.budget.spent_micros, 2_000 + 1_000 + 50_000 + 120_000);
    let s = r
        .core
        .store
        .get_session::<SessionRecord>(&res.session_id)
        .unwrap()
        .unwrap();
    assert!((s.cost_usd - 0.173).abs() < 1e-12, "{}", s.cost_usd);
    assert_eq!(s.usage.cache_creation_1h_input_tokens, 30_000);
    let row = &ledgered(&r, "provider.call")[0];
    assert_eq!(
        row["usage"]["cache_creation_1h_input_tokens"], 30_000,
        "{row}"
    );
    assert!(
        (row["cost_usd"].as_f64().unwrap() - 0.173).abs() < 1e-12,
        "{row}"
    );
    assert_eq!(res.usage.cache_creation_1h_input_tokens, 30_000);
    let h = r.core.health();
    assert_eq!(h.usage_total.cache_creation_1h_input_tokens, 30_000);
    assert!((h.cost_usd_total - 0.173).abs() < 1e-12);
}

/// A turn in a new conversation on `target`.
async fn turn_on(core: &Arc<Core>, target: crate::turn::Target, input: &str) -> TurnSubmitResult {
    let rec = SessionRecord::new(SessionKind::Conversation, None);
    core.store.put_session(&rec.session_id, &rec).unwrap();
    let sink = EventSink::new(core.bus.clone(), &rec.session_id, None);
    core.runner
        .run(TurnRequest {
            session: rec,
            input: Some(input.into()),
            target,
            sink,
            author: "test".into(),
            recompile: None,
            attachments: vec![],
            arrived: None,
            config_wait_us: 0,
            reply_to: None,
        })
        .await
        .unwrap()
}

/// The real header, the persona, the tools note, and the tools, is about
/// 13 KB, which Haiku 4.5 counts at about 4,150 tokens, past its caching
/// minimum of 4,096 (the cache2 lane's live check). A token takes 2 bytes at
/// the fewest, so the header gets its breakpoint on every built-in model,
/// Haiku included (theseus-ev1). A minimum the prefix can never reach, here a
/// config's 16,384 tokens (32,768 bytes), drops the header's breakpoint: the
/// compilation's manifest and the narrative say so, and the conversation's
/// breakpoint stays.
#[tokio::test]
async fn a_header_under_the_models_cache_minimum_gets_no_breakpoint() {
    async fn on_haiku(r: &Rig) -> TurnSubmitResult {
        let (live, _) = r.core.live_profile();
        let haiku = r
            .core
            .runner
            .resolve_target(&live, None, None, Some("claude-haiku-4-5"))
            .unwrap();
        turn_on(&r.core, haiku, "hello").await
    }
    let r = rig(vec![Scripted::text("Small.")]);
    let res = on_haiku(&r).await;
    assert_eq!(res.model, "claude-haiku-4-5");
    let comps = r.core.store.session_compilations(&res.session_id).unwrap();
    let layout = comps[0].manifest.cache.clone().unwrap();
    assert_eq!(layout.min_tokens, 4_096);
    let header = &layout.blocks[0];
    assert_eq!(header.block, "header");
    assert!(header.prefix_bytes >= 8_192 && header.marked, "{layout:?}");
    let sent = r.fake.requests().pop().unwrap();
    assert_eq!(sent.system.len(), 1, "no context files: one block");
    assert_eq!(
        sent.system[0]["cache_control"],
        json!({"type": "ephemeral"})
    );

    let r = rig_with(vec![Scripted::text("Small.")], |cfg| {
        cfg.catalog
            .get_mut("claude-haiku-4-5")
            .unwrap()
            .cache_min_tokens = Some(16_384);
    });
    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
    assert!(r.core.narrator.watch("watcher", tx).is_some());
    let res = on_haiku(&r).await;
    let comps = r.core.store.session_compilations(&res.session_id).unwrap();
    let layout = comps[0].manifest.cache.clone().unwrap();
    assert_eq!(layout.min_tokens, 16_384);
    let header = &layout.blocks[0];
    assert!(header.prefix_bytes < 32_768 && !header.marked && layout.caches);
    let sent = r.fake.requests().pop().unwrap();
    assert!(sent.system[0].get("cache_control").is_none());
    assert_eq!(sent.cache_control, Some(json!({"type": "ephemeral"})));
    let row = &ledgered(&r, "context.compiled")[0];
    assert_eq!(
        row["cache"]["breakpoints"],
        json!(["conversation"]),
        "{row}"
    );
    assert_eq!(row["cache"]["ttl"], "5m");
    let lines = narrated(&r, &res.session_id);
    assert!(
        said(
            &lines,
            "context",
            "no cache breakpoint on the system's header block"
        ),
        "{lines:?}"
    );

    // The template's Sonnet 5.5 (512): the header is marked.
    let r = rig(vec![Scripted::text("Small.")]);
    let res = turn(&r.core, None, "hello").await;
    let comps = r.core.store.session_compilations(&res.session_id).unwrap();
    let layout = comps[0].manifest.cache.clone().unwrap();
    assert_eq!(layout.min_tokens, 512);
    assert!(layout.blocks[0].marked, "{layout:?}");
}

/// A task's own conversation is cached for 5 minutes on a 1-hour profile,
/// while its header keeps the hour its parent's requests write; a
/// conversation's every breakpoint is an hour long (theseus-ev1).
#[tokio::test]
async fn a_task_caches_its_own_conversation_for_five_minutes() {
    let r = rig(vec![Scripted::text("one"), Scripted::text("two")]);
    let (live, _) = r.core.live_profile();
    let mut target = r
        .core
        .runner
        .resolve_target(&live, None, None, None)
        .unwrap();
    assert_eq!(
        target.cache_ttl,
        crate::config::CacheTtl::FiveMinutes,
        "the default"
    );
    // As a profile with `cache_ttl = "1h"` resolves (config's tests parse one).
    target.cache_ttl = crate::config::CacheTtl::OneHour;
    let hour = json!({"type": "ephemeral", "ttl": "1h"});
    let (spec, _) = r
        .core
        .runner
        .request_spec(&target, SessionKind::Conversation);
    assert_eq!(
        (spec.cache_ttl, spec.conversation_ttl),
        (
            crate::config::CacheTtl::OneHour,
            crate::config::CacheTtl::OneHour
        )
    );
    let (task, _) = r.core.runner.request_spec(&target, SessionKind::Task);
    assert_eq!(task.conversation_ttl, crate::config::CacheTtl::FiveMinutes);
    assert_eq!(task.cache_ttl, crate::config::CacheTtl::OneHour);
    let res = turn_on(&r.core, target, "for an hour").await;
    let sent = r.fake.requests().pop().unwrap();
    assert_eq!(sent.system[0]["cache_control"], hour);
    assert_eq!(sent.cache_control, Some(hour));
    let row = &ledgered(&r, "context.compiled")[0];
    assert_eq!(row["cache"]["ttl"], "1h");
    assert_eq!(row["cache"]["conversation_ttl"], "1h");
    assert_eq!(res.output, "one");
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
            attachments: vec![],
            arrived: None,
            config_wait_us: 0,
            reply_to: None,
        })
        .await
        .unwrap_err();
    let te = err.downcast_ref::<crate::turn::TurnError>().unwrap();
    assert_eq!(te.class, "unpriced");
    assert!(format!("{:#}", te.source).contains("[catalog.\"mystery-model\"]"));
    assert!(r.fake.requests().is_empty());
}

// ---------------------------------------------------------------- context files (theseus-58a)

/// A path in the rig's projects directory, from a tweak (which runs before
/// the rig exists).
fn in_projects(cfg: &Config, name: &str) -> String {
    format!("{}/{name}", cfg.tools.projects_dir.as_deref().unwrap())
}

/// Set a file's mtime `secs` into the past, so it is not racy.
fn aged(path: &str, secs: u64) {
    std::fs::File::options()
        .write(true)
        .open(path)
        .unwrap()
        .set_modified(std::time::SystemTime::now() - Duration::from_secs(secs))
        .unwrap();
}

/// The system block of the `i`th provider request.
/// The system the model reads in request `i`: its blocks' texts (the header,
/// then the context files: theseus-ev1), joined as the single block before
/// the split joined them.
fn system_of(r: &Rig, i: usize) -> String {
    r.fake.requests()[i]
        .system
        .iter()
        .map(|b| b["text"].as_str().unwrap())
        .collect::<Vec<_>>()
        .join("\n\n")
}

fn sha16(s: &str) -> String {
    use sha2::{Digest, Sha256};
    hex::encode(Sha256::digest(s.as_bytes()))[..16].to_string()
}

/// A context file's rule reaches the model in the system block, after the
/// persona and the tools note, under a header naming the file. The manifest
/// and the `context.compiled` row carry its digest. The file is written after
/// the core started: nothing reads it at startup.
#[tokio::test]
async fn a_context_file_puts_its_rule_in_the_system_block_and_its_digest_in_the_manifest() {
    let mut path = String::new();
    let r = rig_with(vec![Scripted::text("Four. Theseus")], |cfg| {
        path = in_projects(cfg, "RULES.md");
        cfg.context.files = vec![path.clone()];
    });
    assert_eq!(
        r.core.runner.context_files.reads(),
        0,
        "nothing read at startup"
    );
    let rule = "End every answer with the word 'Theseus'.\n";
    std::fs::write(&path, rule).unwrap();
    let res = turn(&r.core, None, "what is 2 + 2?").await;
    let system = system_of(&r, 0);
    let header = format!("# Context file (system): {path}");
    let at = |needle: &str| {
        system
            .find(needle)
            .unwrap_or_else(|| panic!("{needle:?} is not in:\n{system}"))
    };
    assert!(at(crate::turn::PERSONA) < at("Tools. You act") && at("Tools. You act") < at(&header));
    assert!(
        system.ends_with(&format!(
            "{header}\n\nEnd every answer with the word 'Theseus'."
        )),
        "{system}"
    );
    // The file is the system's second block, after the header (theseus-ev1).
    let sent = &r.fake.requests()[0].system;
    assert_eq!(sent.len(), 2);
    assert!(sent[1]["text"].as_str().unwrap().starts_with(&header));
    assert!(!sent[0]["text"].as_str().unwrap().contains("Context file"));
    let want = crate::compiler::ContextFileRef {
        path: path.clone(),
        digest: Some(sha16(rule)),
        bytes: rule.len() as u64,
        cut: false,
        missing: None,
        persona: None,
    };
    let comps = r.core.store.session_compilations(&res.session_id).unwrap();
    assert_eq!(comps[0].manifest.context_files, vec![want.clone()]);
    assert_eq!(
        ledgered(&r, "context.compiled")[0]["context_files"],
        json!([want])
    );
    let lines = narrated(&r, &res.session_id);
    assert!(
        said(&lines, "context", "tokens, 1 context file."),
        "{}",
        dump(&lines)
    );
}

/// An edit makes exactly one `system_changed` recompile, on the next turn's
/// first loop. An unchanged file is a stat, not a read, and the turn appends.
#[tokio::test]
async fn an_edited_context_file_recompiles_once_and_an_unchanged_one_appends() {
    let mut path = String::new();
    let r = rig_with(
        vec![
            Scripted::text("a Theseus"),
            Scripted::text("b Theseus"),
            Scripted::text("c Ithaca"),
            Scripted::text("d Ithaca"),
        ],
        |cfg| {
            path = in_projects(cfg, "RULES.md");
            cfg.context.files = vec![path.clone()];
        },
    );
    std::fs::write(&path, "End every answer with 'Theseus'.\n").unwrap();
    aged(&path, 60);
    let sid = turn(&r.core, None, "one").await.session_id;
    let reads = r.core.runner.context_files.reads();
    turn(&r.core, Some(&sid), "two").await;
    assert_eq!(
        r.core.runner.context_files.reads(),
        reads,
        "unchanged: not read again"
    );
    let edited = "End every answer with 'Ithaca'.\n";
    std::fs::write(&path, edited).unwrap();
    aged(&path, 30);
    turn(&r.core, Some(&sid), "three").await;
    turn(&r.core, Some(&sid), "four").await;
    let field = |kind: &str, key: &str| -> Vec<String> {
        ledgered(&r, kind)
            .iter()
            .map(|d| d[key].as_str().unwrap_or("").to_string())
            .collect()
    };
    assert_eq!(
        field("context.recompiled", "trigger"),
        ["new_session", "system_changed"]
    );
    assert_eq!(
        field("context.compiled", "decision"),
        ["recompile", "append", "recompile", "append"]
    );
    let (before, after) = (system_of(&r, 1), system_of(&r, 2));
    assert!(before.contains("'Theseus'.") && !before.contains("'Ithaca'"));
    assert!(after.contains("'Ithaca'.") && !after.contains("'Theseus'"));
    assert_eq!(system_of(&r, 3), after);
    let comps = r.core.store.session_compilations(&sid).unwrap();
    assert_eq!(comps.len(), 2);
    assert_eq!(comps[1].trigger, "system_changed");
    assert_eq!(
        comps[1].manifest.context_files[0].digest,
        Some(sha16(edited))
    );
    assert_ne!(
        comps[0].manifest.context_files[0].digest,
        comps[1].manifest.context_files[0].digest
    );
}

/// A missing file does not stop the turn: the block says it is missing, and
/// the daemon warns once (a log line and a ledger row), not once a turn.
#[tokio::test]
async fn a_missing_context_file_warns_once_and_the_turn_runs() {
    let mut path = String::new();
    let r = rig_with(
        vec![Scripted::text("fine"), Scripted::text("still fine")],
        |cfg| {
            path = in_projects(cfg, "GONE.md");
            cfg.context.files = vec![path.clone()];
        },
    );
    let first = turn(&r.core, None, "one").await;
    let second = turn(&r.core, Some(&first.session_id), "two").await;
    assert_eq!(
        (first.output.as_str(), second.output.as_str()),
        ("fine", "still fine")
    );
    assert!(system_of(&r, 1).ends_with(&format!(
        "# Context file (system): {path}\n\n[Missing: the file could not be read (not found).]"
    )));
    assert_eq!(
        ledgered(&r, "context.file_missing"),
        vec![json!({"path": path, "error": "not found", "profile": "sonnet"})]
    );
    let comps = r
        .core
        .store
        .session_compilations(&first.session_id)
        .unwrap();
    let f = &comps[0].manifest.context_files[0];
    assert_eq!(
        (f.missing.as_deref(), f.digest.as_deref()),
        (Some("not found"), None)
    );
    let lines = narrated(&r, &first.session_id);
    assert!(
        said(&lines, "context", "0 context files (1 missing)"),
        "{}",
        dump(&lines)
    );
    assert!(
        said(&lines, "context", "could not be read (not found)"),
        "{}",
        dump(&lines)
    );
}

/// A file over the cap is cut, and the block and the manifest say so.
#[tokio::test]
async fn a_context_file_over_the_cap_is_cut_and_marked_as_cut() {
    use crate::context_files::MAX_BYTES;
    let mut path = String::new();
    let r = rig_with(vec![Scripted::text("ok")], |cfg| {
        path = in_projects(cfg, "BIG.md");
        cfg.context.files = vec![path.clone()];
    });
    std::fs::write(&path, "y".repeat(MAX_BYTES + 1_000)).unwrap();
    let res = turn(&r.core, None, "hi").await;
    let system = system_of(&r, 0);
    assert!(system.ends_with(&format!(
        "{}\n\n[Cut: only the first 65,536 bytes of this file are included.]",
        "y".repeat(MAX_BYTES)
    )));
    let comps = r.core.store.session_compilations(&res.session_id).unwrap();
    let f = &comps[0].manifest.context_files[0];
    assert!(f.cut && f.bytes == MAX_BYTES as u64, "{f:?}");
    assert_eq!(f.digest, Some(sha16(&"y".repeat(MAX_BYTES))));
}

/// A config that names no context files (Eddie's vault config names none)
/// compiles the system block it always did: the persona and the tools note,
/// nothing after them. Its manifest and rows carry no `context_files` key,
/// so a manifest stored before theseus-58a compares equal and appends, and
/// nothing is read or warned.
#[tokio::test]
async fn without_context_files_the_system_block_and_manifest_are_unchanged() {
    let r = rig(vec![Scripted::text("hi")]);
    let res = turn(&r.core, None, "hello").await;
    assert_eq!(
        system_of(&r, 0),
        format!("{}\n\n{}", crate::turn::PERSONA, r.core.tools.system_note())
    );
    let comps = r.core.store.session_compilations(&res.session_id).unwrap();
    let stored = serde_json::to_value(&comps[0].manifest).unwrap();
    assert!(stored.get("context_files").is_none(), "{stored}");
    let back: crate::compiler::Manifest = serde_json::from_value(stored).unwrap();
    assert_eq!(back, comps[0].manifest);
    assert!(ledgered(&r, "context.compiled")[0]
        .get("context_files")
        .is_none());
    assert!(ledgered(&r, "context.file_missing").is_empty());
    assert_eq!(r.core.runner.context_files.reads(), 0);
}

/// Two levels (theseus-c48): the system level's files, then the files of the
/// persona in play (`[context] default_persona`), each under a header naming
/// its level. The manifest records each file's level, the `context.compiled`
/// row the persona, and health the persona in play with its files.
#[tokio::test]
async fn system_then_persona_files_compile_in_that_order_each_labeled_by_its_level() {
    let (mut sys, mut own) = (String::new(), String::new());
    let r = rig_with(vec![Scripted::text("Hello. Ithaca")], |cfg| {
        sys = in_projects(cfg, "USER.md");
        own = in_projects(cfg, "PERSONA.md");
        cfg.context.files = vec![sys.clone()];
        cfg.context.default_persona = Some("theseus".into());
        cfg.personas.insert(
            "theseus".into(),
            crate::config::PersonaConfig {
                files: vec![own.clone()],
            },
        );
    });
    std::fs::write(&sys, "The operator is Eddie.\n").unwrap();
    std::fs::write(&own, "End every answer with 'Ithaca'.\n").unwrap();
    let res = turn(&r.core, None, "hi").await;
    let system = system_of(&r, 0);
    let (a, b) = (
        format!("# Context file (system): {sys}\n\nThe operator is Eddie."),
        format!("# Context file (persona theseus): {own}\n\nEnd every answer with 'Ithaca'."),
    );
    assert!(
        system.ends_with(&format!("{}\n\n{a}\n\n{b}", r.core.tools.system_note())),
        "{system}"
    );
    let comps = r.core.store.session_compilations(&res.session_id).unwrap();
    let levels: Vec<(String, Option<String>)> = comps[0]
        .manifest
        .context_files
        .iter()
        .map(|f| (f.path.clone(), f.persona.clone()))
        .collect();
    assert_eq!(
        levels,
        [(sys.clone(), None), (own.clone(), Some("theseus".into()))]
    );
    let row = &ledgered(&r, "context.compiled")[0];
    assert_eq!(row["persona"], "theseus");
    assert_eq!(row["context_files"][1]["persona"], "theseus");
    assert!(row["context_files"][0].get("persona").is_none(), "{row}");
    let lines = narrated(&r, &res.session_id);
    assert!(
        said(
            &lines,
            "context",
            "tokens, 2 context files, persona theseus."
        ),
        "{}",
        dump(&lines)
    );
    let h = r.core.health().context;
    assert_eq!(
        (
            h.system_files,
            h.persona.as_deref(),
            h.persona_files,
            h.personas
        ),
        (
            vec![sys],
            Some("theseus"),
            vec![own],
            vec!["theseus".to_string()]
        )
    );
}

/// A persona that names no files adds nothing to the system block: it is
/// the system level's alone, as without a persona, and says which persona
/// is in play.
#[tokio::test]
async fn a_persona_that_names_no_files_adds_nothing_to_the_system_block() {
    let mut sys = String::new();
    let r = rig_with(vec![Scripted::text("one"), Scripted::text("two")], |cfg| {
        sys = in_projects(cfg, "USER.md");
        cfg.context.files = vec![sys.clone()];
        cfg.context.default_persona = Some("quiet".into());
        cfg.personas
            .insert("quiet".into(), crate::config::PersonaConfig::default());
    });
    std::fs::write(&sys, "The operator is Eddie.\n").unwrap();
    let res = turn(&r.core, None, "hi").await;
    assert_eq!(
        system_of(&r, 0),
        format!(
            "{}\n\n{}\n\n# Context file (system): {sys}\n\nThe operator is Eddie.",
            crate::turn::PERSONA,
            r.core.tools.system_note()
        )
    );
    let comps = r.core.store.session_compilations(&res.session_id).unwrap();
    assert_eq!(comps[0].manifest.context_files.len(), 1);
    let lines = narrated(&r, &res.session_id);
    assert!(
        said(&lines, "context", "tokens, 1 context file, persona quiet."),
        "{}",
        dump(&lines)
    );
    let h = r.core.health().context;
    assert_eq!(
        (h.persona.as_deref(), h.persona_files.len()),
        (Some("quiet"), 0)
    );
}

/// An edit to a persona's file is one `system_changed` recompile, as an edit
/// to a system file is (theseus-58a), and an unchanged one appends.
#[tokio::test]
async fn an_edited_persona_file_recompiles_once_and_an_unchanged_one_appends() {
    let mut own = String::new();
    let r = rig_with(
        vec![
            Scripted::text("a"),
            Scripted::text("b"),
            Scripted::text("c"),
        ],
        |cfg| {
            own = in_projects(cfg, "PERSONA.md");
            cfg.context.default_persona = Some("theseus".into());
            cfg.personas.insert(
                "theseus".into(),
                crate::config::PersonaConfig {
                    files: vec![own.clone()],
                },
            );
        },
    );
    std::fs::write(&own, "Speak plainly.\n").unwrap();
    aged(&own, 60);
    let sid = turn(&r.core, None, "one").await.session_id;
    std::fs::write(&own, "Speak briefly.\n").unwrap();
    aged(&own, 30);
    turn(&r.core, Some(&sid), "two").await;
    turn(&r.core, Some(&sid), "three").await;
    let field = |kind: &str, key: &str| -> Vec<String> {
        ledgered(&r, kind)
            .iter()
            .map(|d| d[key].as_str().unwrap_or("").to_string())
            .collect()
    };
    assert_eq!(
        field("context.recompiled", "trigger"),
        ["new_session", "system_changed"]
    );
    assert_eq!(
        field("context.compiled", "decision"),
        ["recompile", "recompile", "append"]
    );
    assert!(system_of(&r, 1).ends_with("Speak briefly."));
    let comps = r.core.store.session_compilations(&sid).unwrap();
    assert_eq!(
        comps[1].manifest.context_files[0].digest,
        Some(sha16("Speak briefly.\n"))
    );
}

// ---------------------------------------------------------------- attachments (theseus-9g2)

/// One `turn.submit` over a real protocol connection; the response's result
/// or error, as JSON.
async fn submit(core: &Arc<Core>, p: theseus_protocol::TurnSubmitParams) -> Value {
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
    let (client, server) = tokio::io::duplex(64 * 1024);
    let (sr, sw) = tokio::io::split(server);
    let srv = tokio::spawn(core.clone().serve_connection(sr, sw, "test".into()));
    let (cr, mut cw) = tokio::io::split(client);
    let req = theseus_protocol::Request::new(
        theseus_protocol::Id::Num(1),
        theseus_protocol::method::TURN_SUBMIT,
        serde_json::to_value(p).unwrap(),
    );
    let mut line = serde_json::to_string(&req).unwrap();
    line.push('\n');
    let writer = tokio::spawn(async move {
        cw.write_all(line.as_bytes()).await.unwrap();
        cw
    });
    let mut lines = BufReader::new(cr).lines();
    let out = loop {
        let l = lines.next_line().await.unwrap().unwrap();
        if let theseus_protocol::Message::Response(r) = serde_json::from_str(&l).unwrap() {
            break match (r.result, r.error) {
                (Some(v), _) => v,
                (None, e) => json!({"error": e}),
            };
        }
    };
    let mut cw = writer.await.unwrap();
    cw.shutdown().await.unwrap();
    drop(lines);
    let _ = srv.await;
    out
}

fn attached(
    name: &str,
    media_type: &str,
    text: Option<String>,
    not_read: Option<&str>,
) -> theseus_protocol::Attachment {
    theseus_protocol::Attachment {
        name: name.into(),
        media_type: media_type.into(),
        size: text.as_ref().map_or(20 * 1024 * 1024, |t| t.len() as u64),
        text,
        data: None,
        not_read: not_read.map(str::to_string),
    }
}

fn submit_params(
    session: Option<&str>,
    input: &str,
    attachments: Vec<theseus_protocol::Attachment>,
) -> theseus_protocol::TurnSubmitParams {
    theseus_protocol::TurnSubmitParams {
        session_id: session.map(str::to_string),
        input: input.into(),
        profile: None,
        provider: None,
        model: None,
        author: Some("discord:eddie".into()),
        attachments,
        reply_to: None,
    }
}

/// The user content blocks of the last request the provider saw.
fn last_user_blocks(r: &Rig) -> Vec<Value> {
    let reqs = r.fake.requests();
    let msgs = &reqs.last().unwrap().messages;
    let user = msgs.iter().rev().find(|m| m["role"] == "user").unwrap();
    user["content"].as_array().unwrap().clone()
}

/// Discord sends a long paste as `message.txt`: its text reaches the model
/// under a header that names it and its sender, before the typed text, and
/// the node keeps it whole. It rides in the user node's own frame.
#[tokio::test]
async fn a_text_attachment_reaches_the_model_labeled_with_its_name() {
    let r = rig(vec![
        Scripted::text("warm"),
        Scripted::text("It is PURPLE-OTTER-42."),
    ]);
    let first = submit(&r.core, submit_params(None, "warm up", vec![])).await;
    let sid = first["session_id"].as_str().unwrap().to_string();
    let filler = "Lorem ipsum dolor sit amet, consectetur adipiscing elit. ".repeat(86);
    let paste = format!(
        "{}The launch code is PURPLE-OTTER-42.\n{}",
        &filler[..2_400],
        &filler[..2_565]
    );
    assert_eq!(paste.chars().count(), 5_001);
    let before = r.core.store.stats().unwrap().frames_appended;
    let res = submit(
        &r.core,
        submit_params(
            Some(&sid),
            "What is the launch code?",
            vec![attached(
                "message.txt",
                "text/plain; charset=utf-8",
                Some(paste.clone()),
                None,
            )],
        ),
    )
    .await;
    assert_eq!(res["output"], "It is PURPLE-OTTER-42.", "{res}");
    let frames = r.core.store.stats().unwrap().frames_appended - before;
    assert!(
        frames <= 17,
        "a turn with an attachment wrote {frames} frames"
    );

    let blocks = last_user_blocks(&r);
    assert_eq!(blocks.len(), 2, "the file, then the typed text: {blocks:?}");
    assert_eq!(
        blocks[0]["text"],
        format!("[Attachment message.txt from discord:eddie, 5,001 bytes]\n{paste}")
    );
    assert_eq!(blocks[1]["text"], "What is the launch code?");

    let nodes = r.core.store.session_nodes(&sid).unwrap();
    let user = nodes
        .iter()
        .rev()
        .find_map(|(_, n)| match &n.body {
            Body::UserMessage { text, attachments } => Some((text.clone(), attachments.clone())),
            _ => None,
        })
        .unwrap();
    assert_eq!(user.0, "What is the launch code?");
    assert_eq!(user.1.len(), 1);
    assert_eq!(
        user.1[0].content,
        crate::node::AttachmentContent::Text {
            text: paste,
            cut: false
        }
    );
    // The web UI and `theseus history` show the header under the text.
    let (pos, node) = nodes
        .iter()
        .rev()
        .find(|(_, n)| n.kind_str() == "user_message")
        .unwrap();
    let info = crate::rpc::Core::node_info(*pos, node);
    assert_eq!(
        info.text,
        "What is the launch code?\n[Attachment message.txt from discord:eddie, 5,001 bytes]"
    );
}

/// A text over `[tools].max_read_bytes` is cut on a character boundary and
/// marked; a file the sender did not read is listed with its type, size,
/// and reason.
#[tokio::test]
async fn an_attachment_over_the_limit_is_cut_or_listed_as_not_read_with_the_reason() {
    let r = rig_with(vec![Scripted::text("ok")], |c| {
        c.tools.max_read_bytes = 1_000
    });
    let res = submit(
        &r.core,
        submit_params(
            None,
            "Look at these.",
            vec![
                attached("big.log", "text/plain", Some("é".repeat(1_500)), None),
                attached(
                    "src.zip",
                    "application/zip",
                    None,
                    Some("only text files are read"),
                ),
            ],
        ),
    )
    .await;
    assert_eq!(res["output"], "ok", "{res}");
    let blocks = last_user_blocks(&r);
    let cut = blocks[0]["text"].as_str().unwrap();
    assert!(
        cut.starts_with(
            "[Attachment big.log from discord:eddie, 3,000 bytes; cut to its first 1,000 bytes]\n"
        ),
        "{cut}"
    );
    assert_eq!(cut.split_once('\n').unwrap().1, "é".repeat(500));
    assert_eq!(
        blocks[1]["text"],
        "[Attachment src.zip from discord:eddie, application/zip, 20.0 MB: not read: only text files are read]"
    );
    assert_eq!(blocks[2]["text"], "Look at these.");
}

/// A message that is only an attachment whose download failed still runs
/// its turn: the model reads the listing, and there is no empty text block.
/// An empty input without attachments is still refused.
#[tokio::test]
async fn a_failed_download_is_listed_and_the_turn_still_runs() {
    let r = rig(vec![Scripted::text("The file did not come through.")]);
    let res = submit(
        &r.core,
        submit_params(
            None,
            "",
            vec![attached(
                "message.txt",
                "text/plain",
                None,
                Some("the download failed (HTTP 404)"),
            )],
        ),
    )
    .await;
    assert_eq!(res["output"], "The file did not come through.", "{res}");
    assert_eq!(res["loops"], 1);
    let blocks = last_user_blocks(&r);
    assert_eq!(
        blocks,
        vec![
            json!({"type": "text", "text": "[Attachment message.txt from discord:eddie, text/plain, 20.0 MB: not read: the download failed (HTTP 404)]"})
        ]
    );
    let sid = res["session_id"].as_str().unwrap();
    let title = r
        .core
        .store
        .get_session::<SessionRecord>(sid)
        .unwrap()
        .unwrap()
        .title;
    assert_eq!(
        title.as_deref(),
        Some("message.txt"),
        "a session opened by a file is named for it"
    );

    let refused = submit(&r.core, submit_params(None, "  ", vec![])).await;
    assert_eq!(refused["error"]["message"], "input is empty", "{refused}");
}

// ---------------------------------------------------------------- images (theseus-9g2)

fn image_attached(name: &str, bytes: &[u8]) -> theseus_protocol::Attachment {
    theseus_protocol::Attachment {
        name: name.into(),
        media_type: "image/png".into(),
        size: bytes.len() as u64,
        data: Some(crate::blobs::encode(bytes)),
        ..Default::default()
    }
}

fn blob_files(r: &Rig) -> usize {
    std::fs::read_dir(r.core.store.blobs().dir())
        .map(|d| d.count())
        .unwrap_or(0)
}

/// A PNG sent to a vision model is one image block, after the line that
/// names it and before the typed text. Its bytes are stored once, beside
/// the WAL, and the node holds their digest. The next turn's request begins
/// with the same bytes: the image node renders identically every time.
#[tokio::test]
async fn an_image_is_one_image_block_for_a_vision_model_stored_once_and_rendered_the_same() {
    let r = rig(vec![
        Scripted::text("It says HELLO."),
        Scripted::text("Still HELLO."),
    ]);
    let bytes = crate::attach::tests::png(1280, 720, 2_000);
    let first = submit(
        &r.core,
        submit_params(
            None,
            "What does it say?",
            vec![image_attached("shot.png", &bytes)],
        ),
    )
    .await;
    assert_eq!(first["output"], "It says HELLO.", "{first}");
    let blocks = last_user_blocks(&r);
    assert_eq!(blocks.len(), 3, "{blocks:?}");
    assert_eq!(
        blocks[0]["text"],
        "[Image shot.png from discord:eddie, 2,033 bytes, 1280×720]"
    );
    assert_eq!(blocks[1]["type"], "image");
    assert_eq!(blocks[1]["source"]["type"], "base64");
    assert_eq!(blocks[1]["source"]["media_type"], "image/png");
    assert_eq!(
        crate::blobs::decode(blocks[1]["source"]["data"].as_str().unwrap()).unwrap(),
        bytes
    );
    assert_eq!(blocks[2]["text"], "What does it say?");

    // Stored once, and the node holds the reference, not the bytes.
    assert_eq!(blob_files(&r), 1);
    let sid = first["session_id"].as_str().unwrap().to_string();
    let (_, node) = r
        .core
        .store
        .session_nodes(&sid)
        .unwrap()
        .into_iter()
        .find(|(_, n)| n.kind_str() == "user_message")
        .unwrap();
    let stored = serde_json::to_string(&node).unwrap();
    assert!(stored.contains(&crate::blobs::digest(&bytes)), "{stored}");
    assert!(
        stored.len() < 1_000,
        "the node carries no image bytes: {} bytes",
        stored.len()
    );

    // The same image again: still one blob. The first request's messages
    // are the second's first messages, byte for byte.
    let second = submit(
        &r.core,
        submit_params(
            Some(&sid),
            "And now?",
            vec![image_attached("again.png", &bytes)],
        ),
    )
    .await;
    assert_eq!(second["output"], "Still HELLO.", "{second}");
    assert_eq!(blob_files(&r), 1);
    let reqs = r.fake.requests();
    let (a, b) = (&reqs[0].messages, &reqs[1].messages);
    assert_eq!(
        serde_json::to_vec(&b[..a.len()]).unwrap(),
        serde_json::to_vec(a).unwrap()
    );
    // A cold cache (a restart) renders the same bytes too.
    let cold = crate::blobs::Blobs::new(r.core.store.dir());
    let fresh = cold
        .base64(&crate::blobs::digest(&bytes))
        .expect("the blob is on disk");
    assert_eq!(&*fresh, blocks[1]["source"]["data"].as_str().unwrap());
}

/// A model whose catalog entry has no vision reads one line instead.
#[tokio::test]
async fn a_model_without_vision_reads_a_line_instead_of_the_image() {
    let r = rig_with(vec![Scripted::text("I cannot see it.")], |c| {
        c.catalog.insert(
            "claude-sonnet-5-5".into(),
            crate::catalog::CatalogRow {
                vision: Some(false),
                ..Default::default()
            },
        );
    });
    let bytes = crate::attach::tests::png(800, 600, 1_200_000);
    let res = submit(
        &r.core,
        submit_params(
            None,
            "What does it say?",
            vec![image_attached("photo.png", &bytes)],
        ),
    )
    .await;
    assert_eq!(res["output"], "I cannot see it.", "{res}");
    assert_eq!(
        last_user_blocks(&r),
        vec![
            json!({"type": "text", "text": "[Image photo.png from discord:eddie, 1.1 MB: not shown, this model has no vision]"}),
            json!({"type": "text", "text": "What does it say?"}),
        ]
    );
}

/// The budget estimate counts an image by its pixels, not its base64: a
/// 1 MB PNG would otherwise reserve about 333,000 input tokens.
#[tokio::test]
async fn a_one_megabyte_image_is_estimated_by_its_pixels() {
    let r = rig(vec![Scripted::text("ok")]);
    let bytes = crate::attach::tests::png(1920, 1080, 1_000_000);
    let res = submit(
        &r.core,
        submit_params(None, "Look.", vec![image_attached("big.png", &bytes)]),
    )
    .await;
    assert_eq!(res["output"], "ok", "{res}");
    let est = ledgered(&r, "context.compiled")[0]["est_tokens"]
        .as_u64()
        .unwrap();
    let image = crate::catalog::image_tokens("claude-sonnet-5-5", 1920, 1080);
    assert_eq!(image, 69 * 39);
    let req = &r.fake.requests()[0];
    assert_eq!(req.image_tokens, image);
    let text_only = crate::provider::ProviderRequest {
        image_tokens: 0,
        messages: vec![json!({"role": "user", "content": [{"type": "text", "text": "Look."}]})],
        ..req.clone()
    }
    .estimate_tokens();
    assert!(
        est >= image && est < text_only + image + 100,
        "est {est}, image {image}, text {text_only}"
    );
}

/// `fs.read` of a PNG answers the model with the image inside its
/// `tool_result`, and the result node holds the reference.
#[tokio::test]
async fn fs_read_of_a_png_returns_an_image_block() {
    let r = rig(vec![
        Scripted::tools("", &[("t1", "fs_read", json!({"path": "shot.png"}))]),
        Scripted::text("It is a screenshot."),
    ]);
    let bytes = crate::attach::tests::png(640, 480, 300);
    std::fs::write(r.root.join("shot.png"), &bytes).unwrap();
    let res = turn(&r.core, None, "Read shot.png").await;
    assert_eq!(res.output, "It is a screenshot.");
    let reqs = r.fake.requests();
    let user = reqs[1].messages.last().unwrap();
    let content = &user["content"][0]["content"];
    assert_eq!(user["content"][0]["type"], "tool_result");
    assert!(
        content[0]["text"]
            .as_str()
            .unwrap()
            .ends_with("shot.png is a PNG image, 640×480, 333 bytes."),
        "{content}"
    );
    assert_eq!(content[1]["type"], "image");
    assert_eq!(
        crate::blobs::decode(content[1]["source"]["data"].as_str().unwrap()).unwrap(),
        bytes
    );
    let img = r
        .core
        .store
        .session_nodes(&res.session_id)
        .unwrap()
        .into_iter()
        .find_map(|(_, n)| match n.body {
            Body::ToolResult { image, .. } => image,
            _ => None,
        })
        .expect("the result node holds the image");
    assert_eq!(
        img.content,
        crate::node::AttachmentContent::Image {
            digest: crate::blobs::digest(&bytes),
            width: 640,
            height: 480
        }
    );
    assert_eq!(blob_files(&r), 1);
}

/// An image over 5 MiB is not stored and the model reads the reason.
#[tokio::test]
async fn a_six_megabyte_image_is_refused_with_the_reason() {
    let r = rig(vec![Scripted::text("Too big.")]);
    let bytes = crate::attach::tests::png(4000, 3000, 6 * 1024 * 1024);
    let res = submit(
        &r.core,
        submit_params(None, "", vec![image_attached("huge.png", &bytes)]),
    )
    .await;
    assert_eq!(res["output"], "Too big.", "{res}");
    assert_eq!(
        last_user_blocks(&r),
        vec![
            json!({"type": "text", "text": "[Attachment huge.png from discord:eddie, image/png, 6.0 MB: not read: an image over the 5 MiB limit]"})
        ]
    );
    assert_eq!(blob_files(&r), 0);
}

// ------------------------------------------------- an image the provider refuses (theseus-0s4)

/// The provider's 400 for an image it cannot read, at `messages.<m>.content.<b>`.
fn refused_image(m: usize, b: usize) -> Scripted {
    Scripted::Fail(crate::provider::ProviderError::InvalidRequest {
        status: 400,
        message: format!(
            "invalid_request_error: messages.{m}.content.{b}.image.source.base64.data: Could not \
             process image"
        ),
    })
}

/// The first message's blocks in a request.
fn first_user_blocks(q: &crate::provider::ProviderRequest) -> Vec<Value> {
    q.messages[0]["content"].as_array().unwrap().clone()
}

fn carries_an_image(q: &crate::provider::ProviderRequest) -> bool {
    serde_json::to_string(&q.messages)
        .unwrap()
        .contains(r#""type":"image""#)
}

const TIDE_LINE: &str = "[Image tide.png from discord:eddie, 333 bytes: not shown, the provider \
                         refused it (Could not process image)]";

/// theseus-0s4: the provider refuses the first request with a 400 that names
/// the image's block. The image is marked not shown in the session record,
/// the call is made again at once with the image's line, and the turn
/// answers. A later turn does not send the image again, nor does a
/// recompile. Before, every later request of the session carried it and
/// failed the same way.
#[tokio::test]
async fn an_image_the_provider_refuses_is_shown_as_its_line_and_the_call_made_again() {
    let r = rig(vec![
        refused_image(0, 1),
        Scripted::text("I could not see it."),
        Scripted::text("Later."),
        Scripted::text("After the recompile."),
    ]);
    let bytes = crate::attach::tests::png(640, 480, 300);
    let first = submit(
        &r.core,
        submit_params(
            None,
            "What is this?",
            vec![image_attached("tide.png", &bytes)],
        ),
    )
    .await;
    assert_eq!(first["output"], "I could not see it.", "{first}");
    let reqs = r.fake.requests();
    assert_eq!(reqs.len(), 2, "the refused call and its retry");
    assert_eq!(first_user_blocks(&reqs[0])[1]["type"], "image");
    assert_eq!(
        first_user_blocks(&reqs[1]),
        vec![
            json!({"type": "text", "text": TIDE_LINE}),
            json!({"type": "text", "text": "What is this?"})
        ],
        "the retry carries the image's line, not the image"
    );
    let sid = first["session_id"].as_str().unwrap().to_string();
    let rec: SessionRecord = r.core.store.get_session(&sid).unwrap().unwrap();
    let marked: Vec<&str> = rec.not_shown.iter().map(|n| n.digest.as_str()).collect();
    assert_eq!(marked, [crate::blobs::digest(&bytes).as_str()]);
    let rows = ledgered(&r, "image.not_shown");
    assert_eq!(rows.len(), 1, "{rows:?}");
    assert_eq!(
        (&rows[0]["message_index"], &rows[0]["why"]),
        (&json!(0), &json!("Could not process image"))
    );
    assert!(ledgered(&r, "turn.failed").is_empty(), "the turn answered");

    // A later turn does not resend it.
    let later = submit(&r.core, submit_params(Some(&sid), "And now?", vec![])).await;
    assert_eq!(later["output"], "Later.", "{later}");
    // Nor does a recompile.
    r.core
        .store
        .update_session(&sid, |rec| {
            rec.pending_recompile = Some(crate::compiler::Recompile::Transcript);
            Ok(vec![])
        })
        .unwrap();
    let again = submit(&r.core, submit_params(Some(&sid), "Once more?", vec![])).await;
    assert_eq!(again["output"], "After the recompile.", "{again}");
    let compiled = ledgered(&r, "context.compiled");
    assert_eq!(
        compiled.last().unwrap()["trigger"],
        "manual_transcript",
        "{compiled:?}"
    );
    let reqs = r.fake.requests();
    assert_eq!(reqs.len(), 4);
    for q in &reqs[1..] {
        assert!(
            !carries_an_image(q),
            "an image went again: {:?}",
            q.messages
        );
        assert_eq!(first_user_blocks(q)[0]["text"], TIDE_LINE);
    }
}

/// theseus-0s4: the mark is in the session record, so a restart keeps it: a
/// new core on the same store renders the image's line.
#[tokio::test]
async fn an_image_marked_not_shown_stays_so_after_a_restart() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("w");
    std::fs::create_dir_all(&root).unwrap();
    let cfg = config(&root.canonicalize().unwrap(), dir.path());
    let bytes = crate::attach::tests::png(640, 480, 300);
    let turn_with = |core: &Arc<Core>, rec: SessionRecord, input: &str, files| {
        let (live, _) = core.live_profile();
        let target = core.runner.resolve_target(&live, None, None, None).unwrap();
        let sink = EventSink::new(core.bus.clone(), &rec.session_id, None);
        let req = TurnRequest {
            session: rec,
            input: Some(input.into()),
            target,
            sink,
            author: "discord:eddie".into(),
            recompile: None,
            attachments: files,
            arrived: None,
            config_wait_us: 0,
            reply_to: None,
        };
        let core = core.clone();
        async move { core.runner.run(req).await }
    };
    let sid = {
        let store = Store::open(&dir.path().join("store")).unwrap();
        let fake = Arc::new(FakeProvider::scripted(vec![
            refused_image(0, 1),
            Scripted::text("Not seen."),
        ]));
        let core = Core::build(crate::rpc::Parts::for_tests(
            cfg.clone(),
            fake.clone(),
            store,
        ))
        .unwrap();
        let rec = SessionRecord::new(SessionKind::Conversation, None);
        core.store.put_session(&rec.session_id, &rec).unwrap();
        let sid = rec.session_id.clone();
        let res = turn_with(
            &core,
            rec,
            "What is this?",
            vec![image_attached("tide.png", &bytes)],
        )
        .await
        .unwrap();
        assert_eq!(res.output, "Not seen.");
        assert_eq!(fake.requests().len(), 2);
        sid
    };
    // A restart: a new core on the same store, whose model would answer.
    let store = Store::open(&dir.path().join("store")).unwrap();
    let fake = Arc::new(FakeProvider::scripted(vec![Scripted::text(
        "Still not seen.",
    )]));
    let core = Core::build(crate::rpc::Parts::for_tests(cfg, fake.clone(), store)).unwrap();
    let rec: SessionRecord = core.store.get_session(&sid).unwrap().unwrap();
    assert_eq!(rec.not_shown.len(), 1);
    let res = turn_with(&core, rec, "And after the restart?", vec![])
        .await
        .unwrap();
    assert_eq!(res.output, "Still not seen.");
    let q = &fake.requests()[0];
    assert!(!carries_an_image(q), "{:?}", q.messages);
    assert_eq!(first_user_blocks(q)[0]["text"], TIDE_LINE);
}

/// theseus-0s4: an image the provider took before and refuses now sits
/// under the model's earlier answer, whose thinking was given for it. Its
/// line replaces it, so the retry's compilation strips that thinking
/// (`image_not_shown`), as any change under it does.
#[tokio::test]
async fn an_image_refused_under_an_answer_strips_that_answers_thinking() {
    let answered = Scripted::Blocks {
        blocks: vec![
            json!({"type": "thinking", "thinking": "a tide chart, high water at four", "signature": "sig-tide"}),
            json!({"type": "text", "text": "A tide chart."}),
        ],
        stop_reason: "end_turn".into(),
    };
    let r = rig(vec![
        answered,
        refused_image(0, 1),
        Scripted::text("Noted."),
    ]);
    let bytes = crate::attach::tests::png(640, 480, 300);
    let first = submit(
        &r.core,
        submit_params(
            None,
            "What is this?",
            vec![image_attached("tide.png", &bytes)],
        ),
    )
    .await;
    assert_eq!(first["output"], "A tide chart.", "{first}");
    let sid = first["session_id"].as_str().unwrap().to_string();
    let second = submit(&r.core, submit_params(Some(&sid), "And the low?", vec![])).await;
    assert_eq!(second["output"], "Noted.", "{second}");
    let reqs = r.fake.requests();
    assert_eq!(reqs.len(), 3);
    let thinking = |q: &crate::provider::ProviderRequest| {
        serde_json::to_string(&q.messages)
            .unwrap()
            .contains(r#""type":"thinking""#)
    };
    assert!(
        thinking(&reqs[1]) && carries_an_image(&reqs[1]),
        "the refused request"
    );
    assert!(
        !thinking(&reqs[2]) && !carries_an_image(&reqs[2]),
        "{:?}",
        reqs[2].messages
    );
    assert_eq!(first_user_blocks(&reqs[2])[0]["text"], TIDE_LINE);
    let compiled = ledgered(&r, "context.compiled");
    assert_eq!(
        compiled.last().unwrap()["trigger"],
        "image_not_shown",
        "{compiled:?}"
    );
}

/// theseus-0s4: the same image sent again, and refused the second time, by
/// the later copy's block alone. The mark hides both copies, and the first
/// sat under the model's answer, so the retry strips that answer's thinking
/// too: the API binds a thinking block to every message before it.
#[tokio::test]
async fn an_image_sent_again_and_refused_strips_the_thinking_over_its_first_copy() {
    let answered = Scripted::Blocks {
        blocks: vec![
            json!({"type": "thinking", "thinking": "a tide chart, high water at four", "signature": "sig-tide"}),
            json!({"type": "text", "text": "A tide chart."}),
        ],
        stop_reason: "end_turn".into(),
    };
    let r = rig(vec![
        answered,
        refused_image(2, 1),
        Scripted::text("The same chart."),
    ]);
    let bytes = crate::attach::tests::png(640, 480, 300);
    let first = submit(
        &r.core,
        submit_params(
            None,
            "What is this?",
            vec![image_attached("tide.png", &bytes)],
        ),
    )
    .await;
    assert_eq!(first["output"], "A tide chart.", "{first}");
    let sid = first["session_id"].as_str().unwrap().to_string();
    let second = submit(
        &r.core,
        submit_params(
            Some(&sid),
            "And this one?",
            vec![image_attached("tide.png", &bytes)],
        ),
    )
    .await;
    assert_eq!(second["output"], "The same chart.", "{second}");
    let reqs = r.fake.requests();
    assert_eq!(reqs.len(), 3);
    let thinking = |q: &crate::provider::ProviderRequest| {
        serde_json::to_string(&q.messages)
            .unwrap()
            .contains(r#""type":"thinking""#)
    };
    assert!(
        thinking(&reqs[1]) && carries_an_image(&reqs[1]),
        "the refused request"
    );
    assert!(
        !thinking(&reqs[2]) && !carries_an_image(&reqs[2]),
        "{:?}",
        reqs[2].messages
    );
    for m in [0, 2] {
        assert_eq!(reqs[2].messages[m]["content"][0]["text"], TIDE_LINE, "{m}");
    }
    let rows = ledgered(&r, "image.not_shown");
    assert_eq!(rows.len(), 1, "{rows:?}");
    assert_eq!(rows[0]["message_index"], 2, "the copy the 400 named");
    let compiled = ledgered(&r, "context.compiled");
    assert_eq!(
        compiled.last().unwrap()["trigger"],
        "image_not_shown",
        "{compiled:?}"
    );
}

/// theseus-0s4: Anthropic's 400 for an image it cannot decode names no block
/// ("Could not process image", seen in fb1b's live check). In a session that
/// already holds an image the model answered over, the new image is the one
/// hidden, and the earlier one still shows. Before, two images in the
/// request meant nothing was hidden, and the session stayed poisoned.
#[tokio::test]
async fn a_400_that_names_no_block_hides_the_new_image_and_keeps_the_answered_one() {
    let r = rig(vec![
        Scripted::text("A tide chart."),
        Scripted::Fail(crate::provider::ProviderError::InvalidRequest {
            status: 400,
            message: "invalid_request_error: Could not process image".into(),
        }),
        Scripted::text("I can see only the tide chart."),
    ]);
    let (good, bad) = (
        crate::attach::tests::png(640, 480, 300),
        crate::attach::tests::png(320, 200, 4120),
    );
    let first = submit(
        &r.core,
        submit_params(
            None,
            "What is this?",
            vec![image_attached("tide.png", &good)],
        ),
    )
    .await;
    assert_eq!(first["output"], "A tide chart.", "{first}");
    let sid = first["session_id"].as_str().unwrap().to_string();
    let second = submit(
        &r.core,
        submit_params(
            Some(&sid),
            "And this one?",
            vec![image_attached("reef.png", &bad)],
        ),
    )
    .await;
    assert_eq!(
        second["output"], "I can see only the tide chart.",
        "{second}"
    );
    let reqs = r.fake.requests();
    assert_eq!(reqs.len(), 3);
    // The retry: the answered image still goes; the new one is its line.
    assert_eq!(first_user_blocks(&reqs[2])[1]["type"], "image");
    let line = reqs[2].messages[2]["content"][0]["text"].as_str().unwrap();
    assert!(
        line.starts_with("[Image reef.png from discord:eddie, 4,153 bytes: not shown")
            && line.ends_with("the provider refused it (Could not process image)]"),
        "{line}"
    );
    let rec: SessionRecord = r.core.store.get_session(&sid).unwrap().unwrap();
    let marked: Vec<&str> = rec.not_shown.iter().map(|n| n.digest.as_str()).collect();
    assert_eq!(marked, [crate::blobs::digest(&bad).as_str()]);
    assert!(ledgered(&r, "turn.failed").is_empty(), "the turn answered");
}

// ---------------------------------------------------------------- approval (theseus-sgh)

const EDDIE: &str = "159471966640799744";
const MALLORY: &str = "222222222222222222";

/// A rig whose `[approval]` trusts Eddie and lists these channels.
fn approval_rig(script: Vec<Scripted>, channels: &[&str]) -> Rig {
    let channels: Vec<String> = channels.iter().map(|c| c.to_string()).collect();
    rig_with(script, move |c| {
        c.approval = Some(crate::config::ApprovalConfig {
            trusted_users: vec![format!("discord:{EDDIE}")],
            channels,
        })
    })
}

/// One `action.confirm` over a real protocol connection, accepted as
/// `client` (a label and the surface its listener names): the result, or
/// the error.
async fn answer_as(
    core: &Arc<Core>,
    client: crate::approval::Client,
    corr: &str,
    discord: Option<(&str, Option<&str>)>,
) -> Result<Value, theseus_protocol::RpcError> {
    let params = theseus_protocol::ActionConfirmParams {
        correlation_id: corr.into(),
        approve: true,
        note: None,
        watch: false,
        author: discord.map(|_| "discord:eddie".to_string()),
        discord: origin(discord),
        trust: false,
    };
    rpc_as(
        core,
        client,
        theseus_protocol::method::ACTION_CONFIRM,
        serde_json::to_value(params).unwrap(),
    )
    .await
}

/// What the Discord binding names for a press: a user in channel
/// 444444444444444444, in a guild or (None) a DM.
fn origin(discord: Option<(&str, Option<&str>)>) -> Option<theseus_protocol::DiscordOrigin> {
    discord.map(|(user, guild)| theseus_protocol::DiscordOrigin {
        user_id: user.into(),
        channel_id: "444444444444444444".into(),
        guild_id: guild.map(str::to_string),
    })
}

/// One request over a real protocol connection, accepted as `client`: the
/// result, or the error.
async fn rpc_as(
    core: &Arc<Core>,
    client: crate::approval::Client,
    method: &str,
    params: Value,
) -> Result<Value, theseus_protocol::RpcError> {
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
    let (ours, theirs) = tokio::io::duplex(64 * 1024);
    let (sr, sw) = tokio::io::split(theirs);
    let srv = tokio::spawn(core.clone().serve_connection(sr, sw, client));
    let (cr, mut cw) = tokio::io::split(ours);
    let req = theseus_protocol::Request::new(theseus_protocol::Id::Num(1), method, params);
    let mut line = serde_json::to_string(&req).unwrap();
    line.push('\n');
    cw.write_all(line.as_bytes()).await.unwrap();
    let mut lines = BufReader::new(cr).lines();
    let out = loop {
        let l = lines.next_line().await.unwrap().unwrap();
        if let theseus_protocol::Message::Response(r) = serde_json::from_str(&l).unwrap() {
            break match (r.result, r.error) {
                (Some(v), _) => Ok(v),
                (None, e) => Err(e.unwrap()),
            };
        }
    };
    cw.shutdown().await.unwrap();
    drop(lines);
    let _ = srv.await;
    out
}

fn surface(label: &str, s: crate::approval::Surface) -> crate::approval::Client {
    crate::approval::Client::new(label, s)
}

fn write_script() -> Vec<Scripted> {
    vec![
        Scripted::tools(
            "",
            &[(
                "t1",
                "fs_write",
                json!({"path": "out.txt", "content": "approved\n"}),
            )],
        ),
        Scripted::text("Written."),
    ]
}

/// With `[approval]`, an answer counts only from a trusted user through a
/// trusted channel. Each that does not is refused with the reason, ledgered
/// as `approval.refused` (who, where, why), and narrated, and the call keeps
/// waiting: the action is still planned, the execution still waits, nothing
/// is resolved, and nothing is written. Then Eddie approves in his DM and the
/// write runs.
#[tokio::test]
async fn with_approval_only_a_trusted_user_in_a_trusted_channel_approves() {
    use crate::approval::Surface::{Cli, Discord, Web};
    let r = approval_rig(write_script(), &["discord:dm"]);
    let (sid, mut rx) = watched_session(&r);
    let res = turn(&r.core, Some(&sid), "write out.txt").await;
    let corr = res.awaiting_confirm.clone().expect("the write waits");
    let exec = res.execution_id.clone().unwrap();
    let refusals = [
        // A trusted user through an unlisted surface: the CLI, the web UI,
        // and a guild channel the section does not list.
        (
            answer_as(&r.core, surface("sock#1", Cli), &corr, None).await,
            "the CLI is not a trusted channel ([approval] channels = [\"discord:dm\"])",
            "cli",
        ),
        (
            answer_as(&r.core, surface("web#1", Web), &corr, None).await,
            "the web UI is not a trusted channel",
            "web",
        ),
        (
            answer_as(
                &r.core,
                surface("discord", Discord),
                &corr,
                Some((EDDIE, Some("712398310421561444"))),
            )
            .await,
            "Discord channel 444444444444444444 is not a trusted channel",
            "discord:444444444444444444",
        ),
        // An untrusted user in a trusted channel.
        (
            answer_as(
                &r.core,
                surface("discord", Discord),
                &corr,
                Some((MALLORY, None)),
            )
            .await,
            "discord:222222222222222222 is not a trusted user ([approval] trusted_users)",
            "discord:dm",
        ),
        // Eddie's ids, claimed by a connection that is not the binding.
        (
            answer_as(&r.core, surface("sock#2", Cli), &corr, Some((EDDIE, None))).await,
            "only the Discord binding can name a Discord channel and user",
            "cli",
        ),
    ];
    for (i, (got, why, via)) in refusals.iter().enumerate() {
        let e = got.as_ref().expect_err("refused");
        assert_eq!(e.code, theseus_protocol::error_code::REFUSED, "{i}: {e:?}");
        assert!(e.message.contains(why), "{i}: {}", e.message);
        assert!(e.message.contains("It keeps waiting"), "{i}: {}", e.message);
        assert_eq!(e.data["via"], *via, "{i}: {e:?}");
    }
    // Nothing moved.
    let a = r.core.kernel.action(&corr).unwrap().unwrap();
    assert_eq!(a.state, theseus_kernel::ActionState::Planned);
    let e = r.core.kernel.execution(&exec).unwrap().unwrap();
    assert_eq!(e.state.as_str(), "waiting");
    assert_eq!(r.core.pending_confirms(&sid).unwrap().len(), 1);
    assert!(sent(&mut rx, theseus_protocol::notify::CONFIRM_RESOLVED).is_empty());
    assert!(!r.root.join("out.txt").exists());
    let rows = ledgered(&r, "approval.refused");
    assert_eq!(rows.len(), refusals.len());
    assert_eq!(rows[3]["who"], format!("discord:{MALLORY} (discord:eddie)"));
    assert_eq!(
        (rows[3]["via"].as_str(), rows[3]["tool"].as_str()),
        (Some("discord:dm"), Some("fs.write"))
    );
    assert!(rows[0]["why"]
        .as_str()
        .unwrap()
        .starts_with("the CLI is not"));
    assert!(ledgered(&r, "action.confirm_answered").is_empty());
    let lines = narrated(&r, &sid);
    assert!(
        said(
            &lines,
            "approval",
            "did not count: the CLI is not a trusted channel"
        ),
        "{}",
        dump(&lines)
    );

    // Eddie, in his DM.
    let ok = answer_as(
        &r.core,
        surface("discord", Discord),
        &corr,
        Some((EDDIE, None)),
    )
    .await
    .unwrap();
    assert_eq!(ok["approved"], true);
    let answered = ledgered(&r, "action.confirm_answered");
    assert_eq!(
        (answered[0]["by"].as_str(), answered[0]["via"].as_str()),
        (Some("discord:eddie"), Some("discord:dm"))
    );
    let cont = r.core.continue_execution(&exec).await.unwrap().unwrap();
    assert_eq!(cont.output, "Written.");
    assert_eq!(
        std::fs::read_to_string(r.root.join("out.txt")).unwrap(),
        "approved\n"
    );
}

/// The CLI counts when `[approval]` lists it, and the web UI does not when
/// it is left out; the bare label a test passes is never trusted.
#[tokio::test]
async fn a_listed_cli_approves_and_an_unlisted_web_ui_is_refused() {
    use crate::approval::Surface::{Cli, Web};
    let r = approval_rig(write_script(), &["cli"]);
    let res = turn(&r.core, None, "write out.txt").await;
    let corr = res.awaiting_confirm.clone().unwrap();
    let e = answer_as(&r.core, surface("web#1", Web), &corr, None)
        .await
        .unwrap_err();
    assert!(
        e.message
            .contains("the web UI is not a trusted channel ([approval] channels = [\"cli\"])"),
        "{}",
        e.message
    );
    let e = r
        .core
        .confirm_action(&corr, true, None, "test")
        .unwrap_err();
    assert!(e.to_string().contains("never a trusted channel"), "{e}");
    answer_as(&r.core, surface("sock#1", Cli), &corr, None)
        .await
        .unwrap();
    let answered = ledgered(&r, "action.confirm_answered");
    assert_eq!(answered[0]["via"], "cli");
    // The surface, not the connection's label, as a cancel names it
    // (theseus-qiy).
    assert_eq!(answered[0]["by"], "the CLI");
    let h = r.core.health().approval;
    assert!(h.configured);
    assert_eq!(h.trusted_users, [format!("discord:{EDDIE}")]);
    assert_eq!(
        (h.channels[0].channel.as_str(), h.channels[0].state.as_str()),
        ("cli", "trusted")
    );
}

/// The budget question (theseus-0sg) follows the same rule: the web UI is
/// refused when only the CLI is listed, the spend is not reset, and the
/// session keeps waiting; the CLI's answer resets it.
#[tokio::test]
async fn the_budget_question_follows_the_same_rule() {
    use crate::approval::Surface::{Cli, Web};
    use theseus_kernel::ExecState;
    let script = vec![
        Scripted::tools(
            &"word ".repeat(30_000),
            &[("t1", "text_diff", json!({"a": "x\n", "b": "y\n"}))],
        ),
        Scripted::text("The diff is one line."),
    ];
    let r = rig_with(script, |c| {
        c.kernel.spend_limit_usd = 1.40;
        c.approval = Some(crate::config::ApprovalConfig {
            trusted_users: vec![],
            channels: vec!["cli".into()],
        });
    });
    let res = turn(&r.core, None, "diff these").await;
    assert_eq!(res.stop_reason, "budget");
    let q = res.awaiting_confirm.clone().unwrap();
    let exec = res.execution_id.clone().unwrap();
    let spent = r
        .core
        .kernel
        .execution(&exec)
        .unwrap()
        .unwrap()
        .budget
        .spent_micros;
    let e = answer_as(&r.core, surface("web#1", Web), &q, None)
        .await
        .unwrap_err();
    assert_eq!(e.code, theseus_protocol::error_code::REFUSED);
    assert!(
        e.message.contains("the web UI is not a trusted channel"),
        "{}",
        e.message
    );
    let x = r.core.kernel.execution(&exec).unwrap().unwrap();
    assert_eq!(
        (x.state, x.budget.spent_micros, x.budget.resets),
        (ExecState::Waiting, spent, 0),
        "nothing was reset"
    );
    assert!(ledgered(&r, "budget.reset").is_empty());
    let refused = ledgered(&r, "approval.refused");
    assert_eq!(refused[0]["tool"], "budget.reset");
    assert_eq!(r.core.pending_confirms(&res.session_id).unwrap().len(), 1);

    let ok = answer_as(&r.core, surface("sock#1", Cli), &q, None)
        .await
        .unwrap();
    assert_eq!(
        (ok["approved"].as_bool(), ok["resumes"].as_bool()),
        (Some(true), Some(true))
    );
    let x = r.core.kernel.execution(&exec).unwrap().unwrap();
    assert_eq!((x.budget.spent_micros, x.budget.resets), (0, 1));
    assert_eq!(ledgered(&r, "budget.reset")[0]["by"], "the CLI");
}

/// Without `[approval]` every surface answers as before theseus-sgh: a CLI,
/// a web UI, a Discord, and an unnamed connection each approve a waiting
/// call, and nothing is refused.
#[tokio::test]
async fn without_approval_every_surface_answers_as_before() {
    use crate::approval::Surface::{Cli, Discord, Unnamed, Web};
    for (client, discord) in [
        (surface("sock#1", Cli), None),
        (surface("web#1", Web), None),
        (
            surface("discord", Discord),
            Some((MALLORY, Some("712398310421561444"))),
        ),
        (surface("test", Unnamed), None),
    ] {
        let r = rig(write_script());
        assert!(!r.core.health().approval.configured);
        let res = turn(&r.core, None, "write out.txt").await;
        let corr = res.awaiting_confirm.clone().unwrap();
        let label = client.label.clone();
        let ok = answer_as(&r.core, client, &corr, discord).await;
        assert_eq!(ok.unwrap()["approved"], true, "{label}");
        assert!(ledgered(&r, "approval.refused").is_empty());
    }
}

// ---------------------------------------------------------------- should have asked (theseus-sgh)

fn echo(id: &str, word: &str) -> Scripted {
    Scripted::tools("", &[(id, "proc_run", json!({"argv": ["echo", word]}))])
}

/// `policy.tighten` or `policy.untighten` of proc.run over a connection
/// accepted as `client`, pressed by `discord` when the binding names one.
async fn press_as(
    core: &Arc<Core>,
    method: &str,
    client: crate::approval::Client,
    discord: Option<(&'static str, Option<&'static str>)>,
) -> Result<Value, theseus_protocol::RpcError> {
    let p = json!({"tool": "proc.run", "author": discord.map(|_| "discord:eddie"),
                   "discord": origin(discord)});
    rpc_as(core, client, method, p).await
}

fn tool_row(tools: &Value, name: &str) -> Value {
    tools["tools"]
        .as_array()
        .unwrap()
        .iter()
        .find(|t| t["name"] == name)
        .cloned()
        .unwrap()
}

/// Under notify a call runs with a notice that names its call. One press on
/// it makes the next call of that tool wait for approval, and health, the
/// tool list, and the narrative say so; the ledger row keeps the call's
/// proposal digest and tool, a labeled example for later. An undo makes the
/// tool run with a notice again.
#[tokio::test]
async fn should_have_asked_makes_the_next_call_wait_and_an_undo_notifies_again() {
    use crate::approval::Surface::{Cli, Web};
    use theseus_protocol::{method, notify};
    let r = rig_with(
        vec![
            echo("t1", "first"),
            Scripted::text("Ran with a notice."),
            echo("t2", "second"),
            Scripted::text("It printed."),
            echo("t3", "third"),
            Scripted::text("Ran with a notice again."),
        ],
        |cfg| cfg.policy.enforcement = Posture::Notify,
    );
    let (sid, mut rx) = watched_session(&r);
    let res = turn(&r.core, Some(&sid), "echo first").await;
    assert!(res.awaiting_confirm.is_none(), "notify runs");
    let notices = sent(&mut rx, notify::POLICY_NOTIFIED);
    assert_eq!(notices.len(), 1);
    let corr = notices[0]["correlation_id"].as_str().unwrap().to_string();
    assert_eq!(
        ledgered(&r, "tool.notified")[0]["correlation_id"],
        corr.as_str(),
        "the notice names its call"
    );

    // The press, from the web UI.
    let t = rpc_as(
        &r.core,
        surface("web#1", Web),
        method::POLICY_TIGHTEN,
        json!({"tool": "proc.run", "correlation_id": corr}),
    )
    .await
    .unwrap();
    assert_eq!(
        (
            t["posture"].as_str(),
            t["setting"].as_str(),
            t["config_setting"].as_str()
        ),
        (
            Some("approve"),
            Some("tightened by the web UI"),
            Some("enforcement = notify")
        )
    );
    assert_eq!(
        (t["changed"].as_bool(), t["already"].as_bool()),
        (Some(true), Some(false))
    );
    let digest = r.core.kernel.action(&corr).unwrap().unwrap().args_digest;
    let rows = ledgered(&r, "policy.tightened");
    assert_eq!(rows.len(), 1);
    for (k, want) in [
        ("tool", "proc.run"),
        ("correlation_id", corr.as_str()),
        ("digest", digest.as_str()),
        ("by", "the web UI"),
        ("via", "web"),
        ("posture", "approve"),
    ] {
        assert_eq!(rows[0][k], want, "{k}: {}", rows[0]);
    }
    assert_eq!(
        sent(&mut rx, notify::POLICY_TIGHTENED).len(),
        1,
        "watchers hear it"
    );
    let h = r.core.health();
    assert_eq!(h.tightenings.len(), 1);
    assert_eq!(
        (
            h.tightenings[0].tool.as_str(),
            h.tightenings[0].session_id.as_deref()
        ),
        ("proc.run", Some(sid.as_str()))
    );
    let tools = rpc_as(
        &r.core,
        surface("sock#1", Cli),
        method::TOOL_LIST,
        Value::Null,
    )
    .await
    .unwrap();
    let pr = tool_row(&tools, "proc.run");
    assert_eq!(
        (pr["policy"].as_str(), pr["config_posture"].as_str()),
        (Some("approve"), Some("notify"))
    );
    assert_eq!(pr["tightened"]["by"], "the web UI");
    assert_eq!(
        tool_row(&tools, "fs.write")["policy"],
        "notify",
        "only that tool"
    );
    let lines = narrated(&r, &sid);
    assert!(
        said(
            &lines,
            "approval",
            "proc.run now asks first: tightened by the web UI."
        ),
        "{}",
        dump(&lines)
    );

    // The next call of that tool waits for approval.
    let res = turn(&r.core, Some(&sid), "echo second").await;
    let waiting = res.awaiting_confirm.clone().expect("a tightened tool asks");
    assert!(
        sent(&mut rx, notify::POLICY_NOTIFIED).is_empty(),
        "it asks instead"
    );
    let pending = r.core.pending_confirms(&sid).unwrap();
    assert!(
        pending[0].reason.ends_with(
            "proc.run — approve (tightened by the web UI; the config says enforcement = notify)"
        ),
        "{}",
        pending[0].reason
    );
    assert!(
        results(&r.core, &sid).len() == 1,
        "the second call has not run"
    );
    r.core.confirm_action(&waiting, true, None, "test").unwrap();
    let exec = res.execution_id.clone().unwrap();
    let cont = r.core.continue_execution(&exec).await.unwrap().unwrap();
    assert_eq!(cont.output, "It printed.");

    // The undo, from the CLI: the tool runs with a notice again.
    let u = rpc_as(
        &r.core,
        surface("sock#2", Cli),
        method::POLICY_UNTIGHTEN,
        json!({"tool": "proc.run"}),
    )
    .await
    .unwrap();
    assert_eq!(
        (
            u["posture"].as_str(),
            u["setting"].as_str(),
            u["changed"].as_bool()
        ),
        (Some("notify"), Some("enforcement = notify"), Some(true))
    );
    let rows = ledgered(&r, "policy.untightened");
    assert_eq!(
        (
            rows[0]["by"].as_str(),
            rows[0]["tightened_by"].as_str(),
            rows[0]["correlation_id"].as_str(),
            rows[0]["via"].as_str()
        ),
        (
            Some("the CLI"),
            Some("the web UI"),
            Some(corr.as_str()),
            Some("cli")
        )
    );
    assert!(r.core.health().tightenings.is_empty());
    assert_eq!(sent(&mut rx, notify::POLICY_UNTIGHTENED).len(), 1);
    let res = turn(&r.core, Some(&sid), "echo third").await;
    assert!(res.awaiting_confirm.is_none(), "back to notify");
    assert_eq!(res.output, "Ran with a notice again.");
    assert_eq!(sent(&mut rx, notify::POLICY_NOTIFIED).len(), 1);
    let lines = narrated(&r, &sid);
    assert!(
        said(
            &lines,
            "approval",
            "proc.run is back to what the config says (notify, enforcement = notify): the CLI \
             undid the tightening by the web UI."
        ),
        "{}",
        dump(&lines)
    );
}

/// A tightening never loosens. Under a config `approve`, a press changes
/// nothing: the call waits with the config's own reason. An undo does not
/// loosen past the config. A second press records nothing, a call names
/// only its own tool, and an unknown tool is an error.
#[tokio::test]
async fn a_tightening_never_loosens_and_its_undo_stops_at_the_config() {
    let r = rig_with(vec![echo("t1", "one"), echo("t2", "two")], |cfg| {
        cfg.policy.enforcement = Posture::Notify;
        cfg.policy.tools.insert("proc.run".into(), Posture::Approve);
    });
    let t = r.core.tighten("proc.run", None, "test").unwrap();
    assert_eq!(
        (t.posture.as_str(), t.setting.as_str(), t.changed),
        ("approve", "[policy.tools] \"proc.run\" = approve", false)
    );
    assert!(t.tightening.correlation_id.is_none() && t.tightening.digest.is_none());
    let again = r.core.tighten("proc.run", None, "test").unwrap();
    assert!(again.already && !again.changed);
    assert_eq!(
        ledgered(&r, "policy.tightened").len(),
        1,
        "a second press records nothing"
    );
    let res = turn(&r.core, None, "echo one").await;
    let corr = res.awaiting_confirm.clone().expect("the config asks");
    let p = r.core.pending_confirms(&res.session_id).unwrap();
    assert!(
        p[0].reason
            .ends_with("proc.run — approve ([policy.tools] \"proc.run\" = approve)"),
        "the config's reason, unchanged: {}",
        p[0].reason
    );
    let e = r.core.tighten("fs.write", Some(&corr), "test").unwrap_err();
    assert!(
        e.to_string()
            .contains("is a call of proc.run, not fs.write"),
        "{e}"
    );
    let u = r.core.untighten("proc.run", "test").unwrap();
    assert_eq!((u.posture.as_str(), u.changed), ("approve", false));
    let res = turn(&r.core, None, "echo two").await;
    assert!(res.awaiting_confirm.is_some(), "the config still asks");
    let e = r.core.untighten("proc.run", "test").unwrap_err();
    assert!(e.to_string().contains("proc.run is not tightened"), "{e}");
    let e = r.core.tighten("proc.runn", None, "test").unwrap_err();
    assert!(
        e.to_string().contains("no tool is named \"proc.runn\""),
        "{e}"
    );
    let e = r
        .core
        .tighten("proc.run", Some("act_nope"), "test")
        .unwrap_err();
    assert!(e.to_string().contains("no call act_nope"), "{e}");
}

/// A tightening is in the store, not the process: after a restart the tool
/// still asks, and its first call waits.
#[tokio::test]
async fn a_tightening_survives_a_restart() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("w");
    std::fs::create_dir_all(&root).unwrap();
    let mut cfg = config(&root.canonicalize().unwrap(), dir.path());
    cfg.policy.enforcement = Posture::Notify;
    {
        let store = Store::open(&dir.path().join("store")).unwrap();
        let fake = Arc::new(FakeProvider::scripted(vec![]));
        let core = Core::build(crate::rpc::Parts::for_tests(cfg.clone(), fake, store)).unwrap();
        core.tighten("proc.run", None, "sock#1").unwrap();
    }
    let store = Store::open(&dir.path().join("store")).unwrap();
    let fake = Arc::new(FakeProvider::scripted(vec![echo("t1", "after")]));
    let core = Core::build(crate::rpc::Parts::for_tests(cfg, fake, store)).unwrap();
    let h = core.health();
    assert_eq!(
        (h.tightenings.len(), h.tightenings[0].by.as_str()),
        (1, "sock#1")
    );
    let res = turn(&core, None, "echo after").await;
    assert!(res.awaiting_confirm.is_some(), "still tightened");
    let p = core.pending_confirms(&res.session_id).unwrap();
    assert!(
        p[0].reason.contains("tightened by sock#1"),
        "{}",
        p[0].reason
    );
}

/// With `[approval]`, a press only makes calls ask, so any surface that can
/// answer an approval may make one. The undo loosens, so it takes the same
/// trusted answer an approval does: refused through an untrusted surface,
/// with the reason and an `approval.refused` row, and the tool keeps asking.
/// A connection no listener named and a Discord claim from the CLI are
/// refused either way.
#[tokio::test]
async fn only_a_trusted_answer_undoes_a_tightening() {
    use crate::approval::Surface::{Cli, Discord, Unnamed, Web};
    use theseus_protocol::method;
    let r = approval_rig(vec![], &["discord:dm"]);
    let tighten = |client, discord| press_as(&r.core, method::POLICY_TIGHTEN, client, discord);
    let untighten = |client, discord| press_as(&r.core, method::POLICY_UNTIGHTEN, client, discord);
    // A press counts from the CLI, which is not a trusted channel here, and
    // from an untrusted user in a guild channel.
    tighten(surface("sock#1", Cli), None).await.unwrap();
    let again = tighten(
        surface("discord", Discord),
        Some((MALLORY, Some("712398310421561444"))),
    )
    .await
    .unwrap();
    assert_eq!(again["already"], true);
    for (client, discord, why) in [
        (surface("x", Unnamed), None, "never a trusted channel"),
        (
            surface("sock#2", Cli),
            Some((EDDIE, None)),
            "only the Discord binding can name",
        ),
    ] {
        let e = tighten(client, discord).await.unwrap_err();
        assert_eq!(e.code, theseus_protocol::error_code::REFUSED);
        assert!(e.message.contains(why), "{}", e.message);
        assert!(
            e.message.ends_with("proc.run keeps its posture"),
            "{}",
            e.message
        );
    }
    // The undo takes the whole rule.
    for (client, discord, why) in [
        (
            surface("sock#3", Cli),
            None,
            "the CLI is not a trusted channel",
        ),
        (
            surface("web#1", Web),
            None,
            "the web UI is not a trusted channel",
        ),
        (
            surface("discord", Discord),
            Some((MALLORY, None)),
            "is not a trusted user",
        ),
        (
            surface("sock#4", Cli),
            Some((EDDIE, None)),
            "only the Discord binding can name",
        ),
    ] {
        let e = untighten(client, discord).await.unwrap_err();
        assert_eq!(e.code, theseus_protocol::error_code::REFUSED, "{e:?}");
        assert!(e.message.contains(why), "{}", e.message);
        assert!(
            e.message.ends_with("proc.run keeps asking first"),
            "{}",
            e.message
        );
    }
    assert_eq!(r.core.health().tightenings.len(), 1, "still tightened");
    assert!(ledgered(&r, "policy.untightened").is_empty());
    let refused = ledgered(&r, "approval.refused");
    let undos: Vec<&Value> = refused
        .iter()
        .filter(|x| x["act"] == "policy.untighten")
        .collect();
    assert_eq!(undos.len(), 4);
    assert_eq!(
        (undos[0]["tool"].as_str(), undos[0]["via"].as_str()),
        (Some("proc.run"), Some("cli"))
    );
    assert_eq!(
        refused
            .iter()
            .filter(|x| x["act"] == "policy.tighten")
            .count(),
        2
    );
    let lines = r.core.narrator.tail();
    assert!(
        said(
            &lines,
            "approval",
            "An undo of proc.run's tightening from the CLI through cli did not count"
        ),
        "{}",
        dump(&lines)
    );
    // Eddie, in his DM.
    let ok = untighten(surface("discord", Discord), Some((EDDIE, None)))
        .await
        .unwrap();
    assert_eq!(
        ok["posture"], "approve",
        "the rig's config asks for proc.run"
    );
    let undone = ledgered(&r, "policy.untightened");
    assert_eq!(
        (undone[0]["by"].as_str(), undone[0]["via"].as_str()),
        (Some("discord:eddie"), Some("discord:dm"))
    );
    assert!(r.core.health().tightenings.is_empty());
}

/// Without `[approval]` a press and an undo behave as an answer does
/// today: every surface may make them, and nothing is refused.
#[tokio::test]
async fn without_approval_every_surface_tightens_and_undoes() {
    use crate::approval::Surface::{Cli, Discord, Unnamed, Web};
    use theseus_protocol::method;
    let r = rig_with(vec![], |cfg| cfg.policy.enforcement = Posture::Notify);
    // Each names its surface, as a cancel does; a connection no listener
    // named, its own label (theseus-qiy).
    for (label, s, discord, by) in [
        ("sock#1", Cli, None, "the CLI"),
        ("web#1", Web, None, "the web UI"),
        (
            "discord",
            Discord,
            Some((MALLORY, Some("712398310421561444"))),
            "the Discord binding",
        ),
        ("test", Unnamed, None, "test"),
    ] {
        let p = json!({"tool": "proc.run", "discord": origin(discord)});
        let t = rpc_as(
            &r.core,
            surface(label, s),
            method::POLICY_TIGHTEN,
            p.clone(),
        )
        .await
        .unwrap();
        assert_eq!(
            (t["changed"].as_bool(), t["by"].as_str()),
            (Some(true), Some(by))
        );
        let u = rpc_as(&r.core, surface(label, s), method::POLICY_UNTIGHTEN, p)
            .await
            .unwrap();
        assert_eq!(u["posture"], "notify", "{label}");
    }
    assert!(ledgered(&r, "approval.refused").is_empty());
    assert_eq!(ledgered(&r, "policy.tightened").len(), 4);
    assert_eq!(ledgered(&r, "policy.untightened").len(), 4);
}

// ---------------------------------------------------------------- parallel tool calls (theseus-a60)

mod parallel {
    use super::*;
    use std::collections::HashMap;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Mutex, OnceLock};
    use std::time::Instant;

    use theseus_kernel::ActionState;
    use theseus_protocol::Span;
    use theseus_tools::{Plan, Retry, Tool, ToolClass, ToolCtx, ToolFailure, ToolOutput};

    /// How long each run of a slowed toollet sleeps first, by its tool and
    /// input, and when each run started and ended.
    #[derive(Default)]
    struct Timing {
        delay_ms: Mutex<HashMap<String, u64>>,
        runs: Mutex<Vec<(String, Instant, Instant)>>,
    }

    impl Timing {
        fn set(&self, delays: &[(&str, u64)]) {
            let mut d = self.delay_ms.lock().unwrap();
            for (k, ms) in delays {
                d.insert(k.to_string(), *ms);
            }
        }

        /// The last run of `key` (`fs.read:a.txt`): its start and end.
        fn of(&self, key: &str) -> (Instant, Instant) {
            let runs = self.runs.lock().unwrap();
            let (_, a, b) = runs
                .iter()
                .rev()
                .find(|(k, _, _)| k == key)
                .unwrap_or_else(|| panic!("no run of {key}"));
            (*a, *b)
        }
    }

    /// A toollet that takes a known time: a built-in's plan and output under
    /// its own name and class, after a sleep set by its input's path or
    /// pattern.
    struct Slowed {
        name: &'static str,
        class: ToolClass,
        inner: Arc<dyn Tool>,
        timing: Arc<Timing>,
    }

    impl Tool for Slowed {
        fn name(&self) -> &'static str {
            self.name
        }
        fn description(&self) -> &'static str {
            self.inner.description()
        }
        fn input_schema(&self) -> Value {
            self.inner.input_schema()
        }
        fn class(&self) -> ToolClass {
            self.class
        }
        fn retry(&self) -> Retry {
            self.inner.retry()
        }
        fn plan(&self, input: &Value, ctx: &ToolCtx) -> Result<Plan, String> {
            self.inner.plan(input, ctx)
        }
        fn run(&self, input: &Value, ctx: &ToolCtx) -> Result<ToolOutput, ToolFailure> {
            let on = input
                .get("path")
                .or_else(|| input.get("pattern"))
                .and_then(Value::as_str)
                .unwrap_or_default();
            let key = format!("{}:{on}", self.name);
            let ms = self.timing.delay_ms.lock().unwrap().get(&key).copied();
            let t0 = Instant::now();
            std::thread::sleep(Duration::from_millis(ms.unwrap_or(0)));
            let out = self.inner.run(input, ctx);
            self.timing
                .runs
                .lock()
                .unwrap()
                .push((key, t0, Instant::now()));
            out
        }
    }

    fn slowed(
        name: &'static str,
        class: ToolClass,
        inner: Arc<dyn Tool>,
        timing: &Arc<Timing>,
    ) -> Arc<dyn Tool> {
        Arc::new(Slowed {
            name,
            class,
            inner,
            timing: timing.clone(),
        })
    }

    fn read(id: &str, path: &str) -> (String, &'static str, Value) {
        (id.into(), "fs_read", json!({ "path": path }))
    }

    fn calls(v: &[(String, &'static str, Value)]) -> Scripted {
        let v: Vec<(&str, &str, Value)> = v
            .iter()
            .map(|(id, n, i)| (id.as_str(), *n, i.clone()))
            .collect();
        Scripted::tools("", &v)
    }

    /// The `tool_use_id`s of the last request's tool results, in order.
    fn results_sent(r: &Rig) -> Vec<String> {
        let last = r.fake.requests().pop().unwrap();
        last.messages
            .last()
            .unwrap()
            .get("content")
            .and_then(Value::as_array)
            .unwrap()
            .iter()
            .filter(|b| b["type"] == "tool_result")
            .map(|b| b["tool_use_id"].as_str().unwrap().to_string())
            .collect()
    }

    /// Each node of a session as `user`, `assistant`, `call <id>`, or
    /// `result <id>`, in WAL order.
    fn labels(core: &Core, sid: &str) -> Vec<String> {
        core.store
            .session_nodes(sid)
            .unwrap()
            .into_iter()
            .map(|(_, n)| match &n.body {
                Body::ToolCall { tool_use_id, .. } => format!("call {tool_use_id}"),
                Body::ToolResult { tool_use_id, .. } => format!("result {tool_use_id}"),
                Body::UserMessage { .. } => "user".into(),
                _ => "assistant".into(),
            })
            .collect()
    }

    /// The loop's span named `name`.
    fn span<'a>(trace: &'a Span, lp: usize, name: &str) -> &'a Span {
        trace.children[lp..]
            .iter()
            .find(|s| s.kind == "loop")
            .and_then(|l| l.children.iter().find(|s| s.name == name))
            .unwrap_or_else(|| panic!("no {name} span in loop {lp}"))
    }

    /// A response of 5 `fs.read`s and 2 `fs.grep`s, each taking a known time
    /// and the first the slowest, finishes in about the slowest call, not
    /// the sum. The calls' spans overlap under one `tools` span. Their results
    /// are written as the calls finish, and the next request carries them in
    /// call order, byte for byte what it carries when they finish in order.
    #[tokio::test]
    async fn five_reads_and_two_greps_take_the_slowest_call_not_the_sum() {
        let timing = Arc::<Timing>::default();
        let mut v: Vec<_> = (1..=5)
            .map(|i| read(&format!("t{i}"), &format!("r{i}.txt")))
            .collect();
        v.push(("t6".into(), "fs_grep", json!({"pattern": "alpha"})));
        v.push(("t7".into(), "fs_grep", json!({"pattern": "beta"})));
        let r = rig_full(
            vec![
                calls(&v),
                Scripted::text("Read and searched."),
                calls(&v),
                Scripted::text("Read and searched."),
            ],
            |_| {},
            vec![
                slowed(
                    "fs.read",
                    ToolClass::Read,
                    Arc::new(theseus_tools::fs::Read),
                    &timing,
                ),
                slowed(
                    "fs.grep",
                    ToolClass::Read,
                    Arc::new(theseus_tools::fs::Grep),
                    &timing,
                ),
            ],
        );
        for i in 1..=5 {
            let text = format!("file {i}: alpha\nbeta {i}\n");
            std::fs::write(r.root.join(format!("r{i}.txt")), text).unwrap();
        }
        let keys = [
            "fs.read:r1.txt",
            "fs.read:r2.txt",
            "fs.read:r3.txt",
            "fs.read:r4.txt",
            "fs.read:r5.txt",
            "fs.grep:alpha",
            "fs.grep:beta",
        ];
        // Out of call order: the first call is the slowest, the fourth the fastest.
        let delays = [900, 300, 700, 100, 500, 800, 200];
        let set = |d: &[u64]| {
            let pairs: Vec<(&str, u64)> = keys.iter().copied().zip(d.iter().copied()).collect();
            timing.set(&pairs);
        };
        set(&delays);
        let ask = "read r1.txt to r5.txt, and grep alpha and beta";
        let res = turn(&r.core, None, ask).await;
        assert_eq!((res.loops, res.tool_calls), (2, 7), "{res:?}");

        // Every call started before any finished.
        let runs: Vec<(Instant, Instant)> = keys.iter().map(|k| timing.of(k)).collect();
        let last_start = runs.iter().map(|(a, _)| *a).max().unwrap();
        let first_end = runs.iter().map(|(_, b)| *b).min().unwrap();
        assert!(last_start < first_end, "the calls did not overlap");
        // About the slowest call (900 ms), not the sum (3,500 ms).
        let trace = res.trace.as_ref().unwrap();
        let tools = span(trace, 0, "tools");
        assert_eq!((tools.kind.as_str(), tools.children.len()), ("tools", 7));
        let took_ms = tools.duration_us() / 1000;
        assert!(
            (900..1750).contains(&took_ms),
            "the calls took {took_ms} ms"
        );
        let latest = tools.children.iter().map(|s| s.start_us).max().unwrap();
        let earliest = tools
            .children
            .iter()
            .filter_map(|s| s.end_us)
            .min()
            .unwrap();
        assert!(latest < earliest, "the spans overlap");

        // Written as they finished: the fastest before the slowest...
        let sid = &res.session_id;
        let written = labels(&r.core, sid);
        let at = |l: &str| written.iter().position(|x| x == l).unwrap();
        assert!(at("result t4") < at("result t1"), "{written:?}");
        // ...and sent in call order.
        let order: Vec<String> = (1..=7).map(|i| format!("t{i}")).collect();
        assert_eq!(results_sent(&r), order);

        // The same calls finishing in call order: the same request, byte for byte.
        set(&[100, 150, 200, 250, 300, 350, 400]);
        let again = turn(&r.core, None, ask).await;
        assert_eq!(again.tool_calls, 7);
        let reqs = r.fake.requests();
        assert_eq!(reqs.len(), 4);
        assert_eq!(reqs[3].messages, reqs[1].messages);
        assert_eq!(reqs[3].digest(), reqs[1].digest());
        let written = labels(&r.core, &again.session_id);
        let results: Vec<&String> = written.iter().filter(|l| l.starts_with("result")).collect();
        let in_order: Vec<String> = order.iter().map(|id| format!("result {id}")).collect();
        assert_eq!(results, in_order.iter().collect::<Vec<_>>());
    }

    /// A call's time is its run's own (theseus-a60): seven reads that take
    /// no time say so, though each result waits in the turn's task behind the
    /// frames of the calls beside it (about 7 ms each on this disk).
    #[tokio::test]
    async fn a_calls_time_is_its_own_run_not_its_wait_for_the_turn() {
        let seven: Vec<_> = (1..=7)
            .map(|i| read(&format!("q{i}"), "hello.txt"))
            .collect();
        let r = rig(vec![calls(&seven), Scripted::text("Read seven times.")]);
        std::fs::write(r.root.join("hello.txt"), "hi\n").unwrap();
        let res = turn(&r.core, None, "read hello.txt seven times").await;
        assert_eq!(res.tool_calls, 7);
        let times: Vec<u64> = r
            .core
            .store
            .session_nodes(&res.session_id)
            .unwrap()
            .into_iter()
            .filter_map(|(_, n)| match n.body {
                Body::ToolResult { duration_ms, .. } => duration_ms,
                _ => None,
            })
            .collect();
        assert_eq!(times.len(), 7);
        assert!(times.iter().all(|ms| *ms < 20), "{times:?}");
        let trace = res.trace.as_ref().unwrap();
        let spans = &span(trace, 0, "tools").children;
        let waited: Vec<u64> = spans
            .iter()
            .map(|s| (s.end_us.unwrap() - s.start_us) / 1000)
            .collect();
        eprintln!("results' own times {times:?} ms; their spans in the turn {waited:?} ms");
    }

    /// A write is a barrier: a read of the same path after it reads what it
    /// wrote. A program is one too: the call before it has finished when it
    /// starts, and the calls after it start when it has ended, together.
    #[tokio::test]
    async fn a_write_and_a_program_are_barriers() {
        let timing = Arc::<Timing>::default();
        let r = rig_full(
            vec![
                calls(&[
                    (
                        "w1".into(),
                        "fs_write",
                        json!({"path": "a.txt", "content": "new\n"}),
                    ),
                    read("w2", "a.txt"),
                ]),
                Scripted::text("Written and read."),
                calls(&[
                    read("p1", "x.txt"),
                    ("p2".into(), "test_run", json!({"path": "run.txt"})),
                    read("p3", "y.txt"),
                    read("p4", "z.txt"),
                ]),
                Scripted::text("Done."),
            ],
            |cfg| {
                cfg.policy.tools.insert("fs.write".into(), Posture::Open);
                cfg.policy.tools.insert("test.run".into(), Posture::Open);
            },
            vec![
                slowed(
                    "fs.read",
                    ToolClass::Read,
                    Arc::new(theseus_tools::fs::Read),
                    &timing,
                ),
                slowed(
                    "fs.write",
                    ToolClass::Write,
                    Arc::new(theseus_tools::fs::WriteFile),
                    &timing,
                ),
                slowed(
                    "test.run",
                    ToolClass::Run,
                    Arc::new(theseus_tools::fs::Read),
                    &timing,
                ),
            ],
        );
        std::fs::write(r.root.join("a.txt"), "old\n").unwrap();
        for f in ["x.txt", "y.txt", "z.txt", "run.txt"] {
            std::fs::write(r.root.join(f), format!("{f}\n")).unwrap();
        }
        timing.set(&[
            ("fs.write:a.txt", 300),
            ("fs.read:x.txt", 200),
            ("test.run:run.txt", 300),
            ("fs.read:y.txt", 150),
            ("fs.read:z.txt", 150),
        ]);
        let res = turn(&r.core, None, "write a.txt, then read it").await;
        assert_eq!(res.tool_calls, 2);
        let rs = results(&r.core, &res.session_id);
        assert!(
            rs[1].1.contains("new"),
            "the read ran after the write: {rs:?}"
        );
        let (_, wrote) = timing.of("fs.write:a.txt");
        let (read_at, _) = timing.of("fs.read:a.txt");
        assert!(read_at >= wrote);

        turn(
            &r.core,
            Some(&res.session_id),
            "read x, run, then read y and z",
        )
        .await;
        let (_, x_end) = timing.of("fs.read:x.txt");
        let (run_start, run_end) = timing.of("test.run:run.txt");
        let (y_start, y_end) = timing.of("fs.read:y.txt");
        let (z_start, z_end) = timing.of("fs.read:z.txt");
        assert!(
            run_start >= x_end,
            "the program waited for the read before it"
        );
        assert!(
            y_start >= run_end && z_start >= run_end,
            "the reads waited for it"
        );
        assert!(
            y_start < z_end && z_start < y_end,
            "the reads after it ran together"
        );
        assert_eq!(results_sent(&r), ["p1", "p2", "p3", "p4"].map(String::from));
    }

    /// An approval in the middle: the calls before it run, together; it asks;
    /// the calls after it are not even gated. After the approval, it runs,
    /// then the rest, together. The transcript is the sequential one with
    /// each group's nodes in the order they happened, and the request is the
    /// sequential one.
    #[tokio::test]
    async fn an_approval_in_the_middle_runs_the_calls_before_it_and_the_rest_after_it() {
        let timing = Arc::<Timing>::default();
        let r = rig_full(
            vec![
                calls(&[
                    read("a1", "a.txt"),
                    read("a2", "b.txt"),
                    (
                        "a3".into(),
                        "fs_write",
                        json!({"path": "c.txt", "content": "c\n"}),
                    ),
                    read("a4", "d.txt"),
                    read("a5", "e.txt"),
                ]),
                Scripted::text("All done."),
            ],
            |_| {},
            vec![slowed(
                "fs.read",
                ToolClass::Read,
                Arc::new(theseus_tools::fs::Read),
                &timing,
            )],
        );
        for f in ["a.txt", "b.txt", "d.txt", "e.txt"] {
            std::fs::write(r.root.join(f), format!("{f}\n")).unwrap();
        }
        timing.set(&[
            ("fs.read:a.txt", 250),
            ("fs.read:b.txt", 50),
            ("fs.read:d.txt", 250),
            ("fs.read:e.txt", 50),
        ]);
        let res = turn(&r.core, None, "read a and b, write c, read d and e").await;
        let corr = res.awaiting_confirm.clone().expect("the write asks");
        assert_eq!(res.tool_calls, 3, "a4 and a5 were not gated");
        let sid = res.session_id.clone();
        assert_eq!(
            labels(&r.core, &sid),
            [
                "user",
                "assistant",
                "call a1",
                "call a2",
                "result a2",
                "result a1",
                "call a3"
            ]
        );
        assert_eq!(r.fake.requests().len(), 1);

        r.core.confirm_action(&corr, true, None, "test").unwrap();
        let exec = res.execution_id.clone().unwrap();
        let cont = r.core.continue_execution(&exec).await.unwrap().unwrap();
        assert_eq!(cont.output, "All done.");
        assert!(r.root.join("c.txt").exists());
        let actual = labels(&r.core, &sid);
        assert_eq!(
            actual[7..],
            [
                "result a3",
                "call a4",
                "call a5",
                "result a5",
                "result a4",
                "assistant"
            ]
        );
        // The sequential transcript, with each group's nodes reordered only
        // among themselves.
        let sequential = [
            "user",
            "assistant",
            "call a1",
            "result a1",
            "call a2",
            "result a2",
            "call a3",
            "result a3",
            "call a4",
            "result a4",
            "call a5",
            "result a5",
            "assistant",
        ];
        let group = |v: &[String], s: std::ops::Range<usize>| {
            let mut g = v[s].to_vec();
            g.sort();
            g
        };
        let seq: Vec<String> = sequential.iter().map(|s| s.to_string()).collect();
        assert_eq!(actual.len(), seq.len());
        for s in [2..6, 8..12] {
            assert_eq!(group(&actual, s.clone()), group(&seq, s));
        }
        for i in [0, 1, 6, 7, 12] {
            assert_eq!(actual[i], seq[i]);
        }
        assert_eq!(
            results_sent(&r),
            ["a1", "a2", "a3", "a4", "a5"].map(String::from)
        );
    }

    /// A read that computes for `ms` milliseconds: how many run at once, and
    /// whether the daemon's pool ever held more permits than it has.
    #[derive(Default)]
    struct Busy {
        now: AtomicUsize,
        max: AtomicUsize,
        runs: AtomicUsize,
        over: AtomicUsize,
        pool: OnceLock<Arc<crate::cpu::CpuPool>>,
    }

    impl Tool for Busy {
        fn name(&self) -> &'static str {
            "test.busy"
        }
        fn description(&self) -> &'static str {
            "Computes for a while."
        }
        fn input_schema(&self) -> Value {
            json!({"type": "object", "properties": {"ms": {"type": "integer"}}})
        }
        fn class(&self) -> ToolClass {
            ToolClass::Read
        }
        fn retry(&self) -> Retry {
            Retry::SafeToRepeat
        }
        fn plan(&self, _: &Value, _: &ToolCtx) -> Result<Plan, String> {
            Ok(Plan {
                summary: "compute".into(),
                ..Default::default()
            })
        }
        fn run(&self, input: &Value, _: &ToolCtx) -> Result<ToolOutput, ToolFailure> {
            let n = self.now.fetch_add(1, Ordering::SeqCst) + 1;
            self.max.fetch_max(n, Ordering::SeqCst);
            let pool = self.pool.get().expect("set once the core is built");
            if pool.busy() > pool.size() || pool.busy() == 0 {
                self.over.fetch_add(1, Ordering::SeqCst);
            }
            let ms = input.get("ms").and_then(Value::as_u64).unwrap_or(50);
            let until = Instant::now() + Duration::from_millis(ms);
            let mut x = 1u64;
            while Instant::now() < until {
                x = x
                    .wrapping_mul(6364136223846793005)
                    .wrapping_add(1442695040888963407);
            }
            self.now.fetch_sub(1, Ordering::SeqCst);
            self.runs.fetch_add(1, Ordering::SeqCst);
            Ok(ToolOutput {
                text: format!("{}", x % 7),
                meta: Value::Null,
            })
        }
    }

    /// 32 CPU-bound calls at once, 8 in each of 4 sessions' turns, never hold
    /// more of the daemon's permits than it has cores: each run holds one, and
    /// no more run at once than there are permits.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn thirty_two_cpu_bound_calls_never_hold_more_permits_than_there_are_cores() {
        let busy = Arc::new(Busy::default());
        let eight: Vec<_> = (1..=8)
            .map(|i| (format!("b{i}"), "test_busy", json!({"ms": 60})))
            .collect();
        let r = rig_full(
            (0..4).map(|_| calls(&eight)).collect(),
            |cfg| {
                cfg.policy.tools.insert("test.busy".into(), Posture::Open);
            },
            vec![busy.clone()],
        );
        let pool = r.core.tools.cpu.clone();
        busy.pool.set(pool.clone()).ok().unwrap();
        let stats = || r.core.store.stats().unwrap();
        let s0 = stats();
        // Each turn in its own task, as the daemon runs them.
        let turns = (0..4).map(|_| {
            let core = r.core.clone();
            tokio::spawn(async move { turn(&core, None, "compute").await })
        });
        let done: Vec<_> = futures_util::future::join_all(turns)
            .await
            .into_iter()
            .map(|t| t.unwrap())
            .collect();
        let s1 = stats();
        assert_eq!(done.iter().map(|t| t.tool_calls).sum::<u32>(), 32);
        assert_eq!(busy.runs.load(Ordering::SeqCst), 32);
        let max = busy.max.load(Ordering::SeqCst);
        assert!(
            max <= pool.size(),
            "{max} ran at once on {} cores",
            pool.size()
        );
        assert!(max >= 2.min(pool.size()), "they ran together: {max}");
        assert_eq!(busy.over.load(Ordering::SeqCst), 0, "a run held no permit");
        assert_eq!(pool.busy(), 0, "every permit came back");
        eprintln!(
            "32 calls in 4 sessions: at most {max} at once on {} cores; {} frames, {} fdatasyncs",
            pool.size(),
            s1.frames_appended - s0.frames_appended,
            s1.syncs - s0.syncs
        );
    }

    /// A turn, returning its failure rather than panicking on it.
    async fn try_turn(
        core: &Arc<Core>,
        rec: SessionRecord,
        input: &str,
    ) -> anyhow::Result<TurnSubmitResult> {
        let (live, _) = core.live_profile();
        let target = core.runner.resolve_target(&live, None, None, None)?;
        let sink = EventSink::new(core.bus.clone(), &rec.session_id, None);
        core.runner
            .run(TurnRequest {
                session: rec,
                input: Some(input.into()),
                target,
                sink,
                author: "test".into(),
                recompile: None,
                attachments: vec![],
                arrived: None,
                config_wait_us: 0,
                reply_to: None,
            })
            .await
    }

    /// A cancel while a batch runs leaves no call dispatched: each running
    /// call is cancelled at once, its late completion is recorded when it
    /// comes, and the turn ends because its next provider call is refused.
    #[tokio::test]
    async fn a_cancel_during_a_batch_leaves_no_call_dispatched() {
        let timing = Arc::<Timing>::default();
        let four: Vec<_> = ["a", "b", "c", "d"]
            .iter()
            .map(|f| read(&format!("c_{f}"), &format!("{f}.txt")))
            .collect();
        let r = rig_full(
            vec![calls(&four)],
            |_| {},
            vec![slowed(
                "fs.read",
                ToolClass::Read,
                Arc::new(theseus_tools::fs::Read),
                &timing,
            )],
        );
        for f in ["a", "b", "c", "d"] {
            std::fs::write(r.root.join(format!("{f}.txt")), "x\n").unwrap();
            timing.set(&[(&format!("fs.read:{f}.txt"), 600)]);
        }
        let rec = SessionRecord::new(SessionKind::Conversation, None);
        r.core.store.put_session(&rec.session_id, &rec).unwrap();
        let sid = rec.session_id.clone();
        let core = r.core.clone();
        let running = tokio::spawn(async move { try_turn(&core, rec, "read four").await });
        let dispatched = || {
            r.core
                .kernel
                .actions()
                .unwrap()
                .into_iter()
                .filter(|a| {
                    a.session_id == sid && a.tool == "fs.read" && a.state == ActionState::Dispatched
                })
                .collect::<Vec<_>>()
        };
        let t0 = Instant::now();
        while dispatched().len() < 4 {
            assert!(t0.elapsed() < Duration::from_secs(5), "never dispatched");
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        let exec = dispatched()[0].execution_id.clone();
        let (_, stopped) = r.core.cancel_execution(&exec, "test").await.unwrap();
        assert_eq!(stopped.len(), 4, "the four running calls");
        let err = running
            .await
            .unwrap()
            .expect_err("the next provider call is refused");
        let te = err.downcast_ref::<crate::turn::TurnError>().unwrap();
        assert_eq!(te.class, "kernel", "{err:#}");
        let actions: Vec<_> = r
            .core
            .kernel
            .actions()
            .unwrap()
            .into_iter()
            .filter(|a| a.execution_id == exec)
            .collect();
        for a in &actions {
            assert!(
                a.state.is_settled(),
                "{} {} left {:?}",
                a.tool,
                a.correlation_id,
                a.state
            );
        }
        let calls: Vec<_> = actions.iter().filter(|a| a.tool == "fs.read").collect();
        assert_eq!(calls.len(), 4);
        for a in calls {
            assert_eq!(a.state, ActionState::Cancelled);
            let why = a.resolution.as_deref().unwrap_or_default();
            assert!(why.starts_with("late completion after cancel"), "{why}");
        }
        let e = r.core.kernel.execution(&exec).unwrap().unwrap();
        assert_eq!(e.state.as_str(), "cancelled");
        assert!(e.outstanding.is_empty(), "{:?}", e.outstanding);
    }
}

/// A connection accepted as `surface`, whose peer is `pid`, as a listener
/// reads it (theseus-6qy).
fn surface_of(label: &str, s: crate::approval::Surface, pid: u32) -> crate::approval::Client {
    surface(label, s).with_peer(crate::peer::Peer::process(pid))
}

/// An answer over a real protocol connection accepted as `client`.
async fn answer(
    core: &Arc<Core>,
    client: crate::approval::Client,
    corr: &str,
    approve: bool,
) -> Result<Value, theseus_protocol::RpcError> {
    let params = theseus_protocol::ActionConfirmParams {
        correlation_id: corr.into(),
        approve,
        note: None,
        watch: false,
        author: None,
        discord: None,
        trust: false,
    };
    rpc_as(
        core,
        client,
        theseus_protocol::method::ACTION_CONFIRM,
        serde_json::to_value(params).unwrap(),
    )
    .await
}

/// A Theseus job's process cannot answer an approval (theseus-6qy). With no
/// `[approval]` section, so that the CLI and the web UI answer as before, an
/// approval and a decline from a process under a live job wrapper, through
/// the CLI and through the web UI, are each refused with the job, the pid,
/// and the program. Each is ledgered as `approval.refused` with the asker,
/// narrated as a security event, and announced to every connection, and
/// nothing moves. Then the operator's own answer counts, recorded with its
/// process, and the write runs.
#[tokio::test]
async fn a_jobs_process_cannot_answer_an_approval() {
    use crate::approval::Surface::{Cli, Web};
    let job = crate::peer::Standin::start("act_standin");
    let r = rig(write_script());
    let (sid, mut rx) = watched_session(&r);
    let res = turn(&r.core, Some(&sid), "write out.txt").await;
    let corr = res.awaiting_confirm.clone().expect("the write waits");
    let exec = res.execution_id.clone().unwrap();
    let reason = format!(
        "from a Theseus job's process (job act_standin, pid {}, sleep)",
        job.child
    );
    for (client, approve, via) in [
        (surface_of("sock#7", Cli, job.child), true, "cli"),
        (surface_of("sock#8", Cli, job.child), false, "cli"),
        (surface_of("web#2", Web, job.child), true, "web"),
    ] {
        let e = answer(&r.core, client, &corr, approve)
            .await
            .expect_err("refused");
        assert_eq!(e.code, theseus_protocol::error_code::REFUSED, "{e:?}");
        assert!(e.message.contains(&reason), "{}", e.message);
        assert_eq!(
            (e.data["why"].as_str(), e.data["via"].as_str()),
            (Some(reason.as_str()), Some(via))
        );
    }
    // Nothing moved.
    let a = r.core.kernel.action(&corr).unwrap().unwrap();
    assert_eq!(a.state, theseus_kernel::ActionState::Planned);
    let e = r.core.kernel.execution(&exec).unwrap().unwrap();
    assert_eq!(e.state.as_str(), "waiting");
    assert_eq!(r.core.pending_confirms(&sid).unwrap().len(), 1);
    assert!(!r.root.join("out.txt").exists());
    assert!(ledgered(&r, "action.confirm_answered").is_empty());
    // Loud: the ledger, every connection, the narrative.
    let rows = ledgered(&r, "approval.refused");
    assert_eq!(rows.len(), 3);
    for row in &rows {
        assert_eq!(row["why"], reason.as_str());
        assert_eq!(row["from_job"], true);
        assert_eq!(
            (row["asker"]["job"].as_str(), row["asker"]["pid"].as_u64()),
            (Some("act_standin"), Some(job.child as u64))
        );
        assert_eq!(row["asker"]["wrapper_pid"], job.wrapper);
    }
    assert_eq!(rows[1]["approve"], false);
    let told = sent(&mut rx, theseus_protocol::notify::APPROVAL_REFUSED);
    assert_eq!(told.len(), 3);
    assert_eq!(
        (
            told[0]["act"].as_str(),
            told[0]["session_id"].as_str(),
            told[0]["tool"].as_str()
        ),
        (Some("action.confirm"), Some(sid.as_str()), Some("fs.write"))
    );
    assert!(sent(&mut rx, theseus_protocol::notify::CONFIRM_RESOLVED).is_empty());
    let lines = narrated(&r, &sid);
    assert!(
        said(
            &lines,
            "approval",
            &format!("Refused an answer to fs.write {reason} through cli: a job's process cannot answer an approval")
        ),
        "{}",
        dump(&lines)
    );

    // The operator, outside every job.
    if crate::peer::tests_support::inside_a_job() {
        return;
    }
    let ok = answer(
        &r.core,
        surface_of("sock#1", Cli, std::process::id()),
        &corr,
        true,
    )
    .await
    .unwrap();
    assert_eq!(ok["approved"], true);
    let answered = ledgered(&r, "action.confirm_answered");
    assert_eq!(answered[0]["asker"]["pid"], std::process::id());
    assert!(answered[0]["asker"]["job"].is_null());
    let cont = r.core.continue_execution(&exec).await.unwrap().unwrap();
    assert_eq!(cont.output, "Written.");
    assert_eq!(
        std::fs::read_to_string(r.root.join("out.txt")).unwrap(),
        "approved\n"
    );
}

/// The spend reset from a Theseus job's process is refused, and the session
/// keeps waiting on its budget; the operator's reset counts (theseus-6qy).
#[tokio::test]
async fn a_jobs_process_cannot_reset_the_spend() {
    use crate::approval::Surface::Cli;
    use theseus_kernel::ExecState;
    let job = crate::peer::Standin::start("act_spender");
    let script = vec![
        Scripted::tools(
            &"word ".repeat(30_000),
            &[("t1", "text_diff", json!({"a": "x\n", "b": "y\n"}))],
        ),
        Scripted::text("The diff is one line."),
    ];
    let r = rig_with(script, |c| c.kernel.spend_limit_usd = 1.40);
    let res = turn(&r.core, None, "diff these").await;
    assert_eq!(res.stop_reason, "budget");
    let q = res.awaiting_confirm.clone().unwrap();
    let exec = res.execution_id.clone().unwrap();
    let spent = r
        .core
        .kernel
        .execution(&exec)
        .unwrap()
        .unwrap()
        .budget
        .spent_micros;
    let e = answer(&r.core, surface_of("sock#4", Cli, job.child), &q, true)
        .await
        .unwrap_err();
    assert_eq!(e.code, theseus_protocol::error_code::REFUSED);
    assert!(
        e.message
            .contains("from a Theseus job's process (job act_spender"),
        "{}",
        e.message
    );
    let x = r.core.kernel.execution(&exec).unwrap().unwrap();
    assert_eq!(
        (x.state, x.budget.spent_micros, x.budget.resets),
        (ExecState::Waiting, spent, 0),
        "nothing was reset"
    );
    assert!(ledgered(&r, "budget.reset").is_empty());
    let refused = ledgered(&r, "approval.refused");
    assert_eq!(
        (
            refused[0]["tool"].as_str(),
            refused[0]["asker"]["job"].as_str()
        ),
        (Some("budget.reset"), Some("act_spender"))
    );
    if crate::peer::tests_support::inside_a_job() {
        return;
    }
    let ok = answer(
        &r.core,
        surface_of("sock#1", Cli, std::process::id()),
        &q,
        true,
    )
    .await
    .unwrap();
    assert_eq!(ok["resumes"], true);
    let x = r.core.kernel.execution(&exec).unwrap().unwrap();
    assert_eq!((x.budget.spent_micros, x.budget.resets), (0, 1));
    assert_eq!(
        ledgered(&r, "action.confirm_answered")[0]["asker"]["pid"],
        std::process::id()
    );
}

/// The undo of a tightening loosens, so it is refused from a Theseus job's
/// process, and the tool keeps asking; a "should have asked" press only
/// makes things stricter, so it is accepted from one (theseus-6qy).
#[tokio::test]
async fn a_jobs_process_can_tighten_but_not_undo_a_tightening() {
    use crate::approval::Surface::Cli;
    let job = crate::peer::Standin::start("act_policy");
    let r = rig(vec![]);
    let press = |tool: &str| json!({"tool": tool});
    let pressed = rpc_as(
        &r.core,
        surface_of("sock#5", Cli, job.child),
        theseus_protocol::method::POLICY_TIGHTEN,
        press("fs.patch"),
    )
    .await
    .expect("a press from a job's process counts");
    assert_eq!(pressed["tool"], "fs.patch");
    assert!(r.core.tools.tightened.get("fs.patch").is_some());
    let e = rpc_as(
        &r.core,
        surface_of("sock#6", Cli, job.child),
        theseus_protocol::method::POLICY_UNTIGHTEN,
        press("fs.patch"),
    )
    .await
    .unwrap_err();
    assert_eq!(e.code, theseus_protocol::error_code::REFUSED);
    assert!(
        e.message
            .contains("from a Theseus job's process (job act_policy"),
        "{}",
        e.message
    );
    assert!(
        e.message.contains("fs.patch keeps asking first"),
        "{}",
        e.message
    );
    assert!(
        r.core.tools.tightened.get("fs.patch").is_some(),
        "still tightened"
    );
    let refused = ledgered(&r, "approval.refused");
    assert_eq!(
        (refused[0]["act"].as_str(), refused[0]["from_job"].as_bool()),
        (Some("policy.untighten"), Some(true))
    );
    if crate::peer::tests_support::inside_a_job() {
        return;
    }
    rpc_as(
        &r.core,
        surface_of("sock#1", Cli, std::process::id()),
        theseus_protocol::method::POLICY_UNTIGHTEN,
        press("fs.patch"),
    )
    .await
    .expect("the operator's undo counts");
    assert!(r.core.tools.tightened.get("fs.patch").is_none());
    assert_eq!(
        ledgered(&r, "policy.untightened")[0]["asker"]["pid"],
        std::process::id()
    );
}

// ---------------------------------------------------------------- the limit follows the config (theseus-3pj)

/// A lower `spend_limit_usd`, and a restart: the open session takes it in
/// startup (a config that may act at once), with nothing else changed, and
/// its next turn's first call no longer fits, so the turn asks, as usual.
#[tokio::test]
async fn a_lowered_limit_makes_the_next_turn_over_it_ask() {
    use theseus_kernel::ExecState;
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("w");
    std::fs::create_dir_all(&root).unwrap();
    let mut cfg = config(&root.canonicalize().unwrap(), dir.path());
    cfg.kernel.spend_limit_usd = 5.0;
    let (sid, exec, spent, lifetime) = {
        let store = Store::open(&dir.path().join("store")).unwrap();
        let fake = Arc::new(FakeProvider::scripted(vec![
            Scripted::tools(
                &"word ".repeat(30_000),
                &[("t1", "text_diff", json!({"a": "x\n", "b": "y\n"}))],
            ),
            Scripted::text("One line differs."),
        ]));
        let core = Core::build(crate::rpc::Parts::for_tests(cfg.clone(), fake, store)).unwrap();
        let res = turn(&core, None, "diff these").await;
        assert_eq!(res.stop_reason, "no_tool_calls", "under $5 it fits");
        let exec = res.execution_id.clone().unwrap();
        let e = core.kernel.execution(&exec).unwrap().unwrap();
        let s: SessionRecord = core.store.get_session(&res.session_id).unwrap().unwrap();
        (res.session_id, exec, e.budget.spent_micros, s.cost_usd)
    };
    assert!(spent > 250_000, "30,000 words out: {spent}");

    cfg.kernel.spend_limit_usd = 1.50;
    let store = Store::open(&dir.path().join("store")).unwrap();
    let fake = Arc::new(FakeProvider::scripted(vec![Scripted::text("never asked")]));
    let core = Core::build(crate::rpc::Parts::for_tests(cfg, fake.clone(), store)).unwrap();
    let e = core.kernel.execution(&exec).unwrap().unwrap();
    assert_eq!(
        (e.state, e.budget.limit_micros, e.budget.spent_micros),
        (ExecState::Waiting, 1_500_000, spent),
        "the new limit, and nothing else"
    );
    let r = Rig {
        core: core.clone(),
        fake: fake.clone(),
        root: root.clone(),
        _dir: dir,
    };
    let changed = ledgered(&r, "budget.limit_changed");
    assert_eq!(
        (
            &changed[0]["from_usd"],
            &changed[0]["to_usd"],
            &changed[0]["proceeds"]
        ),
        (&json!(5.0), &json!(1.5), &json!(false))
    );

    // Sonnet 5.5's call reserves its 128,000-token cap ($1.28) and its input:
    // more than the $1.50 limit leaves.
    let res = turn(&core, Some(&sid), "and now?").await;
    assert_eq!(res.stop_reason, "budget");
    assert!(fake.requests().is_empty(), "nothing ran over the new limit");
    let q = res.awaiting_confirm.unwrap();
    let pending = core.pending_confirms(&sid).unwrap();
    assert_eq!(pending[0].correlation_id, q);
    assert_eq!(
        pending[0].reason,
        format!(
            "This session has spent {} of its $1.50 limit. Reset its spend to $0 and continue?",
            crate::narrative::dollars(spent)
        )
    );
    let s: SessionRecord = core.store.get_session(&sid).unwrap().unwrap();
    assert_eq!(s.cost_usd, lifetime, "the lifetime cost is untouched");
}

/// A rig whose secrets are `pairs`, resolved, on the board the scrubber and
/// the broker read (theseus-dcy).
fn rig_secrets(
    script: Vec<Scripted>,
    tweak: impl FnOnce(&mut Config),
    toollets: Vec<Arc<dyn theseus_tools::Tool>>,
    pairs: &[(&str, &str)],
) -> Rig {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("work");
    std::fs::create_dir_all(&root).unwrap();
    let root = root.canonicalize().unwrap();
    let mut cfg = config(&root, dir.path());
    tweak(&mut cfg);
    let store = Store::open(&dir.path().join("store")).unwrap();
    let fake = Arc::new(FakeProvider::scripted(script));
    let board = crate::secrets::SecretBoard::new(
        pairs.iter().map(|(n, _)| n.to_string()),
        std::time::Instant::now(),
    );
    board.publish(
        pairs
            .iter()
            .map(|(n, v)| {
                (
                    n.to_string(),
                    Ok(crate::secrets::Secret::new(v.to_string())),
                )
            })
            .collect(),
        "test",
    );
    let core = Core::build(crate::rpc::Parts {
        toollets,
        secrets: board.clone(),
        scrubber: Arc::new(crate::scrub::Scrubber::from_board(board)),
        ..crate::rpc::Parts::for_tests(cfg, fake.clone(), store)
    })
    .unwrap();
    Rig {
        core,
        fake,
        root,
        _dir: dir,
    }
}

/// A test toollet that asks the broker for its key, and for a secret it was
/// not granted, and says what it got: the key's length, never its value.
struct Keyed;

impl theseus_tools::Tool for Keyed {
    fn name(&self) -> &'static str {
        "test.keyed"
    }
    fn description(&self) -> &'static str {
        "Reports the length of its key."
    }
    fn input_schema(&self) -> Value {
        json!({"type": "object"})
    }
    fn class(&self) -> theseus_tools::ToolClass {
        theseus_tools::ToolClass::Read
    }
    fn retry(&self) -> theseus_tools::Retry {
        theseus_tools::Retry::SafeToRepeat
    }
    fn plan(&self, _: &Value, _: &theseus_tools::ToolCtx) -> Result<theseus_tools::Plan, String> {
        Ok(theseus_tools::Plan {
            summary: "report the key".into(),
            ..Default::default()
        })
    }
    fn run(
        &self,
        _: &Value,
        ctx: &theseus_tools::ToolCtx,
    ) -> Result<theseus_tools::ToolOutput, theseus_tools::ToolFailure> {
        let key = ctx
            .secret("search_key")
            .map_err(theseus_tools::ToolFailure::new)?;
        let other = ctx.secret("github_token").err().unwrap_or_default();
        Ok(theseus_tools::ToolOutput {
            text: format!("key length {}; github_token: {other}", key.len()),
            meta: Value::Null,
        })
    }
}

/// A native toollet gets its secret through the broker (theseus-dcy), as
/// DD5's `web.search` will get its key: only the secret the wiring granted
/// it, at no looser a posture than the secret's (notify, over an open
/// config), with a `secret.granted` row and a use in health. The value is in
/// no node, ledger row, or WAL record.
#[tokio::test]
async fn a_toollet_gets_the_secret_granted_to_it_through_the_broker() {
    let r = rig_secrets(
        vec![
            Scripted::tools("", &[("t1", "test_keyed", json!({}))]),
            Scripted::text("It has its key."),
        ],
        |cfg| cfg.policy.enforcement = Posture::Open,
        vec![Arc::new(Keyed)],
        &[
            ("search_key", "sk-test-5150-value"),
            ("github_token", "gt-test-5150-value"),
        ],
    );
    r.core.tools.broker.grant_tool("test.keyed", "search_key");
    let (res, notices) = watched_turn(&r, "use your key").await;
    assert_eq!(res.output, "It has its key.");
    let rs = results(&r.core, &res.session_id);
    assert_eq!(rs[0].0, ResultStatus::Ok, "{rs:?}");
    assert_eq!(
        rs[0].1,
        format!(
            "key length {}; github_token: test.keyed was not granted github_token",
            "sk-test-5150-value".len()
        )
    );
    assert_eq!(
        notices.len(),
        1,
        "the secret's posture made it a notice: {notices:?}"
    );
    assert_eq!(
        notices[0]["setting"],
        "the broker's posture for search_key, notify by default"
    );
    assert_eq!(notices[0]["granted"], "test.keyed got search_key");
    let rows = ledgered(&r, "secret.granted");
    assert_eq!(rows.len(), 1, "{rows:?}");
    assert_eq!(
        (rows[0]["tool"].as_str(), rows[0]["secret"].as_str()),
        (Some("test.keyed"), Some("search_key"))
    );
    // Health lists the wiring's own grant too (DD5: web.search's key), unused.
    let grants = r.core.health().broker;
    let listed: Vec<_> = grants
        .iter()
        .map(|g| (g.to.as_str(), g.secret.as_str(), g.uses))
        .collect();
    assert_eq!(
        listed,
        [
            ("test.keyed", "search_key", 1),
            ("web.search", "brave_api_key", 0)
        ],
        "{grants:?}"
    );
    // Nowhere on disk: the WAL holds every node and ledger row.
    let store = r._dir.path().join("store");
    let mut stack = vec![store];
    while let Some(d) = stack.pop() {
        for e in std::fs::read_dir(&d).unwrap().flatten() {
            let p = e.path();
            if p.is_dir() {
                stack.push(p);
            } else {
                let b = std::fs::read(&p).unwrap();
                assert!(
                    !b.windows(18).any(|w| w == b"sk-test-5150-value"),
                    "{}",
                    p.display()
                );
            }
        }
    }
}

// ---------------------------------------------------------------- the web tools (DD5)

mod web {
    use super::*;
    use std::time::Instant;

    use crate::config::WebToolsConfig;
    use crate::secrets::{Secret, SecretBoard};
    use crate::web::tests::{serve, web};

    /// A rig whose web tools reach a test's server, standing in for the
    /// built-ins, with `brave_api_key` resolved on the board.
    fn web_rig(script: Vec<Scripted>, port: u16, enforcement: Posture) -> Rig {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("work");
        std::fs::create_dir_all(&root).unwrap();
        let root = root.canonicalize().unwrap();
        let mut cfg = config(&root, dir.path());
        cfg.policy.enforcement = enforcement;
        let store = Store::open(&dir.path().join("store")).unwrap();
        let fake = Arc::new(FakeProvider::scripted(script));
        let board = SecretBoard::new(["brave_api_key".to_string()], Instant::now());
        board.publish(
            [(
                "brave_api_key".to_string(),
                Ok(Secret::new("tv-good".into())),
            )]
            .into(),
            "test",
        );
        let core = Core::build(crate::rpc::Parts {
            toollets: web(port, WebToolsConfig::default(), true).tools(),
            secrets: board,
            ..crate::rpc::Parts::for_tests(cfg, fake.clone(), store)
        })
        .unwrap();
        Rig {
            core,
            fake,
            root,
            _dir: dir,
        }
    }

    /// Each result node's tool, status, and external URL, in WAL order.
    fn externals(core: &Core, sid: &str) -> Vec<(String, ResultStatus, Option<String>)> {
        core.store
            .session_nodes(sid)
            .unwrap()
            .into_iter()
            .filter_map(|(_, n)| match &n.body {
                Body::ToolResult {
                    tool,
                    status,
                    external,
                    ..
                } => Some((
                    tool.clone(),
                    *status,
                    external.as_ref().map(|e| e.url.clone()),
                )),
                _ => None,
            })
            .collect()
    }

    /// Two fetches in one response run together (F3's barrier rule: both
    /// are `Read`): each request reaches the server before either is
    /// answered, and the calls take about one wait, not two. Each result
    /// is marked external, from its URL.
    #[tokio::test]
    async fn two_fetches_in_one_response_run_together_and_are_marked_external() {
        let s = serve().await;
        let a = format!("http://site.test:{}/wait", s.port);
        let b = format!("http://other.test:{}/wait", s.port);
        let r = web_rig(
            vec![
                Scripted::tools(
                    "",
                    &[
                        ("f1", "http_fetch", json!({ "url": a })),
                        ("f2", "http_fetch", json!({ "url": b })),
                    ],
                ),
                Scripted::text("Both fetched."),
            ],
            s.port,
            Posture::Notify,
        );
        let res = turn(&r.core, None, "fetch both").await;
        assert_eq!((res.loops, res.tool_calls), (2, 2), "{res:?}");
        let hits = s.hits.lock().unwrap().clone();
        assert_eq!(hits.len(), 2);
        let last_came = hits.iter().map(|h| h.came).max().unwrap();
        let first_answered = hits.iter().filter_map(|h| h.answered).min().unwrap();
        assert!(
            last_came < first_answered,
            "the fetches ran one after the other"
        );
        let trace = res.trace.as_ref().unwrap();
        let tools = trace
            .children
            .iter()
            .find(|l| l.kind == "loop")
            .and_then(|l| l.children.iter().find(|s| s.name == "tools"))
            .expect("a tools span");
        let took_ms = tools.duration_us() / 1000;
        assert!(
            (400..780).contains(&took_ms),
            "the fetches took {took_ms} ms"
        );
        let mut got = externals(&r.core, &res.session_id);
        got.sort_by(|x, y| x.2.cmp(&y.2));
        let mut want = vec![
            ("http.fetch".to_string(), ResultStatus::Ok, Some(a)),
            ("http.fetch".to_string(), ResultStatus::Ok, Some(b)),
        ];
        want.sort_by(|x, y| x.2.cmp(&y.2));
        assert_eq!(got, want);
        // Each ran under notify, and its notice names its URL.
        let notices = ledgered(&r, "tool.notified");
        assert_eq!(notices.len(), 2);
        assert!(notices
            .iter()
            .all(|n| n["summary"].as_str().unwrap().starts_with("fetch http://")));
    }

    /// A fetch of a loopback address waits for approval at every posture,
    /// `open` included, and says why. Declined, it never connects.
    #[tokio::test]
    async fn a_fetch_of_a_loopback_address_waits_and_a_decline_never_connects() {
        let s = serve().await;
        let url = format!("http://127.0.0.1:{}/plain.txt", s.port);
        let r = web_rig(
            vec![
                Scripted::tools("", &[("f1", "http_fetch", json!({ "url": url }))]),
                Scripted::text("Not fetched: you declined it."),
            ],
            s.port,
            Posture::Open,
        );
        let res = turn(&r.core, None, "fetch it").await;
        let corr = res.awaiting_confirm.clone().expect("it waits for approval");
        let asked = ledgered(&r, "tool.confirm_requested");
        assert_eq!(asked.len(), 1);
        assert_eq!(
            asked[0]["reason"],
            json!(format!(
                "fetch {url}: http.fetch — approve (127.0.0.1 is a loopback address, and a \
                 private address waits for approval)"
            ))
        );
        r.core
            .confirm_action(&corr, false, Some("not that one"), "test")
            .unwrap();
        r.core
            .continue_execution(res.execution_id.as_deref().unwrap())
            .await
            .unwrap();
        assert!(s.paths().is_empty(), "it connected: {:?}", s.paths());
        let got = externals(&r.core, &res.session_id);
        assert_eq!(
            got,
            vec![("http.fetch".to_string(), ResultStatus::Declined, None)]
        );
    }

    /// web.search runs at no looser a posture than its key's: `open` by the
    /// config, it is notified, since `brave_api_key` is notify by default.
    /// The notice says what it got, the ledger has `secret.granted`, health
    /// counts the grant's use, and the key is in no node.
    #[tokio::test]
    async fn a_search_is_held_to_its_keys_posture_and_the_grant_is_counted() {
        let s = serve().await;
        let r = web_rig(
            vec![
                Scripted::tools(
                    "",
                    &[(
                        "s1",
                        "web_search",
                        json!({"query": "rust ignore WalkParallel"}),
                    )],
                ),
                Scripted::text("The first is docs.rs."),
            ],
            s.port,
            Posture::Open,
        );
        let res = turn(&r.core, None, "search").await;
        assert_eq!(res.tool_calls, 1, "{res:?}");
        let got = externals(&r.core, &res.session_id);
        assert_eq!(
            (got[0].0.as_str(), got[0].1),
            ("web.search", ResultStatus::Ok)
        );
        assert!(got[0]
            .2
            .as_deref()
            .unwrap()
            .contains("/search?q=rust+ignore+WalkParallel"));
        let notices = ledgered(&r, "tool.notified");
        assert_eq!(notices.len(), 1);
        assert_eq!(notices[0]["granted"], json!("web.search got brave_api_key"));
        assert_eq!(
            notices[0]["setting"],
            json!("the broker's posture for brave_api_key, notify by default")
        );
        let granted = ledgered(&r, "secret.granted");
        assert_eq!(
            (
                granted.len(),
                granted[0]["tool"].clone(),
                granted[0]["secret"].clone()
            ),
            (1, json!("web.search"), json!("brave_api_key"))
        );
        let grant = r
            .core
            .health()
            .broker
            .into_iter()
            .find(|g| g.to == "web.search")
            .expect("health lists the grant");
        assert_eq!((grant.secret.as_str(), grant.uses), ("brave_api_key", 1));
        for (_, n) in r.core.store.session_nodes(&res.session_id).unwrap() {
            assert!(!serde_json::to_string(&n).unwrap().contains("tv-good"));
        }
        // The session's hold names the query, not the request; the request
        // stays on the node (above) and in the hold, for the record; health
        // says when in local time (theseus-qiy).
        let rec: SessionRecord = r.core.store.get_session(&res.session_id).unwrap().unwrap();
        let held = rec.external.expect("the search holds its session");
        assert_eq!(held.query.as_deref(), Some("rust ignore WalkParallel"));
        assert!(held.url.contains("/search?q=rust+ignore+WalkParallel"));
        assert!(crate::external::source(&held)
            .starts_with("web.search \"rust ignore WalkParallel\", at "));
        let listed = r.core.health().external_text;
        assert_eq!(
            listed[0].held.what(),
            "web.search \"rust ignore WalkParallel\""
        );
        assert_eq!(
            listed[0].since_local,
            crate::wake::local(held.since_ms).hms()
        );
        let read = ledgered(&r, "session.external_read");
        assert_eq!(read[0]["query"], "rust ignore WalkParallel");
    }
}

// ---------------------------------------------------------------- the outbox (theseus-q4v)

/// A session bound to a place, as the Discord binding binds one.
fn bound(core: &Core, place: &str) -> String {
    let rec = SessionRecord::new(SessionKind::Conversation, None);
    core.store.put_session(&rec.session_id, &rec).unwrap();
    core.outbox.bind_place(place, &rec.session_id).unwrap();
    rec.session_id
}

fn posts(core: &Core, target: &str) -> Vec<theseus_kernel::Action> {
    core.outbox.open_for(target)
}

/// A turn in a bound session writes its reply to the outbox, whether or not
/// a binding is there, in the frame that ends the turn: a plain turn still
/// writes 8 frames. The post names the loop's node, and its footer's facts.
#[tokio::test]
async fn a_bound_sessions_reply_rides_in_the_frame_that_ends_its_turn() {
    let r = rig(vec![Scripted::text("first"), Scripted::text("hello")]);
    let sid = bound(&r.core, "dm:42");
    turn(&r.core, Some(&sid), "warm up").await;
    let before = r.core.store.stats().unwrap().frames_appended;
    let from = r.core.store.last_position();
    let res = turn(&r.core, Some(&sid), "hi").await;
    let frames = r.core.store.stats().unwrap().frames_appended - before;
    assert!(
        frames <= 8,
        "a plain turn in a bound session wrote {frames} frames"
    );
    let rows = ledger_after(&r.core, from);
    let planned = rows.iter().rposition(|k| k == "action.planned").unwrap();
    let ended = rows.iter().position(|k| k == "turn.ended").unwrap();
    assert!(
        ended < planned && rows.last().unwrap() == "execution.waiting",
        "{rows:?}"
    );
    let p = posts(&r.core, "discord:dm:42");
    assert_eq!(p.len(), 2, "two turns, two replies");
    let body = crate::outbox::body_of(&p[1]);
    assert_eq!(body["kind"], "reply");
    assert_eq!(body["turn_id"], res.turn_id.as_str());
    assert_eq!(r.core.outbox.reply_texts(body), [(0, "hello".to_string())]);
    assert_eq!(body["result"]["model"], res.model.as_str());
    assert_eq!(
        body["result"]["output"], "",
        "the text is the node's, not a copy"
    );
    assert_eq!(
        p[1].retry_class,
        theseus_kernel::RetryClass::IdempotentWithKey {
            key: "discord.nonce".into()
        }
    );
    // A session that posts nowhere writes no post.
    let other = turn(&r.core, None, "unbound").await;
    assert!(r.core.outbox.target(&other.session_id).is_none());
    assert_eq!(r.core.outbox.status("discord").pending, 2);
}

/// A question's card is written in the frame that plans the question, and
/// an answer writes the card's settle, with who answered.
#[tokio::test]
async fn a_card_is_written_with_its_question_and_settled_by_its_answer() {
    let r = rig(vec![Scripted::tools(
        "",
        &[("t1", "fs_write", json!({"path": "a.txt", "content": "x"}))],
    )]);
    let sid = bound(&r.core, "channel:7");
    let res = turn(&r.core, Some(&sid), "write a").await;
    let q = res.awaiting_confirm.expect("the write waits");
    let p = posts(&r.core, "discord:channel:7");
    let card = p
        .iter()
        .find(|a| crate::outbox::kind_of(a) == "card")
        .expect("a card");
    assert_eq!(crate::outbox::body_of(card)["question"], q.as_str());
    let node = crate::outbox::body_of(card)["node"]
        .as_str()
        .unwrap()
        .to_string();
    let question = r.core.kernel.action(&q).unwrap().unwrap();
    let req = r
        .core
        .question_request(&question, Some(&node))
        .unwrap()
        .expect("the card's question reads back");
    assert_eq!(
        (req.tool.as_str(), req.input["path"].as_str()),
        ("fs.write", Some("a.txt"))
    );
    assert!(r.core.outbox.has_card(&q));
    r.core
        .confirm_action(&q, true, None, "discord:eddie")
        .unwrap();
    let settle = posts(&r.core, "discord:channel:7")
        .into_iter()
        .find(|a| crate::outbox::kind_of(a) == "settle")
        .expect("a settle");
    let body = crate::outbox::body_of(&settle);
    assert_eq!(
        (body["question"].as_str(), body["card"].as_str()),
        (Some(q.as_str()), Some(card.correlation_id.as_str()))
    );
    assert_eq!(
        body["closed"],
        json!({"how": "approved", "by": "discord:eddie"})
    );
    assert_eq!(settle.retry_class, theseus_kernel::RetryClass::SafeToRepeat);
    // Written once: nothing reconciles a second.
    assert_eq!(r.core.outbox.reconcile_cards().unwrap(), 0);
}

/// S1's stale card (theseus-3pj): a raise that withdraws a budget question
/// writes its card's settle, which says so.
#[tokio::test]
async fn a_raise_that_withdraws_a_budget_question_settles_its_card() {
    let r = rig(vec![]);
    let sid = bound(&r.core, "dm:42");
    let card = r
        .core
        .outbox
        .post(
            &sid,
            "",
            "discord:dm:42",
            json!({"kind": "card", "question": "act_q"}),
        )
        .unwrap();
    r.core
        .said_limits_followed(&[theseus_kernel::LimitFollowed {
            execution_id: "exe_1".into(),
            session_id: sid,
            from_micros: 1_400_000,
            to_micros: 3_000_000,
            withdrew: Some("act_q".into()),
            proceeds: true,
        }]);
    let settle = posts(&r.core, "discord:dm:42")
        .into_iter()
        .find(|a| crate::outbox::kind_of(a) == "settle")
        .expect("a settle");
    let body = crate::outbox::body_of(&settle);
    assert_eq!(body["card"], card.correlation_id.as_str());
    assert_eq!(body["closed"]["how"], "withdrawn");
    assert_eq!(
        body["closed"]["note"],
        "the spend limit was raised from $1.40 to $3"
    );
}

/// A question that closed with no event that said so (here, declined in the
/// kernel alone) is found by the level-triggered pass, which the binding runs
/// on connecting and the heartbeat on every beat: one settle, once.
#[tokio::test]
async fn a_card_whose_question_closed_silently_is_settled_by_the_reconcile() {
    let r = rig(vec![Scripted::tools(
        "",
        &[("t1", "fs_write", json!({"path": "a.txt", "content": "x"}))],
    )]);
    let sid = bound(&r.core, "dm:42");
    let q = turn(&r.core, Some(&sid), "write a")
        .await
        .awaiting_confirm
        .unwrap();
    assert_eq!(
        r.core.outbox.reconcile_cards().unwrap(),
        0,
        "it still waits"
    );
    r.core
        .kernel
        .decline_action(&q, "operator", "the web UI closed it")
        .unwrap();
    assert_eq!(r.core.outbox.reconcile_cards().unwrap(), 1);
    assert_eq!(r.core.outbox.reconcile_cards().unwrap(), 0, "once");
    let settle = posts(&r.core, "discord:dm:42")
        .into_iter()
        .find(|a| crate::outbox::kind_of(a) == "settle")
        .unwrap();
    assert_eq!(
        crate::outbox::body_of(&settle)["closed"],
        json!({"how": "declined", "by": "operator", "note": "the web UI closed it"})
    );
}

/// A failed turn's notice goes where its reply would have gone. A transient
/// failure posts at once, and says it is retried with backoff; a lasting one
/// posts when it parks (theseus-ljr, `tests_failures`).
#[tokio::test]
async fn a_failed_turn_posts_its_failure_to_its_place() {
    let r = rig(vec![Scripted::Fail(
        crate::provider::ProviderError::Overloaded {
            message: "no such thing".into(),
        },
    )]);
    let sid = bound(&r.core, "dm:42");
    let rec = r
        .core
        .store
        .get_session::<SessionRecord>(&sid)
        .unwrap()
        .unwrap();
    let (live, _) = r.core.live_profile();
    let target = r
        .core
        .runner
        .resolve_target(&live, None, None, None)
        .unwrap();
    let sink = EventSink::new(r.core.bus.clone(), &sid, None);
    let out = r
        .core
        .runner
        .run(TurnRequest {
            session: rec,
            input: Some("hi".into()),
            target,
            sink,
            author: "test".into(),
            recompile: None,
            attachments: vec![],
            arrived: None,
            config_wait_us: 0,
            reply_to: None,
        })
        .await;
    assert!(out.is_err());
    let p = posts(&r.core, "discord:dm:42");
    assert_eq!(p.len(), 1, "{p:?}");
    let body = crate::outbox::body_of(&p[0]);
    assert_eq!(body["kind"], "failed");
    assert_eq!(
        (&body["then"], &body["turns"]),
        (&json!("backoff"), &json!(1))
    );
    assert!(
        body["error"].as_str().unwrap().contains("no such thing"),
        "{body}"
    );
}

/// The content of each result in a session, by its tool.
fn result_of(core: &Core, sid: &str, tool: &str) -> String {
    core.store
        .session_nodes(sid)
        .unwrap()
        .into_iter()
        .find_map(|(_, n)| match n.body {
            Body::ToolResult {
                tool: t, content, ..
            } if t == tool => Some(content),
            _ => None,
        })
        .unwrap_or_else(|| panic!("no {tool} result"))
}

/// A result too long to show whole says how much the cut left out and the
/// call that returns it, and never that the whole is stored: nothing keeps it
/// (theseus-46v). A read names its rows by number, with the range that
/// returns them; a search, which takes no range, names a narrower one.
#[tokio::test]
async fn a_capped_result_says_what_was_cut_and_the_call_that_returns_it() {
    let r = rig_with(
        vec![
            Scripted::tools(
                "Reading the inventory, then searching it.",
                &[
                    ("t1", "fs_read", json!({"path": "inventory.txt"})),
                    (
                        "t2",
                        "fs_grep",
                        json!({"pattern": "crate", "path": "inventory.txt"}),
                    ),
                ],
            ),
            Scripted::text("Read."),
        ],
        |c| c.tools.result_max_chars = 2_000,
    );
    let body: String = (1..=300)
        .map(|i| format!("crate {i:03} of the inventory\n"))
        .collect();
    std::fs::write(r.root.join("inventory.txt"), &body).unwrap();
    let res = turn(&r.core, None, "read the inventory").await;
    let marker = |c: &str| -> String {
        let (_, rest) = c
            .split_once("\n…[")
            .unwrap_or_else(|| panic!("no cut: {c}"));
        rest.split_once("]…\n").unwrap().0.to_string()
    };

    // The read: the rows left out, by number, are exactly those between
    // the last row shown before the cut and the first after it.
    let read = result_of(&r.core, &res.session_id, "fs.read");
    assert!(read.chars().count() < 2_200, "capped: {read}");
    assert!(!read.contains("stored"), "{read}");
    let m = marker(&read);
    let (head, tail) = read.split_once("\n…[").unwrap();
    let row = |line: &str| -> usize { line.split('\t').next().unwrap().trim().parse().unwrap() };
    let before = row(head.lines().last().unwrap());
    let after = row(tail.split_once("]…\n").unwrap().1.lines().next().unwrap());
    let range = format!(
        "lines {}-{}; fs_read with offset={} and limit={} returns them",
        before + 1,
        after - 1,
        before + 1,
        after - 1 - before
    );
    assert!(
        m.ends_with(&format!(" not shown: {range}")),
        "{m:?} should end with {range:?}"
    );
    assert!(
        m.starts_with(&format!("{} lines (", after - 1 - before)),
        "{m}"
    );

    // The search: no range to name, so a narrower call.
    let grep = result_of(&r.core, &res.session_id, "fs.grep");
    assert!(!grep.contains("stored"), "{grep}");
    assert!(
        marker(&grep).contains("not shown: a narrower search returns them"),
        "{grep}"
    );
}
