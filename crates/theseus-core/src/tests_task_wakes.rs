//! A task's own wakes through the whole core (37b, theseus-7kg): a task sets
//! a one-shot wake, parks on it instead of reporting, and the wake's turn
//! continues it; it reports once, when a turn would wait with no wake left,
//! and its report's wake (`wake_parent`) fires then, once. A cancel drops its
//! wakes, and none fires; a repeating wake is refused in a task; a restart
//! while it is parked still fires the wake once and reports once; and
//! `wake.list` names the task.

use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde_json::{json, Value};
use theseus_kernel::{ExecState, Execution, Wake};
use theseus_protocol::{SessionKind, TurnSubmitResult};

use crate::bus::EventSink;
use crate::node::Body;
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

/// A stand-in model that answers each request from the request itself
/// (`script`), and keeps every request.
struct Model {
    requests: Mutex<Vec<ProviderRequest>>,
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
            let f = FakeProvider::scripted(vec![script(req)]);
            f.stream_message(req, on_delta).await
        })
    }
}

fn text_of(m: &Value) -> String {
    match &m["content"] {
        Value::String(s) => s.clone(),
        Value::Array(b) => b
            .iter()
            .filter_map(|b| b["text"].as_str().or_else(|| b["content"].as_str()))
            .collect::<Vec<_>>()
            .join("\n"),
        _ => String::new(),
    }
}

fn first_user(req: &ProviderRequest) -> String {
    req.messages
        .iter()
        .find(|m| m["role"] == "user")
        .map(text_of)
        .unwrap_or_default()
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

/// A call of `wake_at` with `input`.
fn wake_call(id: &str, input: Value) -> (String, &'static str, Value) {
    (id.to_string(), "wake_at", input)
}

fn calls(text: &str, calls: &[(String, &'static str, Value)]) -> Scripted {
    let calls: Vec<(&str, &str, Value)> = calls
        .iter()
        .map(|(id, n, v)| (id.as_str(), *n, v.clone()))
        .collect();
    Scripted::tools(text, &calls)
}

/// The parent: `START <brief>` starts a task with `wake_parent`, and a
/// report's turn is answered. The task, by its brief:
/// - `CHILD AGAIN <after>`: checks now, sets a wake `after` from now, and
///   parks; the wake's turn checks again and reports;
/// - `CHILD SIX`: asks for six wakes at once (the cap);
/// - `CHILD REPEAT`: asks for a repeating wake and a one-shot one;
/// - `CHILD PLAIN`: reports at once, with no wake.
fn script(req: &ProviderRequest) -> Scripted {
    let (first, last) = (first_user(req), last_user(req));
    if first.contains("CHILD") && !first.starts_with("START ") {
        // The set call's result quotes the wake's line: answer it first.
        if answers_a_call(req) {
            return Scripted::text("Checked once: still building. I will look again.");
        }
        if last.contains("⏰ wake") {
            return Scripted::text("Checked again: the build is green. Done.");
        }
        if let Some(rest) = first.split("CHILD AGAIN ").nth(1) {
            let after = rest
                .split_whitespace()
                .next()
                .unwrap()
                .trim_end_matches(':');
            return calls(
                "Checking now, and again later.",
                &[wake_call(
                    "t_again",
                    json!({"after": after, "note": "check the build again, then report"}),
                )],
            );
        }
        if first.contains("CHILD SIX") {
            let six: Vec<_> = (1..=6)
                .map(|i| {
                    wake_call(
                        &format!("t_{i}"),
                        json!({"after": format!("{i}h"), "note": format!("look {i}")}),
                    )
                })
                .collect();
            return calls("Setting six.", &six);
        }
        if first.contains("CHILD REPEAT") {
            return calls(
                "Setting two.",
                &[
                    wake_call(
                        "t_series",
                        json!({"every": "1h", "note": "check the build every hour"}),
                    ),
                    wake_call(
                        "t_once",
                        json!({"after": "1h", "note": "check the build once"}),
                    ),
                ],
            );
        }
        return Scripted::text("Nothing to wait on. Done at once.");
    }
    if answers_a_call(req) {
        return Scripted::text("Started it.");
    }
    if let Some(brief) = last.strip_prefix("START ") {
        return Scripted::tools(
            "Starting it.",
            &[(
                "t_task",
                "task_create",
                json!({"brief": brief, "budget_usd": 2.0, "wake_parent": true}),
            )],
        );
    }
    Scripted::text("The task reported; noted.")
}

fn config(root: &Path, state: &Path) -> Config {
    let mut cfg = Config::example();
    cfg.server.state_dir = state.to_string_lossy().into_owned();
    cfg.tools.projects_dir = Some(root.to_string_lossy().into_owned());
    cfg.tools.roots = vec![];
    cfg.policy.enforcement = Posture::Notify;
    cfg
}

/// One life of the daemon over the store in `dir`, with its driver.
struct Life {
    core: Arc<Core>,
    model: Arc<Model>,
    driver: tokio::task::JoinHandle<()>,
}

fn life(dir: &Path) -> Life {
    let root = dir.join("work");
    std::fs::create_dir_all(&root).unwrap();
    let cfg = config(&root.canonicalize().unwrap(), dir);
    let store = Store::open(&dir.join("store")).unwrap();
    let model = Arc::new(Model {
        requests: Mutex::default(),
    });
    let core = Core::build(crate::rpc::Parts::for_tests(cfg, model.clone(), store)).unwrap();
    // A DM's person is its owner once the binding binds it (the place rule).
    core.runner.place_rule.bind_one(crate::places::BoundPlace {
        target: TARGET.into(),
        name: "DM".into(),
        private: false,
        ..Default::default()
    });
    let driver = tokio::spawn(crate::harness::drive(core.clone()));
    Life {
        core,
        model,
        driver,
    }
}

impl Life {
    /// End this life: the driver stops, and the store closes with the core.
    async fn stop(self) {
        self.driver.abort();
        let _ = self.driver.await;
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

fn parent_session(core: &Arc<Core>) -> String {
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
            reply_to: None,
        })
        .await
        .unwrap()
}

fn exec(core: &Core, id: &str) -> Execution {
    core.kernel.execution(id).unwrap().unwrap()
}

/// The one task of the parent session `parent`.
fn only_task(core: &Core, parent: &str) -> Execution {
    let rec: SessionRecord = core.store.get_session(parent).unwrap().unwrap();
    let tasks = core.kernel.tasks(rec.execution_id.as_deref()).unwrap();
    assert_eq!(tasks.len(), 1, "{tasks:?}");
    tasks.into_iter().next().unwrap()
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
        .ledger_tail::<crate::ledger::LedgerRow>(100_000)
        .unwrap()
        .into_iter()
        .filter(|(_, r)| r.is_kind(kind))
        .map(|(_, r)| r.data)
        .collect()
}

/// The outbox's report posts, each with its body.
fn reports(core: &Core) -> Vec<Value> {
    core.kernel
        .outbox_actions()
        .unwrap()
        .into_iter()
        .filter(|a| crate::outbox::kind_of(a) == "report")
        .map(|a| {
            assert_eq!(crate::outbox::target_of(&a), TARGET);
            crate::outbox::body_of(&a).clone()
        })
        .collect()
}

/// The text of the wake nodes in the task's session.
fn wake_nodes(core: &Core, task: &Execution) -> Vec<String> {
    core.store
        .session_nodes(&task.session_id)
        .unwrap()
        .into_iter()
        .map(|(_, n)| n)
        .filter(|n| n.author.as_deref().is_some_and(|a| a.starts_with("wake:")))
        .map(|n| match &n.body {
            Body::UserMessage { text, .. } => text.clone(),
            _ => String::new(),
        })
        .collect()
}

/// The text of the report nodes in the parent's session.
fn report_texts(core: &Core, parent: &str) -> Vec<String> {
    core.store
        .session_nodes(parent)
        .unwrap()
        .into_iter()
        .map(|(_, n)| n)
        .filter(|n| n.author.as_deref().is_some_and(|a| a.starts_with("task:")))
        .map(|n| match &n.body {
            Body::UserMessage { text, .. } => text.clone(),
            _ => String::new(),
        })
        .collect()
}

/// The text of every tool result in the task's session, in order.
fn results(core: &Core, task: &Execution) -> Vec<String> {
    core.store
        .session_nodes(&task.session_id)
        .unwrap()
        .into_iter()
        .filter_map(|(_, n)| match &n.body {
            Body::ToolResult { content, .. } => Some(content.clone()),
            _ => None,
        })
        .collect()
}

/// Starts a task with `brief` from a fresh parent, and waits until the
/// task's first turn has parked on its wake.
async fn parked(core: &Arc<Core>, brief: &str) -> (String, Execution) {
    let parent = parent_session(core);
    turn(core, &parent, &format!("START {brief}")).await;
    let task = only_task(core, &parent);
    until("the task parked on its wake", 10, || {
        let e = exec(core, &task.id);
        e.turns >= 1 && e.state == ExecState::Waiting && !e.wakes.is_empty()
    })
    .await;
    (parent, exec(core, &task.id))
}

/// The issue's first test: a task sets a 2 s wake and parks, not reporting;
/// `wake.list` names the task; about 2 s later the wake's turn continues the
/// task, which then reports exactly once, with that turn's words, and its
/// report's wake starts one parent turn.
#[tokio::test]
async fn a_task_sets_a_wake_parks_wakes_and_then_reports_once() {
    let dir = tempfile::tempdir().unwrap();
    let l = life(dir.path());
    let t0 = Instant::now();
    let (parent, task) = parked(&l.core, "CHILD AGAIN 2s: check the build").await;
    // Parked: waiting on input with its wake beside it, its first turn done,
    // and nothing reported.
    assert_eq!(task.wake, Some(Wake::Input));
    assert_eq!(task.wakes.len(), 1);
    assert_eq!(task.turns, 1);
    assert!(
        reports(&l.core).is_empty(),
        "a parked task has not reported"
    );
    assert!(rows(&l.core, "task.report_wake").is_empty());
    let pid = task.parent.clone().unwrap();
    let (task_spent, parent_spent) = (
        task.budget.spent_micros,
        exec(&l.core, &pid).budget.spent_micros,
    );
    // `wake.list` names the task, and its reply would go where it reports.
    let listed = l.core.wakes(None, None).unwrap();
    assert_eq!(listed.len(), 1);
    let short = crate::task::short(&task.session_id);
    assert_eq!(listed[0].task.as_deref(), Some(short.as_str()));
    assert_eq!(listed[0].session_id, task.session_id);
    assert_eq!(listed[0].target.as_deref(), Some(TARGET));
    assert_eq!(
        l.core.wakes(None, Some(TARGET)).unwrap(),
        listed,
        "the place's `/wakes` lists it"
    );
    // The wake's turn continues the task, and it reports.
    until("the task's report", 10, || {
        exec(&l.core, &task.id).state == ExecState::Complete
    })
    .await;
    assert!(
        t0.elapsed() >= Duration::from_millis(1_900),
        "not before its time: {:?}",
        t0.elapsed()
    );
    let done = exec(&l.core, &task.id);
    assert_eq!(done.turns, 2, "the first turn, and the wake's");
    assert_eq!(done.ended_reason.as_deref(), Some("reported"));
    assert!(done.wakes.is_empty());
    let nodes = wake_nodes(&l.core, &task);
    assert_eq!(nodes.len(), 1);
    assert!(
        nodes[0].ends_with("check the build again, then report"),
        "{nodes:?}"
    );
    let posted = reports(&l.core);
    assert_eq!(posted.len(), 1, "one report");
    assert_eq!(posted[0]["outcome"], "complete");
    assert_eq!(posted[0]["turns"], 2);
    // The wake's turn spent from the task's carve, and the parent's spend
    // counts it.
    let more = done.budget.spent_micros - task_spent;
    assert!(more > 0, "the wake's turn costs");
    assert!(
        exec(&l.core, &pid).budget.spent_micros - parent_spent >= more,
        "the parent's spend holds the task's"
    );
    assert!(done.budget.limit_micros <= theseus_kernel::usd_to_micros(2.0));
    // The report's wake fires once, at the real end, and the parent reads
    // the wake's turn's words as the report.
    until("the report's parent turn", 10, || {
        let p = exec(&l.core, &pid);
        p.turns == 2 && p.state == ExecState::Waiting
    })
    .await;
    let read = report_texts(&l.core, &parent);
    assert_eq!(read.len(), 1, "{read:?}");
    assert!(
        read[0].contains("Checked again: the build is green."),
        "{read:?}"
    );
    assert_eq!(rows(&l.core, "task.report_wake").len(), 1);
    assert_eq!(rows(&l.core, "wake.fired").len(), 1);
    tokio::time::sleep(Duration::from_millis(1_000)).await;
    assert_eq!(reports(&l.core).len(), 1, "still one report");
    assert_eq!(exec(&l.core, &pid).turns, 2, "one parent turn");
    assert_eq!(exec(&l.core, &task.id).turns, 2);
    assert!(l.core.wakes(None, None).unwrap().is_empty());
    l.stop().await;
}

/// The issue's second test, with the cap: a task holds 5 wakes at most (the
/// sixth is refused, readably), parks on them, and a cancel of the task drops
/// all five in its frame, freeing their slots; none fires, and it reports
/// once, as cancelled.
#[tokio::test]
async fn a_cancelled_tasks_wakes_never_fire_and_the_cancel_frees_their_slots() {
    let dir = tempfile::tempdir().unwrap();
    let l = life(dir.path());
    let (_, task) = parked(&l.core, "CHILD SIX").await;
    assert_eq!(task.wakes.len(), 5, "the cap");
    let refused = results(&l.core, &task)
        .into_iter()
        .filter(|r| r.contains("already has 5 pending wakes"))
        .count();
    assert_eq!(refused, 1, "the sixth is refused");
    assert_eq!(l.core.wakes(None, None).unwrap().len(), 5);
    l.core
        .task_cancel_by(&task.session_id, "discord:operator")
        .await
        .unwrap();
    let e = exec(&l.core, &task.id);
    assert_eq!(e.state, ExecState::Cancelled);
    assert!(e.wakes.is_empty(), "the cancel drops its wakes");
    assert!(l.core.wakes(None, None).unwrap().is_empty(), "slots freed");
    let dropped = rows(&l.core, "wake.cancelled");
    assert_eq!(dropped.len(), 5);
    assert!(dropped
        .iter()
        .all(|r| r["why"] == "the execution was cancelled"));
    let posted = reports(&l.core);
    assert_eq!(posted.len(), 1);
    assert_eq!(posted[0]["outcome"], "cancelled");
    // The first was due an hour out; move nothing, and wait past a tick or
    // two: nothing fires, and the task takes no turn.
    tokio::time::sleep(Duration::from_millis(1_500)).await;
    assert!(rows(&l.core, "wake.fired").is_empty());
    assert_eq!(exec(&l.core, &task.id).turns, 1);
    assert_eq!(reports(&l.core).len(), 1, "one report");
    assert!(
        rows(&l.core, "task.report_wake").is_empty(),
        "a cancelled task wakes nothing"
    );
    l.stop().await;
}

/// A cancelled task's due wake never fires: the cancel lands before its
/// time, and past its time nothing runs.
#[tokio::test]
async fn a_cancelled_tasks_wake_never_fires_past_its_time() {
    let dir = tempfile::tempdir().unwrap();
    let l = life(dir.path());
    let (_, task) = parked(&l.core, "CHILD AGAIN 2s: check the build").await;
    l.core
        .task_cancel_by(&task.session_id, "discord:operator")
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_millis(3_000)).await;
    assert!(rows(&l.core, "wake.fired").is_empty(), "nothing fired");
    assert!(wake_nodes(&l.core, &task).is_empty());
    let e = exec(&l.core, &task.id);
    assert_eq!((e.state, e.turns), (ExecState::Cancelled, 1));
    assert_eq!(reports(&l.core).len(), 1);
    l.stop().await;
}

/// The operator cancels a parked task's only wake: the task goes on at once,
/// finds nothing new, ends, and reports once, with what it last said; its
/// report's wake fires once. A task is never left waiting with no wake.
#[tokio::test]
async fn a_cancel_of_a_parked_tasks_last_wake_ends_it_and_it_reports_once() {
    let dir = tempfile::tempdir().unwrap();
    let l = life(dir.path());
    let (parent, task) = parked(&l.core, "CHILD AGAIN 1h: check the build").await;
    let short = crate::task::short(&task.wakes[0].id);
    l.core.wake_cancel_by(&short, "the CLI").unwrap();
    until("the task's report", 10, || {
        exec(&l.core, &task.id).state == ExecState::Complete
    })
    .await;
    let pid = task.parent.clone().unwrap();
    until("the report's parent turn", 10, || {
        let p = exec(&l.core, &pid);
        p.turns == 2 && p.state == ExecState::Waiting
    })
    .await;
    tokio::time::sleep(Duration::from_millis(600)).await;
    let done = exec(&l.core, &task.id);
    assert_eq!(done.turns, 2, "the first turn, and the one that ended it");
    assert!(
        wake_nodes(&l.core, &task).is_empty(),
        "the wake never fired"
    );
    assert!(rows(&l.core, "wake.fired").is_empty());
    assert!(rows(&l.core, "execution.queued")
        .iter()
        .any(|q| q["why"] == "wake_cancelled" && q["execution_id"] == task.id));
    assert_eq!(reports(&l.core).len(), 1, "one report");
    assert_eq!(rows(&l.core, "task.report_wake").len(), 1);
    let read = report_texts(&l.core, &parent);
    assert_eq!(read.len(), 1);
    assert!(
        read[0].contains("Checked once: still building."),
        "{read:?}"
    );
    l.stop().await;
}

/// A repeating wake in a task is refused, in words a model can act on, and
/// a one-shot one in the same turn is set: the task parks on it.
#[tokio::test]
async fn a_repeating_wake_in_a_task_is_refused_and_a_one_shot_one_is_not() {
    let dir = tempfile::tempdir().unwrap();
    let l = life(dir.path());
    let (_, task) = parked(&l.core, "CHILD REPEAT").await;
    let rs = results(&l.core, &task);
    assert_eq!(rs.len(), 2, "{rs:?}");
    let series = rs.iter().find(|r| r.contains("repeating")).expect("{rs:?}");
    assert!(series.starts_with("Refused: "), "{series}");
    assert!(series.contains("a task must end"), "{series}");
    assert!(series.contains("Set a one-shot wake instead"), "{series}");
    assert!(
        rs.iter().any(|r| r.starts_with("Set wake ")),
        "the one-shot one is set: {rs:?}"
    );
    assert_eq!(task.wakes.len(), 1);
    assert!(task.wakes[0].repeat.is_none());
    assert_eq!(rows(&l.core, "wake.set").len(), 1);
    assert!(reports(&l.core).is_empty(), "parked, not reported");
    l.core
        .task_cancel_by(&task.session_id, "discord:operator")
        .await
        .unwrap();
    l.stop().await;
}

/// A task with no wake ends and reports at its first turn, as before 37b.
#[tokio::test]
async fn a_task_with_no_wake_reports_at_once() {
    let dir = tempfile::tempdir().unwrap();
    let l = life(dir.path());
    let parent = parent_session(&l.core);
    turn(&l.core, &parent, "START CHILD PLAIN").await;
    let task = only_task(&l.core, &parent);
    until("the task's report", 10, || {
        exec(&l.core, &task.id).state == ExecState::Complete
    })
    .await;
    assert_eq!(exec(&l.core, &task.id).turns, 1);
    assert_eq!(reports(&l.core).len(), 1);
    assert!(rows(&l.core, "wake.set").is_empty());
    l.stop().await;
}

/// A restart while a task is parked on its wake: the next life's driver
/// fires the wake once, the task's turn continues it, and it reports once,
/// with its report's wake firing once.
#[tokio::test]
async fn a_restart_while_a_task_is_parked_fires_its_wake_once_and_reports_once() {
    let dir = tempfile::tempdir().unwrap();
    let l = life(dir.path());
    let (_, task) = parked(&l.core, "CHILD AGAIN 3s: check the build").await;
    let first_life_requests = l.model.requests.lock().unwrap().len();
    l.stop().await;
    let l = life(dir.path());
    until("the task's report", 15, || {
        exec(&l.core, &task.id).state == ExecState::Complete
    })
    .await;
    let pid = task.parent.clone().unwrap();
    until("the report's parent turn", 10, || {
        let p = exec(&l.core, &pid);
        p.turns == 2 && p.state == ExecState::Waiting
    })
    .await;
    tokio::time::sleep(Duration::from_millis(1_000)).await;
    assert_eq!(wake_nodes(&l.core, &task).len(), 1, "fired once");
    assert_eq!(rows(&l.core, "wake.fired").len(), 1);
    assert_eq!(reports(&l.core).len(), 1, "reported once");
    assert_eq!(rows(&l.core, "task.report_wake").len(), 1);
    assert_eq!(exec(&l.core, &task.id).turns, 2);
    assert_eq!(exec(&l.core, &pid).turns, 2);
    assert!(
        first_life_requests >= 3,
        "the parent's two calls and the task's first"
    );
    l.stop().await;
}
