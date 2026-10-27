//! The self backlog (theseus-pw1q.7), through the core: a seeded day's
//! ledger files the right items, deduplicated, with their evidence; the same
//! cause tomorrow adds evidence and files nothing new; the rank is
//! deterministic; a keel-marked item never starts; `backlog_starts` caps the
//! starts; a dropped item never comes back; a non-owner's drop is refused
//! with an `approval.refused` row; and with the mode off or the switch
//! halted a night reads nothing and writes nothing.

use std::sync::Arc;

use serde_json::{json, Value};
use theseus_protocol::rsi::{
    SelfBacklogParams, SelfDropParams, SelfMode, SelfMoveParams, SelfResumeParams,
};
use theseus_protocol::tasks::TaskState;

use super::tests::test_core;
use super::{Core, Parts};
use crate::approval::{Answerer, Refusal, Surface};
use crate::fact::rsi_pr::SelfBranchBuilt;
use crate::ledger::LedgerRow;
use crate::provider::FakeProvider;
use crate::test_support::rows;

const OWNER: u64 = 271_828_182_845_904_523;
const SOMEONE: u64 = 314_159_265_358_979_323;
const DAY: u64 = 86_400_000;
const HOUR: u64 = 3_600_000;

fn core_with(mode: SelfMode, starts: u32, max_open: u32) -> Arc<Core> {
    core_starting(mode, Some(starts), 1, max_open)
}

/// [`core_with`] with `backlog_starts` as the config has it (unset: the
/// pipeline's free slots) and `pipeline_builds` slots.
fn core_starting(mode: SelfMode, starts: Option<u32>, slots: u32, max_open: u32) -> Arc<Core> {
    let base = test_core("the tide is out");
    let mut cfg = (*base.cfg).clone();
    cfg.self_improve.mode = mode;
    // The host's disk is no test's business: 0 never warns.
    cfg.server.disk_warn_mb = 0;
    cfg.self_improve.backlog_starts = starts;
    cfg.self_improve.pipeline_builds = slots;
    cfg.self_improve.backlog_max_open = max_open;
    cfg.places.owner = Some(vec![format!("discord:{OWNER}")]);
    // A started item opens a tree (theseus-qh2tt): the repo is a throwaway
    // one, kept for the process (a few kilobytes in the temp dir).
    let repo = crate::rsi::tests_tree::fixture();
    cfg.self_improve.repo = Some(repo.place.repo.display().to_string());
    std::mem::forget(repo);
    let core = Core::build(Parts::for_tests(
        cfg,
        Arc::new(FakeProvider::default()),
        base.store.clone(),
    ))
    .unwrap();
    core.tools
        .self_pr
        .set_env(crate::rsi::tests_tree::hermetic());
    core
}

/// A core that acts, with the switch released.
fn acting(starts: u32, max_open: u32) -> Arc<Core> {
    let c = core_with(SelfMode::Act, starts, max_open);
    c.self_resume(&SelfResumeParams::default(), &Answerer::from("the CLI"))
        .unwrap();
    c
}

fn cli() -> Answerer {
    Answerer::from("the CLI")
}

fn stranger() -> Answerer {
    Answerer {
        label: format!("discord:{SOMEONE}"),
        surface: Surface::Discord,
        discord: Some(theseus_protocol::DiscordOrigin {
            user_id: SOMEONE.to_string(),
            channel_id: "900000000000000007".into(),
            guild_id: Some("900000000000000001".into()),
        }),
        agent: None,
    }
}

/// A ledger row at `at` ms.
fn seed(c: &Core, kind: &str, data: Value, at: u64) -> u64 {
    let mut row = LedgerRow::named(kind, None, None, data);
    row.at_unix_ms = at;
    c.store.append_ledger(&row).unwrap()
}

fn failed_turn(c: &Core, reason: &str, at: u64) -> u64 {
    seed(
        c,
        "turn.failed",
        json!({"loops": 2, "reason": reason, "cost_usd": 0.4, "tool_calls": 1}),
        at,
    )
}

fn invalid_input(c: &Core, tool: &str, at: u64) {
    seed(
        c,
        "tool.invalid_input",
        json!({"tool": tool, "reason": "not json"}),
        at,
    );
}

/// Now, read after the seeding: the index dates a row by the frame's write
/// time, so a night reads the rows written before it.
fn tick() -> u64 {
    theseus_protocol::now_unix_ms()
}

fn listed(c: &Core) -> Vec<theseus_protocol::rsi::SelfBacklogItem> {
    c.self_backlog(&SelfBacklogParams { all: true })
        .unwrap()
        .items
}

/// `listed` with every score read at `now_ms`: a score decays with the clock,
/// so two reads at the wall clock's two instants can differ by one
/// milli-point (theseus-w68w5); at one instant they agree exactly.
fn listed_at(c: &Core, now_ms: u64) -> Vec<theseus_protocol::rsi::SelfBacklogItem> {
    c.self_backlog_at(&SelfBacklogParams { all: true }, now_ms)
        .unwrap()
        .items
}

/// One day of the ledger: three failed turns (one cause), two invalid inputs
/// (a noisy cause that needs two), one refused act twice (a keel cause).
fn seed_day(c: &Core, t0: u64) {
    for i in 0..3 {
        failed_turn(
            c,
            "timeout: the provider did not answer",
            t0 - (i + 1) * HOUR,
        );
    }
    invalid_input(c, "fs.read", t0 - 5 * HOUR);
    invalid_input(c, "fs.read", t0 - 6 * HOUR);
    invalid_input(c, "text.diff", t0 - 7 * HOUR);
    for i in 0..2 {
        seed(
            c,
            "approval.refused",
            json!({"act": "self.resume", "who": "discord:1", "why": "not the owner"}),
            t0 - (8 + i) * HOUR,
        );
    }
}

#[test]
fn a_seeded_days_ledger_files_the_right_items_with_their_evidence() {
    let c = acting(0, 20);
    let t0 = theseus_protocol::now_unix_ms();
    seed_day(&c, t0);
    let n1 = tick();
    let night = c.backlog_night(n1).unwrap();
    assert_eq!(night.gate, "allowed");
    let items = listed(&c);
    let causes: Vec<&str> = items.iter().map(|i| i.cause.as_str()).collect();
    assert!(causes.contains(&"turn.failed:timeout"), "{causes:?}");
    assert!(causes.contains(&"tool.invalid_input:fs.read"), "{causes:?}");
    assert!(
        !causes.contains(&"tool.invalid_input:text.diff"),
        "one sighting of a noisy cause files nothing: {causes:?}"
    );
    assert_eq!(items.len(), 3, "{causes:?}");
    let turn = items
        .iter()
        .find(|i| i.cause == "turn.failed:timeout")
        .unwrap();
    assert_eq!(turn.count, 3);
    assert_eq!(turn.evidence.len(), 3, "{:?}", turn.evidence);
    assert!(
        turn.evidence.iter().all(|e| e.starts_with("ledger:")),
        "{:?}",
        turn.evidence
    );
    assert_eq!(turn.state, "proposed");
    assert_eq!(turn.keel, None);
    let refused = items
        .iter()
        .find(|i| i.cause == "approval.refused:self.resume")
        .unwrap();
    assert_eq!(refused.state, "waiting_human");
    assert_eq!(refused.keel.as_deref(), Some("security"));
    // Every item is a task under one parent, filed by "self", not started.
    let tasks = crate::task_graph::all(&c.store).unwrap();
    let parents: Vec<_> = tasks
        .iter()
        .filter(|t| t.title == super::super::rsi::backlog::PARENT_TITLE)
        .collect();
    assert_eq!(parents.len(), 1);
    for t in tasks.iter().filter(|t| t.parent.is_some()) {
        assert_eq!(t.parent.as_deref(), Some(parents[0].id.as_str()));
        assert_eq!(t.origin.principal, "self");
        assert!(t.origin.by_model && t.session.is_none());
    }
    assert_eq!(rows(&c, "self.backlog.filed").len(), 3);
    assert_eq!(rows(&c, "self.backlog.ranked").len(), 3);
    assert!(
        night.started.is_empty(),
        "backlog_starts = 0 starts nothing"
    );
}

#[test]
fn the_same_cause_tomorrow_adds_evidence_and_files_nothing_new() {
    let c = acting(0, 20);
    let t0 = theseus_protocol::now_unix_ms();
    seed_day(&c, t0);
    let n1 = tick();
    let first = c.backlog_night(n1).unwrap();
    assert_eq!(first.filed.len(), 3);
    let before = listed(&c);
    let tasks_before = crate::task_graph::all(&c.store).unwrap().len();
    failed_turn(&c, "timeout: again", n1 + 3 * HOUR);
    failed_turn(&c, "timeout: and again", n1 + 4 * HOUR);
    let second = c.backlog_night(n1 + DAY).unwrap();
    assert!(second.filed.is_empty(), "{second:?}");
    assert_eq!(second.updated.len(), 1);
    assert_eq!(
        crate::task_graph::all(&c.store).unwrap().len(),
        tasks_before
    );
    let after = listed(&c);
    let was = before
        .iter()
        .find(|i| i.cause == "turn.failed:timeout")
        .unwrap();
    let now = after
        .iter()
        .find(|i| i.cause == "turn.failed:timeout")
        .unwrap();
    assert_eq!(now.id, was.id);
    assert_eq!(now.count, 5);
    assert_eq!(now.evidence.len(), was.evidence.len() + 2);
    // A night run twice over the same window adds nothing.
    let again = c.backlog_night(n1 + DAY).unwrap();
    assert!(
        again.filed.is_empty() && again.updated.is_empty(),
        "{again:?}"
    );
}

#[test]
fn the_rank_is_a_function_of_the_rows_alone() {
    let order = |c: &Core| -> Vec<String> {
        c.self_backlog(&SelfBacklogParams::default())
            .unwrap()
            .items
            .into_iter()
            .map(|i| format!("{}:{}", i.rank, i.cause))
            .collect()
    };
    let (a, b) = (acting(0, 20), acting(0, 20));
    let t0 = theseus_protocol::now_unix_ms();
    seed_day(&a, t0);
    // The same rows written in the other order.
    for i in (0..3).rev() {
        failed_turn(
            &b,
            "timeout: the provider did not answer",
            t0 - (i + 1) * HOUR,
        );
    }
    for i in (0..2).rev() {
        seed(
            &b,
            "approval.refused",
            json!({"act": "self.resume", "who": "x", "why": "y"}),
            t0 - (8 + i) * HOUR,
        );
    }
    invalid_input(&b, "text.diff", t0 - 7 * HOUR);
    invalid_input(&b, "fs.read", t0 - 6 * HOUR);
    invalid_input(&b, "fs.read", t0 - 5 * HOUR);
    let n1 = tick();
    a.backlog_night(n1).unwrap();
    b.backlog_night(n1).unwrap();
    assert_eq!(order(&a), order(&b));
    assert_eq!(
        order(&a)[0],
        "1:turn.failed:timeout",
        "a person waited, thrice: first"
    );
}

#[test]
fn a_keel_marked_item_never_starts_and_the_starts_are_capped() {
    let c = acting(1, 20);
    let t0 = theseus_protocol::now_unix_ms();
    seed_day(&c, t0);
    // A third unmarked cause, to show the cap: only one of the two start.
    seed(
        &c,
        "action.failed",
        json!({"producer": "proc", "duration_ms": 10}),
        t0 - HOUR,
    );
    let n1 = tick();
    let night = c.backlog_night(n1).unwrap();
    assert_eq!(night.started.len(), 1, "backlog_starts = 1: {night:?}");
    let started = &night.started[0];
    let items = listed(&c);
    let marked = items
        .iter()
        .find(|i| i.cause == "approval.refused:self.resume")
        .unwrap();
    assert_eq!(
        marked.state, "waiting_human",
        "a keel item waits for the owner"
    );
    assert!(marked.session.is_none());
    let running: Vec<_> = items.iter().filter(|i| i.state == "in_progress").collect();
    assert_eq!(running.len(), 1);
    assert_eq!(running[0].id, started.item);
    assert_eq!(rows(&c, "self.backlog.started").len(), 1);
    // The session runs under the reserved principal, with no owner handle.
    let task = crate::task_graph::get(&c.store, &started.item)
        .unwrap()
        .unwrap();
    assert_eq!(task.state, TaskState::InProgress);
    let session = task.session.unwrap();
    let exec = c
        .kernel
        .open_executions()
        .unwrap()
        .into_iter()
        .find(|e| e.session_id == session)
        .expect("the item's session has an execution");
    assert_eq!(exec.authority.principal, "self");
    assert!(
        started.objective.contains("self.propose_change"),
        "{}",
        started.objective
    );
    // A second night starts the next one, never the marked one.
    let second = c.backlog_night(n1 + DAY).unwrap();
    assert_eq!(second.started.len(), 1);
    let third = c.backlog_night(n1 + 2 * DAY).unwrap();
    assert_eq!(
        third.started.len(),
        1,
        "the third unmarked cause: {third:?}"
    );
    let fourth = c.backlog_night(n1 + 3 * DAY).unwrap();
    assert!(
        fourth.started.is_empty(),
        "only the keel item is left: {fourth:?}"
    );
    assert_eq!(rows(&c, "self.backlog.started").len(), 3);
    let after = listed(&c);
    assert!(after
        .iter()
        .any(|i| i.cause == "approval.refused:self.resume" && i.state == "waiting_human"));
}

#[test]
fn a_dropped_item_never_comes_back() {
    let c = acting(0, 20);
    let t0 = theseus_protocol::now_unix_ms();
    seed_day(&c, t0);
    let n1 = tick();
    c.backlog_night(n1).unwrap();
    let item = listed(&c)
        .into_iter()
        .find(|i| i.cause == "turn.failed:timeout")
        .unwrap();
    let r = c
        .self_drop(
            &SelfDropParams {
                item: item.id.clone(),
                ..Default::default()
            },
            &cli(),
        )
        .unwrap();
    assert!(r.changed && r.cause == "turn.failed:timeout");
    assert!(
        !c.self_drop(
            &SelfDropParams {
                item: item.id.clone(),
                ..Default::default()
            },
            &cli()
        )
        .unwrap()
        .changed
    );
    let dropped = rows(&c, "self.backlog.dropped");
    assert_eq!(dropped.len(), 1);
    assert_eq!(dropped[0]["cause"], "turn.failed:timeout");
    failed_turn(&c, "timeout: once more", n1 + 2 * HOUR);
    failed_turn(&c, "timeout: once more", n1 + 3 * HOUR);
    let night = c.backlog_night(n1 + DAY).unwrap();
    assert!(
        night.filed.is_empty() && night.updated.is_empty(),
        "{night:?}"
    );
    assert_eq!(night.kept_out, 1);
    let back = c.self_backlog(&SelfBacklogParams { all: true }).unwrap();
    assert!(back.dropped.contains(&"turn.failed:timeout".to_string()));
    assert_eq!(
        back.items.iter().find(|i| i.id == item.id).unwrap().state,
        "abandoned"
    );
    assert!(
        back.items
            .iter()
            .filter(|i| i.cause == "turn.failed:timeout")
            .count()
            == 1
    );
}

#[test]
fn a_non_owners_drop_or_move_is_refused_and_ledgered() {
    let c = acting(0, 20);
    let t0 = theseus_protocol::now_unix_ms();
    seed_day(&c, t0);
    c.backlog_night(tick()).unwrap();
    let item = listed(&c)[0].id.clone();
    let now = tick();
    let before = listed_at(&c, now);
    let e = c
        .self_drop(
            &SelfDropParams {
                item: item.clone(),
                ..Default::default()
            },
            &stranger(),
        )
        .unwrap_err();
    assert!(e.downcast_ref::<Refusal>().is_some(), "{e:#}");
    let e = c
        .self_move(
            &SelfMoveParams {
                item: item.clone(),
                rank: 2,
                ..Default::default()
            },
            &stranger(),
        )
        .unwrap_err();
    assert!(e.downcast_ref::<Refusal>().is_some(), "{e:#}");
    // A job's shell is refused even for the owner's own surface.
    let e = c
        .self_drop(
            &SelfDropParams {
                item,
                from_job: Some("ses_0000aa".into()),
                ..Default::default()
            },
            &cli(),
        )
        .unwrap_err();
    assert!(e.downcast_ref::<Refusal>().is_some(), "{e:#}");
    let refused: Vec<Value> = rows(&c, "approval.refused")
        .into_iter()
        .filter(|r| r["act"] == "self.drop" || r["act"] == "self.move")
        .collect();
    assert_eq!(refused.len(), 3, "{refused:?}");
    assert_eq!(
        listed_at(&c, now),
        before,
        "nothing changed, scores included"
    );
    assert!(rows(&c, "self.backlog.dropped").is_empty());
}

#[test]
fn a_move_pins_the_item_and_the_size_cap_drops_the_lowest() {
    let c = acting(0, 2);
    let t0 = theseus_protocol::now_unix_ms();
    seed_day(&c, t0);
    let n1 = tick();
    let night = c.backlog_night(n1).unwrap();
    assert_eq!((night.filed.len(), night.too_low), (2, 1), "{night:?}");
    assert!(
        night.trimmed.is_empty(),
        "nothing was filed to drop: {night:?}"
    );
    let open: Vec<_> = listed(&c).into_iter().filter(|i| i.rank > 0).collect();
    assert_eq!(open.len(), 2, "backlog_max_open = 2: {open:?}");
    let (top, second) = (&open[0], &open[1]);
    assert_eq!((top.rank, second.rank), (1, 2));
    let r = c
        .self_move(
            &SelfMoveParams {
                item: second.id.clone(),
                rank: 1,
                ..Default::default()
            },
            &cli(),
        )
        .unwrap();
    assert_eq!((r.was, r.rank), (2, 1));
    let after: Vec<_> = listed(&c).into_iter().filter(|i| i.rank > 0).collect();
    assert_eq!(after[0].id, second.id);
    assert!(after[0].pinned);
    // The pin survives a night.
    failed_turn(&c, "timeout: yet again", n1 + HOUR);
    c.backlog_night(n1 + DAY).unwrap();
    let again: Vec<_> = listed(&c).into_iter().filter(|i| i.rank > 0).collect();
    assert_eq!(again[0].id, second.id, "the owner's order stays");
    assert!(rows(&c, "self.backlog.ranked")
        .iter()
        .any(|r| r["by"] == "owner"));
}

#[test]
fn with_the_mode_off_or_the_switch_halted_a_night_reads_and_files_nothing() {
    let t0 = theseus_protocol::now_unix_ms();
    for c in [
        core_with(SelfMode::Off, 1, 20),
        core_with(SelfMode::Act, 1, 20),
    ] {
        seed_day(&c, t0);
        let pos = c.store.last_position();
        let night = c.backlog_night(tick()).unwrap();
        assert!(matches!(night.gate.as_str(), "off" | "halted"), "{night:?}");
        assert_eq!(night.candidates, 0);
        assert!(night.filed.is_empty() && night.started.is_empty());
        assert_eq!(c.store.last_position(), pos, "not one frame was written");
        assert!(c.store.get_meta::<Value>("self.backlog").unwrap().is_none());
    }
    // Resumed, then halted again between nights: nothing more happens.
    let c = acting(1, 20);
    seed_day(&c, t0);
    c.self_halt(&Default::default(), &cli()).unwrap();
    let pos = c.store.last_position();
    assert_eq!(c.backlog_night(tick()).unwrap().gate, "halted");
    assert_eq!(c.store.last_position(), pos);
}

/// A started item's session is a self step's (seams S1-S3): its turn's model
/// call is held and settled on the self budget, under the item's id, though
/// the session has no task of its own (a join fix: the budget counted task
/// sessions only, so a started item spent outside both lines).
#[tokio::test]
async fn a_started_items_turn_spends_on_the_self_budget_under_the_item() {
    let c = acting(1, 20);
    let t0 = theseus_protocol::now_unix_ms();
    seed_day(&c, t0);
    let night = c.backlog_night(tick()).unwrap();
    let item = night.started[0].item.clone();
    let session = crate::task_graph::get(&c.store, &item)
        .unwrap()
        .unwrap()
        .session
        .unwrap();
    let rec: crate::session::SessionRecord = c.store.get_session(&session).unwrap().unwrap();
    assert!(rec.task.is_none(), "the session has no task of its own");
    let (live, _) = c.live_profile();
    let target = c.runner.resolve_target(&live, None, None, None).unwrap();
    let sink = crate::bus::EventSink::new(c.bus.clone(), &session, None);
    c.runner
        .run(crate::turn::TurnRequest {
            prompt: None,
            surface: None,
            session: rec,
            input: Some("begin".into()),
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
        .unwrap();
    let reserved = rows(&c, "self.budget.reserved");
    let settled = rows(&c, "self.budget.settled");
    assert_eq!((reserved.len(), settled.len()), (1, 1), "{settled:?}");
    assert_eq!(reserved[0]["branch"], item.as_str());
    assert_eq!(settled[0]["branch"], item.as_str());
    // An ordinary conversation's turn on the same core is not counted.
    let plain =
        crate::session::SessionRecord::new(theseus_protocol::SessionKind::Conversation, None);
    c.store.put_session(&plain.session_id, &plain).unwrap();
    let target = c.runner.resolve_target(&live, None, None, None).unwrap();
    let sink = crate::bus::EventSink::new(c.bus.clone(), &plain.session_id, None);
    c.runner
        .run(crate::turn::TurnRequest {
            prompt: None,
            surface: None,
            session: plain,
            input: Some("hello".into()),
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
        .unwrap();
    assert_eq!(rows(&c, "self.budget.settled").len(), 1);
}

/// `self.evidence`'s cause read (theseus-cudar): the rows of one cause key
/// in a time range, from the same read the night makes, so a self session
/// never greps the store for them. Planted reverts: a wrong key, or a range
/// that ends before the rows, finds none.
#[test]
fn the_evidence_tool_reads_a_causes_rows_in_a_range() {
    let c = acting(0, 20);
    let t0 = theseus_protocol::now_unix_ms();
    failed_turn(&c, "kernel: the call panicked", t0);
    failed_turn(&c, "kernel: the call panicked", t0);
    failed_turn(&c, "timeout: the provider did not answer", t0);
    let state = c.tools.self_pr.state_dir().to_path_buf();
    let call =
        |input: Value| crate::rsi::evidence::run(&state, &c.store, tick(), &input).map(|(t, _)| t);
    let said =
        call(json!({"kind": "cause", "cause": "turn.failed:kernel", "since_hours": 1})).unwrap();
    assert!(said.starts_with("2 rows of `turn.failed:kernel`"), "{said}");
    assert!(
        said.contains("turn failed (kernel)") && said.contains("ledger:"),
        "{said}"
    );
    let other =
        call(json!({"kind": "cause", "cause": "turn.failed:timeout", "since_hours": 1})).unwrap();
    assert!(other.starts_with("1 rows of"), "{other}");
    let none =
        call(json!({"kind": "cause", "cause": "turn.failed:nothing", "since_hours": 1})).unwrap();
    assert!(none.starts_with("0 rows"), "{none}");
    let past = call(json!({"kind": "cause", "cause": "turn.failed:kernel", "since_hours": 48, "until_hours": 2})).unwrap();
    assert!(
        past.starts_with("0 rows"),
        "a range that ended before the rows: {past}"
    );
}

/// An item filed under the old words lists in plain ones, in every read of
/// the backlog: a stored "Turns fail with timeout" reads as a new item's
/// "Turns failing with timeout" (the owner, 2026-10-22; theseus-jsdj5).
#[test]
fn an_item_filed_under_the_old_words_lists_in_plain_ones() {
    let c = acting(0, 20);
    seed_day(&c, theseus_protocol::now_unix_ms());
    c.backlog_night(tick()).unwrap();
    let item = listed(&c)
        .into_iter()
        .find(|i| i.cause == "turn.failed:timeout")
        .unwrap();
    assert_eq!(item.title, "Turns failing with timeout");
    let mut t = crate::task_graph::get(&c.store, &item.id).unwrap().unwrap();
    t.title = "Turns fail with timeout".into();
    c.store
        .append(&[crate::task_graph::record(&t).unwrap()])
        .unwrap();
    let old = listed(&c).into_iter().find(|i| i.id == item.id).unwrap();
    assert_eq!(old.title, "Turns failing with timeout");
}

/// The night starts as many items as the pipeline has free build slots
/// (theseus-o1h3f.6), and a `backlog_starts` the config sets still wins.
#[test]
fn the_night_starts_as_many_as_the_pipeline_has_free_slots() {
    let night_of = |starts: Option<u32>, slots: u32| {
        let c = core_starting(SelfMode::Act, starts, slots, 20);
        c.self_resume(&SelfResumeParams::default(), &cli()).unwrap();
        let t0 = theseus_protocol::now_unix_ms();
        seed_day(&c, t0);
        for _ in 0..3 {
            seed(
                &c,
                "action.failed",
                json!({"producer": "proc", "duration_ms": 10}),
                t0 - HOUR,
            );
        }
        c.backlog_night(tick()).unwrap().started.len()
    };
    assert_eq!(night_of(None, 1), 1, "one free slot: one start");
    assert_eq!(night_of(None, 2), 2, "two free slots: two starts");
    assert_eq!(night_of(Some(1), 2), 1, "a set value wins over the slots");
    assert_eq!(night_of(Some(0), 2), 0, "0 files and ranks only");
}

/// A build in flight takes one of the free slots: with three slots and two
/// items running, a night with two more ready causes starts one, not two
/// (theseus-awycv).
#[test]
fn a_build_in_flight_takes_a_slot_from_the_night() {
    let c = core_starting(SelfMode::Act, None, 3, 20);
    c.self_resume(&SelfResumeParams::default(), &cli()).unwrap();
    let t0 = theseus_protocol::now_unix_ms();
    seed_day(&c, t0);
    let n1 = tick();
    // The day holds two unmarked causes, and three slots are free.
    let first = c.backlog_night(n1).unwrap();
    assert_eq!(first.started.len(), 2, "no build in flight: {first:?}");
    let running = |c: &Core| {
        listed(c)
            .iter()
            .filter(|i| i.state == "in_progress")
            .count()
    };
    assert_eq!(running(&c), 2);
    // Two more causes the next day: the one free slot starts one of them.
    for producer in ["proc", "fs"] {
        for k in 0..3 {
            seed(
                &c,
                "action.failed",
                json!({"producer": producer, "duration_ms": 10}),
                n1 + (3 + k) * HOUR,
            );
        }
    }
    let second = c.backlog_night(n1 + DAY).unwrap();
    assert_eq!(
        second.started.len(),
        1,
        "one slot free of three: {second:?}"
    );
    assert_eq!(running(&c), 3);
}

/// The night's free slots are the pipeline's own: an item `in_progress`
/// whose build has gone green since it started is reviewing, not building,
/// and does not hold a slot (theseus-awycv). With three slots and two items
/// started, one going green leaves two free, so the next night starts both
/// of its two ready, not one.
#[test]
fn a_green_build_frees_its_slot_for_the_night() {
    let c = core_starting(SelfMode::Act, None, 3, 20);
    c.self_resume(&SelfResumeParams::default(), &cli()).unwrap();
    let t0 = theseus_protocol::now_unix_ms();
    seed_day(&c, t0);
    let n1 = tick();
    let first = c.backlog_night(n1).unwrap();
    assert_eq!(first.started.len(), 2, "no build in flight: {first:?}");
    let building: Vec<_> = listed(&c)
        .into_iter()
        .filter(|i| i.state == "in_progress")
        .map(|i| i.id)
        .collect();
    assert_eq!(building.len(), 2);
    // One of them builds green: its branch waits on a review.
    c.rec(None).record(&SelfBranchBuilt {
        item: &building[0],
        issue: "theseus-sail1",
        branch: "self/20261026-sail-1",
        head: Some(&format!("{:0>40}", 1)),
        gate: "green",
        report: None,
        cost_usd: 1.25,
        stopped_at: None,
        why: "Trim the sails.",
        numbers: Value::Null,
    });
    for producer in ["proc", "fs"] {
        for k in 0..3 {
            seed(
                &c,
                "action.failed",
                json!({"producer": producer, "duration_ms": 10}),
                n1 + (3 + k) * HOUR,
            );
        }
    }
    let second = c.backlog_night(n1 + DAY).unwrap();
    assert_eq!(
        second.started.len(),
        2,
        "one build in flight of three slots, the other in review: {second:?}"
    );
}

mod infra;
