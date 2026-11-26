//! The self pipeline through the core (theseus-pw1q.16), over a fake
//! builder (the backlog's own start, no turn), a stand-in host
//! (`capacity::tests::Stand`) and reviews written as self-join writes them:
//! N builds at once and never N+1; the guard holds on low disk, high
//! pressure and full trees with the slots free; the budget's line stops new
//! starts and says so; a rejected review returns once, then parks; a halt
//! stops every build at the next tick; a dropped build's trees are deleted;
//! and, over self-join's throwaway repository, one join pass at a time and a
//! joined item's tree deleted. Every start opens a real tree off a throwaway
//! repository (theseus-qh2tt), so the rig's repo is one.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde_json::{json, Value};
use theseus_protocol::rsi::{
    SelfDropParams, SelfHaltParams, SelfMode, SelfPipelineParams, SelfResumeParams,
};
use theseus_protocol::tasks::{TaskEvidence, TaskState};

use super::{Builder, Seams, Tick};

// Registered here, in a test file, so the tests of `live()` stand with the tests and not with the
// code they judge.
#[path = "tests_jobs.rs"]
mod tests_jobs;

use crate::approval::Answerer;
use crate::fact::rsi_join::SelfReviewVerdict;
use crate::fact::rsi_pr::SelfBranchBuilt;
use crate::ledger::LedgerRow;
use crate::provider::FakeProvider;
use crate::rsi::capacity::tests::Stand;
use crate::rsi::capacity::CPU_LOAD_BUSY;
use crate::rsi::tests_tree::{fixture, hermetic, Fixture};
use crate::store::Store;
use crate::{Config, Core};

const HOUR: u64 = 3_600_000;

/// The backlog's own start, and no turn: what a build does is the test's.
#[derive(Default)]
pub(super) struct Fake {
    pub(super) starts: Mutex<Vec<(String, Option<String>)>>,
    /// The first turn's input of each start.
    pub(super) objectives: Mutex<Vec<String>>,
}

impl Builder for Fake {
    fn name(&self) -> &'static str {
        "fake"
    }

    fn start(
        &self,
        core: &Arc<Core>,
        item: &str,
        findings: Option<&str>,
    ) -> anyhow::Result<Option<String>> {
        let Some(s) = core.backlog_start_item(item)? else {
            return Ok(None);
        };
        self.starts
            .lock()
            .unwrap()
            .push((item.to_string(), findings.map(str::to_string)));
        self.objectives.lock().unwrap().push(s.objective);
        Ok(Some(s.session_id))
    }
}

pub(super) struct Rig {
    pub(super) core: Arc<Core>,
    pub(super) fake: Arc<Fake>,
    host: Arc<Stand>,
    pub(super) grace: Duration,
    pub(super) f: Fixture,
}

/// The three problems the rig's night files, as the ledger rows that show them.
pub(super) fn causes() -> [(&'static str, Value); 3] {
    [
        (
            "turn.failed",
            json!({"loops": 2, "reason": "timeout: no answer", "cost_usd": 0.4}),
        ),
        (
            "action.failed",
            json!({"producer": "proc", "duration_ms": 10}),
        ),
        (
            "turn.failed",
            json!({"loops": 1, "reason": "overloaded", "cost_usd": 0.1}),
        ),
    ]
}

pub(super) fn rig_with(slots: u32, day_usd: f64) -> Rig {
    let f = fixture();
    let store = Store::open(&f.place.state.join("store")).unwrap();
    let mut cfg = Config::example();
    cfg.server.state_dir = f.place.state.to_string_lossy().into_owned();
    cfg.server.disk_warn_mb = 0;
    cfg.self_improve.mode = SelfMode::Act;
    cfg.self_improve.repo = Some(f.place.repo.to_string_lossy().into_owned());
    cfg.self_improve.backlog_starts = Some(0);
    cfg.self_improve.pipeline_builds = slots;
    cfg.self_improve.budget.day_usd = day_usd;
    let core = Core::build(crate::rpc::Parts::for_tests(
        cfg,
        Arc::new(FakeProvider::default()),
        store,
    ))
    .unwrap();
    core.tools.self_pr.set_env(hermetic());
    core.self_resume(&SelfResumeParams::default(), &Answerer::from("the CLI"))
        .unwrap();
    // Three causes, three items, filed and ranked by a night.
    let t0 = theseus_protocol::now_unix_ms();
    for (kind, data) in causes() {
        let mut row = LedgerRow::named(kind, None, None, data);
        row.at_unix_ms = t0 - HOUR;
        core.store.append_ledger(&row).unwrap();
    }
    let night = core.backlog_night(theseus_protocol::now_unix_ms()).unwrap();
    assert_eq!(night.filed.len(), 3, "{night:?}");
    Rig {
        core,
        fake: Arc::default(),
        host: Arc::default(),
        grace: Duration::from_secs(3600),
        f,
    }
}

impl Rig {
    pub(super) fn seams(&self) -> Seams {
        Seams {
            builder: self.fake.clone(),
            probe: self.host.clone(),
            grace: self.grace,
            ..Seams::default()
        }
    }

    pub(super) async fn tick(&self) -> Tick {
        self.core.pipeline_tick(&self.seams()).await.unwrap()
    }

    pub(super) fn rows(&self, kind: &str) -> Vec<Value> {
        self.core
            .store
            .ledger_tail::<LedgerRow>(10_000)
            .unwrap()
            .into_iter()
            .filter(|(_, r)| r.kind == kind)
            .map(|(_, r)| r.data)
            .collect()
    }

    pub(super) fn state(&self, item: &str) -> TaskState {
        crate::task_graph::get(&self.core.store, item)
            .unwrap()
            .unwrap()
            .state
    }

    /// A green branch for `item`, as self-pr's row says one.
    pub(super) fn built(&self, item: &str, n: u32) -> (String, String) {
        let branch = format!("self/20261015-{}-{n}", crate::rsi::tree::slug(item));
        let head = format!("{n:0>40}");
        self.core.rec(None).record(&SelfBranchBuilt {
            item,
            issue: "theseus-sail1",
            branch: &branch,
            head: Some(&head),
            gate: "green",
            report: None,
            cost_usd: 1.25,
            stopped_at: None,
            why: "Trim the sails.",
            numbers: Value::Null,
        });
        // A green branch closes its tree, as `self.propose_change` does.
        if let Some(t) = crate::rsi::tree::find(&self.f.place, item) {
            crate::rsi::tree::close(&self.f.place, &t).unwrap();
        }
        (branch, head)
    }

    /// A rejecting review, written as self-join writes one: its row, and the
    /// item back to proposed with the reasons in its evidence.
    fn reject(&self, item: &str, branch: &str, head: &str) {
        self.reject_with(item, branch, head, "the test passes without the fix");
    }

    /// [`Self::reject`] with the review's own finding.
    pub(super) fn reject_with(&self, item: &str, branch: &str, head: &str, finding: &str) {
        let fact = SelfReviewVerdict {
            item: item.into(),
            branch: branch.into(),
            head: head.into(),
            reviewer: "fable (claude-fable-5-2)".into(),
            model: "claude-fable-5-2".into(),
            asked: "claude-fable-5-2".into(),
            verdict: "reject".into(),
            planted_revert: "failed".into(),
            proof_check: None,
            reasons: vec![finding.into()],
            cost_usd: 0.5,
        };
        let mut t = crate::task_graph::get(&self.core.store, item)
            .unwrap()
            .unwrap();
        t.state = TaskState::Proposed;
        t.session = None;
        t.version += 1;
        t.evidence.push(TaskEvidence {
            node: None,
            identity: format!("review:{branch}@{}", &head[..12]),
            note: Some(format!("rejected by fable: {finding}")),
            by: "self-join".into(),
            at_ms: theseus_protocol::now_unix_ms(),
        });
        let rec = self.core.rec(None);
        self.core
            .store
            .append(&[
                rec.row(&fact).unwrap(),
                crate::task_graph::record(&t).unwrap(),
            ])
            .unwrap();
    }

    pub(super) fn view(&self) -> theseus_protocol::rsi::SelfPipelineResult {
        self.core
            .self_pipeline_with(&SelfPipelineParams { week: true }, &self.seams())
            .unwrap()
    }
}

/// With two slots and three ready items, two build at once and the third
/// waits with one `held` row however often the pipeline ticks; a green
/// branch frees its slot and the third starts.
#[tokio::test]
async fn n_builds_run_at_once_and_never_n_plus_one() {
    let r = rig_with(2, 30.0);
    let t = r.tick().await;
    assert_eq!(t.started.len(), 2, "{t:?}");
    assert_eq!(t.hold.as_deref(), Some("slots"));
    for _ in 0..3 {
        let t = r.tick().await;
        assert!(t.started.is_empty(), "never a third: {t:?}");
    }
    assert_eq!(r.rows("self.pipeline.started").len(), 2);
    let held = r.rows("self.pipeline.held");
    assert_eq!(held.len(), 1, "one row while the reason holds: {held:?}");
    assert_eq!(held[0]["hold"], "slots");
    let v = r.view();
    assert_eq!((v.ready.len(), v.building.len()), (1, 2));
    assert_eq!(v.room.hold.as_deref(), Some("slots"));
    // A green branch frees its slot: the third starts, the branch waits on
    // its review.
    let first = t_first(&r);
    r.built(&first, 1);
    let t = r.tick().await;
    assert_eq!(t.started.len(), 1, "{t:?}");
    assert!(t.join_due);
    let v = r.view();
    assert_eq!(
        (v.ready.len(), v.building.len(), v.reviewing.len()),
        (0, 2, 1)
    );
    assert_eq!(r.fake.starts.lock().unwrap().len(), 3);
    let w = v.week.unwrap();
    assert_eq!(w.days.last().unwrap().started, 3);
    assert_eq!(w.days.last().unwrap().built, 1);
}

fn t_first(r: &Rig) -> String {
    r.fake.starts.lock().unwrap()[0].0.clone()
}

/// The guard reads the host with the slots free: low disk, memory pressure
/// and CPU load each hold the start with its own row, as do open trees; with
/// room again it starts.
#[tokio::test]
async fn the_guard_holds_on_low_disk_pressure_and_trees_with_the_slots_free() {
    let r = rig_with(2, 30.0);
    *r.host.free_gb.lock().unwrap() = Some(39);
    let t = r.tick().await;
    assert!(t.started.is_empty());
    assert_eq!(t.hold.as_deref(), Some("disk"));
    *r.host.free_gb.lock().unwrap() = Some(400);
    *r.host.memory.lock().unwrap() = Some(25.0);
    assert_eq!(r.tick().await.hold.as_deref(), Some("memory"));
    *r.host.memory.lock().unwrap() = Some(1.0);
    // The CPU hold reads the load a core, not pressure (theseus-1eemp).
    *r.host.load.lock().unwrap() = Some(2.0);
    assert_eq!(r.tick().await.hold.as_deref(), Some("cpu"));
    *r.host.load.lock().unwrap() = None;
    let place = r.core.tools.self_pr.place("/nowhere");
    for slug in ["tsk-left-a", "tsk-left-b"] {
        std::fs::create_dir_all(place.trees().join(slug)).unwrap();
    }
    let t = r.tick().await;
    assert_eq!(t.hold.as_deref(), Some("slots"), "two trees fill two slots");
    assert!(r.fake.starts.lock().unwrap().is_empty());
    std::fs::remove_dir_all(place.trees()).unwrap();
    let t = r.tick().await;
    assert_eq!(t.started.len(), 2, "{t:?}");
    let held: Vec<String> = r
        .rows("self.pipeline.held")
        .iter()
        .map(|h| h["hold"].as_str().unwrap().to_string())
        .collect();
    assert_eq!(held, ["disk", "memory", "cpu", "slots", "slots"]);
    let disk = &r.rows("self.pipeline.held")[0];
    assert!(
        disk["why"]
            .as_str()
            .unwrap()
            .contains("39.0 GB free, under [self] min_free_gb = 40"),
        "{disk}"
    );
}

/// What `theseus self pipeline` is given is what the guard read: the load a
/// core the CPU hold keys on, and the line it holds at, whatever CPU pressure
/// says. The room once carried only pressure, which the guard had stopped
/// reading, so the surface printed a number no start waited on
/// (theseus-1eemp, 2026-11-11). These assert on the room's fields, not on
/// the words around them.
#[tokio::test]
async fn the_room_carries_the_load_the_guard_holds_on_and_its_line() {
    let r = rig_with(2, 30.0);
    // Pressure says saturated, the load says room: a start may go, and the
    // room shows the load it went on.
    *r.host.cpu.lock().unwrap() = Some(94.03);
    *r.host.load.lock().unwrap() = Some(0.5);
    let v = r.view();
    assert_eq!(v.room.hold, None, "{:?}", v.room);
    assert_eq!(v.room.cpu_load, Some(0.5));
    assert_eq!(v.room.cpu_load_busy, Some(CPU_LOAD_BUSY));
    // Pressure says idle, the load says saturated: the start waits on the
    // room's own load, which is at or over the room's own line, and the
    // sentence names that figure.
    *r.host.cpu.lock().unwrap() = Some(3.2);
    *r.host.load.lock().unwrap() = Some(2.07);
    let v = r.view();
    assert_eq!(v.room.hold.as_deref(), Some("cpu"), "{:?}", v.room);
    let (load, line) = (v.room.cpu_load.unwrap(), v.room.cpu_load_busy.unwrap());
    assert_eq!(load, 2.07);
    assert!(load >= line, "held at {load} under {line}");
    assert!(
        v.room
            .why
            .as_deref()
            .unwrap()
            .contains(&format!("{load:.2}")),
        "{:?}",
        v.room
    );
    // The line is the one the guard holds at: just under it starts.
    *r.host.load.lock().unwrap() = Some(CPU_LOAD_BUSY - 0.01);
    let v = r.view();
    assert_eq!(v.room.hold, None, "{:?}", v.room);
    assert!(v.room.cpu_load.unwrap() < v.room.cpu_load_busy.unwrap());
    // A load that cannot be read is no figure, not a made-up one.
    *r.host.load.lock().unwrap() = None;
    let v = r.view();
    assert_eq!((v.room.hold, v.room.cpu_load), (None, None));
}

/// At the self budget's day line no build starts, and the row says why.
#[tokio::test]
async fn the_budgets_day_line_stops_new_starts_and_says_so() {
    let r = rig_with(2, 1.0);
    let b = &r.core.runner.self_budget;
    let crate::rsi::budget::Reserved::Granted(res) = b.reserve("test", None, 1.0).unwrap() else {
        panic!("the reservation fits the line");
    };
    b.settle(res, 1.0).unwrap();
    let t = r.tick().await;
    assert!(t.started.is_empty(), "{t:?}");
    assert_eq!(t.hold.as_deref(), Some("budget"));
    let held = r.rows("self.pipeline.held");
    assert_eq!(held.len(), 1);
    assert!(
        held[0]["why"]
            .as_str()
            .unwrap()
            .contains("day line is reached: $1.00 of $1.00"),
        "{held:?}"
    );
    assert_eq!(r.view().room.hold.as_deref(), Some("budget"));
}

/// A rejected review sends its item back to ready once, with the findings
/// in its next build; a second rejection drops it with a notice (it used
/// to park it for the owner).
#[tokio::test]
async fn a_rejected_review_returns_once_then_parks_for_the_owner() {
    let r = rig_with(1, 30.0);
    r.tick().await;
    let item = t_first(&r);
    let (branch, head) = r.built(&item, 1);
    r.reject(&item, &branch, &head);
    let t = r.tick().await;
    assert_eq!(
        t.started,
        std::slice::from_ref(&item),
        "built again at once: {t:?}"
    );
    let returned = r.rows("self.pipeline.returned");
    assert_eq!(returned.len(), 1);
    assert_eq!(returned[0]["branch"], branch.as_str());
    let starts = r.fake.starts.lock().unwrap().clone();
    assert_eq!(starts.len(), 2);
    assert!(
        starts[1]
            .1
            .as_deref()
            .unwrap()
            .contains("the test passes without the fix"),
        "{starts:?}"
    );
    // Ticking again says nothing more.
    r.tick().await;
    assert_eq!(r.rows("self.pipeline.returned").len(), 1);
    let (branch, head) = r.built(&item, 2);
    r.reject(&item, &branch, &head);
    let t = r.tick().await;
    assert!(!t.started.contains(&item), "{t:?}");
    // Dropped with a notice, never parked for the owner (theseus-o1h3f.6).
    assert_eq!(r.state(&item), TaskState::Failed);
    assert_eq!(r.rows("self.pipeline.parked").len(), 1);
    assert_eq!(r.rows("self.pipeline.returned").len(), 1);
    let v = r.view();
    assert!(!v.held.iter().any(|h| h.item == item), "{:?}", v.held);
    assert!(!v.ready.iter().any(|h| h.item == item));
}

/// A halt stops every build in flight at the next tick (each returns to
/// ready) and starts nothing until the owner's resume.
#[tokio::test]
async fn a_halt_stops_every_build_at_the_next_tick() {
    let r = rig_with(2, 30.0);
    let t = r.tick().await;
    assert_eq!(t.started.len(), 2);
    r.core
        .self_halt(&SelfHaltParams::default(), &Answerer::from("the CLI"))
        .unwrap();
    let t = r.tick().await;
    assert_eq!(t.gate, "halted");
    assert_eq!(t.stopped.len(), 2, "{t:?}");
    for item in &t.stopped {
        assert_eq!(r.state(item), TaskState::Proposed);
    }
    let ended = r.rows("self.pipeline.ended");
    assert_eq!(ended.len(), 2);
    assert!(ended.iter().all(|e| e["reason"] == "halted"));
    let t = r.tick().await;
    assert!(t.started.is_empty() && t.stopped.is_empty(), "{t:?}");
    assert_eq!(r.fake.starts.lock().unwrap().len(), 2);
    r.core
        .self_resume(&SelfResumeParams::default(), &Answerer::from("the CLI"))
        .unwrap();
    let t = r.tick().await;
    assert_eq!(t.started.len(), 2, "the resume starts them again: {t:?}");
}

/// A build whose turn ended with no green branch frees its slot, its item
/// failed; a dropped item's build tree is deleted, never moved.
#[tokio::test]
async fn ended_and_dropped_builds_free_their_slot_and_their_trees() {
    let mut r = rig_with(1, 30.0);
    r.tick().await;
    let first = t_first(&r);
    let place = r.core.tools.self_pr.place("/nowhere");
    let target = place.targets().join(crate::rsi::tree::slug(&first));
    std::fs::create_dir_all(target.join("debug")).unwrap();
    std::fs::write(target.join("debug").join("big"), "build output").unwrap();
    // The builder's own build folder (`rsi::builder_dir`) goes with the item as well.
    let builder = crate::rsi::builder_dir::dir_in(&place.state, &crate::rsi::tree::slug(&first));
    std::fs::create_dir_all(builder.join("debug")).unwrap();
    std::fs::write(builder.join("debug").join("big"), "what a builder compiled").unwrap();
    r.core
        .self_drop(
            &SelfDropParams {
                item: first.clone(),
                ..Default::default()
            },
            &Answerer::from("the CLI"),
        )
        .unwrap();
    let t = r.tick().await;
    assert!(!target.exists(), "the build tree is deleted");
    assert!(!builder.exists(), "the builder's build folder is deleted");
    let cleared = r.rows("self.pipeline.cleared");
    assert_eq!(cleared.len(), 1);
    assert_eq!(cleared[0]["reason"], "dropped");
    assert_eq!(t.started.len(), 1, "the slot is free: {t:?}");
    let second = r.fake.starts.lock().unwrap()[1].0.clone();
    assert!(!place.root().join("trash").exists());
    // Its turn ends with no green branch: past the grace, it failed.
    r.grace = Duration::ZERO;
    let t = r.tick().await;
    assert_eq!(r.state(&second), TaskState::Failed);
    let ended = r.rows("self.pipeline.ended");
    assert_eq!(ended.len(), 1);
    assert_eq!(ended[0]["item"], second.as_str());
    assert_eq!(t.started.len(), 1, "the third starts in its slot: {t:?}");
}

/// With the mode off a tick reads nothing and writes nothing.
#[tokio::test]
async fn mode_off_does_nothing() {
    let r = rig_with(2, 30.0);
    let mut cfg = (*r.core.cfg).clone();
    cfg.self_improve.mode = SelfMode::Off;
    let off = Core::build(crate::rpc::Parts::for_tests(
        cfg,
        Arc::new(FakeProvider::default()),
        r.core.store.clone(),
    ))
    .unwrap();
    let t = off.pipeline_tick(&r.seams()).await.unwrap();
    assert_eq!(t.gate, "off");
    assert!(t.started.is_empty() && t.said.is_empty());
    assert!(r.rows("self.pipeline.held").is_empty());
}

/// Two join passes at once (the pipeline's and the join tender's) run one
/// after the other: the branch is reviewed once, its card asked once, and
/// after the owner's yes joined once; the pipeline's own pass is not spawned
/// while one runs.
#[tokio::test]
async fn one_join_pass_at_a_time() {
    use crate::rsi::join::tests::{rig, Slow, PROVED, SONNET};
    let j = rig(SelfMode::Act);
    let (branch, _) = j.seed("trim-the-sails", PROVED, SONNET);
    let slow = Arc::new(Slow::default());
    let seams = crate::rsi::join::Seams {
        reviewer: slow.clone(),
        ..j.seams()
    };
    let (a, b) = tokio::join!(j.core.self_join_pass(&seams), j.core.self_join_pass(&seams));
    let (a, b) = (a.unwrap(), b.unwrap());
    assert_eq!(a.parked.len() + b.parked.len(), 2, "{a:?} {b:?}");
    assert_eq!(
        crate::rsi::join::tests_publish::owner_says_yes(&j.core),
        1,
        "one card for the head"
    );
    let (a, b) = tokio::join!(j.core.self_join_pass(&seams), j.core.self_join_pass(&seams));
    let (a, b) = (a.unwrap(), b.unwrap());
    assert_eq!(slow.asks(), 1, "one review of one head");
    assert_eq!(slow.most(), 1, "never two reviews at once");
    assert_eq!(j.rows("self.review.verdict").len(), 1);
    assert_eq!(j.rows("self.joined").len(), 1);
    let stops: Vec<_> = (j.rows("self.join.stopped").into_iter())
        .filter(|d| d["phase"] != crate::rsi::join::publish::PHASE)
        .collect();
    assert!(stops.is_empty(), "no second join tried: {stops:?}");
    assert_eq!(a.joined.len() + b.joined.len(), 1, "{a:?} {b:?}");
    assert!(a
        .joined
        .iter()
        .chain(&b.joined)
        .all(|(br, _)| *br == branch));
    // The pipeline's pass is not spawned beside one running.
    let p = Seams {
        join: seams.clone(),
        ..Seams::default()
    };
    // The first is held inside its pass (the join tender's pass holds the
    // lock), so it is surely in flight while the second is asked.
    let held = j.core.rsi.pipeline.joins.lock().await;
    let (x, y) = tokio::join!(j.core.pipeline_join(&p), async {
        let y = j.core.pipeline_join(&p).await;
        drop(held);
        y
    });
    assert!(
        x.unwrap().is_some(),
        "the first pass ran once the lock was free"
    );
    assert!(
        y.unwrap().is_none(),
        "one asked while another runs is not run"
    );
}

/// A joined item's leftover tree is deleted by the next tick.
#[tokio::test]
async fn a_joined_items_tree_is_deleted() {
    use crate::rsi::join::tests::{rig, PROVED, SONNET};
    let j = rig(SelfMode::Act);
    let item = "trim-the-sails";
    j.seed(item, PROVED, SONNET);
    let place = j
        .core
        .tools
        .self_pr
        .place(&j.f.place.repo.display().to_string());
    let t = crate::rsi::tree::open(&place, item, theseus_protocol::now_unix_ms()).unwrap();
    use crate::rsi::join::tests_publish::PassYes as _;
    let pass = j.core.self_join_pass_yes(&j.seams()).await.unwrap();
    assert_eq!(pass.joined.len(), 1, "{pass:?}");
    let seams = Seams {
        probe: Arc::new(Stand::default()),
        join: j.seams(),
        ..Seams::default()
    };
    j.core.pipeline_tick(&seams).await.unwrap();
    assert!(
        !t.dir.exists() && !t.target.exists(),
        "the trees are deleted"
    );
    let cleared = j.rows("self.pipeline.cleared");
    assert_eq!(cleared.len(), 1, "{cleared:?}");
    assert_eq!(cleared[0]["reason"], "joined");
}

#[tokio::test]
async fn a_deferred_halt_sweep_is_retried() {
    let r = rig_with(1, 30.0);
    assert_eq!(r.tick().await.started.len(), 1);
    r.core
        .self_halt(&SelfHaltParams::default(), &Answerer::from("the CLI"))
        .unwrap();
    r.core.store.shape_window(true);
    assert!(r.tick().await.stopped.is_empty());
    r.core.store.shape_window(false);
    assert_eq!(r.tick().await.stopped.len(), 1);
}

#[tokio::test]
async fn a_failed_halt_sweep_is_not_marked_complete() {
    let r = rig_with(1, 30.0);
    r.tick().await;
    r.core
        .self_halt(&SelfHaltParams::default(), &Answerer::from("the CLI"))
        .unwrap();
    r.core.store.fail_ledger_pages();
    assert!(r.core.pipeline_tick(&r.seams()).await.is_err());
    assert!(!r
        .core
        .rsi
        .pipeline
        .halt_swept
        .load(std::sync::atomic::Ordering::SeqCst));
}

#[tokio::test]
async fn a_staged_ended_build_cannot_overwrite_the_owners_drop() {
    let r = rig_with(1, 100.0);
    let first = r.tick().await;
    let item = &first.started[0];
    let mut q = super::queues::Queues::default();
    let old = crate::task_graph::get(&r.core.store, item)
        .unwrap()
        .unwrap();
    q.tasks.push(old.clone());
    q.ended.push(super::queues::Entry {
        item: item.clone(),
        ..Default::default()
    });
    let mut w = super::Write {
        mark: super::Mark::read(&r.core.store).unwrap(),
        frame: vec![],
        expected: vec![],
        said: vec![],
        dirty: false,
        told: vec![],
    };
    r.core.pipeline_ended(&r.f.place, &q, &mut w).await.unwrap();
    r.core
        .self_drop(
            &SelfDropParams {
                item: item.clone(),
                ..Default::default()
            },
            &Answerer::from("the CLI"),
        )
        .unwrap();
    let dropped = crate::task_graph::get(&r.core.store, item)
        .unwrap()
        .unwrap();
    let _ = r.core.pipeline_write(&mut w);
    assert_eq!(
        crate::task_graph::get(&r.core.store, item)
            .unwrap()
            .unwrap(),
        dropped
    );
}

/// While a join waits for the owner's card (issue 53), the pipeline still starts a build: a second item's tree is
/// opened and its builder's session begun by the backlog's own start, in the same fixture as the parked join, and
/// nothing is published.
#[tokio::test]
async fn a_build_starts_while_a_join_waits_for_the_owners_card() {
    use crate::rsi::join::tests::{rig_with as join_rig, PROVED, SONNET};
    let j = join_rig(SelfMode::Act, |c| {
        c.server.disk_warn_mb = 0;
        c.self_improve.backlog_starts = Some(0);
        c.self_improve.pipeline_builds = 2;
        c.self_improve.budget.day_usd = 30.0;
    });
    let main = git_main(&j.f.upstream);
    let (branch, head) = j.seed("trim-the-sails", PROVED, SONNET);
    let parked = j.core.self_join_pass(&j.seams()).await.unwrap();
    assert_eq!(parked.parked, [(branch, head)], "{parked:?}");
    let t0 = theseus_protocol::now_unix_ms();
    for (kind, data) in causes() {
        let mut row = LedgerRow::named(kind, None, None, data);
        row.at_unix_ms = t0 - HOUR;
        j.core.store.append_ledger(&row).unwrap();
    }
    let night = j
        .core
        .backlog_night(theseus_protocol::now_unix_ms())
        .unwrap();
    assert!(!night.filed.is_empty(), "{night:?}");
    let fake = Arc::new(Fake::default());
    let seams = Seams {
        builder: fake.clone(),
        probe: Arc::new(Stand::default()),
        join: j.seams(),
        grace: Duration::from_secs(3600),
        ..Seams::default()
    };
    let t = j.core.pipeline_tick(&seams).await.unwrap();
    assert!(!t.started.is_empty(), "a build started: {t:?}");
    assert_eq!(fake.starts.lock().unwrap().len(), t.started.len());
    let waiting = (j.core.confirm_list().unwrap().into_iter())
        .filter(|c| c.tool == crate::rsi::join::publish::ACK)
        .count();
    assert_eq!(waiting, 1, "the card still waits");
    assert_eq!(git_main(&j.f.upstream), main, "nothing published");
}

fn git_main(upstream: &std::path::Path) -> String {
    crate::rsi::tests_tree::git(upstream, &["rev-parse", "refs/heads/main"])
}
