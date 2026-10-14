//! The self budget at the turn's model call (theseus-pw1q.3, seam S2): a
//! turn of a `"self"`-principal task's session reserves and settles every
//! model call on the self budget, an ordinary task's and a conversation's
//! are not counted, a line stops the call and says so in the turn's failure,
//! and the mode off spends nothing and writes no row.

use serde_json::Value;
use theseus_protocol::rsi::SelfMode;
use theseus_protocol::SessionKind;

use crate::rsi::SELF_PRINCIPAL;
use crate::session::{SessionRecord, TaskOf};
use crate::task_graph::{self as graph, TaskState};
use crate::tests_task_graph::{rig_on, rows, session, turn, Rig};

/// A task session whose record names `principal`, opened by `parent`.
fn task_session(
    r: &Rig,
    parent: &str,
    principal: &str,
    parent_of: Option<&str>,
) -> (String, String) {
    principal_session(&r.core, parent, principal, parent_of)
}

/// A task session on `core` whose record names `principal`: its session id
/// and its task's.
pub(crate) fn principal_session(
    core: &crate::Core,
    parent: &str,
    principal: &str,
    parent_of: Option<&str>,
) -> (String, String) {
    let rec = SessionRecord::new(SessionKind::Task, None);
    let sid = rec.session_id.clone();
    core.store.put_session(&sid, &rec).unwrap();
    let task = make_task(core, &sid, parent, principal, parent_of);
    (sid, task)
}

/// Make `sid` a task's session, opened by `parent`, with a task record whose
/// origin names `principal`: the task's id.
pub(crate) fn make_task(
    core: &crate::Core,
    sid: &str,
    parent: &str,
    principal: &str,
    parent_of: Option<&str>,
) -> String {
    let mut rec: SessionRecord = core.store.get_session(sid).unwrap().unwrap();
    rec.task = Some(TaskOf {
        parent_session: parent.into(),
        parent_execution: "exe_none".into(),
        by: "call_none".into(),
        target: None,
        arrangement: None,
        check: None,
    });
    core.store.put_session(sid, &rec).unwrap();
    let task = graph::NewTask {
        id: graph::of_session(sid),
        title: "Tidy the harbour notes",
        objective: "Tidy the harbour notes.".into(),
        acceptance: vec![],
        parent: parent_of.map(str::to_string),
        deps: vec![],
        session: Some(sid.to_string()),
        origin: graph::TaskOrigin {
            session: parent.into(),
            principal: principal.into(),
            by_model: false,
        },
        state: TaskState::InProgress,
    }
    .build(theseus_protocol::now_unix_ms());
    core.store.append(&[graph::record(&task).unwrap()]).unwrap();
    task.id
}

/// A turn that may fail: the self budget's refusal ends it with an error.
async fn try_turn(r: &Rig, sid: &str) -> anyhow::Result<theseus_protocol::TurnSubmitResult> {
    try_turn_on(&r.core, sid).await
}

/// [`try_turn`] on any core.
async fn try_turn_on(
    core: &std::sync::Arc<crate::Core>,
    sid: &str,
) -> anyhow::Result<theseus_protocol::TurnSubmitResult> {
    let rec: SessionRecord = core.store.get_session(sid).unwrap().unwrap();
    let (live, _) = core.live_profile();
    let target = core.runner.resolve_target(&live, None, None, None).unwrap();
    let sink = crate::bus::EventSink::new(core.bus.clone(), sid, None);
    core.runner
        .run(crate::turn::TurnRequest {
            prompt: None,
            session: rec,
            input: Some("go".into()),
            target,
            sink,
            author: "test".into(),
            recompile: None,
            attachments: vec![],
            arrived: None,
            reply_to: None,
            answer_at: None,
        })
        .await
}

fn self_rig(mode: SelfMode, branch_usd: f64) -> Rig {
    rig_on(tempfile::tempdir().unwrap(), |cfg| {
        cfg.self_improve.mode = mode;
        cfg.self_improve.budget.branch_usd = branch_usd;
    })
}

fn kinds(r: &Rig) -> Vec<String> {
    r.core
        .store
        .ledger_tail::<crate::ledger::LedgerRow>(100_000)
        .unwrap()
        .into_iter()
        .map(|(_, row)| row.kind)
        .filter(|k| k.starts_with("self.budget."))
        .collect()
}

/// A `"self"` task's turn reserves its model call and settles it at the
/// call's cost, on the task's branch; an ordinary task's turn and a
/// conversation's touch no account.
#[tokio::test]
async fn a_self_turns_model_call_is_counted_and_an_ordinary_ones_is_not() {
    let r = self_rig(SelfMode::Act, 10.0);
    let parent = session(&r.core);
    // An ordinary task and a conversation: nothing on the budget.
    let (plain, _) = task_session(&r, &parent, "operator", None);
    turn(&r.core, &plain, "go").await;
    turn(&r.core, &parent, "hello").await;
    assert!(kinds(&r).is_empty(), "{:?}", kinds(&r));
    assert_eq!(r.core.runner.self_budget.view().unwrap().day_spent_usd, 0.0);
    // A self task's: one hold, one settle, at the turn's cost.
    let (sid, task) = task_session(&r, &parent, SELF_PRINCIPAL, None);
    let out = turn(&r.core, &sid, "go").await;
    let reserved = rows(&r.core, "self.budget.reserved");
    let settled = rows(&r.core, "self.budget.settled");
    assert_eq!((reserved.len(), settled.len()), (1, 1), "{:?}", kinds(&r));
    assert_eq!(reserved[0].data["branch"], task.as_str());
    assert_eq!(settled[0].data["branch"], task.as_str());
    let spent = settled[0].data["spent_usd"].as_f64().unwrap();
    assert!(spent > 0.0, "the call cost something: {settled:?}");
    assert!(
        (spent - out.cost_usd.unwrap_or(0.0)).abs() < 1e-5,
        "{spent} vs {out:?}"
    );
    let reserved_usd = reserved[0].data["reserved_usd"].as_f64().unwrap();
    assert!(reserved_usd >= spent, "the hold is the worst case");
    let v = r.core.runner.self_budget.view().unwrap();
    assert_eq!(v.branches.len(), 1);
    assert_eq!(v.branches[0].branch, task);
    assert!((v.day_spent_usd - spent).abs() < 1e-5);
    assert_eq!(v.branches[0].open_usd, 0.0, "nothing is left open");
}

/// A subtask of a self task spends on its root's branch.
#[tokio::test]
async fn a_self_tasks_subtask_spends_on_its_roots_branch() {
    let r = self_rig(SelfMode::Act, 10.0);
    let parent = session(&r.core);
    let (_root_sid, root) = task_session(&r, &parent, SELF_PRINCIPAL, None);
    let (child, _) = task_session(&r, &parent, SELF_PRINCIPAL, Some(&root));
    turn(&r.core, &child, "go").await;
    let settled = rows(&r.core, "self.budget.settled");
    assert_eq!(settled.len(), 1);
    assert_eq!(settled[0].data["branch"], root.as_str());
}

/// A branch at its line: the call is not made, the turn fails with the
/// budget's words, and a `self.budget.stopped` row says so. Its spend so far
/// is kept.
#[tokio::test]
async fn a_line_stops_the_call_and_says_so_in_the_turn_and_the_ledger() {
    // A line under any call's worst case.
    let r = self_rig(SelfMode::Act, 0.000_001);
    let parent = session(&r.core);
    let (sid, task) = task_session(&r, &parent, SELF_PRINCIPAL, None);
    let calls_before = r.model.requests.lock().unwrap().len();
    let err = try_turn(&r, &sid).await.unwrap_err();
    let text = format!("{err:#}");
    assert!(
        text.contains("self_budget") && text.contains("branch line"),
        "{text}"
    );
    assert_eq!(
        r.model.requests.lock().unwrap().len(),
        calls_before,
        "no request went out"
    );
    let stopped = rows(&r.core, "self.budget.stopped");
    assert_eq!(stopped.len(), 1, "{:?}", kinds(&r));
    assert_eq!(stopped[0].data["line"], "branch");
    assert_eq!(stopped[0].data["branch"], task.as_str());
    assert!(rows(&r.core, "self.budget.reserved").is_empty());
}

/// The mode off: a self task's turn is stopped, spends nothing, and writes
/// no budget row.
#[tokio::test]
async fn the_mode_off_spends_nothing_and_writes_no_row_in_a_turn() {
    let r = self_rig(SelfMode::Off, 10.0);
    let parent = session(&r.core);
    let (sid, _) = task_session(&r, &parent, SELF_PRINCIPAL, None);
    let err = try_turn(&r, &sid).await.unwrap_err();
    assert!(format!("{err:#}").contains("is off"), "{err:#}");
    assert!(kinds(&r).is_empty(), "{:?}", kinds(&r));
    assert!(r.model.requests.lock().unwrap().is_empty());
    assert!(r.core.runner.self_budget.view().is_none());
    assert!(r
        .core
        .store
        .get_meta::<Value>(crate::rsi::budget::BUDGET_KEY)
        .unwrap()
        .is_none());
}

/// A self call cut off with its usage unknown settles at its worst case,
/// never at zero: money that may have been spent counts as spent (a join
/// fix's test: settling it at zero passed every other test).
#[tokio::test]
async fn a_self_call_whose_cost_is_unknown_settles_at_its_worst_case() {
    use crate::provider::{FakeProvider, ProviderError, Scripted};
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("work");
    std::fs::create_dir_all(&root).unwrap();
    let mut cfg = crate::Config::example();
    cfg.server.state_dir = dir.path().to_string_lossy().into_owned();
    cfg.tools.projects_dir = Some(root.canonicalize().unwrap().to_string_lossy().into_owned());
    cfg.tools.roots = vec![];
    cfg.self_improve.mode = SelfMode::Act;
    let model = std::sync::Arc::new(FakeProvider::scripted(vec![
        Scripted::Fail(ProviderError::Truncated { elapsed_ms: 5 }),
        Scripted::text("never sent"),
    ]));
    let store = crate::store::Store::open(&dir.path().join("store")).unwrap();
    let core = crate::Core::build(crate::rpc::Parts::for_tests(cfg, model, store)).unwrap();
    let parent = SessionRecord::new(SessionKind::Conversation, None);
    core.store.put_session(&parent.session_id, &parent).unwrap();
    let (sid, _) = principal_session(&core, &parent.session_id, SELF_PRINCIPAL, None);
    let _ = try_turn_on(&core, &sid).await;
    let reserved = rows(&core, "self.budget.reserved");
    let settled = rows(&core, "self.budget.settled");
    assert!(!settled.is_empty(), "the cut call settled");
    let held = reserved[0].data["reserved_usd"].as_f64().unwrap();
    let spent = settled[0].data["spent_usd"].as_f64().unwrap();
    assert!(held > 0.0);
    assert!(
        (spent - held).abs() < 1e-6,
        "an unknown cost settles at the hold, {held}: {spent}"
    );
}
