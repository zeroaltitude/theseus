//! The next block, raised ahead, rides in its own frame's record
//! (theseus-xkbs, theseus-5o3d): a sink frame that finds less than half the
//! current block left raises the next one in memory, then builds the frame's
//! budget record, under the `blocks` lock, so the raise is written before
//! any reservation can draw on it (`ShadowBudget::ahead`). A reservation
//! that fits the raised block writes no frame of its own, and is always
//! inside a written record.

use std::time::{Duration, Instant};

use theseus_judge::fake::FakeJev;

use super::spend::{self, Stored, BLOCK_MICROS, META_KEY};
use crate::tests_judge::{rig_with, texts};
use crate::tests_sink_backlog::{settled, written};

/// More than half the day's first block settles; the sink's next frame
/// raises the next block, and its record holds the raise, as memory does. A
/// reservation drawn on the raised block then writes no frame, and the
/// store's record covers it.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_block_raised_ahead_is_in_its_frames_own_record() {
    let jev = FakeJev::start().unwrap();
    let r = rig_with(texts(1), Some(&jev), |_| {});
    let (store, svc) = (&r.core.store, r.core.runner.judge.clone());
    svc.jev().unwrap();
    let stored = || store.get_meta::<Stored>(META_KEY).unwrap().unwrap();
    // The sink raises on the wall clock's day.
    let today = spend::local_day(theseus_protocol::now_unix_ms());
    assert!(svc.reserve(&today, 100), "the day's first block");
    assert_eq!(stored().reserved_micros, BLOCK_MICROS);
    let spent = BLOCK_MICROS * 6 / 10;
    svc.budget.settle(&today, 100, spent, true, false);
    let need = BLOCK_MICROS / 2;
    assert!(
        svc.budget.needs_frame(&today, need),
        "less than half the block is left"
    );
    // One of the sink's frames.
    svc.settle(&settled(0));
    let t0 = Instant::now();
    while written(store) == 0 {
        assert!(t0.elapsed() < Duration::from_secs(20), "the row is written");
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    if spend::local_day(theseus_protocol::now_unix_ms()) != today {
        eprintln!("the test crossed a local midnight: the frame raised nothing for {today}");
        return;
    }
    let memory: Stored = serde_json::from_slice(&svc.budget.record().unwrap().payload).unwrap();
    assert_eq!(memory.reserved_micros, 2 * BLOCK_MICROS, "raised ahead");
    assert_eq!(stored(), memory, "the frame's record holds the raise");
    // A reservation the raised block holds writes no frame, and is inside
    // what the store's record holds.
    assert!(!svc.budget.needs_frame(&today, need));
    let frames = store.stats().unwrap().frames_appended;
    assert!(svc.reserve(&today, need));
    assert_eq!(
        store.stats().unwrap().frames_appended,
        frames,
        "no frame of its own"
    );
    assert!(
        stored().reserved_micros >= spent + need,
        "the store's record covers a call drawn on the raised block: {:?}",
        stored()
    );
}
