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

/// A frame's staged blobs go to the disk as one batch (`Blobs::put_many`):
/// one sync a blob and one for the directory, where a put a blob made two
/// each.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_frames_blobs_are_synced_together_with_one_directory_sync() {
    const N: usize = 20;
    let jev = FakeJev::start().unwrap();
    let r = rig_with(texts(1), Some(&jev), |_| {});
    let judge = r.core.runner.judge.clone();
    judge.jev().unwrap();
    let blobs = r.core.store.blobs();
    let before = blobs.syncs();
    for i in 0..N {
        let digest = judge.stage_blob(format!("{{\"state\": {i}}}").as_bytes());
        let mut j = settled(i);
        j.context["blob"] = json!(digest);
        judge.settle(&j);
    }
    let t0 = Instant::now();
    while written(&r.core.store) < N {
        assert!(
            t0.elapsed() < Duration::from_secs(20),
            "the rows are written"
        );
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert_eq!(
        blobs.syncs() - before,
        N as u64 + 1,
        "a sync a blob, and the directory's once"
    );
}
