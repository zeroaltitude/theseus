//! A clean stop flushes the judge's sink (theseus-ych4): every judgment
//! settled and not yet written is in the store before the stop's last
//! checkpoint, so a restart reads every one. A SIGKILL loses the queue
//! (`judge/sink.rs`'s module doc), which this does not test.

use std::sync::Arc;
use std::time::{Duration, Instant};

use theseus_judge::fake::FakeJev;

use serde_json::json;

use crate::judge::sink::{MAX_ROWS, STOP_ROWS};
use crate::provider::FakeProvider;
use crate::rpc::{Core, Parts};
use crate::store::Store;
use crate::tests_judge::{board, config, texts, warm};
use crate::tests_sink_backlog::{settled, written};

/// The judgments settled before the stop: more than a few frames' worth.
const N: usize = 1_100;
/// The judgments among them whose state's blob is staged.
const BLOBS: usize = 300;

/// N judgments settle while a turn runs, so the sink holds them all, a
/// staged state's blob named by each of the first [`BLOBS`]; a clean stop
/// writes the blobs as one batch (a sync each and the directory's once),
/// then the rows in as few frames as a stop's frame size allows
/// (theseus-ehkp), and a restart on the same store finds all N and their
/// blobs.
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
    let mut digests = Vec::new();
    for i in 0..N {
        let mut j = settled(i);
        if i < BLOBS {
            let d = judge.stage_blob(format!("{{\"a staged state\": {i}}}").as_bytes());
            j.context["blob"] = json!(d);
            digests.push(d);
        }
        judge.settle(&j);
    }
    tokio::time::sleep(crate::judge::FLUSH_EVERY + Duration::from_millis(500)).await;
    assert_eq!(written(&core.store), 0, "the sink waits for the turn");
    assert_eq!(judge.unwritten(), N);
    let frames_before = core.store.stats().unwrap().frames_appended;
    let syncs_before = core.store.blobs().syncs();
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
        N.div_ceil(STOP_ROWS) as u64,
        "in as few frames as it takes"
    );
    // The writer wrote the front frame's blobs before its wait for the
    // turn; the stop writes the rest.
    assert_eq!(
        core.store.blobs().syncs() - syncs_before,
        (BLOBS - MAX_ROWS) as u64 + 1,
        "the blobs as one batch: a sync each, the directory's once"
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
    assert!(
        digests.iter().all(|d| core.store.blobs().read(d).is_some()),
        "and every blob a row names"
    );
}

/// A measure, not a test (`--run-ignored only`): a clean stop with about
/// 3,000 judgments queued, 700 of them naming a staged blob, as the review's
/// stop held (theseus-ehkp). Prints the flush's time, its frames, and its
/// blobs' syncs.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "a measure: run it alone"]
async fn measure_a_stop_with_a_backlog_and_its_blobs() {
    const QUEUED: usize = 3_000;
    const STAGED: usize = 700;
    let jev = FakeJev::start().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let cfg = config(dir.path(), Some(&jev));
    let store = Store::open(&dir.path().join("store")).unwrap();
    let mut p = Parts::for_tests(cfg, Arc::new(FakeProvider::scripted(texts(1))), store);
    p.secrets = board();
    let core = Core::build(p).unwrap();
    warm(&core);
    let judge = core.runner.judge.clone();
    judge.jev().unwrap();
    let running = core.runner.pass.turns().begin().await;
    for i in 0..QUEUED {
        let mut j = settled(i);
        if (i * STAGED) % QUEUED < STAGED {
            let state = format!(
                "{{\"a staged state\": {i}, \"pad\": \"{}\"}}",
                "x".repeat(1_500)
            );
            j.context["blob"] = json!(judge.stage_blob(state.as_bytes()));
        }
        judge.settle(&j);
    }
    tokio::time::sleep(crate::judge::FLUSH_EVERY + Duration::from_millis(500)).await;
    let (frames, syncs) = (
        core.store.stats().unwrap().frames_appended,
        core.store.blobs().syncs(),
    );
    let t0 = Instant::now();
    let n = theseus_store::blocking(|| judge.flush_sink());
    let took = t0.elapsed();
    eprintln!(
        "a stop's flush: {n} judgments in {:.1} ms, {} frames, {} blob syncs ({} turn running)",
        took.as_secs_f64() * 1000.0,
        core.store.stats().unwrap().frames_appended - frames,
        core.store.blobs().syncs() - syncs,
        core.runner.pass.turns().running(),
    );
    drop(running);
}
