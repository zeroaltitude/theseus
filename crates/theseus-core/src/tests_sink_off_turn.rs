//! The judge's frames that a turn's end used to bring at once, kept off the
//! turns beside them (theseus-xkbs): `categorize.v1`'s mark rides in the
//! sink's frame beside its judgment's row, and a judgment no turn waits on
//! writes the shadow budget's block between turns, before its call. A
//! running turn sees neither frame land; each lands after it, the block
//! before Jev sees the call.

use std::time::{Duration, Instant};

use theseus_judge::fake::{FakeJev, Scripted as Jev};

use crate::judge::categorize::{Mark, EVERY, MARK_PREFIX};
use crate::judge::spend;
use crate::store::Store;
use crate::tests_categorize::{harbor_at, moorings, rig, session};
use crate::tests_judge::{rig_with, texts, turn, until_judged};

fn frames(store: &Store) -> u64 {
    store.stats().unwrap().frames_appended
}

fn budget(store: &Store) -> Option<spend::Stored> {
    store.get_meta(spend::META_KEY).unwrap()
}

/// Wait (on the runtime's timer) until `f` holds.
async fn until(what: &str, mut f: impl FnMut() -> bool) {
    let t0 = Instant::now();
    while !f() {
        assert!(t0.elapsed() < Duration::from_secs(20), "{what}");
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
}

/// The tenth human message brings categorize's decision while a turn runs:
/// no frame lands while it runs (neither the day's first block nor the
/// mark), and Jev is not called. Once it ends, the block lands before the
/// call. A second exchange end before the row is written reads the moved
/// mark at once, and judges nothing twice; the mark lands with its row, in
/// the sink's one frame.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn categorizes_block_and_mark_wait_for_the_running_turn() {
    let jev = FakeJev::start().unwrap();
    harbor_at(&jev, "harbor", 0.93);
    let r = rig(Some(&jev), 12, |_| {});
    let (store, judge) = (&r.core.store, r.core.runner.judge.clone());
    let turns = r.core.runner.pass.turns().clone();
    let sid = session(&r.core, None);
    let key = format!("{MARK_PREFIX}{sid}");
    moorings(&r.core, &sid, EVERY - 1).await;
    let read = judge.categorize_records_read();
    let tenth = moorings(&r.core, &sid, 1).await;
    let running = turns.begin().await;
    let at = frames(store);
    until("the decision reads the session", || {
        judge.categorize_records_read() > read
    })
    .await;
    tokio::time::sleep(crate::judge::FLUSH_EVERY + Duration::from_secs(1)).await;
    assert_eq!(frames(store), at, "no frame while the turn runs");
    assert!(jev.seen().is_empty(), "the call waits for its block");
    assert!(budget(store).is_none() && store.get_meta::<Mark>(&key).unwrap().is_none());
    drop(running);
    until("Jev is called", || !jev.seen().is_empty()).await;
    assert!(
        budget(store).is_some(),
        "the block is written before the call"
    );
    assert_eq!(frames(store), at + 1, "the block's frame alone");

    // The row waits for this turn; an exchange end meanwhile reads the mark
    // the decision moved.
    let running = turns.begin().await;
    let read = judge.categorize_records_read();
    judge.after_turn(&tenth, false);
    until("the second decision reads the session", || {
        judge.categorize_records_read() > read
    })
    .await;
    tokio::time::sleep(Duration::from_millis(500)).await;
    assert_eq!(
        jev.seen().len(),
        1,
        "the same messages are not judged twice"
    );
    assert!(
        store.get_meta::<Mark>(&key).unwrap().is_none(),
        "the mark waits with its row"
    );
    let at = frames(store);
    drop(running);
    let judged = crate::tests_categorize::until_judged(store, 1).await;
    let mark: Mark = store.get_meta(&key).unwrap().expect("the mark is written");
    assert_eq!(
        Some(mark.judgment.as_str()),
        judged[0].1.data["id"].as_str()
    );
    assert_eq!(frames(store), at + 1, "the row and its mark in one frame");
}

/// A turn that loop.v1 judges ends, and another begins at once: the day's
/// first block, which the judgment needs, is not written while the second
/// runs, and Jev is not called; once it ends, the block lands, then the
/// call, then the row.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_loop_judgments_first_block_waits_for_the_running_turn() {
    let jev = FakeJev::start().unwrap();
    jev.script(
        "work_state",
        Jev::Choice {
            option: "complete".into(),
            confidence: 0.95,
        },
    );
    let r = rig_with(texts(1), Some(&jev), |_| {});
    let store = &r.core.store;
    let res = turn(&r.core, None, "Say done.").await;
    assert_eq!(res.stop_reason, "no_tool_calls");
    let running = r.core.runner.pass.turns().begin().await;
    let at = frames(store);
    tokio::time::sleep(crate::judge::FLUSH_EVERY + Duration::from_secs(1)).await;
    assert_eq!(frames(store), at, "no frame while the turn runs");
    assert!(jev.seen().is_empty(), "the call waits for its block");
    assert!(budget(store).is_none());
    drop(running);
    until("Jev is called", || !jev.seen().is_empty()).await;
    assert!(
        budget(store).is_some(),
        "the block is written before the call"
    );
    let rows = until_judged(store, 1).await;
    assert_eq!(rows[0].1.turn_id.as_deref(), Some(res.turn_id.as_str()));
}
