//! The sink writes a frame's staged blobs before it waits for a moment
//! between turns (theseus-ehkp): the blobs touch no WAL, so a turn that
//! begins while they are written runs at once, and the writer's guard
//! covers the frame's append alone. The frame still lands after its blobs,
//! and between turns.

use std::time::{Duration, Instant};

use serde_json::json;
use theseus_judge::fake::FakeJev;

use crate::tests_judge::{rig_with, texts};
use crate::tests_sink_backlog::{settled, written};

/// A judgment naming a staged blob settles, and its blob's write is held
/// (a disk under a neighbour's IO): a turn that begins meanwhile is not
/// held; no frame lands while it runs; once the blob is written and the
/// turn has ended, the row lands, its blob on disk before it.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_turn_beginning_while_a_frames_blobs_are_written_runs_at_once() {
    let jev = FakeJev::start().unwrap();
    let r = rig_with(texts(1), Some(&jev), |_| {});
    let judge = r.core.runner.judge.clone();
    judge.jev().unwrap();
    let turns = r.core.runner.pass.turns().clone();
    let blobs = r.core.store.blobs();
    let digest = judge.stage_blob(b"{\"a state\": \"a turn waited on it\"}");
    assert!(!blobs.path(&digest).exists(), "staged, not written");
    let hold = blobs.hold_puts();
    let mut j = settled(0);
    j.context["blob"] = json!(digest);
    judge.settle(&j);
    // The window passes, and the writer is in the blob's put.
    tokio::time::sleep(crate::judge::FLUSH_EVERY + Duration::from_millis(500)).await;
    assert_eq!(written(&r.core.store), 0, "the blob is held");
    let t0 = Instant::now();
    let running = tokio::time::timeout(Duration::from_secs(2), turns.begin())
        .await
        .expect("a turn begins at once while a frame's blobs are written");
    eprintln!("the turn began after {:?}", t0.elapsed());
    let at = r.core.store.stats().unwrap().frames_appended;
    // The blob is written; the frame waits for the turn.
    drop(hold);
    let t0 = Instant::now();
    while !blobs.path(&digest).exists() {
        assert!(
            t0.elapsed() < Duration::from_secs(10),
            "the blob is written"
        );
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    tokio::time::sleep(crate::memory_pass::QUIET + Duration::from_millis(500)).await;
    assert_eq!(
        r.core.store.stats().unwrap().frames_appended,
        at,
        "no frame while the turn runs"
    );
    assert_eq!(written(&r.core.store), 0);
    drop(running);
    let t0 = Instant::now();
    while written(&r.core.store) == 0 {
        assert!(t0.elapsed() < Duration::from_secs(10), "the row is written");
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert!(
        t0.elapsed() >= crate::memory_pass::QUIET,
        "between turns: {:?} after the turn ended",
        t0.elapsed()
    );
    assert_eq!(
        blobs.read(&digest).as_deref(),
        Some(&b"{\"a state\": \"a turn waited on it\"}"[..])
    );
}
