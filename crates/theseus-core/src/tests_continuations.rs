//! Continuations keep their profile and their input (theseus-kol), through
//! the whole core with two stand-in providers: `anthropic`, the live
//! profile's (`sonnet`), and `zai`, the `glm` profile's, each a scripted
//! `FakeProvider` that keeps its requests.
//! - a continuation whose model call fails (a 529, a 400) leaves the job's
//!   late result for its retry, which answers from it, on the same profile;
//! - a failed turn's retry runs on the profile the failed turn ran on, not
//!   the session's turn before it, nor the live one;
//! - a late result that lands while a turn runs is answered by the turn the
//!   driver takes for it;
//! - a late result leaves the queue only in the frame that writes it;
//! - GLM's thinking never reaches Anthropic when the session moves on.

use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use serde_json::json;
use theseus_protocol::{SessionKind, TurnSubmitResult};

use crate::bus::EventSink;
use crate::node::Body;
use crate::policy::Posture;
use crate::provider::{FakeProvider, ProviderError, ProviderRequest, Scripted};
use crate::session::SessionRecord;
use crate::store::Store;
use crate::turn::TurnRequest;
use crate::{Config, Core};

struct Rig {
    core: Arc<Core>,
    /// The live profile's provider (`anthropic`, claude-sonnet-5-5).
    sonnet: Arc<FakeProvider>,
    /// The `glm` profile's provider (`zai`, glm-5.3-flash).
    glm: Arc<FakeProvider>,
    _dir: tempfile::TempDir,
}

fn config(root: &Path, state: &Path) -> Config {
    let mut cfg = Config::example();
    cfg.server.state_dir = state.to_string_lossy().into_owned();
    cfg.tools.projects_dir = Some(root.to_string_lossy().into_owned());
    cfg.tools.roots = vec![];
    cfg.tools.proc_sync_secs = 1;
    cfg.policy.tools.insert("proc.run".into(), Posture::Open);
    cfg
}

/// `sonnet` answers from its script, then "fake reply"; `glm` likewise.
fn rig(sonnet: Vec<Scripted>, glm: FakeProvider) -> Rig {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("work");
    std::fs::create_dir_all(&root).unwrap();
    let cfg = config(&root.canonicalize().unwrap(), dir.path());
    let store = Store::open(&dir.path().join("store")).unwrap();
    let sonnet = Arc::new(FakeProvider::scripted(sonnet));
    let glm = Arc::new(glm);
    let mut parts = crate::rpc::Parts::for_tests(cfg, sonnet.clone(), store);
    parts.providers.insert("zai".into(), glm.clone());
    let core = Core::build(parts).unwrap();
    Rig {
        core,
        sonnet,
        glm,
        _dir: dir,
    }
}

fn glm(script: Vec<Scripted>) -> FakeProvider {
    FakeProvider::scripted(script)
}

/// One input turn, on `profile` (`None`: the live one), in `session` or a
/// new one.
async fn turn(
    core: &Arc<Core>,
    session: Option<&str>,
    profile: Option<&str>,
    input: &str,
) -> anyhow::Result<TurnSubmitResult> {
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
    let target = core.runner.resolve_target(&live, profile, None, None)?;
    let sink = EventSink::new(core.bus.clone(), &rec.session_id, None);
    core.runner
        .run(TurnRequest {
            prompt: None,
            session: rec,
            input: Some(input.into()),
            target,
            sink,
            author: "test".into(),
            recompile: None,
            attachments: vec![],
            arrived: None,
            reply_to: None,
        })
        .await
}

/// The slow job: answered `background` after `proc_sync_secs` (1 s), done
/// at 1.5 s.
fn slow_job() -> Scripted {
    Scripted::tools(
        "",
        &[(
            "t1",
            "proc_run",
            json!({"argv": ["bash", "-c", "sleep 1.5; echo slow done"]}),
        )],
    )
}

/// Drain the spool until the job's completion has queued the execution.
async fn wait_queued(core: &Core, exec: &str) {
    for _ in 0..200 {
        tokio::time::sleep(Duration::from_millis(50)).await;
        core.heartbeat("test");
        if core.kernel.execution(exec).unwrap().unwrap().state.as_str() == "queued" {
            return;
        }
    }
    panic!("the job's completion never queued the execution");
}

/// Whether the request's messages carry the slow job's late result.
fn reads_the_late_result(req: &ProviderRequest) -> bool {
    let text = serde_json::to_string(&req.messages).unwrap();
    text.contains("Background result") && text.contains("slow done")
}

fn late_nodes(core: &Core, sid: &str) -> usize {
    core.store
        .session_nodes(sid)
        .unwrap()
        .iter()
        .filter(|(_, n)| matches!(n.body, Body::ToolResult { late: true, .. }))
        .count()
}

/// The job's result arrives, and the continuation that would read it fails
/// at its model call: a 529 (overloaded, transient) or a 400 (invalid
/// request). The failed turn already wrote the late result into the session;
/// its retry, which the failure queued, must answer from it, on `glm`, and
/// never end `nothing_new`. Before theseus-kol the retry found nothing new,
/// and the job's result went unanswered for good.
#[tokio::test]
async fn a_failed_continuation_leaves_the_late_result_for_its_retry() {
    let failures = [
        (
            "529",
            ProviderError::Overloaded {
                message: "overloaded_error".into(),
            },
        ),
        (
            "400",
            ProviderError::InvalidRequest {
                status: 400,
                message: "invalid_request_error: messages.1.content.0: Invalid signature in thinking block".into(),
            },
        ),
    ];
    for (name, failure) in failures {
        let r = rig(
            vec![],
            glm(vec![
                slow_job(),
                Scripted::text("Started; I'll report back."),
                Scripted::Fail(failure),
                Scripted::text("The slow job finished: slow done."),
            ]),
        );
        let res = turn(&r.core, None, Some("glm"), "run the slow build")
            .await
            .unwrap();
        assert_eq!(res.output, "Started; I'll report back.");
        let exec = res.execution_id.clone().unwrap();
        wait_queued(&r.core, &exec).await;

        let failed = r.core.continue_execution(&exec).await;
        assert!(failed.is_err(), "{name}: the first continuation fails");
        assert_eq!(late_nodes(&r.core, &res.session_id), 1, "{name}");
        assert!(reads_the_late_result(&r.glm.requests()[2]), "{name}");
        let e = r.core.kernel.execution(&exec).unwrap().unwrap();
        assert_eq!(
            (e.state.as_str(), e.resume_pending),
            ("queued", true),
            "{name}: the failure queued its retry"
        );

        let retry = r.core.continue_execution(&exec).await.unwrap().unwrap();
        assert!(retry.continuation);
        assert_eq!(
            retry.output, "The slow job finished: slow done.",
            "{name}: the retry answers from the job's result ({})",
            retry.stop_reason
        );
        assert_eq!(retry.profile, "glm", "{name}");
        let asked = r.glm.requests();
        assert_eq!(asked.len(), 4, "{name}");
        assert!(reads_the_late_result(&asked[3]), "{name}");
        assert!(
            r.sonnet.requests().is_empty(),
            "{name}: nothing went to the live profile"
        );
        assert_eq!(late_nodes(&r.core, &res.session_id), 1, "{name}: once");
    }
}

/// The session's turn before ran on the live profile (`sonnet`); this one is
/// asked on `glm`, and its model call fails. The failure's retry runs on
/// `glm`, the profile of the turn that failed: a failed turn records its
/// target as a finished one does. Before theseus-kol the failure wrote back
/// the session's old target, and the retry went to Sonnet.
#[tokio::test]
async fn a_failed_turns_retry_runs_on_the_profile_it_was_asked_on() {
    let r = rig(
        vec![Scripted::text("Hello from Sonnet.")],
        glm(vec![
            Scripted::Fail(ProviderError::Overloaded {
                message: "overloaded_error".into(),
            }),
            Scripted::text("Hello from GLM."),
        ]),
    );
    let first = turn(&r.core, None, None, "hello").await.unwrap();
    assert_eq!(first.profile, "sonnet");
    let sid = first.session_id.clone();
    let failed = turn(&r.core, Some(&sid), Some("glm"), "now you, GLM").await;
    assert!(failed.is_err());
    let rec: SessionRecord = r.core.store.get_session(&sid).unwrap().unwrap();
    assert_eq!(rec.last_target.unwrap().profile, "glm");

    let exec = first.execution_id.clone().unwrap();
    let retry = r.core.continue_execution(&exec).await.unwrap().unwrap();
    assert_eq!(retry.profile, "glm");
    assert_eq!(retry.output, "Hello from GLM.");
    assert_eq!(r.sonnet.requests().len(), 1, "only the first turn");
}

/// A late result that lands while a turn runs is taken at that turn's end,
/// after its model last read the session, and the driver takes another turn
/// for it: that turn must call the model, not end `nothing_new`.
#[tokio::test]
async fn a_late_result_that_lands_during_a_turn_is_answered_by_the_next() {
    let r = rig(
        vec![],
        FakeProvider {
            delay_ms: 300,
            ..glm(vec![
                slow_job(),
                Scripted::text("Started; I'll report back."),
                Scripted::text("Nothing yet."),
                Scripted::text("The slow job finished: slow done."),
            ])
        },
    );
    let res = turn(&r.core, None, Some("glm"), "run the slow build")
        .await
        .unwrap();
    let (sid, exec) = (res.session_id.clone(), res.execution_id.clone().unwrap());
    // The job's completion waits in the spool, undrained.
    let corr = r.core.kernel.open_actions().unwrap()[0]
        .correlation_id
        .clone();
    for _ in 0..200 {
        if matches!(r.core.spool.read_completion(&corr), Ok(Some(_))) {
            break;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    // A second turn; while its model call is in flight, the heartbeat
    // drains the completion, which queues it for the next turn.
    let core = r.core.clone();
    let sid2 = sid.clone();
    let second = tokio::spawn(async move {
        turn(&core, Some(&sid2), Some("glm"), "anything new?")
            .await
            .unwrap()
    });
    while r.glm.requests().len() < 3 {
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    r.core.heartbeat("test");
    let second = second.await.unwrap();
    assert_eq!(second.output, "Nothing yet.");
    assert_eq!(late_nodes(&r.core, &sid), 1, "taken at the turn's end");
    let e = r.core.kernel.execution(&exec).unwrap().unwrap();
    assert_eq!(e.state.as_str(), "queued", "woken for the late result");

    let next = r.core.continue_execution(&exec).await.unwrap().unwrap();
    assert_eq!(
        next.output, "The slow job finished: slow done.",
        "({})",
        next.stop_reason
    );
    assert!(reads_the_late_result(r.glm.requests().last().unwrap()));
}

/// A late result leaves the execution's queue only in the frame that writes
/// its node: when that frame fails (a full disk), the result is still
/// queued, and the next turn writes and answers it. Before theseus-kol the
/// queue was cleared in a frame of its own, and the result was lost.
#[tokio::test]
async fn a_late_result_leaves_the_queue_only_with_its_node() {
    let r = rig(
        vec![],
        glm(vec![
            slow_job(),
            Scripted::text("Started; I'll report back."),
            Scripted::text("The slow job finished: slow done."),
        ]),
    );
    let res = turn(&r.core, None, Some("glm"), "run the slow build")
        .await
        .unwrap();
    let (sid, exec) = (res.session_id.clone(), res.execution_id.clone().unwrap());
    wait_queued(&r.core, &exec).await;
    r.core.store.fail_turn_frame(|records| {
        records.iter().any(|rec| {
            rec.kind == theseus_store::kinds::NODE
                && String::from_utf8_lossy(&rec.payload).contains("\"late\":true")
        })
    });
    assert!(r.core.continue_execution(&exec).await.is_err());
    assert_eq!(late_nodes(&r.core, &sid), 0);
    let e = r.core.kernel.execution(&exec).unwrap().unwrap();
    assert_eq!(e.queued_results.len(), 1, "still queued");
    assert_eq!(e.state.as_str(), "queued");

    let next = r.core.continue_execution(&exec).await.unwrap().unwrap();
    assert_eq!(next.output, "The slow job finished: slow done.");
    assert_eq!(late_nodes(&r.core, &sid), 1);
    assert!(reads_the_late_result(r.glm.requests().last().unwrap()));
}

/// GLM answers with a thinking block, z.ai's signature on it; then the
/// session moves to the live profile (Sonnet), as an operator's switch does.
/// The request to Anthropic carries none of GLM's thinking, and Anthropic's
/// own thinking goes back to Anthropic.
#[tokio::test]
async fn glms_thinking_never_reaches_anthropic() {
    let r = rig(
        vec![Scripted::Blocks {
            blocks: vec![
                json!({"type": "thinking", "thinking": "Sonnet's plan.", "signature": "anthropic-signature"}),
                json!({"type": "text", "text": "Sonnet here."}),
            ],
            stop_reason: "end_turn".into(),
        }],
        glm(vec![Scripted::Blocks {
            blocks: vec![
                json!({"type": "thinking", "thinking": "GLM's plan.", "signature": "zai-signature"}),
                json!({"type": "text", "text": "GLM here."}),
            ],
            stop_reason: "end_turn".into(),
        }]),
    );
    let first = turn(&r.core, None, Some("glm"), "hi GLM").await.unwrap();
    assert_eq!(first.output, "GLM here.");
    let sid = first.session_id;
    let second = turn(&r.core, Some(&sid), None, "hi Sonnet").await.unwrap();
    assert_eq!(second.output, "Sonnet here.");
    let sent = serde_json::to_string(&r.sonnet.requests()[0].messages).unwrap();
    assert!(!sent.contains("zai-signature"), "{sent}");
    assert!(!sent.contains("GLM's plan."), "{sent}");
    assert!(sent.contains("GLM here."), "its text stays: {sent}");

    // Back to GLM: Sonnet's thinking stays with Anthropic too.
    turn(&r.core, Some(&sid), Some("glm"), "and GLM again")
        .await
        .unwrap();
    let back = serde_json::to_string(&r.glm.requests()[1].messages).unwrap();
    assert!(!back.contains("anthropic-signature"), "{back}");
    assert!(back.contains("Sonnet here."), "{back}");
}
