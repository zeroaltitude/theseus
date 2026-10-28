//! The steer (theseus-qu1lc), through the whole core: a person's message sent
//! while a turn runs interrupts that turn's waits and joins it at its next
//! loop, instead of waiting for the turn to end and running as one of its
//! own. Jobs run in process (`InlineLauncher`), as in `tests_job_handles`.

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde_json::{json, Value};
use theseus_protocol::{SessionKind, TurnSubmitResult};
use tokio::task::JoinHandle;

use crate::bus::EventSink;
use crate::node::{Body, ResultStatus};
use crate::policy::Posture;
use crate::provider::{FakeProvider, Scripted};
use crate::session::SessionRecord;
use crate::store::Store;
use crate::turn::admission::Class;
use crate::turn::TurnRequest;
use crate::{Config, Core};

struct Rig {
    core: Arc<Core>,
    fake: Arc<FakeProvider>,
    drain: JoinHandle<()>,
    _dir: tempfile::TempDir,
}

impl Drop for Rig {
    fn drop(&mut self) {
        self.drain.abort();
    }
}

/// A deployment whose `proc.run` runs in process, waiting `sync` seconds,
/// with a drain every 20 ms; `delay_ms` is each model call's latency.
fn rig(script: Vec<Scripted>, sync: u64, delay_ms: u64, edit: impl FnOnce(&mut Config)) -> Rig {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("work");
    std::fs::create_dir_all(&root).unwrap();
    let mut cfg = Config::example();
    cfg.server.state_dir = dir.path().join("state").to_string_lossy().into_owned();
    cfg.tools.projects_dir = Some(root.canonicalize().unwrap().to_string_lossy().into_owned());
    cfg.tools.roots = vec![];
    cfg.tools.proc_sync_secs = sync;
    cfg.policy.enforcement = Posture::Notify;
    cfg.policy.tools.remove("proc.run");
    // Messages are answered as turns here, not judged.
    cfg.memory.mode = crate::config::MemoryMode::Off;
    edit(&mut cfg);
    let store = Store::open(&dir.path().join("store")).unwrap();
    let mut fake = FakeProvider::scripted(script);
    fake.delay_ms = delay_ms;
    let fake = Arc::new(fake);
    let core = Core::build(crate::rpc::Parts::for_tests(cfg, fake.clone(), store)).unwrap();
    let c = core.clone();
    let drain = tokio::spawn(async move {
        loop {
            c.drain_spool();
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    });
    Rig {
        core,
        fake,
        drain,
        _dir: dir,
    }
}

fn request(core: &Arc<Core>, session: &SessionRecord, input: &str) -> TurnRequest {
    let (live, _) = core.live_profile();
    let target = core.runner.resolve_target(&live, None, None, None).unwrap();
    let sink = EventSink::new(core.bus.clone(), &session.session_id, None);
    TurnRequest {
        prompt: None,
        surface: None,
        session: session.clone(),
        input: Some(input.into()),
        target,
        sink,
        author: "test".into(),
        recompile: None,
        attachments: vec![],
        arrived: None,
        reply_to: None,
        answer_at: None,
    }
}

impl Rig {
    /// A new conversation.
    fn session(&self) -> SessionRecord {
        let r = SessionRecord::new(SessionKind::Conversation, None);
        self.core.store.put_session(&r.session_id, &r).unwrap();
        r
    }

    /// A turn of `session` with `input`, running now.
    fn submit(&self, session: &SessionRecord, input: &str) -> JoinHandle<TurnSubmitResult> {
        let core = self.core.clone();
        let rec = core
            .store
            .get_session::<SessionRecord>(&session.session_id)
            .unwrap()
            .unwrap();
        let req = request(&core, &rec, input);
        tokio::spawn(async move { core.runner.run(req).await.unwrap() })
    }

    /// Until the model has been asked `n` times.
    async fn until_calls(&self, n: usize) {
        let t0 = Instant::now();
        while self.fake.requests().len() < n {
            assert!(t0.elapsed() < Duration::from_secs(20), "the model call {n}");
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    }

    /// Request `i`'s messages, as JSON text.
    fn request(&self, i: usize) -> String {
        serde_json::to_string(&self.fake.requests()[i].messages).unwrap()
    }

    /// The result of call `id` in `session`.
    fn result(&self, session: &str, id: &str) -> (ResultStatus, String, Value) {
        self.core
            .store
            .session_nodes(session)
            .unwrap()
            .into_iter()
            .find_map(|(_, n)| match n.body {
                Body::ToolResult {
                    tool_use_id,
                    status,
                    content,
                    meta,
                    late: false,
                    ..
                } if tool_use_id == id => Some((status, content, meta)),
                _ => None,
            })
            .unwrap_or_else(|| panic!("no result for {id}"))
    }

    /// The texts of the session's user messages, in order.
    fn said(&self, session: &str) -> Vec<String> {
        self.core
            .store
            .session_nodes(session)
            .unwrap()
            .into_iter()
            .filter_map(|(_, n)| match n.body {
                Body::UserMessage { text, .. } => Some(text),
                _ => None,
            })
            .collect()
    }

    /// The ledger rows of `kind`.
    fn rows(&self, kind: &str) -> Vec<Value> {
        self.core
            .store
            .ledger_tail::<crate::ledger::LedgerRow>(10_000)
            .unwrap()
            .into_iter()
            .map(|(_, r)| r)
            .filter(|r| r.kind == kind)
            .map(|r| serde_json::to_value(r).unwrap())
            .collect()
    }
}

fn background(argv: &[&str]) -> Value {
    json!({"argv": argv, "background": true})
}

/// The case the owner measured: the model waits on a build with `job_wait`,
/// and a message sent meanwhile is heard at once. The wait returns saying the
/// job still runs and a message arrived; the message is in the very next
/// request, after the wait's result; the job goes on; and the message is
/// written once, by the running turn, which answers it: no second turn runs.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_message_sent_during_job_wait_returns_the_wait_and_joins_the_next_request() {
    let r = rig(
        vec![
            Scripted::tools(
                "Starting it.",
                &[("t1", "proc_run", background(&["sleep", "4"]))],
            ),
            Scripted::tools(
                "Waiting.",
                &[("t2", "job_wait", json!({"job": "j1", "timeout_secs": 60}))],
            ),
            Scripted::text("Stopping it."),
        ],
        10,
        0,
        |_| {},
    );
    let s = r.session();
    let a = r.submit(&s, "build it and wait");
    r.until_calls(2).await;
    tokio::time::sleep(Duration::from_millis(400)).await;
    let sent = Instant::now();
    let b = r.submit(&s, "cancel it");
    let a = a.await.unwrap();
    let b = b.await.unwrap();
    assert!(
        sent.elapsed() < Duration::from_secs(5),
        "answered {:?} after the message, not at the wait's end",
        sent.elapsed()
    );
    let sid = &s.session_id;
    let (status, text, meta) = r.result(sid, "t2");
    assert_eq!(status, ResultStatus::Ok, "{text}");
    assert!(
        text.contains("still running") && text.contains("message arrived"),
        "{text}"
    );
    assert_eq!(meta["interrupted_by"], "message");
    assert_eq!(meta["job"], "j1");
    // In the next call's request, after the wait's result.
    let next = r.request(2);
    let (wait, msg) = (next.find("message arrived"), next.find("cancel it"));
    assert!(wait.is_some() && msg > wait, "{next}");
    // One turn: the same answer to both, one node, no fourth call.
    assert_eq!(
        a.turn_id, b.turn_id,
        "the message was answered by the running turn"
    );
    assert_eq!(r.fake.requests().len(), 3, "no turn of its own");
    assert_eq!(
        r.said(sid),
        ["build it and wait", "cancel it"],
        "written once"
    );
    assert_eq!(r.rows("turn.steered").len(), 1);
    // The job goes on, and its end still arrives by itself.
    let job = r
        .core
        .store
        .session_nodes(sid)
        .unwrap()
        .into_iter()
        .find_map(|(_, n)| match n.body {
            Body::ToolResult {
                tool_use_id,
                correlation_id,
                ..
            } if tool_use_id == "t1" => correlation_id,
            _ => None,
        })
        .expect("the job's call");
    let action = r.core.kernel.action(&job).unwrap().unwrap();
    assert_eq!(
        action.state,
        theseus_kernel::ActionState::Dispatched,
        "it runs on"
    );
    // Its end still arrives by itself, for the turn after this one.
    let t0 = Instant::now();
    let ended = loop {
        let a = r.core.kernel.action(&job).unwrap().unwrap();
        if a.state == theseus_kernel::ActionState::Succeeded {
            break a;
        }
        assert!(
            t0.elapsed() < Duration::from_secs(20),
            "the job's end {:?}",
            a.state
        );
        tokio::time::sleep(Duration::from_millis(50)).await;
    };
    let exec = r
        .core
        .kernel
        .execution(&ended.execution_id)
        .unwrap()
        .unwrap();
    assert!(
        exec.queued_results.contains(&job),
        "{:?}",
        exec.queued_results
    );
}

/// Two messages sent during one model call wait for that call's boundary,
/// which the call never loses, and join in the order they arrived.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn messages_sent_during_a_model_call_wait_for_its_boundary_in_order() {
    let r = rig(
        vec![
            Scripted::tools(
                "Looking.",
                &[("t1", "proc_run", json!({"argv": ["echo", "hi"]}))],
            ),
            Scripted::text("Done."),
        ],
        10,
        700,
        |_| {},
    );
    let s = r.session();
    let a = r.submit(&s, "look");
    r.until_calls(1).await;
    tokio::time::sleep(Duration::from_millis(100)).await;
    let first = r.submit(&s, "first thing");
    tokio::time::sleep(Duration::from_millis(150)).await;
    let second = r.submit(&s, "second thing");
    let (a, first, second) = (
        a.await.unwrap(),
        first.await.unwrap(),
        second.await.unwrap(),
    );
    assert_eq!(
        (a.turn_id.as_str(), first.turn_id.as_str()),
        (a.turn_id.as_str(), a.turn_id.as_str())
    );
    assert_eq!(
        second.turn_id, a.turn_id,
        "answered once, by the running turn"
    );
    let sid = &s.session_id;
    // The first call went out without them; the second has both, in order.
    assert!(!r.request(0).contains("first thing"));
    let next = r.request(1);
    let (one, two) = (next.find("first thing"), next.find("second thing"));
    assert!(one.is_some() && two > one, "{next}");
    assert_eq!(r.fake.requests().len(), 2);
    assert_eq!(r.said(sid), ["look", "first thing", "second thing"]);
    // A command the message found short still gave its result.
    let (status, text, _) = r.result(sid, "t1");
    assert_eq!(status, ResultStatus::Ok, "{text}");
}

/// The turn's own foreground wait yields the same way: the command goes to
/// the background as when its bound passes, the result says a message
/// arrived, and the job runs on.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_foreground_command_goes_to_the_background_for_a_message() {
    let r = rig(
        vec![
            Scripted::tools(
                "Running it.",
                &[("t1", "proc_run", json!({"argv": ["sleep", "30"]}))],
            ),
            Scripted::text("Heard you."),
        ],
        40,
        0,
        |_| {},
    );
    let s = r.session();
    let a = r.submit(&s, "run it");
    r.until_calls(1).await;
    tokio::time::sleep(Duration::from_millis(1500)).await;
    let sent = Instant::now();
    let b = r.submit(&s, "how is it going");
    let (a, b) = (a.await.unwrap(), b.await.unwrap());
    assert!(
        sent.elapsed() < Duration::from_secs(5),
        "{:?}",
        sent.elapsed()
    );
    assert_eq!(a.turn_id, b.turn_id);
    let (status, text, meta) = r.result(&s.session_id, "t1");
    assert_eq!(status, ResultStatus::Background, "{text}");
    assert!(text.contains("a message arrived"), "{text}");
    assert_eq!(meta["interrupted_by"], "message");
    assert_eq!(meta["job"], "j1");
    assert!(r.request(1).contains("how is it going"));
}

/// A batch's step waits the same way: past `STEER_AFTER` a message sends the
/// running step to the background at once, the result says a message arrived
/// and that the steps after it will not run, and the message joins the turn
/// (a planted revert of the batch's yield was missed before this test).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_batch_step_goes_to_the_background_for_a_message() {
    let r = rig(
        vec![
            Scripted::tools(
                "Running the steps.",
                &[(
                    "t1",
                    "proc_run",
                    json!({"steps": [{"argv": ["true"]}, {"argv": ["sleep", "30"]}, {"argv": ["true"]}]}),
                )],
            ),
            Scripted::text("Heard you."),
        ],
        40,
        0,
        |_| {},
    );
    let s = r.session();
    let a = r.submit(&s, "run the steps");
    r.until_calls(1).await;
    tokio::time::sleep(Duration::from_millis(1500)).await;
    let sent = Instant::now();
    let b = r.submit(&s, "how far along");
    let (a, b) = (a.await.unwrap(), b.await.unwrap());
    assert!(
        sent.elapsed() < Duration::from_secs(5),
        "{:?}",
        sent.elapsed()
    );
    assert_eq!(a.turn_id, b.turn_id, "the message joined the running turn");
    let (status, text, meta) = r.result(&s.session_id, "t1");
    assert_eq!(status, ResultStatus::Background, "{text}");
    assert!(text.contains("because a message arrived"), "{text}");
    assert_eq!(meta["interrupted_by"], "message");
    assert!(r.request(1).contains("how far along"));
}

/// With `steer_queued` off a message waits for the turn to end and runs as
/// the next turn, as it always did: the wait is not interrupted.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn with_the_switch_off_a_message_queues_as_a_turn_of_its_own() {
    let r = rig(
        vec![
            Scripted::tools(
                "Starting.",
                &[("t1", "proc_run", background(&["sleep", "30"]))],
            ),
            Scripted::tools(
                "Waiting.",
                &[("t2", "job_wait", json!({"job": "j1", "timeout_secs": 2}))],
            ),
            Scripted::text("Still going."),
            Scripted::text("Answering you."),
        ],
        10,
        0,
        |c| c.kernel.steer_queued = false,
    );
    let s = r.session();
    let a = r.submit(&s, "build it");
    r.until_calls(2).await;
    tokio::time::sleep(Duration::from_millis(300)).await;
    let b = r.submit(&s, "cancel it");
    let (a, b) = (a.await.unwrap(), b.await.unwrap());
    let (_, text, meta) = r.result(&s.session_id, "t2");
    assert!(text.starts_with("Still running after 2 s."), "{text}");
    assert!(meta.get("interrupted_by").is_none(), "{meta}");
    assert_ne!(a.turn_id, b.turn_id, "its own turn");
    assert_eq!(r.fake.requests().len(), 4);
    assert!(
        !r.request(2).contains("cancel it"),
        "not in the running turn"
    );
    assert!(r.rows("turn.steered").is_empty());
}

/// A wake, a continuation or a task's turn queued behind a running turn is
/// no person's message: it raises no signal and offers itself to no turn.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn only_a_persons_input_is_offered_to_the_running_turn() {
    let r = rig(vec![], 10, 0, |_| {});
    let s = r.session();
    let turning = r.core.tools.job_waits.steers.open(&s.session_id, true);
    let req = request(&r.core, &s, "a message");
    let now = Instant::now();
    for class in [Class::Wake, Class::Task] {
        assert!(
            r.core.runner.steered(&req, class, now).await.is_none(),
            "{class:?}"
        );
    }
    let mut no_input = request(&r.core, &s, "x");
    no_input.input = None;
    assert!(r
        .core
        .runner
        .steered(&no_input, Class::Person, now)
        .await
        .is_none());
    let mut prompt = request(&r.core, &s, "x");
    prompt.prompt = Some(crate::mcp::prompts::PromptInput {
        author: "prompt:srv/p".into(),
        messages: vec![],
    });
    assert!(r
        .core
        .runner
        .steered(&prompt, Class::Person, now)
        .await
        .is_none());
    assert!(!r.core.tools.job_waits.steers.any(), "nothing was offered");
    drop(turning);
}

/// A message's node and the `turn.steered` row are one frame, so a crash
/// leaves it written once or not at all. A frame that fails is the case: the
/// turn faults, nothing of the message is written, and the message queues
/// and runs as its own turn: delivered once.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_steer_frame_that_fails_leaves_the_message_to_run_once_as_a_turn() {
    let r = rig(
        vec![
            Scripted::tools(
                "Starting.",
                &[("t1", "proc_run", background(&["sleep", "30"]))],
            ),
            Scripted::tools(
                "Waiting.",
                &[("t2", "job_wait", json!({"job": "j1", "timeout_secs": 60}))],
            ),
            Scripted::text("never read"),
            Scripted::text("Answering you."),
        ],
        10,
        0,
        |_| {},
    );
    let frames: Arc<Mutex<Vec<Vec<String>>>> = Arc::default();
    let seen = frames.clone();
    r.core.store.fail_turn_frame(move |records| {
        let kinds: Vec<String> = records
            .iter()
            .map(|rec| match rec.kind {
                theseus_store::kinds::LEDGER => serde_json::from_slice::<Value>(&rec.payload)
                    .map(|v| v["kind"].as_str().unwrap_or("?").to_string())
                    .unwrap_or_default(),
                theseus_store::kinds::NODE => format!(
                    "node:{}",
                    String::from_utf8_lossy(&rec.payload).contains("cancel it")
                ),
                _ => String::new(),
            })
            .collect();
        let steering = kinds.iter().any(|k| k == "turn.steered");
        seen.lock().unwrap().push(kinds);
        steering
    });
    let s = r.session();
    let a = r.submit(&s, "build it");
    r.until_calls(2).await;
    tokio::time::sleep(Duration::from_millis(300)).await;
    let b = r.submit(&s, "cancel it");
    let _ = a.await;
    let _ = b.await.unwrap();
    // The one frame that held the steer row held the message's node, and
    // failed: no row of it was written, and the message was written once.
    let seen = frames.lock().unwrap();
    let steering: Vec<&Vec<String>> = seen
        .iter()
        .filter(|f| f.iter().any(|k| k == "turn.steered"))
        .collect();
    assert_eq!(steering.len(), 1, "{seen:?}");
    assert!(steering[0].iter().any(|k| k == "node:true"), "{steering:?}");
    assert!(r.rows("turn.steered").is_empty(), "the frame failed");
    assert_eq!(
        r.said(&s.session_id)
            .iter()
            .filter(|t| *t == "cancel it")
            .count(),
        1,
        "delivered once"
    );
}
