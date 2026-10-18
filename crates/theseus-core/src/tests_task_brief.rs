//! A task's model (theseus-ypnxt.7): `task.start` with no profile inherited the
//! parent's `last_target`, and a task's first turn (the driver's, with no
//! message of its own) asked route.v6 nothing, so the routed arm was never
//! what a task paid. The rig is `tests_route`'s, with the driver running the
//! tasks and a fake Jev scripted for the brief's class.

use theseus_judge::fake::FakeJev;
use theseus_kernel::ExecState;
use theseus_protocol::task_start::{TaskStartParams, TaskStartResult};

use crate::approval::{Answerer, Surface};
use crate::session::{SessionRecord, TargetRef};
use crate::tests_route::{decided, mode, rig, session, Rig};
use crate::tests_tasks::{exec, until};

fn cli() -> Answerer {
    Answerer {
        label: "the CLI".into(),
        surface: Surface::Cli,
        discord: None,
    }
}

/// A rig whose Home last ran on `parent`, with the driver taking tasks' turns.
fn rig_from(jev: Option<&FakeJev>, parent: &str, tweak: impl FnOnce(&mut crate::Config)) -> Rig {
    let r = rig(jev, 4, tweak);
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

fn params(brief: &str, profile: Option<&str>) -> TaskStartParams {
    TaskStartParams {
        brief: brief.into(),
        profile: profile.map(str::to_string),
        ..Default::default()
    }
}

/// Start a task and wait for its first turn to end; its session record.
async fn run(r: &Rig, p: &TaskStartParams) -> (TaskStartResult, SessionRecord) {
    let started = r.core.task_start(p, cli()).unwrap();
    let id = started.session_id.replacen("ses_", "exe_", 1);
    until("the task's turn", || {
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

/// Today (pinned before the change): a task started with no profile from a
/// Fable conversation runs on Fable, whatever its brief is, and no verdict is
/// asked for it.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_task_with_no_profile_runs_on_its_parents_model() {
    let jev = FakeJev::start().unwrap();
    mode(&jev, "routine_coding", 0.95);
    let r = rig_from(Some(&jev), "fable", |_| {});
    let (_, rec) = run(&r, &params("Fix the typo in the quay notice.", None)).await;
    assert_eq!(ran_on(&r), "claude-fable-5-1");
    assert_eq!(rec.last_target.unwrap().profile, "fable");
    assert!(decided(&r.core.store).is_empty(), "no route was decided");
    assert_eq!(jev.connections(), 0, "Jev was not asked");
}

/// A profile asked for is a pin, before and after: the task runs on it.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_task_started_with_a_profile_runs_on_it() {
    let jev = FakeJev::start().unwrap();
    mode(&jev, "routine_coding", 0.95);
    let r = rig_from(Some(&jev), "fable", |_| {});
    let (_, rec) = run(&r, &params("Fix the typo in the quay notice.", Some("opus"))).await;
    assert_eq!(ran_on(&r), "claude-opus-5-5");
    assert_eq!(rec.last_target.unwrap().profile, "opus");
    assert_eq!(jev.connections(), 0, "a pinned task asks nothing");
}
