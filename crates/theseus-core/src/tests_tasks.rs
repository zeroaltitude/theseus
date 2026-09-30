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

/// `task.create { brief, wake_parent: true }` (W1), with a budget.
fn start_waking(briefs: &[&str], budget_usd: f64) -> Scripted {
    let calls: Vec<(String, &str, Value)> = briefs
        .iter()
        .enumerate()
        .map(|(i, b)| {
            (
                format!("t_task{i}"),
                "task_create",
                json!({"brief": b, "budget_usd": budget_usd, "wake_parent": true}),
            )
        })
        .collect();
    let calls: Vec<(&str, &str, Value)> = calls
        .iter()
        .map(|(id, n, v)| (id.as_str(), *n, v.clone()))
        .collect();
    Scripted::tools("Starting.", &calls)
}

/// The parent's execution, by its session.
fn parent_exec(core: &Core, sid: &str) -> Execution {
    let rec: SessionRecord = core.store.get_session(sid).unwrap().unwrap();
    exec(core, rec.execution_id.as_deref().unwrap())
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
    // Nothing started a parent turn: the task asked for none (W1), and the
    // driver's ticks since took none.
    assert_eq!(pe.state, ExecState::Waiting);
    tokio::time::sleep(Duration::from_millis(600)).await;
    let pe = exec(&r.core, &pe.id);
    assert_eq!((pe.state, pe.turns), (ExecState::Waiting, 2));
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

// ---------------------------------------------------------------- the report's wake (W1, theseus-lji)

/// The ledger's rows of one kind, oldest first.
fn rows(core: &Core, kind: &str) -> Vec<Value> {
    core.store
        .ledger_tail::<crate::ledger::LedgerRow>(100_000)
        .unwrap()
        .into_iter()
        .filter(|(_, r)| r.is_kind(kind))
        .map(|(_, r)| r.data)
        .collect()
}

/// A task opened with `wake_parent` starts one parent turn when it reports:
/// the turn runs by itself, its input is the report, and its reply, posted
/// where the parent posts, says which report started it. No turn follows.
#[tokio::test]
async fn a_task_with_wake_parent_starts_one_parent_turn_that_reads_its_report() {
    let r = rig(
        |req| {
            let (first, last) = (first_user(req), last_user(req));
            if first.contains("CHILD: count") {
                return Scripted::text("One, two, three. Counted to three.");
            }
            if answers_a_call(req) {
                return Scripted::text("Started it; its report starts my next turn.");
            }
            if last.starts_with("START") {
                return start_waking(&["CHILD: count to three"], 1.5);
            }
            Scripted::text("The task counted to three; the next step is four.")
        },
        |_| {},
    );
    let parent = parent_session(&r.core);
    turn(&r.core, &parent, "START a chain").await;
    let task = only_task(&r.core, &parent);
    assert!(task.wake_parent);
    let pid = task.parent.clone().unwrap();
    until("the report's turn ended", || {
        let p = exec(&r.core, &pid);
        p.turns == 2 && p.state == ExecState::Waiting
    })
    .await;
    // Its input was the report.
    let req = r.model.requests.lock().unwrap().last().cloned().unwrap();
    let last = last_user(&req);
    assert!(last.contains("[Report from task"), "{last}");
    assert!(
        last.contains("One, two, three. Counted to three."),
        "{last}"
    );
    assert_eq!(report_nodes(&r.core, &parent).len(), 1);
    // Its reply goes where the parent posts, under the report's line.
    let replies: Vec<_> = posts(&r.core, "reply")
        .into_iter()
        .filter(|a| a.session_id == parent)
        .collect();
    assert_eq!(replies.len(), 2, "the first turn's, and the report's");
    assert_eq!(crate::outbox::target_of(&replies[1]), TARGET);
    let body = crate::outbox::body_of(&replies[1]);
    let short = crate::task::short(&task.session_id);
    assert_eq!(
        body["reports"][0]["text"],
        format!("📋 task {short} reported")
    );
    assert_eq!(body["result"]["continuation"], true);
    assert!(crate::outbox::body_of(&replies[0]).get("reports").is_none());
    // The ledger names the report's wake, with the task and the parent.
    let wake = &rows(&r.core, "task.report_wake")[0];
    assert_eq!(wake["task"], task.id);
    assert_eq!(wake["execution_id"], pid);
    assert!(rows(&r.core, "execution.queued")
        .iter()
        .any(|q| q["why"] == "report" && q["execution_id"] == pid));
    assert_eq!(rows(&r.core, "task.reports_read")[0]["woke"][0], task.id);
    // One turn, and none follows.
    tokio::time::sleep(Duration::from_millis(600)).await;
    assert_eq!(exec(&r.core, &pid).turns, 2);
}

/// Two reports that land together start one parent turn, which reads both.
/// The parent is busy when they land, and the frame that ends its turn
/// queues it once.
#[tokio::test]
async fn two_reports_that_land_together_start_one_parent_turn() {
    let mut r = rig(
        |req| {
            let (first, last) = (first_user(req), last_user(req));
            if first.contains("CHILD: one") {
                return Scripted::text("Report one.");
            }
            if first.contains("CHILD: two") {
                return Scripted::text("Report two.");
            }
            if answers_a_call(req) {
                return Scripted::text("Started both.");
            }
            if last.starts_with("PARENT") {
                return start_waking(&["CHILD: one", "CHILD: two"], 1.5);
            }
            Scripted::text("Both reported.")
        },
        |_| {},
    );
    *r.model.hold.lock().unwrap() = Some("PARENT".into());
    let parent = parent_session(&r.core);
    r.model.gate.add_permits(1);
    let (core, sid) = (r.core.clone(), parent.clone());
    let first = tokio::spawn(async move { turn(&core, &sid, "PARENT start two tasks").await });
    // Its first call passes; its second, which reads the two ids, waits.
    r.entered.recv().await.unwrap();
    r.entered.recv().await.unwrap();
    let pid = parent_exec(&r.core, &parent).id;
    let tasks = r.core.kernel.tasks(Some(&pid)).unwrap();
    assert_eq!(tasks.len(), 2);
    until("both tasks complete", || {
        tasks
            .iter()
            .all(|t| exec(&r.core, &t.id).state == ExecState::Complete)
    })
    .await;
    let busy = exec(&r.core, &pid);
    assert_eq!(busy.state, ExecState::Running);
    assert_eq!(
        busy.report_wakes.len(),
        2,
        "the busy parent keeps both asks"
    );
    r.model.gate.add_permits(1);
    first.await.unwrap();
    // The one report turn: its call waits at the gate too.
    r.entered.recv().await.unwrap();
    r.model.gate.add_permits(1);
    until("the report's turn ended", || {
        let p = exec(&r.core, &pid);
        p.turns == 2 && p.state == ExecState::Waiting
    })
    .await;
    let req = r.model.requests.lock().unwrap().last().cloned().unwrap();
    let last = last_user(&req);
    assert!(
        last.contains("Report one.") && last.contains("Report two."),
        "{last}"
    );
    assert_eq!(report_nodes(&r.core, &parent).len(), 2);
    tokio::time::sleep(Duration::from_millis(600)).await;
    assert_eq!(exec(&r.core, &pid).turns, 2, "one turn read both");
}

/// A parent at its limit, when a report starts its turn, asks as any turn
/// does: the turn reads the report, and its call waits on the budget.
#[tokio::test]
async fn a_report_that_starts_a_turn_at_the_parents_limit_asks() {
    let mut r = rig(
        |req| {
            let (first, last) = (first_user(req), last_user(req));
            if first.contains("CHILD: spend") {
                // About $1.45 out, which the parent's spend counts.
                return Scripted::text(&"word ".repeat(145_000));
            }
            if answers_a_call(req) {
                return Scripted::text("Started.");
            }
            if last.starts_with("START") {
                return start_waking(&["CHILD: spend"], 1.35);
            }
            Scripted::text("this call does not fit")
        },
        |c| c.kernel.spend_limit_usd = 2.70,
    );
    *r.model.hold.lock().unwrap() = Some("CHILD".into());
    let parent = parent_session(&r.core);
    turn(&r.core, &parent, "START").await;
    let task = only_task(&r.core, &parent);
    let pid = task.parent.clone().unwrap();
    r.entered.recv().await.unwrap();
    r.model.gate.add_permits(1);
    until("the parent waits on its budget", || {
        matches!(exec(&r.core, &pid).wake, Some(Wake::Budget { .. }))
    })
    .await;
    let p = exec(&r.core, &pid);
    assert_eq!(p.turns, 2, "the report's turn ran");
    assert_eq!(
        report_nodes(&r.core, &parent).len(),
        1,
        "it read the report first"
    );
    let q = p.budget.question.expect("a budget question");
    assert!(posts(&r.core, "card")
        .iter()
        .any(|c| crate::outbox::body_of(c)["question"] == q.as_str()));
    assert!(
        r.model
            .requests
            .lock()
            .unwrap()
            .iter()
            .all(|q| !last_user(q).contains("[Report from task")),
        "the call that did not fit was not made"
    );
}

/// A cancelled task wakes nothing: its report reaches the place and, at the
/// parent's next turn, the parent; no turn starts for it.
#[tokio::test]
async fn a_cancelled_task_with_wake_parent_wakes_nothing() {
    let mut r = rig(
        |req| {
            if first_user(req).contains("CHILD") {
                return Scripted::text("finished anyway");
            }
            if answers_a_call(req) {
                return Scripted::text("Started.");
            }
            start_waking(&["CHILD: wait"], 1.5)
        },
        |_| {},
    );
    *r.model.hold.lock().unwrap() = Some("CHILD".into());
    let parent = parent_session(&r.core);
    turn(&r.core, &parent, "START").await;
    let task = only_task(&r.core, &parent);
    r.entered.recv().await.unwrap();
    r.core
        .task_cancel_by(&task.session_id, "discord:eddie")
        .unwrap();
    r.model.gate.add_permits(1);
    until("the held turn ended", || !r.core.kernel.is_held(&task.id)).await;
    tokio::time::sleep(Duration::from_millis(600)).await;
    let p = exec(&r.core, &task.parent.clone().unwrap());
    assert_eq!(
        (p.state, p.turns),
        (ExecState::Waiting, 1),
        "no turn started"
    );
    assert_eq!(p.reports, vec![task.id.clone()]);
    assert!(p.report_wakes.is_empty());
    assert!(rows(&r.core, "task.report_wake").is_empty());
    assert_eq!(posts(&r.core, "report").len(), 1, "the place heard it");
}

// ---------------------------------------------------------------- `/stop` keeps the conversation (W1)

/// `/stop` while the model answers: the turn keeps the answer, runs none of
/// its calls, posts no reply, and parks the same execution on input. The
/// next message continues the same session, and its history is compiled.
#[tokio::test]
async fn a_stop_halts_the_turn_and_the_next_message_continues_the_session() {
    let mut r = rig(
        |req| {
            if last_user(req).starts_with("SECOND") {
                return Scripted::text("You stopped me before the file was written.");
            }
            Scripted::tools(
                "I will write it.",
                &[("w1", "fs_write", json!({"path": "a.txt", "content": "x\n"}))],
            )
        },
        |_| {},
    );
    *r.model.hold.lock().unwrap() = Some("FIRST".into());
    let sid = parent_session(&r.core);
    let (core, s) = (r.core.clone(), sid.clone());
    let first = tokio::spawn(async move { turn(&core, &s, "FIRST write a file").await });
    r.entered.recv().await.unwrap();
    let eid = parent_exec(&r.core, &sid).id;
    let stop = r.core.stop_execution(&eid, "discord:eddie").unwrap();
    assert!(stop.stopped && stop.turn_running, "{stop:?}");
    assert!(stop.stopped_actions.is_empty(), "the model call finishes");
    r.model.gate.add_permits(1);
    let res = first.await.unwrap();
    assert_eq!(res.stop_reason, "stopped");
    // Its call did not run, and says why.
    let not_run = r
        .core
        .store
        .session_nodes(&sid)
        .unwrap()
        .into_iter()
        .find_map(|(_, n)| match n.body {
            Body::ToolResult {
                tool,
                status,
                content,
                ..
            } if tool == "fs.write" => Some((status, content)),
            _ => None,
        })
        .unwrap();
    assert_eq!(not_run.0, ResultStatus::Cancelled);
    assert!(
        not_run
            .1
            .contains("stopped this turn (/stop, by discord:eddie)"),
        "{}",
        not_run.1
    );
    let root = std::path::PathBuf::from(r.core.cfg.tools.projects_dir.clone().unwrap());
    assert!(!root.join("a.txt").exists(), "nothing was written");
    assert!(
        posts(&r.core, "reply").is_empty(),
        "a stopped turn posts no reply"
    );
    assert!(posts(&r.core, "failed").is_empty(), "nor a failure");
    let e = parent_exec(&r.core, &sid);
    assert_eq!(
        (e.id.as_str(), e.state, e.wake.clone(), e.stopped.clone()),
        (eid.as_str(), ExecState::Waiting, Some(Wake::Input), None)
    );
    // The next message: the same session and execution, its history compiled.
    *r.model.hold.lock().unwrap() = None;
    let second = turn(&r.core, &sid, "SECOND what happened?").await;
    assert_eq!(second.session_id, sid);
    assert_eq!(second.execution_id.as_deref(), Some(eid.as_str()));
    assert_eq!(second.output, "You stopped me before the file was written.");
    let req = r.model.requests.lock().unwrap().last().cloned().unwrap();
    let seen = serde_json::to_string(&req.messages).unwrap();
    assert!(seen.contains("FIRST write a file"), "{seen}");
    assert!(seen.contains("I will write it."), "{seen}");
    assert!(seen.contains("/stop, by discord:eddie"), "{seen}");
    assert_eq!(posts(&r.core, "reply").len(), 1);
}

/// `/stop` between turns declines an approval that waits: its card settles
/// as stopped, nothing waits for the operator, and the next message's turn
/// hears that the call was not run.
#[tokio::test]
async fn a_stop_declines_a_waiting_approval_and_the_next_turn_hears_it() {
    let r = rig(
        |req| {
            if last_user(req).starts_with("SECOND") {
                return Scripted::text("Understood: not written.");
            }
            Scripted::tools(
                "Writing.",
                &[("w1", "fs_write", json!({"path": "a.txt", "content": "x\n"}))],
            )
        },
        |c| c.policy.enforcement = Posture::Approve,
    );
    let sid = parent_session(&r.core);
    let res = turn(&r.core, &sid, "FIRST write a file").await;
    let q = res.awaiting_confirm.clone().expect("it asks");
    let eid = parent_exec(&r.core, &sid).id;
    let stop = r.core.stop_execution(&eid, "discord:eddie").unwrap();
    assert!(stop.stopped && !stop.turn_running, "{stop:?}");
    assert_eq!(stop.declined, vec![q.clone()]);
    let settles: Vec<_> = posts(&r.core, "settle")
        .into_iter()
        .filter(|a| crate::outbox::body_of(a)["question"] == q.as_str())
        .collect();
    assert_eq!(settles.len(), 1);
    assert_eq!(
        crate::outbox::body_of(&settles[0])["closed"]["how"],
        "stopped"
    );
    assert!(r.core.kernel.pending_confirms().unwrap().is_empty());
    let e = parent_exec(&r.core, &sid);
    assert_eq!(
        (e.state, e.wake.clone()),
        (ExecState::Waiting, Some(Wake::Input))
    );
    let second = turn(&r.core, &sid, "SECOND never mind").await;
    assert_eq!(second.output, "Understood: not written.");
    let req = r.model.requests.lock().unwrap().last().cloned().unwrap();
    let seen = serde_json::to_string(&req.messages).unwrap();
    assert!(seen.contains("stopped by discord:eddie"), "{seen}");
    let root = std::path::PathBuf::from(r.core.cfg.tools.projects_dir.clone().unwrap());
    assert!(!root.join("a.txt").exists());
}
