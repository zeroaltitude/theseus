//! The nightly sync's wall-clock deadline (theseus-ectzg).
//!
//! `import.sync.record` was capped on the day's spend and on nothing else, so
//! the daemon's half of one `theseus import sync` could run for as long as it
//! liked while the client waited in its one `await`. The unit's
//! `TimeoutStartSec=` bounds the client, not this, so it made the blockage
//! visible without stopping the work.
//!
//! What is asserted here is that a run which passes its cap **stops where it
//! is, returns, and says so** -- not that a cap is configured. Each test runs
//! the record's own path (`Core::sync_record`) with the cap as its parameter,
//! so the planted one is a millisecond's, and the one beside it is the
//! daemon's own. Every name is invented.

use std::time::Duration;

use serde_json::json;
use theseus_judge::fake::FakeJev;
use theseus_protocol::import::{ImportEpisodesParams, ImportLine};
use theseus_protocol::import_sync::{ImportSyncRecord, ImportTouched};

use crate::import::session_id_of;
use crate::import::sync::{SyncState, STATE_KEY};
use crate::import::tests_append::{sync as episode, TAG};
use crate::judge::people::tests::{answer, people_on, script};
use crate::tests_categorize::{rig, Rig};

/// The daemon's own cap, as `import.sync.record` passes it.
const LIVE: Duration = Duration::from_secs(90 * 60);

/// Two OpenClaw sessions imported under `TAG`, and the record a run would
/// send for them.
fn imported(r: &Rig) -> ImportSyncRecord {
    let episodes = [episode(20, TAG, &[0, 2, 4]), episode(21, TAG, &[0, 2])];
    let lines: Vec<ImportLine> = episodes
        .iter()
        .enumerate()
        .map(|(i, e)| ImportLine {
            line: i as u64 + 1,
            text: serde_json::to_string(e).unwrap(),
        })
        .collect();
    let out = crate::import::write::import_batch(
        &r.core.store,
        &ImportEpisodesParams {
            file: "sync.jsonl".into(),
            lines,
            ..Default::default()
        },
        "test",
    )
    .unwrap();
    assert!(out.rejected.is_empty(), "{:?}", out.rejected);
    ImportSyncRecord {
        started_ms: theseus_protocol::now_unix_ms(),
        cursor: Some(json!({"at": 1})),
        episodes: 2,
        messages: 5,
        imported: 2,
        appended: 0,
        skipped: 0,
        rejected: vec![],
        touched: episodes
            .iter()
            .map(|e| ImportTouched {
                session_id: session_id_of(e["episode_id"].as_str().unwrap()),
                tag: TAG.to_string(),
            })
            .collect(),
        error: None,
        no_people: false,
    }
}

/// What the run left for the next night.
fn left_for_next_run(r: &Rig) -> Vec<String> {
    let state: SyncState = r.core.store.get_meta(STATE_KEY).unwrap().unwrap();
    state.people_todo
}

/// A run that is already past its deadline when the people step begins stops
/// there: it RETURNS, the result carries `stopped`, nothing was swept, and
/// every session it did not reach is still in the state for the next night.
/// The deadline is a stop, never an error.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_run_past_its_deadline_stops_and_returns_what_it_has() {
    let jev = FakeJev::start().unwrap();
    script(&jev);
    let r = rig(Some(&jev), 0, people_on);
    let rec = imported(&r);
    let waiting: Vec<String> = rec.touched.iter().map(|t| t.session_id.clone()).collect();
    r.fake
        .when_tool(crate::judge::people::extract::TOOL, answer(json!([])));

    // The plant: a cap of ZERO, which `Cap::new` spends at the instant it is
    // made -- `at` is `Instant::now()`, and `Instant` is monotone, so the first
    // `passed()` is true on every machine by construction.
    //
    // It was `from_millis(1)` and that was a RACE, not a plant: it assumed the
    // record's first step (two store frames) costs more than a millisecond, which
    // is true on this x86 box and FALSE on the M1 Max. There the people step began
    // inside the cap, started a tag, and stopped at the next check instead of the
    // first -- so `stopped` was set, `tags` was not empty, and the ARM gate of
    // 2026-11-12 13:19 went red on line 113 while the same tree was green here.
    // A test about WHICH check catches the deadline must not be timed against the
    // host's speed. Sibling `..._stops_at_the_deadline_it_is_handed` had it right
    // all along: hand it an instant that is already spent.
    let out = r
        .core
        .sync_record(rec, "test", Duration::ZERO)
        .await
        .expect("a run past its deadline returns, it does not error");

    let people = out.people.expect("the people step reports what it did");
    let why = people
        .stopped
        .as_deref()
        .expect("a step that stopped at the deadline says so");
    assert!(
        why.contains("deadline"),
        "the reason names the deadline: {why}"
    );
    assert_eq!(people.tags, Vec::<String>::new(), "no tag was begun");
    assert_eq!((people.swept, people.judged), (0, 0), "{people:?}");
    assert_eq!(people.spent_usd, 0.0, "a stopped step spends nothing");
    assert_eq!(
        people.left,
        waiting.len() as u64,
        "every session waits for the next run"
    );
    assert_eq!(left_for_next_run(&r), waiting, "and the state holds them");
    assert!(
        r.fake.requests().is_empty(),
        "nothing was read: {:?}",
        r.fake.requests()
    );
}

/// The plant removed: the same record, the same rig, under the daemon's own
/// deadline. The run completes -- the tags are passed, the sessions are
/// swept, nothing is `stopped`, and the state is left with nothing owed.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_same_run_inside_its_deadline_completes() {
    let jev = FakeJev::start().unwrap();
    script(&jev);
    let r = rig(Some(&jev), 0, people_on);
    let rec = imported(&r);
    let waiting = rec.touched.len();
    r.fake
        .when_tool(crate::judge::people::extract::TOOL, answer(json!([])));

    let out = r.core.sync_record(rec, "test", LIVE).await.unwrap();

    let people = out.people.expect("the people step reports what it did");
    assert_eq!(
        people.stopped, None,
        "a run inside its deadline stops for nothing: {people:?}"
    );
    assert_eq!(people.tags, vec![TAG.to_string()], "{people:?}");
    assert_eq!(people.swept as usize, waiting, "{people:?}");
    assert_eq!(people.left, 0, "nothing is owed: {people:?}");
    assert_eq!(left_for_next_run(&r), Vec::<String>::new());
}

/// The extraction's own session loop honours the deadline it is handed, so a
/// run whose tags finished inside the cap and whose sweep did not still stops
/// with its sessions kept. `people_sync` without one is uncapped, which is
/// what the sweep's own tender passes.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_extractions_session_loop_stops_at_the_deadline_it_is_handed() {
    let jev = FakeJev::start().unwrap();
    script(&jev);
    let r = rig(Some(&jev), 0, people_on);
    let rec = imported(&r);
    let waiting: Vec<String> = rec.touched.iter().map(|t| t.session_id.clone()).collect();
    r.fake
        .when_tool(crate::judge::people::extract::TOOL, answer(json!([])));

    let past = std::time::Instant::now() - Duration::from_secs(1);
    let s = r
        .core
        .people_sync_within(waiting.clone(), Some(past))
        .await
        .unwrap();
    assert!(s.due > 0, "the plan had sessions to pass: {s:?}");
    assert_eq!(s.swept, 0, "none was begun: {s:?}");
    let why = s.stopped.as_deref().expect("it says why it stopped");
    assert!(why.contains("deadline"), "{why}");
    assert!(
        r.fake.requests().is_empty(),
        "no session was read: {:?}",
        r.fake.requests()
    );

    // Without a deadline the same plan is passed in full.
    let open = r.core.people_sync(waiting.clone()).await.unwrap();
    assert_eq!(open.stopped, None, "{open:?}");
    assert_eq!(open.swept as usize, waiting.len(), "{open:?}");
}
