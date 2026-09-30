//! Task sessions through the whole core (DD7, theseus-qn2): `task.create`
//! returns at once, the child runs under the driver while the parent answers,
//! its report lands once in the outbox and once in the parent's session, the
//! budget is carved and counted, depth two is refused, and a cancel reports
//! once.

use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde_json::{json, Value};
use theseus_kernel::{ExecState, Execution, Wake};
use theseus_protocol::{SessionKind, TurnSubmitResult};

use crate::bus::EventSink;
use crate::node::{Body, Node, ResultStatus};
use crate::policy::Posture;
use crate::provider::{
    DeltaSink, FakeProvider, Provider, ProviderFuture, ProviderRequest, Scripted,
};
use crate::session::SessionRecord;
use crate::store::Store;
use crate::turn::TurnRequest;
use crate::{Config, Core};

/// The place the parent sessions post to.
const PLACE: &str = "dm:42";
const TARGET: &str = "discord:dm:42";

/// A stand-in model for task scenarios. `script` answers each request from
/// the request itself. A call whose first user message contains `hold` says
/// it began and waits at the gate, once per permit.
struct Model {
    script: Box<dyn Fn(&ProviderRequest) -> Scripted + Send + Sync>,
    requests: Mutex<Vec<ProviderRequest>>,
    hold: Mutex<Option<String>>,
    entered: tokio::sync::mpsc::UnboundedSender<String>,
    gate: tokio::sync::Semaphore,
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
            let first = first_user(req);
            let hold = self.hold.lock().unwrap().clone();
            if hold.is_some_and(|h| first.contains(&h)) {
                let _ = self.entered.send(first.clone());
                self.gate.acquire().await.unwrap().forget();
            }
            let answer = (self.script)(req);
            let f = FakeProvider::scripted(vec![answer]);
            f.stream_message(req, on_delta).await
        })
    }
}

/// A message's text blocks, joined.
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

/// Whether the request's last message answers a tool call.
fn answers_a_call(req: &ProviderRequest) -> bool {
    req.messages
        .last()
        .and_then(|m| m["content"].as_array())
        .is_some_and(|c| c.iter().any(|b| b["type"] == "tool_result"))
}

struct Rig {
    core: Arc<Core>,
    model: Arc<Model>,
    entered: tokio::sync::mpsc::UnboundedReceiver<String>,
    _dir: tempfile::TempDir,
}

fn rig(
    script: impl Fn(&ProviderRequest) -> Scripted + Send + Sync + 'static,
    tweak: impl FnOnce(&mut Config),
) -> Rig {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("work");
    std::fs::create_dir_all(&root).unwrap();
    let mut cfg = config(&root.canonicalize().unwrap(), dir.path());
    tweak(&mut cfg);
    let store = Store::open(&dir.path().join("store")).unwrap();
    let (tx, entered) = tokio::sync::mpsc::unbounded_channel();
    let model = Arc::new(Model {
        script: Box::new(script),
        requests: Mutex::default(),
        hold: Mutex::default(),
        entered: tx,
        gate: tokio::sync::Semaphore::new(0),
    });
    let core = Core::build(crate::rpc::Parts::for_tests(cfg, model.clone(), store)).unwrap();
    // The driver takes every task's turns, as the daemon's does.
    tokio::spawn(crate::harness::drive(core.clone()));
    Rig {
        core,
        model,
        entered,
        _dir: dir,
    }
}

fn config(root: &Path, state: &Path) -> Config {
    let mut cfg = Config::example();
    cfg.server.state_dir = state.to_string_lossy().into_owned();
    cfg.tools.projects_dir = Some(root.to_string_lossy().into_owned());
    cfg.tools.roots = vec![];
    cfg.policy.enforcement = Posture::Notify;
    cfg
}

/// A conversation session that posts to `PLACE`.
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
            config_wait_us: 0,
            reply_to: None,
        })
        .await
        .unwrap()
}

fn start(brief: &str, budget_usd: Option<f64>) -> Scripted {
    let mut input = json!({ "brief": brief });
    if let Some(b) = budget_usd {
        input["budget_usd"] = json!(b);
    }
    Scripted::tools("Starting it.", &[("t_task", "task_create", input)])
}

/// The one task of `parent`'s execution.
fn only_task(core: &Core, parent_sid: &str) -> Execution {
    let rec: SessionRecord = core.store.get_session(parent_sid).unwrap().unwrap();
    let tasks = core.kernel.tasks(rec.execution_id.as_deref()).unwrap();
    assert_eq!(tasks.len(), 1, "{tasks:?}");
    tasks.into_iter().next().unwrap()
}

async fn until(what: &str, mut f: impl FnMut() -> bool) {
    let t0 = Instant::now();
    while !f() {
        assert!(t0.elapsed() < Duration::from_secs(20), "no {what} in 20 s");
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

fn exec(core: &Core, id: &str) -> Execution {
    core.kernel.execution(id).unwrap().unwrap()
}

/// The outbox's posts of one kind.
fn posts(core: &Core, kind: &str) -> Vec<theseus_kernel::Action> {
    core.kernel
        .outbox_actions()
        .unwrap()
        .into_iter()
        .filter(|a| crate::outbox::kind_of(a) == kind)
        .collect()
}

/// The report nodes in a session.
fn report_nodes(core: &Core, sid: &str) -> Vec<Node> {
    core.store
        .session_nodes(sid)
        .unwrap()
        .into_iter()
        .map(|(_, n)| n)
        .filter(|n| n.author.as_deref().is_some_and(|a| a.starts_with("task:")))
        .collect()
}

/// The whole path: a parent turn starts a task and ends at once; a second
/// parent turn is answered while the child runs; the child's report lands
/// once in the outbox and, at the parent's next turn, once in its session,
/// which the model reads; the child's own turns post nothing.
#[tokio::test]
async fn a_task_runs_on_its_own_and_reports_once_to_the_place_and_the_parent() {
    let mut r = rig(
        |req| {
            let (first, last) = (first_user(req), last_user(req));
            if first.contains("CHILD: count") {
                return Scripted::text("One, two, three. Counted to three.");
            }
            if answers_a_call(req) {
                return Scripted::text("Started it; it reports here.");
            }
            match last.as_str() {
                l if l.starts_with("START") => start("CHILD: count to three", Some(1.5)),
                l if l.starts_with("SECOND") => Scripted::text("Four."),
                _ => Scripted::text("It counted to three."),
            }
        },
        |_| {},
    );
    *r.model.hold.lock().unwrap() = Some("CHILD".into());
    let parent = parent_session(&r.core);
    let t0 = Instant::now();
    let first = turn(&r.core, &parent, "START a task").await;
    assert!(first.output.contains("Started it"), "{}", first.output);
    let task = only_task(&r.core, &parent);
    assert_eq!(task.budget.limit_micros, 1_500_000);
    // The child's first call begins, and waits at the gate.
    let brief = r.entered.recv().await.unwrap();
    assert!(brief.contains("You cannot start tasks"), "{brief}");
    assert!(brief.ends_with("CHILD: count to three"), "{brief}");
    let second = turn(&r.core, &parent, "SECOND what is 2 + 2?").await;
    assert_eq!(second.output, "Four.");
    assert_eq!(
        exec(&r.core, &task.id).state,
        ExecState::Running,
        "the second answer came while the task ran"
    );
    assert!(
        t0.elapsed() < Duration::from_secs(10),
        "neither parent turn waited for the task"
    );
    r.model.gate.add_permits(1);
    until("task complete", || {
        exec(&r.core, &task.id).state == ExecState::Complete
    })
    .await;
    // Once through the outbox, naming the task, and nothing else from it.
    let reports = posts(&r.core, "report");
    assert_eq!(reports.len(), 1, "one report");
    let body = crate::outbox::body_of(&reports[0]);
    assert_eq!(crate::outbox::target_of(&reports[0]), TARGET);
    assert_eq!(body["task"], task.session_id);
    assert_eq!(body["outcome"], "complete");
    assert_eq!(
        r.core
            .outbox
            .said(body["node"].as_str().unwrap())
            .as_deref(),
        Some("One, two, three. Counted to three.")
    );
    assert!(
        posts(&r.core, "reply")
            .iter()
            .all(|a| a.session_id != task.session_id),
        "the task's turns post no replies"
    );
    let pe = exec(&r.core, &task.parent.clone().unwrap());
    assert_eq!(pe.reports, vec![task.id.clone()]);
    assert!(
        report_nodes(&r.core, &parent).is_empty(),
        "nothing is written into the parent until its next turn"
    );
    // Nothing started a parent turn.
    assert_eq!(pe.state, ExecState::Waiting);
    // The parent's next turn reads it, once.
    let third = turn(&r.core, &parent, "THIRD what did the task find?").await;
    assert_eq!(third.output, "It counted to three.");
    let req = r.model.requests.lock().unwrap().last().cloned().unwrap();
    let seen = serde_json::to_string(&req.messages).unwrap();
    assert!(seen.contains("[Report from task"), "{seen}");
    assert!(
        seen.contains("One, two, three. Counted to three."),
        "{seen}"
    );
    let nodes = report_nodes(&r.core, &parent);
    assert_eq!(nodes.len(), 1);
    assert!(exec(&r.core, &pe.id).reports.is_empty());
    turn(&r.core, &parent, "FOURTH anything else?").await;
    assert_eq!(report_nodes(&r.core, &parent).len(), 1, "read once");
    assert_eq!(posts(&r.core, "report").len(), 1, "posted once");
}

/// The budget: the child's limit is carved from the parent, which reserves
/// it; the child's cost is the parent's spend too; and the child stops at its
/// own limit and asks, with a card that names it, though its parent has more
/// left.
#[tokio::test]
async fn a_tasks_spend_counts_against_its_parent_and_it_asks_at_its_carve() {
    let mut r = rig(
        |req| {
            let first = first_user(req);
            if first.contains("CHILD: diff") {
                // About $0.30 out, and a second call that needs $1.28 more.
                return Scripted::tools(
                    &"word ".repeat(30_000),
                    &[("t1", "text_diff", json!({"a": "x\n", "b": "y\n"}))],
                );
            }
            if answers_a_call(req) {
                return Scripted::text("Started.");
            }
            start("CHILD: diff two lines", Some(1.40))
        },
        |_| {},
    );
    *r.model.hold.lock().unwrap() = Some("CHILD".into());
    let parent = parent_session(&r.core);
    turn(&r.core, &parent, "START").await;
    let task = only_task(&r.core, &parent);
    let pid = task.parent.clone().unwrap();
    r.entered.recv().await.unwrap();
    let before = exec(&r.core, &pid);
    assert_eq!(
        before
            .budget
            .reservations
            .get(&theseus_kernel::carve_key(&task.id)),
        Some(&1_400_000),
        "the parent reserves the carve"
    );
    r.model.gate.add_permits(1);
    until("the task waits on its budget", || {
        matches!(exec(&r.core, &task.id).wake, Some(Wake::Budget { .. }))
    })
    .await;
    let (t, p) = (exec(&r.core, &task.id), exec(&r.core, &pid));
    assert!(t.budget.spent_micros > 200_000, "{:?}", t.budget);
    assert_eq!(
        p.budget.spent_micros,
        before.budget.spent_micros + t.budget.spent_micros,
        "the parent's spend includes the child's"
    );
    assert_eq!(
        p.budget.reservations[&theseus_kernel::carve_key(&task.id)],
        1_400_000 - t.budget.spent_micros,
        "the carve shrank by what the child spent"
    );
    // It asks, where the parent's questions go, and the card names it.
    let q = t.budget.question.expect("a budget question");
    let cards = posts(&r.core, "card");
    let card = cards
        .iter()
        .find(|c| crate::outbox::body_of(c)["question"] == q.as_str())
        .expect("the question's card");
    assert_eq!(crate::outbox::target_of(card), TARGET);
    let asked = r
        .core
        .question_request(&r.core.kernel.action(&q).unwrap().unwrap(), None)
        .unwrap()
        .unwrap();
    assert_eq!(
        asked.task.as_ref().map(|t| t.short.as_str()),
        Some(crate::task::short(&task.session_id).as_str())
    );
    assert!(asked.reason.starts_with("Task "), "{}", asked.reason);
    assert!(p.budget.available() > 1_400_000, "the parent had more left");
}

/// A request for more than the parent has left is capped at what it has
/// left, and the result says so.
#[tokio::test]
async fn a_request_for_more_than_the_parent_has_left_is_capped() {
    let r = rig(
        |req| {
            if first_user(req).contains("CHILD") {
                return Scripted::text("done");
            }
            if answers_a_call(req) {
                return Scripted::text("Started.");
            }
            start("CHILD: anything", Some(1_000_000.0))
        },
        |c| c.kernel.spend_limit_usd = 20.0,
    );
    let parent = parent_session(&r.core);
    turn(&r.core, &parent, "START").await;
    let task = only_task(&r.core, &parent);
    let result = r
        .core
        .store
        .session_nodes(&parent)
        .unwrap()
        .into_iter()
        .find_map(|(_, n)| match n.body {
            Body::ToolResult {
                tool,
                content,
                meta,
                ..
            } if tool == "task.create" => Some((content, meta)),
            _ => None,
        })
        .unwrap();
    assert_eq!(result.1["capped"], true, "{}", result.0);
    assert!(result.0.contains("you asked for $1000000"), "{}", result.0);
    let left = result.1["left_usd"].as_f64().unwrap();
    assert!(left > 18.0 && left < 20.0, "{left}");
    assert_eq!(
        task.budget.limit_micros,
        theseus_kernel::usd_to_micros(left),
        "capped at what the parent had left"
    );
}

/// Depth one: a task's `task.create` is refused, it says why, and no
/// grandchild is opened; the task finishes and reports as usual.
#[tokio::test]
async fn a_task_cannot_start_tasks() {
    let r = rig(
        |req| {
            let first = first_user(req);
            if first.contains("CHILD: nest") {
                if answers_a_call(req) {
                    return Scripted::text("I could not start a subtask.");
                }
                return start("GRANDCHILD", None);
            }
            if answers_a_call(req) {
                return Scripted::text("Started.");
            }
            start("CHILD: nest a task", None)
        },
        |_| {},
    );
    let parent = parent_session(&r.core);
    turn(&r.core, &parent, "START").await;
    let task = only_task(&r.core, &parent);
    until("task complete", || {
        exec(&r.core, &task.id).state == ExecState::Complete
    })
    .await;
    let refused = r
        .core
        .store
        .session_nodes(&task.session_id)
        .unwrap()
        .into_iter()
        .find_map(|(_, n)| match n.body {
            Body::ToolResult {
                tool,
                status,
                content,
                ..
            } if tool == "task.create" => Some((status, content)),
            _ => None,
        })
        .unwrap();
    assert_eq!(refused.0, ResultStatus::Error);
    assert!(refused.1.contains("depth one"), "{}", refused.1);
    assert!(
        r.core.kernel.tasks(Some(&task.id)).unwrap().is_empty(),
        "no grandchild"
    );
    assert_eq!(r.core.kernel.tasks(None).unwrap().len(), 1);
    assert_eq!(posts(&r.core, "report").len(), 1);
}

/// A cancel stops the child while its call runs, and reports "cancelled" to
/// the place once: a second cancel says nothing, and the turn that was
/// running writes no report of its own when it ends.
#[tokio::test]
async fn a_cancel_stops_the_task_and_reports_once() {
    let mut r = rig(
        |req| {
            if first_user(req).contains("CHILD") {
                return Scripted::text("finished anyway");
            }
            if answers_a_call(req) {
                return Scripted::text("Started.");
            }
            start("CHILD: wait", None)
        },
        |_| {},
    );
    *r.model.hold.lock().unwrap() = Some("CHILD".into());
    let parent = parent_session(&r.core);
    turn(&r.core, &parent, "START").await;
    let task = only_task(&r.core, &parent);
    r.entered.recv().await.unwrap();
    let res = r
        .core
        .task_cancel_by(&crate::task::short(&task.session_id), "discord:eddie")
        .unwrap();
    assert_eq!(res.task.state, "cancelled");
    let again = r
        .core
        .task_cancel_by(&task.session_id, "discord:eddie")
        .unwrap();
    assert_eq!(again.task.state, "cancelled");
    r.model.gate.add_permits(1);
    // The held turn goes on, finds the task cancelled, and ends.
    until("the held turn ended", || !r.core.kernel.is_held(&task.id)).await;
    tokio::time::sleep(Duration::from_millis(100)).await;
    let reports = posts(&r.core, "report");
    assert_eq!(reports.len(), 1, "reported once");
    let body = crate::outbox::body_of(&reports[0]);
    assert_eq!(body["outcome"], "cancelled");
    assert_eq!(body["reason"], "cancelled by discord:eddie");
    let t = exec(&r.core, &task.id);
    assert_eq!(t.state, ExecState::Cancelled);
    let p = exec(&r.core, &task.parent.clone().unwrap());
    assert_eq!(p.reports, vec![task.id.clone()]);
    assert!(
        !p.budget
            .reservations
            .contains_key(&theseus_kernel::carve_key(&task.id)),
        "the carve is released once the cancelled call settles"
    );
    // The parent reads it next turn.
    turn(&r.core, &parent, "what happened?").await;
    let nodes = report_nodes(&r.core, &parent);
    assert_eq!(nodes.len(), 1);
    match &nodes[0].body {
        Body::UserMessage { text, .. } => {
            assert!(text.contains("cancelled by discord:eddie"), "{text}")
        }
        other => panic!("{other:?}"),
    }
}
