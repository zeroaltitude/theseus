//! A task's model (theseus-ypnxt.7): `task.start` with no profile inherited the
//! parent's `last_target`, and a task's first turn (the driver's, with no
//! message of its own) asked route.v8 nothing, so the routed arm was never
//! what a task paid. The rig is `tests_route`'s, with the driver running the
//! tasks and a fake Jev scripted for the brief's class.

use theseus_judge::fake::{FakeJev, FakeMode};
use theseus_kernel::ExecState;
use theseus_protocol::task_start::{TaskStartParams, TaskStartResult};

use crate::approval::{Answerer, Surface};
use crate::session::{SessionRecord, TargetRef};
use crate::test_support::until_event;
use crate::tests_route::{decided, mode, session, Rig};
use crate::tests_tasks::exec;

pub(crate) fn cli() -> Answerer {
    Answerer {
        label: "the CLI".into(),
        surface: Surface::Cli,
        discord: None,
        agent: None,
    }
}

/// A rig whose Home last ran on `parent`, with the driver taking tasks' turns.
fn rig_from(jev: Option<&FakeJev>, parent: &str, tweak: impl FnOnce(&mut crate::Config)) -> Rig {
    rig_from_parts(jev, parent, tweak, |_| {})
}

/// [`rig_from`], with `parts` edited after the load's check.
fn rig_from_parts(
    jev: Option<&FakeJev>,
    parent: &str,
    tweak: impl FnOnce(&mut crate::Config),
    parts: impl FnOnce(&mut crate::rpc::Parts),
) -> Rig {
    let r = crate::tests_route::rig_parts(jev, 4, tweak, parts);
    tokio::spawn(crate::harness::drive(r.core.clone()));
    let home = r.core.home().unwrap();
    let (live, _) = r.core.live_profile();
    let at = r
        .core
        .runner
        .resolve_target(&live, Some(parent), None, None)
        .unwrap();
    let mut rec: SessionRecord = r.core.store.get_session(&home.session_id).unwrap().unwrap();
    rec.last_target = Some(TargetRef::from(&at));
    r.core.store.put_session(&rec.session_id, &rec).unwrap();
    r
}

pub(crate) fn params(brief: &str, profile: Option<&str>) -> TaskStartParams {
    TaskStartParams {
        brief: brief.into(),
        profile: profile.map(str::to_string),
        ..Default::default()
    }
}

/// Start a task and wait for its first turn to end; its session record.
pub(crate) async fn run(r: &Rig, p: &TaskStartParams) -> (TaskStartResult, SessionRecord) {
    let started = r.core.task_start(p, cli()).unwrap();
    let id = started.session_id.replacen("ses_", "exe_", 1);
    until_event("the task's first turn", || {
        exec(&r.core, &id).state == ExecState::Complete
    })
    .await;
    let rec = session(&r.core, &started.session_id);
    (started, rec)
}

/// The model of the one request the task's turn sent.
fn ran_on(r: &Rig) -> String {
    let all: Vec<_> = r
        .claude
        .requests()
        .into_iter()
        .chain(r.zai.requests())
        .collect();
    assert_eq!(all.len(), 1, "one model call: the task's own turn");
    all[0].model.clone()
}

pub(crate) const BRIEF: &str = "Fix the typo in the quay notice.";

/// Only a trivial brief runs on Haiku (theseus-31pgz).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_trivial_brief_runs_on_haiku() {
    let jev = FakeJev::start().unwrap();
    mode(&jev, "trivial", 0.95);
    let r = rig_from(Some(&jev), "fable", |_| {});
    let (_, rec) = run(&r, &params("Say hello to the quay.", None)).await;
    assert_eq!(ran_on(&r), "claude-haiku-5-5");
    assert_eq!(rec.last_target.unwrap().profile, "haiku");
}

/// A design brief from a cheap conversation goes up: the base is the class's.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_design_brief_from_a_cheap_conversation_goes_to_opus() {
    let jev = FakeJev::start().unwrap();
    mode(&jev, "sophisticated", 0.95);
    let r = rig_from(Some(&jev), "sonnet", |_| {});
    let (_, rec) = run(&r, &params("Weigh two designs for the quay ledger.", None)).await;
    assert_eq!(ran_on(&r), "claude-opus-5-5");
    assert_eq!(rec.last_target.unwrap().profile, "opus");
}

/// Jev off: the parent's model, as before.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_task_runs_on_its_parents_model_with_jev_off() {
    let r = rig_from(None, "fable", |c| c.judge.enabled = false);
    let (_, rec) = run(&r, &params(BRIEF, None)).await;
    assert_eq!(ran_on(&r), "claude-fable-5-1");
    assert_eq!(rec.last_target.unwrap().profile, "fable");
    assert!(decided(&r.core.store).is_empty());
}

/// A verdict under its class's bar: the parent's model.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_verdict_under_its_bar_leaves_the_parents_model() {
    let jev = FakeJev::start().unwrap();
    mode(&jev, "routine_coding", 0.1);
    let r = rig_from(Some(&jev), "fable", |_| {});
    let (_, rec) = run(&r, &params(BRIEF, None)).await;
    assert_eq!(ran_on(&r), "claude-fable-5-1");
    assert_eq!(rec.last_target.unwrap().profile, "fable");
    assert_eq!(decided(&r.core.store)[0]["reason"], "unsure");
}

/// A verdict past the route wait: the parent's model for the first turn, and
/// the verdict, released, sets no base now and waits for no later message.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_late_verdict_sets_no_base() {
    let jev = FakeJev::start().unwrap();
    mode(&jev, "routine_coding", 0.95);
    jev.set_mode(FakeMode::Held);
    let r = rig_from(Some(&jev), "fable", |c| {
        c.routing.max_wait_ms = 300;
        c.judge.total_secs = 30;
    });
    let (started, rec) = run(&r, &params(BRIEF, None)).await;
    assert_eq!(ran_on(&r), "claude-fable-5-1");
    assert_eq!(rec.last_target.unwrap().profile, "fable");
    assert_eq!(decided(&r.core.store)[0]["reason"], "late");
    jev.release();
    tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    assert!(!r.core.runner.judge.has_late(&started.session_id));
    let after = session(&r.core, &started.session_id);
    assert_eq!(after.last_target.unwrap().profile, "fable");
}

/// A profile asked for is a pin, before and after: the task runs on it.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_task_started_with_a_profile_runs_on_it() {
    let jev = FakeJev::start().unwrap();
    mode(&jev, "routine_coding", 0.95);
    let r = rig_from(Some(&jev), "fable", |_| {});
    let (_, rec) = run(&r, &params(BRIEF, Some("opus"))).await;
    assert_eq!(ran_on(&r), "claude-opus-5-5");
    assert_eq!(rec.last_target.unwrap().profile, "opus");
    assert_eq!(jev.connections(), 0, "a pinned task asks nothing");
}
