//! The sink's backlog (theseus-s1am): a pass keeps one clock from its start
//! until its queue is empty, so a backlog that waited out the quiet bound on
//! a busy daemon is written in the next gaps between turns, frame after
//! frame, and never inside a turn. A clock restarted at every frame wrote
//! one 32-row frame a quiet bound while the backlog grew.

use std::time::{Duration, Instant};

use serde_json::json;
use theseus_judge::fake::FakeJev;
use theseus_judge::Judgment;

use crate::memory_pass::{Timing, QUIET};
use crate::store::Store;
use crate::tests_judge::{rig_with, texts};

/// The judgments settled.
const N: usize = 1_500;
/// The test's quiet bound: a pass's frames take any gap between turns after
/// it (the build's is 120 s).
const QUIET_BOUND: Duration = Duration::from_secs(3);
/// A few gaps between turns, past the quiet bound and the time the sink
/// takes to write `N` with no turn running.
const GAPS: Duration = Duration::from_secs(3);

/// A settled `loop.v1` judgment, as the recording hands it to the sink.
pub(crate) fn settled(i: usize) -> Judgment {
    serde_json::from_value(json!({
        "id": format!("jdg_backlog_{i:05}"), "pack": "loop.v1", "version": 1, "pack_sha256": "00",
        "point": "loop_end", "mode": "shadow", "model": "jev-1.13.0", "answered_by": "jev-1.13.0",
        "model_drift": false,
        "state": {"sha256": "00", "bytes": 10, "tokens": 3, "cap_tokens": 4000,
            "builder": "loop", "builder_version": 1, "truncated": [], "dropped": []},
        "questions": 2, "answers": [], "call": null,
        "timing": {"queued_ms": 0, "http_ms": 9, "total_ms": 9},
        "usage": null, "cost_micros": 10, "reserve_micros": 10,
        "outcome": {"outcome": "answered"}, "circuit": null, "rate_limit": {},
        "context": {"session": "ses_backlog", "turn": format!("trn_backlog_{i}"),
            "decision": "no_tool_calls", "class": "task"},
    }))
    .unwrap()
}

/// The `judge:loop` rows written.
pub(crate) fn written(store: &Store) -> usize {
    store.scope_after("judge:loop", 0).unwrap().len()
}

/// 1,500 judgments settle while turns run back to back, never a quiet
/// stretch apart (300 ms each, 200 ms between): the whole backlog is
/// written within the quiet bound, three times the time this sink takes to write
/// 1,500 with no turn running (the rate of main's sink, which wrote them as
/// they landed, measured first on the same machine and load), and a few
/// gaps; and no frame lands between a turn's start and its end. The bug's
/// sink took a quiet bound a frame: N / 32 frames, about 140 s here.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_backlog_drains_in_the_gaps_once_its_pass_passes_the_quiet_bound() {
    let jev = FakeJev::start().unwrap();
    let r = rig_with(texts(1), Some(&jev), |_| {});
    let judge = &r.core.runner.judge;
    judge
        .sink_timing
        .set(Timing {
            quiet: QUIET,
            quiet_bound: QUIET_BOUND,
            busy_bound: Duration::from_secs(120),
            ..Timing::default()
        })
        .unwrap_or_else(|_| panic!("the sink's timing is set once"));
    // The writer's task starts with the judge's build.
    judge.jev().unwrap();
    // The rate with no turn running: N written as fast as the frames go.
    let t0 = Instant::now();
    for i in N..2 * N {
        judge.settle(&settled(i));
    }
    while judge.unwritten() > 0 || written(&r.core.store) < N {
        assert!(
            t0.elapsed() < Duration::from_secs(100),
            "no turn runs, and the sink writes"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    let free = t0.elapsed();
    let bound = QUIET_BOUND + free * 3 + GAPS;
    let turns = r.core.runner.pass.turns().clone();
    let t0 = Instant::now();
    let (mut settled_n, mut turns_n) = (0, 0);
    loop {
        if settled_n < N {
            for i in settled_n..settled_n + 300 {
                judge.settle(&settled(i));
            }
            settled_n += 300;
        }
        let running = turns.begin().await;
        turns_n += 1;
        let at = r.core.store.last_position();
        tokio::time::sleep(Duration::from_millis(300)).await;
        assert_eq!(
            r.core.store.last_position(),
            at,
            "no frame lands inside a turn (turn {turns_n}, {:?} in)",
            t0.elapsed()
        );
        drop(running);
        if judge.unwritten() == 0 && written(&r.core.store) == 2 * N {
            break;
        }
        assert!(
            t0.elapsed() < bound,
            "{} of {N} written after {:?}, past {bound:?} (with no turn, {free:?}): the backlog drains near the rate of the sink that wrote at once",
            written(&r.core.store) - N,
            t0.elapsed()
        );
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
    let took = t0.elapsed();
    eprintln!(
        "1,500 judgments: {free:?} with no turn running; {took:?} beside turns, bound {bound:?}"
    );
    assert!(
        took >= QUIET_BOUND,
        "no quiet stretch before the bound: {took:?}"
    );
}
