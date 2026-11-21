//! The ladder through the core: the job an exchange's end starts writes its
//! `did` line on a background job's model, its call held and priced; a task's
//! end starts none; the build of a place's history counts and prices in a dry
//! run and writes nothing, then writes within its cap. Every name is invented.

use std::sync::Arc;
use std::time::Duration;

use super::did::{self, Outcome};
use super::tests::{exchange_at, Fake};
use super::N;
use crate::config::assembly::{Assembly, AssemblySetting};
use crate::config::Config;
use crate::judge::categorize::ExchangeEnd;
use crate::provider::{FakeProvider, Scripted};
use crate::rpc::Core;

const PLACE: &str = "local:/srv/tides";

fn core(lines: &[&str]) -> (tempfile::TempDir, Arc<Core>, Arc<FakeProvider>) {
    core_with(lines, Some(Assembly::View))
}

fn core_with(
    lines: &[&str],
    assembly: Option<Assembly>,
) -> (tempfile::TempDir, Arc<Core>, Arc<FakeProvider>) {
    let dir = tempfile::tempdir().unwrap();
    let mut cfg = Config::example();
    cfg.context.assembly = assembly.map(AssemblySetting::All);
    cfg.server.state_dir = dir.path().to_string_lossy().into_owned();
    let store = crate::store::Store::open(&dir.path().join("store")).unwrap();
    let fake = Arc::new(FakeProvider::scripted(
        lines.iter().map(|l| Scripted::text(l)).collect(),
    ));
    let core = Core::build(crate::rpc::Parts::for_tests(cfg, fake.clone(), store)).unwrap();
    (dir, core, fake)
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

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_exchanges_end_writes_its_line_off_the_turns_path() {
    let (_d, core, fake) = core(&["Read gauge 7 and found it 2 mm high."]);
    let turn = exchange_at(&core.store, "ses_t", PLACE, 1_000, false);
    let end = |task| {
        let e = ExchangeEnd {
            session_id: "ses_t".into(),
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
    let day: super::Day = core.store.get_meta(super::DAY_KEY).unwrap().unwrap();
    assert!((day.spent_usd - d.cost_usd).abs() < 1e-12);
}

#[tokio::test]
async fn a_build_counts_and_prices_in_a_dry_run_then_writes_within_its_cap() {
    let (_d, core, _) = core(&[]);
    let total = 2 * N + 3;
    for i in 0..total as u64 {
        exchange_at(&core.store, "ses_t", PLACE, 1_000 + i, false);
    }
    let w = Fake {
        cost: 0.01,
        ..Fake::default()
    };
    let dry = core.ladder_build_with(&w, PLACE, true, None).await.unwrap();
    assert_eq!(
        (dry.exchanges, dry.lines, dry.folds),
        (total as u64, total as u64, 1)
    );
    assert!(dry.estimated_usd > 0.0);
    assert!(w.asks.lock().unwrap().is_empty(), "a dry run asks nothing");
    assert!(core
        .store
        .get_meta::<super::Day>(super::DAY_KEY)
        .unwrap()
        .is_none());
    // A cap of five calls: five lines, then it stops and says why.
    let capped = core
        .ladder_build_with(&w, PLACE, false, Some(0.05))
        .await
        .unwrap();
    assert_eq!(capped.lines, 5);
    assert!(capped.stopped.is_some());
    let full = core
        .ladder_build_with(&w, PLACE, false, Some(10.0))
        .await
        .unwrap();
    assert_eq!(
        (full.lines_had, full.lines, full.folds),
        (5, total as u64 - 5, 1)
    );
    let l = super::read::ladder(&core.store, PLACE, 0).unwrap();
    assert_eq!(l.window.len(), total - N);
}

#[tokio::test]
async fn a_sealed_threads_session_is_hidden_and_a_plain_one_is_not() {
    let (_d, core, _) = core(&[]);
    assert!(!core.ladder_hidden("ses_plain"));
    assert!(core.ladder_hidden(&format!("{}x", crate::import::SESSION_PREFIX)));
    let w = Fake::default();
    let turn = exchange_at(&core.store, "ses_plain", PLACE, 1_000, false);
    let hidden = |sid: &str| core.ladder_hidden(sid);
    let out = did::write(&core.store, &w, &turn, &hidden, None)
        .await
        .unwrap();
    assert!(matches!(out, Outcome::Written(_)), "{out:?}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_place_on_the_transcript_spends_nothing_on_summaries() {
    let (_d, core, fake) = core_with(&["Never asked."], None);
    let turn = exchange_at(&core.store, "ses_t", PLACE, 1_000, false);
    let e = ExchangeEnd {
        session_id: "ses_t".into(),
        execution_id: String::new(),
        turn_id: turn.clone(),
    };
    core.runner.judge.ladder_at_exchange_end(&e, false);
    assert_eq!(until_line(&core, &turn).await, None);
    assert!(fake.requests().is_empty());
}
