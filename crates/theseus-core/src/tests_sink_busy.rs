//! The sink's busy-bound guard (theseus-ju99): a backlog keeps one clock
//! from its pass's start (theseus-s1am), but a frame's wait runs from no
//! more than the quiet bound ago (`sink::since`), so on a daemon whose turn
//! never ends a frame lands beside it once every `busy_bound - quiet_bound`,
//! never all of a long backlog back to back once the busy bound has passed.

use std::time::Duration;

use theseus_judge::fake::FakeJev;

use crate::memory_pass::Timing;
use crate::tests_judge::{rig_with, texts};
use crate::tests_sink_backlog::settled;

const QUIET: Duration = Duration::from_millis(500);
const QUIET_BOUND: Duration = Duration::from_secs(1);
const BUSY_BOUND: Duration = Duration::from_secs(3);
/// How long the one turn runs.
const HELD: Duration = Duration::from_secs(14);
/// Judgments settled each tick, a tick every 100 ms: 1,120 over the turn,
/// 35 frames' worth.
const EACH: usize = 8;
const TICK: Duration = Duration::from_millis(100);

/// One turn runs 14 s while judgments settle all through it: the frames
/// that land inside it are at most one per `busy_bound - quiet_bound`
/// (the guard writes 6 here; with the clock at the pass's start alone,
/// every frame after the busy bound lands at once, 36).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_turn_that_never_ends_gets_a_frame_beside_it_once_a_busy_stretch() {
    let jev = FakeJev::start().unwrap();
    let r = rig_with(texts(1), Some(&jev), |_| {});
    let judge = r.core.runner.judge.clone();
    judge
        .sink_timing
        .set(Timing {
            quiet: QUIET,
            quiet_bound: QUIET_BOUND,
            busy_bound: BUSY_BOUND,
            ..Timing::default()
        })
        .unwrap_or_else(|_| panic!("the sink's timing is set once"));
    judge.jev().unwrap();
    let turns = r.core.runner.pass.turns().clone();
    let running = turns.begin().await;
    let before = r.core.store.stats().unwrap().frames_appended;
    let t0 = tokio::time::Instant::now();
    let mut i = 0;
    let mut tick = tokio::time::interval(TICK);
    while t0.elapsed() < HELD {
        tick.tick().await;
        for _ in 0..EACH {
            judge.settle(&settled(i));
            i += 1;
        }
    }
    let inside = r.core.store.stats().unwrap().frames_appended - before;
    drop(running);
    let most = HELD
        .as_millis()
        .div_ceil((BUSY_BOUND - QUIET_BOUND).as_millis()) as u64;
    eprintln!("{i} judgments settled; {inside} frames inside the {HELD:?} turn, at most {most}");
    assert!(
        inside >= 1,
        "the busy bound writes beside a turn that never ends"
    );
    assert!(
        inside <= most,
        "{inside} frames inside the turn, past one per busy stretch ({most})"
    );
}
