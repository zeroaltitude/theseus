//! The task graph through the whole core (M7 step 39a, theseus-ext.6): plan
//! items, a split, a close with evidence, a stale edit refused under CAS, a
//! layer-1 change that waits for the operator (accepted, declined, and a
//! shared place's answer refused), evidence never removed, a task session's
//! record closed by its report, the view in the request's tail and not in a
//! plain turn's, and a store from before the bump. The pure rules are
//! `task_graph::tests`.

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde_json::{json, Value};
use theseus_protocol::{SessionKind, TurnSubmitResult};
use theseus_store::{kinds, NewRecord, Store as _};

use crate::approval::Answerer;
use crate::bus::EventSink;
use crate::ledger::LedgerRow;
use crate::node::{Body, Node, ResultStatus};
use crate::policy::Posture;
use crate::provider::{
    DeltaSink, FakeProvider, Provider, ProviderFuture, ProviderRequest, Scripted,
};
use crate::session::SessionRecord;
use crate::store::Store;
use crate::task_graph::{self as graph, TaskRecord, TaskState};
use crate::turn::TurnRequest;
use crate::{Config, Core};

const PLACE: &str = "dm:42";
const TARGET: &str = "discord:dm:42";

/// The calls the next request that answers no call makes; a request that
/// answers one is answered with text.
type Next = Arc<Mutex<Vec<(String, String, Value)>>>;

struct Model {
    next: Next,
    requests: Mutex<Vec<ProviderRequest>>,
    /// A task's own turns answer with this.
    report: String,
}

fn answers_a_call(req: &ProviderRequest) -> bool {
    req.messages
        .last()
        .and_then(|m| m["content"].as_array())
        .is_some_and(|c| c.iter().any(|b| b["type"] == "tool_result"))
}

fn first_user(req: &ProviderRequest) -> String {
    req.messages
        .iter()
        .find(|m| m["role"] == "user")
        .map(|m| match &m["content"] {
            Value::String(s) => s.clone(),
            Value::Array(b) => b
                .iter()
                .find_map(|b| b["text"].as_str().map(str::to_string))
                .unwrap_or_default(),
            _ => String::new(),
        })
        .unwrap_or_default()
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
            let answer = if first_user(req).starts_with("[Task ") {
                Scripted::text(&self.report)
            } else if answers_a_call(req) {
                Scripted::text("Done.")
            } else {
                let calls = std::mem::take(&mut *self.next.lock().unwrap());
                if calls.is_empty() {
                    Scripted::text("Noted.")
                } else {
                    let calls: Vec<(&str, &str, Value)> = calls
                        .iter()
                        .map(|(id, n, v)| (id.as_str(), n.as_str(), v.clone()))
                        .collect();
                    Scripted::tools("On it.", &calls)
                }
            };
            FakeProvider::scripted(vec![answer])
                .stream_message(req, on_delta)
                .await
        })
    }
}

struct Rig {
    core: Arc<Core>,
    model: Arc<Model>,
    next: Next,
    _dir: tempfile::TempDir,
}

fn rig_on(dir: tempfile::TempDir, tweak: impl FnOnce(&mut Config)) -> Rig {
    let root = dir.path().join("work");
    std::fs::create_dir_all(&root).unwrap();
    let mut cfg = Config::example();
    cfg.server.state_dir = dir.path().to_string_lossy().into_owned();
    cfg.tools.projects_dir = Some(root.canonicalize().unwrap().to_string_lossy().into_owned());
    cfg.tools.roots = vec![];
    cfg.policy.enforcement = Posture::Open;
    tweak(&mut cfg);
    let store = Store::open(&dir.path().join("store")).unwrap();
    let next: Next = Arc::default();
    let model = Arc::new(Model {
        next: next.clone(),
        requests: Mutex::default(),
        report: "Charted every buoy in the outer harbour.".into(),
    });
    let core = Core::build(crate::rpc::Parts::for_tests(cfg, model.clone(), store)).unwrap();
    core.runner.place_rule.bind_one(crate::places::BoundPlace {
        target: TARGET.into(),
        name: "DM".into(),
        private: false,
        ..Default::default()
    });
    tokio::spawn(crate::harness::drive(core.clone()));
    Rig {
        core,
        model,
        next,
        _dir: dir,
    }
}

fn rig() -> Rig {
    rig_on(tempfile::tempdir().unwrap(), |_| {})
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
        .unwrap()
}

impl Rig {
    /// The next turn's calls: (wire name, input) each, each with an id of
    /// its own.
    fn calls(&self, calls: &[(&str, Value)]) {
        static N: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        *self.next.lock().unwrap() = calls
            .iter()
            .map(|(n, v)| {
                let i = N.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                (format!("t_{i}"), n.to_string(), v.clone())
            })
            .collect();
    }

    fn task(&self, id: &str) -> TaskRecord {
        graph::get(&self.core.store, id).unwrap().unwrap()
    }

    fn tasks(&self) -> Vec<TaskRecord> {
        graph::all(&self.core.store).unwrap()
    }

    fn by_title(&self, title: &str) -> TaskRecord {
        self.tasks().into_iter().find(|t| t.title == title).unwrap()
    }
}

fn nodes(core: &Core, sid: &str) -> Vec<Node> {
    core.store
        .session_nodes(sid)
        .unwrap()
        .into_iter()
        .map(|(_, n)| n)
        .collect()
}

/// Each result of `tool` in a session, in order: its status and content.
fn results(core: &Core, sid: &str, tool: &str) -> Vec<(ResultStatus, String)> {
    nodes(core, sid)
        .into_iter()
        .filter_map(|n| match n.body {
            Body::ToolResult {
                tool: t,
                status,
                content,
                ..
            } if t == tool => Some((status, content)),
            _ => None,
        })
        .collect()
}

fn rows(core: &Core, kind: &str) -> Vec<LedgerRow> {
    core.store
        .ledger_tail::<LedgerRow>(100_000)
        .unwrap()
        .into_iter()
        .map(|(_, r)| r)
        .filter(|r| r.kind == kind)
        .collect()
}

async fn until(what: &str, mut f: impl FnMut() -> bool) {
    let t0 = Instant::now();
    while !f() {
        assert!(t0.elapsed() < Duration::from_secs(20), "no {what} in 20 s");
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

/// Every stored version of a task, oldest first: the WAL keeps each.
fn versions(core: &Core, id: &str) -> Vec<TaskRecord> {
    core.store
        .inner()
        .tail_of_kind(kinds::TASK, usize::MAX)
        .unwrap()
        .into_iter()
        .filter(|r| r.key.as_deref() == Some(id))
        .map(|r| r.decode::<TaskRecord>().unwrap())
        .collect()
}

fn plan(title: &str) -> (&'static str, Value) {
    ("task_create", json!({"title": title}))
}

/// Three changes planned as three tasks, the second split into two, the
/// first closed with a commit as its evidence, and an edit that names a
/// version the task has moved past refused with the task as it is now
/// (§2.4; the design's first 39a test). Plan items need no arrangement.
#[tokio::test]
#[expect(clippy::too_many_lines, reason = "one scenario, step by step")]
async fn a_plan_splits_closes_with_evidence_and_refuses_a_stale_edit() {
    let r = rig();
    let c = &r.core;
    let s = session(c);
    r.calls(&[
        plan("Fix the tide table parser"),
        plan("Chart the outer harbour"),
        plan("Paint the lighthouse"),
    ]);
    turn(c, &s, "plan these as three tasks").await;
    let all = r.tasks();
    assert_eq!(all.len(), 3, "{all:?}");
    for t in &all {
        assert_eq!(
            (t.version, t.state, t.session.as_deref()),
            (1, TaskState::Accepted, None)
        );
        assert_eq!(t.origin.session, s);
    }
    assert_eq!(rows(c, "task.created").len(), 3);
    assert!(
        rows(c, "task.arrangement_refused").is_empty(),
        "a plan item needs no arrangement"
    );

    // Split the second into two: it moves to v2, with two children under it.
    let second = r.by_title("Chart the outer harbour");
    r.calls(&[(
        "task_split",
        json!({"id": second.id, "version": 1, "into": ["Chart the north side", "Chart the south side"]}),
    )]);
    turn(c, &s, "split the second into two").await;
    let second = r.task(&second.id);
    assert_eq!(second.version, 2);
    let all = r.tasks();
    let kids = graph::children(&all, &second.id);
    assert_eq!(
        kids.iter().map(|k| k.title.as_str()).collect::<Vec<_>>(),
        ["Chart the north side", "Chart the south side"]
    );
    assert_eq!(rows(c, "task.split").len(), 1);

    // An edit at v1 of a task now at v2 is refused, and says so.
    r.calls(&[(
        "task_update",
        json!({"id": second.id, "version": 1, "patch": {"title": "Chart the harbour"}}),
    )]);
    turn(c, &s, "rename it").await;
    let (status, text) = results(c, &s, "task.update").pop().unwrap();
    assert_eq!(status, ResultStatus::Error, "{text}");
    assert!(
        text.contains("changed since you read it: v1 → v2"),
        "{text}"
    );
    assert!(
        text.contains("\"Chart the outer harbour\""),
        "the record as it is now: {text}"
    );
    assert_eq!(r.task(&second.id).title, "Chart the outer harbour");
    let stale = rows(c, "task.stale_refused");
    assert_eq!(stale.len(), 1);
    assert_eq!(stale[0].data["named"], 1);
    assert_eq!(stale[0].data["version"], 2);

    // Close the first, with a commit as its evidence.
    let first = r.by_title("Fix the tide table parser");
    r.calls(&[(
        "task_close",
        json!({"id": first.id, "version": 1, "outcome": "done",
               "evidence": [{"identity": "commit:0a1b2c3d", "note": "the parser fix"}]}),
    )]);
    turn(c, &s, "close the first, with the commit as its evidence").await;
    let first = r.task(&first.id);
    assert_eq!((first.state, first.version), (TaskState::Done, 2));
    assert_eq!(first.evidence.len(), 1);
    assert_eq!(first.evidence[0].identity, "commit:0a1b2c3d");
    assert!(
        first.evidence[0]
            .node
            .as_deref()
            .is_some_and(|n| n.starts_with("msg_")),
        "{first:?}"
    );
    let closed = rows(c, "task.closed");
    assert_eq!(closed.len(), 1);
    assert_eq!(closed[0].data["evidence"][0]["identity"], "commit:0a1b2c3d");

    // A closed task takes no second close, and its evidence stays: every
    // stored version's evidence begins with the one before it.
    r.calls(&[(
        "task_close",
        json!({"id": first.id, "version": 2, "outcome": "done", "evidence": [{"identity": "job:act_9"}]}),
    )]);
    turn(c, &s, "close it again").await;
    let (status, _) = results(c, &s, "task.close").pop().unwrap();
    assert_eq!(status, ResultStatus::Error);
    let vs = versions(c, &first.id);
    assert_eq!(vs.iter().map(|v| v.version).collect::<Vec<_>>(), [1, 2]);
    for w in vs.windows(2) {
        assert!(
            w[1].evidence.starts_with(&w[0].evidence),
            "evidence only appends"
        );
        assert_eq!(w[1].version, w[0].version + 1);
    }

    // The next turn sees the graph in its request's tail, with versions, and
    // `context.compiled` says what it showed.
    turn(c, &s, "where are we").await;
    let req = r.model.requests.lock().unwrap().last().unwrap().clone();
    let last = req.messages.last().unwrap()["content"]
        .as_array()
        .unwrap()
        .clone();
    let view = last.last().unwrap()["text"].as_str().unwrap().to_string();
    assert!(
        view.starts_with("[The task graph in this conversation's scope: 4 open, 1 closed."),
        "{view}"
    );
    assert!(
        view.contains(&format!(
            "{} \"Chart the outer harbour\" [accepted] owner agent, v2",
            second.id
        )),
        "{view}"
    );
    assert!(view.contains("  - "), "children are indented: {view}");
    let compiled = rows(c, "context.compiled");
    let shown = &compiled.last().unwrap().data["tasks"];
    assert_eq!(
        (shown["open"].as_u64(), shown["closed"].as_u64()),
        (Some(4), Some(1)),
        "{shown}"
    );

    // task.list carries the records beside the task sessions, and task.get
    // reads one with its children.
    let listed = c.task_records(Some(&s)).unwrap();
    assert_eq!(listed.len(), 5);
    let got = c
        .task_get(theseus_protocol::tasks::TaskGetParams {
            id: second.short().into(),
        })
        .unwrap();
    assert_eq!(got.task.id, second.id);
    assert_eq!(got.children.len(), 2);
}

/// A plain turn, in a session whose scope holds no task (while another's
/// does), carries no view, and its `context.compiled` names none: its
/// request and its token count are what they were before 39a.
#[tokio::test]
async fn a_plain_turn_carries_no_view() {
    let r = rig();
    let c = &r.core;
    let busy = session(c);
    r.calls(&[plan("Fix the tide table parser")]);
    turn(c, &busy, "plan it").await;
    let plain = SessionRecord::new(SessionKind::Conversation, None);
    c.store.put_session(&plain.session_id, &plain).unwrap();
    turn(c, &plain.session_id, "what is the tide at noon").await;
    let req = r.model.requests.lock().unwrap().last().unwrap().clone();
    let text = serde_json::to_string(&req.messages).unwrap();
    assert!(!text.contains("[The task graph"), "{text}");
    assert!(
        !text.contains("cache_control"),
        "no breakpoint moves: {text}"
    );
    let compiled = rows(c, "context.compiled");
    let last = compiled.last().unwrap();
    assert_eq!(last.session_id.as_deref(), Some(plain.session_id.as_str()));
    assert!(last.data.get("tasks").is_none(), "{:?}", last.data);
}

/// Who answers a layer-1 question from a shared place: a person in a guild
/// channel.
fn from_a_guild() -> Answerer {
    Answerer {
        label: "discord:77".into(),
        surface: crate::approval::Surface::Discord,
        discord: Some(theseus_protocol::DiscordOrigin {
            user_id: "77".into(),
            channel_id: "314159265358979323".into(),
            guild_id: Some("900000000000000001".into()),
        }),
    }
}

/// A change to a task's acceptance is a proposal that waits for the
/// operator, at every posture (`task.update` is open here): written on the
/// task with its card, the task unchanged. A shared place's answer is
/// refused and the proposal stays; the operator's yes applies it in one
/// frame (`task.change_accepted`), and the next turn's view shows it. A no
/// leaves the task as it was (`task.change_declined`).
#[tokio::test]
async fn a_layer_one_change_waits_and_accept_applies_it_and_decline_leaves_it() {
    let r = rig_on(tempfile::tempdir().unwrap(), |cfg| {
        cfg.policy.tools.insert("task.update".into(), Posture::Open);
        cfg.policy.tools.insert("task.close".into(), Posture::Open);
    });
    let c = &r.core;
    let s = session(c);
    r.calls(&[(
        "task_create",
        json!({"title": "Chart the outer harbour", "acceptance": ["a chart exists"]}),
    )]);
    turn(c, &s, "plan it").await;
    let t = r.by_title("Chart the outer harbour");
    assert_eq!(t.acceptance, ["a chart exists"]);

    r.calls(&[(
        "task_update",
        json!({"id": t.id, "version": 1, "patch": {"acceptance": ["every buoy has a depth on the chart"]}}),
    )]);
    turn(c, &s, "change its acceptance to every buoy having a depth").await;
    let asked = c.confirm_list().unwrap();
    assert_eq!(asked.len(), 1, "{asked:?}");
    let q = &asked[0];
    assert_eq!(q.tool, "task.update");
    assert!(q.reason.contains("layer 1"), "{}", q.reason);
    let waiting = r.task(&t.id);
    assert_eq!(waiting.version, 1, "a proposal changes nothing yet");
    assert_eq!(waiting.acceptance, ["a chart exists"]);
    let p = waiting.proposal.clone().unwrap();
    assert_eq!(p.card, q.correlation_id);
    assert_eq!(
        p.acceptance.as_deref(),
        Some(&["every buoy has a depth on the chart".to_string()][..])
    );
    assert_eq!(rows(c, "task.change_proposed").len(), 1);

    // A shared place's word does not count: refused, and the proposal stays.
    assert!(c
        .confirm_action(&q.correlation_id, true, None, from_a_guild())
        .is_err());
    assert!(r.task(&t.id).proposal.is_some());
    assert_eq!(r.task(&t.id).acceptance, ["a chart exists"]);

    // The operator's yes applies it, in the frame that settles the call.
    c.confirm_action(&q.correlation_id, true, None, "cli")
        .unwrap();
    until("the accepted change", || r.task(&t.id).version == 2).await;
    let applied = r.task(&t.id);
    assert_eq!(applied.acceptance, ["every buoy has a depth on the chart"]);
    assert!(applied.proposal.is_none());
    assert_eq!(rows(c, "task.change_accepted").len(), 1);
    until("the turn's end", || c.confirm_list().unwrap().is_empty()).await;
    tokio::time::sleep(Duration::from_millis(100)).await;
    turn(c, &s, "what does it take now").await;
    let req = r.model.requests.lock().unwrap().last().unwrap().clone();
    let view = serde_json::to_string(&req.messages).unwrap();
    assert!(
        view.contains("accept: every buoy has a depth on the chart, v2"),
        "{view}"
    );

    // A no leaves it as it was.
    r.calls(&[(
        "task_update",
        json!({"id": t.id, "version": 2, "patch": {"objective": "chart the inner harbour instead"}}),
    )]);
    turn(c, &s, "make it the inner harbour").await;
    let q = c.confirm_list().unwrap().pop().unwrap();
    assert!(r.task(&t.id).proposal.is_some());
    c.confirm_action(&q.correlation_id, false, Some("keep the outer one"), "cli")
        .unwrap();
    let left = r.task(&t.id);
    assert_eq!(left.version, 2);
    assert!(left.proposal.is_none());
    assert_ne!(left.objective, "chart the inner harbour instead");
    assert_eq!(rows(c, "task.change_declined").len(), 1);
    until("the declined call's answer", || {
        results(c, &s, "task.update")
            .last()
            .is_some_and(|(st, _)| *st == ResultStatus::Declined)
    })
    .await;
    assert_eq!(r.task(&t.id).version, 2);

    // Abandoning waits too.
    tokio::time::sleep(Duration::from_millis(100)).await;
    r.calls(&[(
        "task_close",
        json!({"id": t.id, "version": 2, "outcome": "abandoned"}),
    )]);
    turn(c, &s, "drop it").await;
    let q = c.confirm_list().unwrap().pop().unwrap();
    assert!(q.reason.contains("abandoning"), "{}", q.reason);
    assert_eq!(r.task(&t.id).state, TaskState::Accepted);
    assert!(r.task(&t.id).proposal.as_ref().is_some_and(|p| p.abandon));
}

/// What the operator says to start a task, and the quote of it (M5 27).
const ASK: &str = "START: chart every buoy of the outer harbour, please";
const QUOTE: &str = "chart every buoy of the outer harbour, please";

/// A task session's record is written with its session, `in_progress`, its
/// objective the arrangement's piece; it reads its execution's state; its
/// report closes it `done` with the report as its evidence, in the frame
/// that ends it. 27's refusal stands for a `brief` without an arrangement.
#[tokio::test]
async fn a_task_sessions_record_is_closed_by_its_report() {
    let r = rig();
    let c = &r.core;
    let s = session(c);
    // Without an arrangement, a brief is refused as 27 refuses it.
    r.calls(&[("task_create", json!({"brief": "Chart the buoys."}))]);
    turn(c, &s, "chart them").await;
    assert_eq!(
        results(c, &s, "task.create").pop().unwrap().0,
        ResultStatus::Error
    );
    assert!(r.tasks().is_empty(), "a refused task writes no record");
    r.calls(&[(
        "task_create",
        json!({"brief": "Chart every buoy of the outer harbour and report the depths.",
               "title": "Chart the buoys",
               "arrangement": {"pieces": [{"quote": QUOTE, "role": "objective"}]}}),
    )]);
    turn(c, &s, ASK).await;
    let t = r.by_title("Chart the buoys");
    let task_session = t.session.clone().unwrap();
    assert_eq!(t.id, graph::of_session(&task_session));
    assert!(t.objective.contains(QUOTE), "{}", t.objective);
    until("the task's report", || {
        r.task(&t.id).state == TaskState::Done
    })
    .await;
    let done = r.task(&t.id);
    assert_eq!(done.version, 2);
    assert_eq!(done.evidence.len(), 1);
    assert_eq!(done.evidence[0].identity, format!("report:{task_session}"));
    let report = nodes(c, &task_session)
        .into_iter()
        .rev()
        .find(|n| matches!(n.body, Body::AssistantMessage { .. }))
        .unwrap();
    assert_eq!(done.evidence[0].node.as_deref(), Some(report.id.as_str()));
    let closed = rows(c, "task.closed");
    assert_eq!(closed.len(), 1);
    assert_eq!(closed[0].data["by"], "report");
    // Listed both ways: its session from its execution, and its record.
    let listed = c
        .task_list(theseus_protocol::TaskListParams::default())
        .unwrap();
    assert_eq!(listed.tasks.len(), 1);
    assert_eq!(listed.records.len(), 1);
    assert_eq!(listed.records[0].state, TaskState::Done);
}

/// A task session that fails closes `failed`, and one a cancel ended stays
/// open and reads `suspended` (closed_by_report's rule, on stored
/// executions).
#[tokio::test]
async fn a_failure_closes_a_task_failed_and_a_cancel_suspends_it() {
    let r = rig();
    let c = &r.core;
    let s = session(c);
    let mut e: theseus_kernel::Execution = serde_json::from_value(json!({
        "id": "exe_00000000000000000000000000000091", "schema": 2,
        "session_id": "ses_00000000000000000000000000000091", "kind": "task",
        "state": "failed", "authority": {"principal": "operator", "ceilings": {}},
        "budget": {"limit_micros": 1000000, "spent_micros": 0, "reserved_micros": 0,
                   "held_unknown_micros": 0, "reservations": {}, "resets": 0},
        "outstanding": [], "queued_results": [], "turns": 1, "interrupted": 0,
        "resume_pending": false, "ended_reason": "the model was unavailable",
        "created_at_ms": 0, "updated_at_ms": 0
    }))
    .unwrap();
    let rec = graph::NewTask {
        id: graph::of_session(&e.session_id),
        title: "Chart the buoys",
        objective: "chart them".into(),
        acceptance: vec![],
        parent: None,
        deps: vec![],
        session: Some(e.session_id.clone()),
        origin: graph::TaskOrigin {
            session: s,
            principal: "operator".into(),
        },
        state: TaskState::InProgress,
    }
    .build(1);
    c.store.append(&[graph::record(&rec).unwrap()]).unwrap();
    let failed = graph::closed_by_report(&c.store, &e, None)
        .unwrap()
        .unwrap();
    assert_eq!((failed.state, failed.version), (TaskState::Failed, 2));
    assert_eq!(
        failed.evidence[0].note.as_deref(),
        Some("the model was unavailable")
    );
    e.state = theseus_kernel::ExecState::Cancelled;
    assert!(graph::closed_by_report(&c.store, &e, None)
        .unwrap()
        .is_none());
    assert_eq!(graph::state_now(&rec, Some(&e)), TaskState::Suspended);
}

/// A crash between steps: what was written survives a reopen, and a task
/// session from a store written before 39a's bump, with no record, still
/// lists from its execution, and its turns see no graph.
#[tokio::test]
async fn records_survive_a_reopen_and_older_task_sessions_still_list() {
    let r = rig();
    let c = &r.core;
    let s = session(c);
    r.calls(&[
        plan("Fix the tide table parser"),
        (
            "task_create",
            json!({"brief": "Chart every buoy of the outer harbour and report the depths.",
                   "arrangement": {"pieces": [{"quote": QUOTE, "role": "objective"}]}}),
        ),
    ]);
    turn(c, &s, ASK).await;
    until("the task's report", || {
        r.tasks().iter().any(|t| t.state == TaskState::Done)
    })
    .await;
    let before = r.tasks();
    assert_eq!(before.len(), 2);

    // The log, read again by a fresh open, as after a crash: every record
    // decodes, and the graph is what it was. And the store as a build
    // before the bump would have written it: every record but the task
    // records, under a format-9 manifest.
    let copy_to = |with_tasks: bool| {
        let dir = tempfile::tempdir().unwrap();
        let all = c.store.inner().scan(1, None, usize::MAX).unwrap();
        let copy = Store::open(&dir.path().join("store")).unwrap();
        let frame: Vec<NewRecord> = all
            .iter()
            .filter(|r| with_tasks || r.kind != kinds::TASK)
            .map(|r| NewRecord {
                kind: r.kind,
                key: r.key.clone(),
                scope: r.scope.clone(),
                payload: r.payload.clone(),
            })
            .collect();
        copy.append(&frame).unwrap();
        dir
    };
    let reopened = copy_to(true);
    let old = copy_to(false);
    std::fs::write(
        old.path().join("store/MANIFEST.json"),
        r#"{"format":9,"engine":"redb"}"#,
    )
    .unwrap();
    let again = rig_on(reopened, |_| {});
    assert_eq!(again.tasks(), before);

    let older = rig_on(old, |_| {});
    let listed = older
        .core
        .task_list(theseus_protocol::TaskListParams::default())
        .unwrap();
    assert_eq!(
        listed.tasks.len(),
        1,
        "the task session lists from its execution"
    );
    assert!(listed.records.is_empty());
    turn(&older.core, &s, "and now").await;
    let req = older.model.requests.lock().unwrap().last().unwrap().clone();
    assert!(!serde_json::to_string(&req.messages)
        .unwrap()
        .contains("[The task graph"));
}
