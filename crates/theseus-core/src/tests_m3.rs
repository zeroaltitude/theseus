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
    let store = Store::open(&dir.path().join("store")).unwrap();
    let fake = Arc::new(FakeProvider::scripted(script));
    let core = Core::build(crate::rpc::Parts::for_tests(cfg, fake.clone(), store)).unwrap();
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
/// every turn. A plain one-loop turn writes 8: 27 before theseus-hco removed
/// the hook rows, 17 before theseus-qa0 let the rows that are no state
/// transition ride in the next frame and planned, authorized, and dispatched
/// the provider call in one. A change that adds one raises this on purpose.
/// The rows keep the order they had when each was its own frame. The
/// template turns the narrative on, and a subscriber is watching: the
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
    let from = r.core.store.last_position();
    let res = turn(&r.core, Some(&first.session_id), "hi").await;
    assert_eq!(res.loops, 1);
    let frames = r.core.store.stats().unwrap().frames_appended - before;
    assert!(frames <= 8, "a plain turn wrote {frames} frames");
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
            "execution.results_consumed",
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
            attachments: vec![],
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
fn system_of(r: &Rig, i: usize) -> String {
    r.fake.requests()[i].system[0]["text"]
        .as_str()
        .unwrap()
        .to_string()
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
        cfg.model.context_files = vec![path.clone()];
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
    let header = format!("# Context file: {path}");
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
    let want = crate::compiler::ContextFileRef {
        path: path.clone(),
        digest: Some(sha16(rule)),
        bytes: rule.len() as u64,
        cut: false,
        missing: None,
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
            cfg.model.context_files = vec![path.clone()];
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
            cfg.model.context_files = vec![path.clone()];
        },
    );
    let first = turn(&r.core, None, "one").await;
    let second = turn(&r.core, Some(&first.session_id), "two").await;
    assert_eq!(
        (first.output.as_str(), second.output.as_str()),
        ("fine", "still fine")
    );
    assert!(system_of(&r, 1).ends_with(&format!(
        "# Context file: {path}\n\n[Missing: the file could not be read (not found).]"
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
        cfg.model.context_files = vec![path.clone()];
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
    assert_eq!(answered[0]["by"], "sock#1");
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
    assert_eq!(ledgered(&r, "budget.reset")[0]["by"], "sock#1");
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
            Some("tightened by web#1"),
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
        ("by", "web#1"),
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
    assert_eq!(pr["tightened"]["by"], "web#1");
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
            "proc.run now asks first: tightened by web#1."
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
            "proc.run — approve (tightened by web#1; the config says enforcement = notify)"
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
            Some("sock#2"),
            Some("web#1"),
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
            "proc.run is back to what the config says (notify, enforcement = notify): sock#2 \
             undid the tightening by web#1."
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
            "An undo of proc.run's tightening from sock#3 through cli did not count"
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
    for (label, s, discord) in [
        ("sock#1", Cli, None),
        ("web#1", Web, None),
        (
            "discord",
            Discord,
            Some((MALLORY, Some("712398310421561444"))),
        ),
        ("test", Unnamed, None),
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
            (Some(true), Some(label))
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
