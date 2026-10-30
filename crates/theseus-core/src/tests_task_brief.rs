//! A task's model (theseus-ypnxt.7): `task.start` with no profile inherited the
//! parent's `last_target`, and a task's first turn (the driver's, with no
//! message of its own) asked route.v8 nothing, so the routed arm was never
//! what a task paid. The rig is `tests_route`'s, with the driver running the
//! tasks and a fake Jev scripted for the brief's class.

use theseus_judge::fake::{FakeJev, FakeMode};
use theseus_kernel::ExecState;
use theseus_protocol::task_start::{TaskStartParams, TaskStartResult};

use crate::approval::{Answerer, Surface};
use crate::places::BoundPlace;
use crate::session::{SessionRecord, TargetRef};
use crate::test_support::until_event;
use crate::tests_route::{decided, mode, session, Rig};
use crate::tests_tasks::exec;
use theseus_protocol::PlaceCeiling;

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

/// The rule (theseus-ypnxt.7). Before it, this test held that a task started
/// with no profile from a Fable conversation ran on Fable and asked Jev
/// nothing (`assert_eq!(ran_on(&r), "claude-fable-5-1")`, `decided` empty,
/// `jev.connections() == 0`). Now the brief's verdict, routine coding, picks
/// the class's first usable profile, which is the task's base; one request
/// asks it, route.v8's alone. A routine-coding brief starts on `haikuhi`, and
/// its checkpoints move it up (the owner, 2026-10-18: start cheap, escalate;
/// theseus-31pgz, theseus-ypnxt.9); a trivial brief runs on Haiku, below.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_task_with_no_profile_runs_on_the_profile_its_briefs_class_picks() {
    let jev = FakeJev::start().unwrap();
    mode(&jev, "routine_coding", 0.95);
    let r = rig_from(Some(&jev), "fable", |_| {});
    let (_, rec) = run(&r, &params(BRIEF, None)).await;
    assert_eq!(ran_on(&r), "claude-haiku-5-5");
    assert_eq!(rec.last_target.unwrap().profile, "haikuhi");
    assert!(
        rec.routed.is_none(),
        "the class's profile is the base, not a move"
    );
    assert_eq!(jev.connections(), 1, "one request: route.v8 alone");
    let d = decided(&r.core.store);
    assert_eq!(d.len(), 1);
    assert_eq!(
        (
            d[0]["source"].as_str(),
            d[0]["from"].as_str(),
            d[0]["profile"].as_str()
        ),
        (Some("task brief"), Some("fable"), Some("haikuhi"))
    );
    assert_eq!(d[0]["mode"], "routine_coding");
    assert!(d[0].get("follows").is_none());
}

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

/// review-fixes' floor holds for a brief too (the two rows' join): a brief
/// that names a sensitive name never makes a profile of restricted origin the
/// task's base; the class's next profile is, not the `[model]` default.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_brief_that_names_a_sensitive_name_skips_restricted_profiles() {
    let jev = FakeJev::start().unwrap();
    mode(&jev, "deep_coding", 0.95);
    // The list after the load's check, which refuses a restricted profile in
    // a mode's list (theseus-a1wvq.4): the floor holds on one anyway.
    let r = rig_from_parts(
        Some(&jev),
        "fable",
        |c| c.privacy.sensitive_names = vec!["Examplecorp".into()],
        |p| p.cfg.routing.classes.deep_coding.profiles = vec!["glm53".into(), "sonnet".into()],
    );
    let brief = "Fix the typo in Examplecorp's quay notice.";
    let (_, rec) = run(&r, &params(brief, None)).await;
    assert_eq!(ran_on(&r), "claude-sonnet-5-5");
    assert_eq!(rec.last_target.unwrap().profile, "sonnet");
    assert!(r.zai.requests().is_empty());
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

/// Closed (the acting pack asks no `model`), a routine-coding brief starts on
/// `haikuhi` even when a config moved it out of the class list, as it always
/// did: the row changes nothing until the pack flips.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_closed_routine_brief_still_starts_on_haikuhi() {
    let jev = FakeJev::start().unwrap();
    mode(&jev, "routine_coding", 0.95);
    let r = rig_from(Some(&jev), "fable", |c| {
        c.routing.classes.routine_coding.profiles = vec!["sonnet".into()];
    });
    let (_, rec) = run(&r, &params(BRIEF, None)).await;
    assert_eq!(ran_on(&r), "claude-haiku-5-5");
    assert_eq!(rec.last_target.unwrap().profile, "haikuhi");
}

/// Open (theseus-ej6bj: the acting pack asks `model`, `CODING_BASE` no longer
/// stands): a brief starts on the model Jev picks, and a config that moved
/// `haikuhi` out of the class list does not get it back.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_open_routine_brief_starts_where_its_class_sends_it() {
    let jev = FakeJev::start().unwrap();
    // Open, the verdict is Jev's pick of an option (the unified switch,
    // `route_open`): the class list no longer decides, and a config that moved
    // `haikuhi` out of it changes nothing.
    crate::tests_job_ask::pick(&jev, "sonnet", 0.95);
    let r = rig_from(Some(&jev), "fable", |c| {
        c.name_every_provider_key();
        c.routing.classes.routine_coding.profiles = vec!["haiku".into()];
    });
    r.core
        .runner
        .judge
        .job_pack_with(crate::tests_job_ask::model_pack());
    let (_, rec) = run(&r, &params(BRIEF, None)).await;
    assert_eq!(ran_on(&r), "claude-sonnet-5-5");
    assert_eq!(rec.last_target.unwrap().profile, "sonnet");
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

/// A profile asked for is a pin even with a verdict at hand (the pin test
/// above holds the request count); and a place's cap still caps a brief.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_capped_places_task_is_never_routed_above_the_cap() {
    let jev = FakeJev::start().unwrap();
    mode(&jev, "sophisticated", 0.95);
    let r = rig_from(Some(&jev), "sonnet", |_| {});
    r.core.bind_places(vec![BoundPlace {
        persona: None,
        target: "discord:channel:100000000000000099".into(),
        name: "#berth".into(),
        private: true,
        guild: Some("100000000000000004".into()),
        ceiling: Some(PlaceCeiling {
            profile: Some("sonnet".into()),
            ..Default::default()
        }),
    }]);
    let home = r.core.home().unwrap();
    r.core
        .outbox
        .bind_place("channel:100000000000000099", &home.session_id)
        .unwrap();
    let p = params("Weigh two designs for the quay ledger.", None);
    let (_, rec) = run(&r, &p).await;
    assert_eq!(ran_on(&r), "claude-sonnet-5-5");
    assert_eq!(rec.last_target.unwrap().profile, "sonnet");
    assert_eq!(decided(&r.core.store)[0]["reason"], "capped");
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
