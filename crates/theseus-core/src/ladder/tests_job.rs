//! The ladder through the core: the job an exchange's end starts writes its
//! `did` line on a background job's model, its call reserved and priced; a
//! task's end and a place on the transcript start none; the build counts and
//! prices in a dry run, and spends within the smaller of its cap and the
//! day's line, failed calls counted; `/new` is a durable mark; and every
//! source is summarised, whatever its provenance. Every name is invented.

use std::sync::Arc;
use std::time::Duration;

use theseus_protocol::SessionLink;

use super::build::Allowance;
use super::did::{self, Outcome};
use super::tests::{exchange_at, Fake};
use super::{Day, DayPurse, Purse, Reserved, DAY_KEY, FOLD_STEP, WINDOW_FLOOR};
use crate::config::Config;
use crate::judge::categorize::ExchangeEnd;
use crate::provider::{FakeProvider, Scripted};
use crate::rpc::Core;
use crate::session::SessionRecord;
use theseus_protocol::SessionKind;

const PLACE: &str = "local:/srv/tides";

fn core(lines: &[&str]) -> (tempfile::TempDir, Arc<Core>, Arc<FakeProvider>) {
    let dir = tempfile::tempdir().unwrap();
    let mut cfg = Config::example();
    cfg.server.state_dir = dir.path().to_string_lossy().into_owned();
    let store = crate::store::Store::open(&dir.path().join("store")).unwrap();
    let fake = Arc::new(FakeProvider::scripted(
        lines.iter().map(|l| Scripted::text(l)).collect(),
    ));
    let mut parts = crate::rpc::Parts::for_tests(cfg, fake.clone(), store);
    parts.ladder_writer = true;
    let core = Core::build(parts).unwrap();
    (dir, core, fake)
}

/// A conversation's session, recorded: provenance that proves no thread.
fn session(core: &Core) -> String {
    let r = SessionRecord::new(SessionKind::Conversation, None);
    core.store.put_session(&r.session_id, &r).unwrap();
    r.session_id
}

async fn until_line(core: &Core, turn: &str) -> Option<did::DidLine> {
    for _ in 0..80 {
        if let Some(d) = did::did(&core.store, turn).unwrap() {
            return Some(d);
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    None
}

fn money(cap_usd: Option<f64>, over_day_line: bool) -> Allowance {
    Allowance {
        cap_usd,
        over_day_line,
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_exchanges_end_writes_its_line_off_the_turns_path() {
    let (_d, core, fake) = core(&["Read gauge 7 and found it 2 mm high."]);
    let sid = session(&core);
    let turn = exchange_at(&core.store, &sid, PLACE, 1_000, false);
    let end = |task| {
        let e = ExchangeEnd {
            session_id: sid.clone(),
            execution_id: String::new(),
            turn_id: turn.clone(),
        };
        core.runner.judge.ladder_at_exchange_end(&e, task);
    };
    end(true);
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert!(fake.requests().is_empty(), "a task's end starts no job");
    end(false);
    let d = until_line(&core, &turn).await.expect("the line is written");
    assert_eq!(d.line, "Read gauge 7 and found it 2 mm high.");
    let asked = &fake.requests()[0];
    assert_eq!(
        asked.system[0]["text"].as_str().unwrap().split(' ').next(),
        Some("Below")
    );
    assert!(asked.tools.is_empty());
    assert!(d.cost_usd > 0.0, "priced, as every background call is");
    let day: Day = core.store.get_meta(DAY_KEY).unwrap().unwrap();
    assert!((day.spent_usd - d.cost_usd).abs() < 1e-12);
    assert!(day.reserved.is_empty(), "the reservation was settled");
}

#[tokio::test]
async fn a_build_counts_and_prices_in_a_dry_run_then_writes_within_its_cap() {
    let (_d, core, _) = core(&[]);
    let sid = session(&core);
    let total = WINDOW_FLOOR + FOLD_STEP + 3;
    for i in 0..total as u64 {
        exchange_at(&core.store, &sid, PLACE, 1_000 + i, false);
    }
    let w = Fake {
        cost: 0.01,
        ..Fake::default()
    };
    let dry = core
        .ladder_build_with(&w, PLACE, true, money(None, false))
        .await
        .unwrap();
    assert_eq!(
        (dry.exchanges, dry.lines, dry.folds),
        (total as u64, total as u64, 1)
    );
    assert!(dry.estimated_usd > 0.0 && !dry.needs_over_day_line);
    assert!(!dry.skipped, "every laddered place is built");
    assert!(w.asks.lock().unwrap().is_empty(), "a dry run asks nothing");
    assert!(core.store.get_meta::<Day>(DAY_KEY).unwrap().is_none());
    // A cap of five calls: five lines, then it stops, and folds nothing.
    let capped = core
        .ladder_build_with(&w, PLACE, false, money(Some(0.05), false))
        .await
        .unwrap();
    assert_eq!((capped.lines, capped.folds), (5, 0));
    assert!(capped.stopped.is_some());
    assert!((capped.spent_usd - 0.05).abs() < 1e-9);
    let full = core
        .ladder_build_with(&w, PLACE, false, money(Some(10.0), false))
        .await
        .unwrap();
    assert_eq!(
        (full.lines_had, full.lines, full.folds),
        (5, total as u64 - 5, 1)
    );
    let l = super::read::ladder(&core.store, PLACE, None).unwrap();
    assert_eq!(l.window.len(), total - FOLD_STEP);
}

#[tokio::test]
async fn a_cap_is_the_smaller_of_its_own_and_the_days_line_unless_given_leave() {
    let (_d, core, _) = core(&[]);
    let sid = session(&core);
    for i in 0..10 {
        exchange_at(&core.store, &sid, PLACE, 1_000 + i, false);
    }
    let today = crate::judge::spend::local_day(theseus_protocol::now_unix_ms());
    let spent = Day {
        day: today.clone(),
        spent_usd: super::DAY_USD - 0.02,
        reserved: Vec::new(),
    };
    core.store.put_meta(DAY_KEY, &spent).unwrap();
    let w = Fake {
        cost: 0.01,
        ..Fake::default()
    };
    let dry = core
        .ladder_build_with(&w, PLACE, true, money(Some(5.0), false))
        .await
        .unwrap();
    assert!(dry.needs_over_day_line, "{dry:?}");
    let b = core
        .ladder_build_with(&w, PLACE, false, money(Some(5.0), false))
        .await
        .unwrap();
    assert_eq!(
        b.lines, 2,
        "a cap of $5 is held to the $0.02 the day has left"
    );
    assert!(core
        .ladder_build_with(&w, PLACE, false, money(None, true))
        .await
        .is_err());
    let over = core
        .ladder_build_with(&w, PLACE, false, money(Some(0.03), true))
        .await
        .unwrap();
    assert_eq!(over.lines, 3, "leave to pass the line, up to its own cap");
}

#[tokio::test]
async fn an_unsettled_reservation_counts_as_spent() {
    let (_d, core, _) = core(&[]);
    let today = crate::judge::spend::local_day(theseus_protocol::now_unix_ms());
    let crashed = Day {
        day: today,
        spent_usd: 0.0,
        reserved: vec![Reserved {
            id: "lres_lost".into(),
            usd: super::DAY_USD - 0.01,
        }],
    };
    core.store.put_meta(DAY_KEY, &crashed).unwrap();
    let purse = DayPurse::new(&core.store, None, false);
    assert!((purse.left().unwrap() - 0.01).abs() < 1e-9);
    assert!(purse.reserve(0.02).is_err());
    // A reservation is written before its call.
    let id = purse.reserve(0.01).unwrap();
    let day: Day = core.store.get_meta(DAY_KEY).unwrap().unwrap();
    assert!(day.reserved.iter().any(|r| r.id == id));
    purse.settle(&id, 0.004);
    let day: Day = core.store.get_meta(DAY_KEY).unwrap().unwrap();
    assert_eq!(day.reserved.len(), 1);
    assert!((day.spent_usd - 0.004).abs() < 1e-9);
}

#[tokio::test]
async fn a_failed_calls_cost_is_the_builds_and_it_folds_nothing_after() {
    let (_d, core, _) = core(&[]);
    let sid = session(&core);
    for i in 0..(WINDOW_FLOOR + FOLD_STEP) as u64 {
        exchange_at(&core.store, &sid, PLACE, 1_000 + i, false);
    }
    let w = Fake {
        cost: 0.01,
        fail: 1.into(),
        ..Fake::default()
    };
    let b = core
        .ladder_build_with(&w, PLACE, false, money(Some(1.0), false))
        .await
        .unwrap();
    assert_eq!((b.lines, b.folds), (0, 0));
    assert!((b.spent_usd - 0.01).abs() < 1e-9, "{b:?}");
    assert!(b.stopped.is_some());
}

#[tokio::test]
async fn new_is_a_durable_mark_for_a_bound_place_and_a_terminal_has_none() {
    let (_d, core, _) = core(&[]);
    let sid = session(&core);
    let bound = "discord:channel:777";
    for place in [bound, PLACE] {
        for i in 0..5 {
            exchange_at(&core.store, &sid, place, 1_000 + i, false);
        }
    }
    let w = Fake::default();
    let build = |place: &'static str| core.ladder_build_with(&w, place, true, money(None, false));
    assert_eq!(
        build(bound).await.unwrap().exchanges,
        5,
        "no mark: no reset"
    );
    core.ladder_mark_new(bound).unwrap();
    let now = theseus_protocol::now_unix_ms();
    for i in 0..3 {
        exchange_at(&core.store, &sid, bound, now + 10 + i, false);
    }
    let b = build(bound).await.unwrap();
    assert_eq!(b.exchanges, 3, "only what came after /new");
    assert!(b.since.is_some());
    assert_eq!(super::new_mark(&core.store, bound).unwrap(), b.since);
    // The terminal's place has no /new: all of it counts.
    assert_eq!(build(PLACE).await.unwrap().exchanges, 5);
    assert_eq!(super::new_mark(&core.store, PLACE).unwrap(), None);
    // A mark that does not read is an error, never "no reset".
    core.store
        .put_meta(&format!("{}{PLACE}", super::NEW_PREFIX), &"broken")
        .unwrap();
    assert!(super::new_mark(&core.store, PLACE).is_err());
    assert!(build(PLACE).await.is_err());
}

/// Anything goes (2026-11-22): a thread's earlier session, whatever its thread's seal, is summarised like
/// any other.
#[tokio::test]
async fn a_threads_earlier_session_is_summarised_like_any_other() {
    let (_d, core, _) = core(&[]);
    let plain = session(&core);
    let mut old = SessionRecord::new(SessionKind::Conversation, None);
    old.superseded_by = Some(SessionLink {
        session_id: plain.clone(),
        at_ms: 1,
        place: Some("thread:tide-gauges".into()),
    });
    core.store.put_session(&old.session_id, &old).unwrap();
    let w = Fake::default();
    let purse = DayPurse::new(&core.store, None, false);
    // A thread's earlier session, a session with no record, and an imported one's id: each summarised.
    let imported = format!("{}x", crate::import::SESSION_PREFIX);
    for (i, sid) in [
        old.session_id.as_str(),
        "ses_never_recorded",
        imported.as_str(),
    ]
    .into_iter()
    .enumerate()
    {
        let turn = exchange_at(&core.store, sid, PLACE, 1_000 + i as u64, false);
        let out = did::write(&core.store, &w, &turn, &purse).await.unwrap();
        assert!(matches!(out, Outcome::Written(_)), "{sid}: {out:?}");
        assert!(did::did(&core.store, &turn).unwrap().is_some(), "{sid}");
    }
    assert_eq!(w.asks.lock().unwrap().len(), 3, "a model read each");
    assert!(core
        .store
        .get_session::<SessionRecord>(&old.session_id)
        .unwrap()
        .is_some());
    assert_ne!(old.session_id, plain);
    assert!(old.superseded_by.is_some());
}

#[tokio::test]
async fn a_correction_the_judge_named_is_kept_above_the_first_level() {
    use super::build::Standing;
    use super::fold::Flags;
    let (_d, core, _) = core(&[]);
    let sid = session(&core);
    let turn = exchange_at(&core.store, &sid, PLACE, 1_000, false);
    let ask = crate::exchange::exchange(&core.store, &turn)
        .unwrap()
        .unwrap()
        .ask
        .unwrap();
    let flags = Standing {
        sources: Vec::new(),
        store: &core.store,
    };
    assert!(!flags.correction(&turn, &ask));
    let kinds = ["new_ask", "follow_up", "correction", "other"];
    let j = crate::tests_learning::judgment(
        "jdg_1",
        "classify.v4",
        vec![crate::tests_learning::choice(
            "kind",
            "correction",
            0.9,
            &kinds,
        )],
        serde_json::json!({"turn": turn}),
        5,
    );
    let rec = super::correction_record(&j, Some(&turn)).expect("a correction is flagged");
    core.store.append(&[rec]).unwrap();
    assert!(flags.correction(&turn, &ask));
    assert!(
        !flags.correction(&turn, "msg_other"),
        "only the message that opened it"
    );
    let mut other = j;
    other.pack = "loop.v1".into();
    assert!(super::correction_record(&other, Some(&turn)).is_none());
}
