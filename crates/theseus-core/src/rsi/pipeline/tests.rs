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
use crate::approval::Answerer;
use crate::fact::rsi_join::SelfReviewVerdict;
use crate::fact::rsi_pr::SelfBranchBuilt;
use crate::ledger::LedgerRow;
use crate::provider::FakeProvider;
use crate::rsi::capacity::tests::Stand;
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
    for (kind, data) in [
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
    ] {
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
            reviewer: "fable (claude-fable-5-1)".into(),
            model: "claude-fable-5-1".into(),
            verdict: "reject".into(),
            planted_revert: "failed".into(),
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
            .self_pipeline(&SelfPipelineParams { week: true })
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

/// The guard reads the host with the slots free: low disk, memory and CPU
/// pressure each hold the start with its own row, as do open trees; with
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
    *r.host.cpu.lock().unwrap() = Some(90.0);
    assert_eq!(r.tick().await.hold.as_deref(), Some("cpu"));
    *r.host.cpu.lock().unwrap() = None;
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
/// after the other: the branch is reviewed once and joined once, and the
/// pipeline's own pass is not spawned while one runs.
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
    assert_eq!(slow.asks(), 1, "one review of one head");
    assert_eq!(slow.most(), 1, "never two reviews at once");
    assert_eq!(j.rows("self.review.verdict").len(), 1);
    assert_eq!(j.rows("self.joined").len(), 1);
    assert!(
        j.rows("self.join.stopped").is_empty(),
        "no second join tried"
    );
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
    let pass = j.core.self_join_pass(&j.seams()).await.unwrap();
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
