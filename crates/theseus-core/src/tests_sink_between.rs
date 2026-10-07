//! The judge's sink writes only between turns (theseus-0j2.8): its frame
//! waits while a turn runs, as the memory pass's and consolidation's do
//! (`memory_pass::turns`), and is written once none has run for the pass's
//! quiet stretch, its rows and their sentences in it.

use std::time::{Duration, Instant};

use theseus_judge::fake::{FakeJev, Scripted as Jev};

use crate::tests_judge::{judged, rig_with, texts, turn, until_judged};

/// A judgment that lands while a turn runs is not written until that turn
/// has ended: no `judge.call` row in the store past the sink's window while
/// the turn runs, and the row once it ends.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_judgments_frame_waits_for_the_running_turn_to_end() {
    let jev = FakeJev::start().unwrap();
    jev.script(
        "work_state",
        Jev::Choice {
            option: "complete".into(),
            confidence: 0.95,
        },
    );
    let r = rig_with(texts(1), Some(&jev), |_| {});
    let res = turn(&r.core, None, "Say done.").await;
    assert_eq!(res.stop_reason, "no_tool_calls");
    // A turn begins as the judgment goes out, and runs past the sink's
    // window.
    let running = r.core.runner.pass.turns().begin().await;
    let t0 = Instant::now();
    while jev.seen().is_empty() {
        assert!(t0.elapsed() < Duration::from_secs(20), "no call to Jev");
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    tokio::time::sleep(crate::judge::FLUSH_EVERY + Duration::from_secs(1)).await;
    assert!(
        judged(&r.core.store).is_empty(),
        "no judgment's frame while a turn runs"
    );
    let ended = Instant::now();
    drop(running);
    let rows = until_judged(&r.core.store, 1).await;
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].1.turn_id.as_deref(), Some(res.turn_id.as_str()));
    assert!(
        ended.elapsed() >= crate::memory_pass::QUIET,
        "written a quiet stretch after the turn ended"
    );
}
