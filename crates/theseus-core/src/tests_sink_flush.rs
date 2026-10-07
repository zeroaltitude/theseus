//! A clean stop flushes the judge's sink (theseus-ych4): every judgment
//! settled and not yet written is in the store before the stop's last
//! checkpoint, so a restart reads every one. A SIGKILL loses the queue
//! (`judge/sink.rs`'s module doc), which this does not test.

use std::sync::Arc;
use std::time::{Duration, Instant};

use theseus_judge::fake::FakeJev;

use crate::judge::sink::MAX_ROWS;
use crate::provider::FakeProvider;
use crate::rpc::{Core, Parts};
use crate::store::Store;
use crate::tests_judge::{board, config, texts, warm};
use crate::tests_sink_backlog::{settled, written};

/// The judgments settled before the stop: more than a few frames' worth.
const N: usize = 200;

/// N judgments settle while a turn runs, so the sink holds them all; a
/// clean stop writes them, in as few frames as the sink's size allows, and
/// a restart on the same store finds all N.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_clean_stop_writes_every_settled_judgment_before_the_store_closes() {
    let jev = FakeJev::start().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let build = || {
        let cfg = config(dir.path(), Some(&jev));
        let store = Store::open(&dir.path().join("store")).unwrap();
        let mut p = Parts::for_tests(cfg, Arc::new(FakeProvider::scripted(texts(1))), store);
        p.secrets = board();
        let core = Core::build(p).unwrap();
        warm(&core);
        core
    };
    let core = build();
    let judge = core.runner.judge.clone();
    judge.jev().unwrap();
    // A turn runs throughout, so the sink writes nothing on its own.
    let running = core.runner.pass.turns().begin().await;
    for i in 0..N {
        judge.settle(&settled(i));
    }
    tokio::time::sleep(crate::judge::FLUSH_EVERY + Duration::from_millis(500)).await;
    assert_eq!(written(&core.store), 0, "the sink waits for the turn");
    assert_eq!(judge.unwritten(), N);
    let frames_before = core.store.stats().unwrap().frames_appended;
    let t0 = Instant::now();
    core.finish_stop().await;
    let took = t0.elapsed();
    assert_eq!(judge.unwritten(), 0, "the stop took every judgment");
    assert_eq!(written(&core.store), N, "and wrote each before closing");
    // The stop's own frames besides the sink's: none here (no post, no
    // terminal, no web row waits).
    let frames = core.store.stats().unwrap().frames_appended - frames_before;
    assert_eq!(
        frames,
        N.div_ceil(MAX_ROWS) as u64,
        "in as few frames as it takes"
    );
    eprintln!("the stop's flush of {N} judgments: {took:?} for the whole finish_stop");
    // Nothing the sink writes after the stop's checkpoint.
    judge.settle(&settled(N));
    drop(running);
    tokio::time::sleep(
        crate::memory_pass::QUIET + crate::judge::FLUSH_EVERY + Duration::from_millis(500),
    )
    .await;
    assert_eq!(
        written(&core.store),
        N,
        "nothing written after the stop's checkpoint"
    );
    drop(judge);
    drop(core);

    let core = build();
    assert_eq!(written(&core.store), N, "a restart reads all {N}");
}
