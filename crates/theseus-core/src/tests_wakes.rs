//! Wakes through the whole core (DD8, theseus-cff): `wake.at` sets a pending
//! wake, and at its time the driver runs a turn in the same session whose
//! input is the note, marked as a wake. Input before it runs as usual and
//! leaves it pending; a restart keeps it, and it fires once; one due while
//! the daemon was down runs after startup, marked late; a cancel clears it;
//! the cap refuses a sixth, readably; the turn's reply posts through the
//! outbox; a task cannot set one.

use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde_json::{json, Value};
use theseus_kernel::{ExecState, Execution, Wake};
use theseus_protocol::{SessionKind, TurnSubmitResult};

use crate::bus::EventSink;
use crate::node::{Body, Node};
use crate::policy::Posture;
use crate::provider::{
    DeltaSink, FakeProvider, Provider, ProviderFuture, ProviderRequest, Scripted,
};
use crate::session::SessionRecord;
use crate::store::Store;
use crate::turn::TurnRequest;
use crate::{Config, Core};

const PLACE: &str = "dm:42";
const TARGET: &str = "discord:dm:42";

/// A stand-in model that answers each request from the request itself, and
/// keeps every request.
struct Model {
    requests: Mutex<Vec<ProviderRequest>>,
    /// How many of the next requests that answer a wake fail with a 529
    /// (theseus-4lx).
    fail_wake_turns: std::sync::atomic::AtomicU32,
}

impl Provider for Model {
    fn name(&self) -> &str {
        "fake"
    }
    fn stream_message<'a>(
        &'a self,
        req: &'a ProviderRequest,
        on_delta: DeltaSink<'a>,
    ) -> ProviderFuture<'a> {
        Box::pin(async move {
            self.requests.lock().unwrap().push(req.clone());
            let overloaded = last_user(req).contains("⏰ wake")
                && self
                    .fail_wake_turns
                    .fetch_update(
                        std::sync::atomic::Ordering::SeqCst,
                        std::sync::atomic::Ordering::SeqCst,
                        |n| n.checked_sub(1),
                    )
                    .is_ok();
            let next = if overloaded {
                Scripted::Fail(crate::provider::ProviderError::Overloaded {
                    message: "busy".into(),
                })
            } else {
                script(req)
            };
            let f = FakeProvider::scripted(vec![next]);
            f.stream_message(req, on_delta).await
        })
    }
}

fn text_of(m: &Value) -> String {
    match &m["content"] {
        Value::String(s) => s.clone(),
        Value::Array(b) => b
            .iter()
            .filter_map(|b| b["text"].as_str())
            .collect::<Vec<_>>()
            .join("\n"),
        _ => String::new(),
    }
}

fn last_user(req: &ProviderRequest) -> String {
    req.messages
        .iter()
        .rev()
        .find(|m| m["role"] == "user")
        .map(text_of)
        .unwrap_or_default()
}

fn answers_a_call(req: &ProviderRequest) -> bool {
    req.messages
        .last()
        .and_then(|m| m["content"].as_array())
        .is_some_and(|c| c.iter().any(|b| b["type"] == "tool_result"))
}

/// `WAKE <after>: <note>` sets a wake; `SIX` asks for six at once; a wake's
/// input is answered; anything else is echoed.
fn script(req: &ProviderRequest) -> Scripted {
    let last = last_user(req);
    if answers_a_call(req) {
        return Scripted::text("Wake set.");
    }
    if last.contains("⏰ wake") {
        return Scripted::text("Checked the build: it is green.");
    }
    if let Some(rest) = last.strip_prefix("WAKE ") {
        let (after, note) = rest.split_once(": ").unwrap();
        return Scripted::tools(
            "Setting it.",
            &[("t_wake", "wake_at", json!({"after": after, "note": note}))],
        );
    }
    if last.starts_with("SIX") {
        let calls: Vec<(String, Value)> = (1..=6)
            .map(|i| {
                (
                    format!("t_{i}"),
                    json!({"after": format!("{i}h"), "note": format!("note {i}")}),
                )
            })
            .collect();
        let calls: Vec<(&str, &str, Value)> = calls
            .iter()
            .map(|(id, v)| (id.as_str(), "wake_at", v.clone()))
            .collect();
        return Scripted::tools("Setting six.", &calls);
    }
    Scripted::text(&format!("You said: {last}"))
}

fn config(root: &Path, state: &Path) -> Config {
    let mut cfg = Config::example();
    cfg.server.state_dir = state.to_string_lossy().into_owned();
    cfg.tools.projects_dir = Some(root.to_string_lossy().into_owned());
    cfg.tools.roots = vec![];
    cfg.policy.enforcement = Posture::Notify;
    cfg
}

/// One life of the daemon over the store in `dir`: a core, and the driver
/// when `drive` says so. Drop it (with `stop`) to end the life.
struct Life {
    core: Arc<Core>,
    model: Arc<Model>,
    driver: Option<tokio::task::JoinHandle<()>>,
}

fn life(dir: &Path, drive: bool) -> Life {
    let root = dir.join("work");
    std::fs::create_dir_all(&root).unwrap();
    let cfg = config(&root.canonicalize().unwrap(), dir);
    let store = Store::open(&dir.join("store")).unwrap();
    let model = Arc::new(Model {
        requests: Mutex::default(),
        fail_wake_turns: Default::default(),
    });
    let core = Core::build(crate::rpc::Parts::for_tests(cfg, model.clone(), store)).unwrap();
    let driver = drive.then(|| tokio::spawn(crate::harness::drive(core.clone())));
    Life {
        core,
        model,
        driver,
    }
}

impl Life {
    /// End this life: the driver stops, the turns it started end, and the
    /// store closes with the core.
    async fn stop(self) {
        if let Some(d) = self.driver {
            d.abort();
            let _ = d.await;
        }
        let core = self.core;
        let t0 = Instant::now();
        while Arc::strong_count(&core) > 1 {
            assert!(
                t0.elapsed() < Duration::from_secs(10),
                "the core is still held"
            );
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    }
}

fn session(core: &Arc<Core>) -> String {
    let rec = SessionRecord::new(SessionKind::Conversation, None);
    core.store.put_session(&rec.session_id, &rec).unwrap();
    core.outbox.bind_place(PLACE, &rec.session_id).unwrap();
    rec.session_id
}

async fn turn(core: &Arc<Core>, sid: &str, input: &str) -> TurnSubmitResult {
    let rec: SessionRecord = core.store.get_session(sid).unwrap().unwrap();
    let (live, _) = core.live_profile();
    let target = core.runner.resolve_target(&live, None, None, None).unwrap();
    let sink = EventSink::new(core.bus.clone(), sid, None);
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

fn exec_of(core: &Core, sid: &str) -> Execution {
    let rec: SessionRecord = core.store.get_session(sid).unwrap().unwrap();
    core.kernel
        .execution(rec.execution_id.as_deref().unwrap())
        .unwrap()
        .unwrap()
}

/// The wake nodes in a session.
fn wake_nodes(core: &Core, sid: &str) -> Vec<Node> {
    core.store
        .session_nodes(sid)
        .unwrap()
        .into_iter()
        .map(|(_, n)| n)
        .filter(|n| n.author.as_deref().is_some_and(|a| a.starts_with("wake:")))
        .collect()
}

fn node_text(n: &Node) -> &str {
    match &n.body {
        Body::UserMessage { text, .. } => text,
        _ => "",
    }
}

fn replies(core: &Core) -> Vec<Value> {
    core.kernel
        .outbox_actions()
        .unwrap()
        .into_iter()
        .filter(|a| crate::outbox::kind_of(a) == "reply")
        .map(|a| {
            assert_eq!(crate::outbox::target_of(&a), TARGET);
            crate::outbox::body_of(&a).clone()
        })
        .collect()
}

async fn until(what: &str, secs: u64, mut f: impl FnMut() -> bool) {
    let t0 = Instant::now();
    while !f() {
        assert!(
            t0.elapsed() < Duration::from_secs(secs),
            "no {what} in {secs} s"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

fn rows(core: &Core, kind: &str) -> Vec<Value> {
    core.store
        .ledger_tail::<crate::ledger::LedgerRow>(10_000)
        .unwrap()
        .into_iter()
        .map(|(_, r)| r)
        .filter(|r| r.kind == kind)
        .map(|r| r.data)
        .collect()
}

/// `after = "2s"`: the turn that sets it returns at once, the session waits
/// on input with the wake beside it, and about 2 s later, not before, the
/// driver runs a turn whose input is the note, marked as a wake. Its reply
/// posts through the outbox, with the wake's line.
#[tokio::test]
async fn a_wake_after_two_seconds_runs_a_turn_with_its_note_and_not_before() {
    let dir = tempfile::tempdir().unwrap();
    let l = life(dir.path(), true);
    let sid = session(&l.core);
    let t0 = Instant::now();
    let res = turn(&l.core, &sid, "WAKE 2s: check the build").await;
    assert!(res.output.ends_with("Wake set."), "{}", res.output);
    assert!(
        t0.elapsed() < Duration::from_secs(2),
        "the setting turn returns at once"
    );
    let e = exec_of(&l.core, &sid);
    assert_eq!(
        (e.state, e.wake.clone()),
        (ExecState::Waiting, Some(Wake::Input))
    );
    assert_eq!(e.wakes.len(), 1);
    let w = e.wakes[0].clone();
    assert_eq!(w.note, "check the build");
    assert_eq!(w.target.as_deref(), Some(TARGET));
    assert!(w.due_at_ms.abs_diff(w.set_at_ms + 2_000) < 50);
    // The tool's result says when, and what the turn's input will be.
    let result = l
        .core
        .store
        .session_nodes(&sid)
        .unwrap()
        .into_iter()
        .find_map(|(_, n)| match n.body {
            Body::ToolResult { content, tool, .. } if tool == "wake.at" => Some(content),
            _ => None,
        })
        .unwrap();
    assert!(result.starts_with("Set wake "), "{result}");
    assert!(result.contains("⏰ wake (set "), "{result}");
    assert!(result.contains("1 of 5 wakes are pending"), "{result}");
    assert!(
        result.contains("whose input is this line:\n⏰ wake (set ")
            && result.ends_with("): check the build"),
        "the line ends the result, with the note as it was: {result}"
    );

    until("the wake's turn", 10, || {
        !wake_nodes(&l.core, &sid).is_empty()
    })
    .await;
    let nodes = wake_nodes(&l.core, &sid);
    assert_eq!(nodes.len(), 1);
    let n = &nodes[0];
    assert!(n.created_at_ms >= w.due_at_ms, "not before its time");
    assert!(
        n.created_at_ms - w.due_at_ms < 1_500,
        "within a tick: {}",
        n.created_at_ms - w.due_at_ms
    );
    let set = crate::wake::local(w.set_at_ms).hm();
    assert_eq!(
        node_text(n),
        format!("⏰ wake (set {set}): check the build")
    );
    until("the wake turn's reply", 10, || replies(&l.core).len() == 2).await;
    let reply = replies(&l.core).pop().unwrap();
    assert_eq!(reply["wakes"][0]["wake_id"], w.id);
    assert_eq!(reply["wakes"][0]["text"], node_text(n));
    assert_eq!(reply["result"]["session_id"], sid);
    // The model read the wake as the user's message and answered it.
    let last = l.model.requests.lock().unwrap().last().cloned().unwrap();
    assert_eq!(last_user(&last), node_text(n));
    until("the session to park", 10, || {
        let e = exec_of(&l.core, &sid);
        e.state == ExecState::Waiting && e.wakes.is_empty()
    })
    .await;
    let fired = rows(&l.core, "wake.fired");
    assert_eq!(fired.len(), 1);
    assert!(fired[0]["late_ms"].as_u64().unwrap() < 1_500);
    assert_eq!(fired[0]["while_down"], false);
    l.stop().await;
}

/// theseus-4lx: a wake's turn that fails before its reply (a 529) leaves the
/// wake's node in the session, and its retry answers it. The retry took no
/// wake, so it once posted its reply without the wake's line, and, when the
/// session's place had moved on (`/new`) between the two, nowhere at all. Now
/// it frames the reply from the wake's unanswered node, and posts where the
/// wake was set, which the take kept.
#[tokio::test]
async fn a_wake_turns_retry_keeps_the_wakes_line_and_where_it_was_set() {
    let dir = tempfile::tempdir().unwrap();
    let l = life(dir.path(), true);
    let sid = session(&l.core);
    turn(&l.core, &sid, "WAKE 1s: check the build").await;
    l.model
        .fail_wake_turns
        .store(1, std::sync::atomic::Ordering::SeqCst);
    let asked = |l: &Life| {
        l.model
            .requests
            .lock()
            .unwrap()
            .iter()
            .filter(|r| last_user(r).contains("⏰ wake"))
            .count()
    };
    // The wake's first turn fails, and nothing is posted for it.
    until("the failed attempt", 10, || asked(&l) >= 1).await;
    // `/new` meanwhile: the place moves on to a new session before the retry.
    let _new = session(&l.core);
    assert_eq!(
        l.core.outbox.target(&sid),
        None,
        "the session posts nowhere"
    );
    // The retry answers the wake, and its reply goes where the wake was set.
    until("the retry's reply", 15, || replies(&l.core).len() == 2).await;
    assert!(asked(&l) >= 2, "the wake was asked twice");
    let nodes = wake_nodes(&l.core, &sid);
    assert_eq!(nodes.len(), 1, "one node: the retry took no second wake");
    let reply = replies(&l.core).pop().unwrap();
    assert_eq!(reply["result"]["session_id"], sid);
    assert_eq!(reply["wakes"].as_array().map(Vec::len), Some(1), "{reply}");
    assert_eq!(reply["wakes"][0]["text"], node_text(&nodes[0]));
    l.stop().await;
}

/// Input before the due time runs a turn and leaves the wake pending, and
/// the wake still fires later.
#[tokio::test]
async fn input_before_the_due_time_runs_a_turn_and_the_wake_still_fires() {
    let dir = tempfile::tempdir().unwrap();
    let l = life(dir.path(), true);
    let sid = session(&l.core);
    turn(&l.core, &sid, "WAKE 3s: check the build").await;
    let res = turn(&l.core, &sid, "what is 17 times 23?").await;
    assert_eq!(res.output, "You said: what is 17 times 23?");
    let e = exec_of(&l.core, &sid);
    assert_eq!(e.wakes.len(), 1, "still pending");
    assert!(wake_nodes(&l.core, &sid).is_empty());
    until("the wake's turn", 10, || {
        wake_nodes(&l.core, &sid).len() == 1
    })
    .await;
    until("its reply", 10, || replies(&l.core).len() == 3).await;
    tokio::time::sleep(Duration::from_millis(600)).await;
    assert_eq!(wake_nodes(&l.core, &sid).len(), 1, "once");
    l.stop().await;
}

/// A restart before the due time: the next life's driver runs the wake,
/// once.
#[tokio::test]
async fn a_restart_before_the_due_time_still_fires_once() {
    let dir = tempfile::tempdir().unwrap();
    let l = life(dir.path(), false);
    let sid = session(&l.core);
    turn(&l.core, &sid, "WAKE 2s: check the build").await;
    l.stop().await;
    let l = life(dir.path(), true);
    until("the wake's turn", 10, || {
        wake_nodes(&l.core, &sid).len() == 1
    })
    .await;
    let n = &wake_nodes(&l.core, &sid)[0];
    assert!(!node_text(n).contains("late"), "{}", node_text(n));
    tokio::time::sleep(Duration::from_millis(1_200)).await;
    assert_eq!(wake_nodes(&l.core, &sid).len(), 1, "once");
    assert_eq!(rows(&l.core, "wake.fired").len(), 1);
    l.stop().await;
}

/// Down across the due time: the next life's startup queues it, and it runs
/// marked late, by how much, with the reason, once.
#[tokio::test]
async fn a_wake_due_while_the_daemon_was_down_runs_after_startup_marked_late() {
    let dir = tempfile::tempdir().unwrap();
    let l = life(dir.path(), false);
    let sid = session(&l.core);
    turn(&l.core, &sid, "WAKE 1s: check the build").await;
    let w = exec_of(&l.core, &sid).wakes[0].clone();
    l.stop().await;
    // Down for its time and 6 s past it, so it runs over 5 s late.
    tokio::time::sleep(Duration::from_millis(7_000)).await;
    let l = life(dir.path(), true);
    until("the wake's turn", 10, || {
        wake_nodes(&l.core, &sid).len() == 1
    })
    .await;
    let text = node_text(&wake_nodes(&l.core, &sid)[0]).to_string();
    let set = crate::wake::local(w.set_at_ms).hm();
    let due = crate::wake::local(w.due_at_ms).hms();
    assert!(
        text.starts_with(&format!("⏰ wake (set {set}, due {due}, ")),
        "{text}"
    );
    assert!(
        text.ends_with(" late: the daemon was not running then): check the build"),
        "{text}"
    );
    let fired = rows(&l.core, "wake.fired");
    assert_eq!(fired.len(), 1);
    assert!(
        fired[0]["late_ms"].as_u64().unwrap() >= 6_000,
        "{:?}",
        fired[0]
    );
    assert_eq!(fired[0]["while_down"], true);
    assert_eq!(
        rows(&l.core, "execution.queued").last().unwrap()["why"],
        "wake"
    );
    tokio::time::sleep(Duration::from_millis(600)).await;
    assert_eq!(wake_nodes(&l.core, &sid).len(), 1, "once");
    l.stop().await;
}

/// A cancel clears the wake, names who cancelled it, and nothing fires.
#[tokio::test]
async fn a_cancel_clears_the_wake_and_nothing_fires() {
    let dir = tempfile::tempdir().unwrap();
    let l = life(dir.path(), true);
    let sid = session(&l.core);
    turn(&l.core, &sid, "WAKE 2s: check the build").await;
    let listed = l.core.wakes(Some(&sid), None).unwrap();
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].note, "check the build");
    assert_eq!(listed[0].target.as_deref(), Some(TARGET));
    assert_eq!(l.core.health().wakes, listed, "health lists it");
    assert_eq!(l.core.wakes(None, Some(TARGET)).unwrap(), listed);
    assert!(l.core.wakes(None, Some("discord:dm:7")).unwrap().is_empty());
    let r = l.core.wake_cancel_by(&listed[0].short, "the CLI").unwrap();
    assert_eq!(r.wake.wake_id, listed[0].wake_id);
    assert!(l.core.wakes(None, None).unwrap().is_empty());
    let again = l
        .core
        .wake_cancel_by(&listed[0].short, "the CLI")
        .unwrap_err();
    assert!(again.to_string().contains("no pending wake"), "{again}");
    tokio::time::sleep(Duration::from_millis(3_000)).await;
    assert!(wake_nodes(&l.core, &sid).is_empty(), "nothing fired");
    assert_eq!(rows(&l.core, "wake.cancelled")[0]["by"], "the CLI");
    l.stop().await;
}

/// Five pending wakes at most: the sixth call is refused, and the refusal
/// says why and lists the five.
#[tokio::test]
async fn the_cap_is_enforced_with_a_readable_refusal() {
    let dir = tempfile::tempdir().unwrap();
    let l = life(dir.path(), false);
    let sid = session(&l.core);
    turn(&l.core, &sid, "SIX wakes").await;
    let results: Vec<(String, bool)> = l
        .core
        .store
        .session_nodes(&sid)
        .unwrap()
        .into_iter()
        .filter_map(|(_, n)| match n.body {
            Body::ToolResult {
                content,
                tool,
                is_error,
                ..
            } if tool == "wake.at" => Some((content, is_error)),
            _ => None,
        })
        .collect();
    assert_eq!(results.len(), 6);
    assert!(results[..5]
        .iter()
        .all(|(c, e)| !e && c.starts_with("Set wake")));
    let (refusal, is_error) = &results[5];
    assert!(is_error);
    assert!(
        refusal.starts_with(
            "Refused: this session already has 5 pending wakes, the most it may hold ("
        ),
        "{refusal}"
    );
    assert!(refusal.contains(": note 1;"), "{refusal}");
    assert!(refusal.contains("theseus cancel"), "{refusal}");
    assert_eq!(exec_of(&l.core, &sid).wakes.len(), 5);
    l.stop().await;
}
