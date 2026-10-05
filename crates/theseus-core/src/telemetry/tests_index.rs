//! The index tender's gauges and restarts (theseus-gfi4; M6 §2.13), sampled
//! from health's block: through the tender tests' stand-in and stand-in OS,
//! into a pipeline that posts to a receiver.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde_json::Value;

use super::tests::{flushed, last_metrics, pipeline, points_of, tuning, Receiver};
use super::Telemetry;
use crate::config::IndexConfig;
use crate::tender::{HEALTH_DEADLINE, STATUS_DEADLINE};
use crate::tests_tender::{settle, signal, stand_in, supervisor, FakeOs};

/// The one point of `name`, as an integer; `None` with no point.
fn int_of(metrics: &[Value], name: &str) -> Option<u64> {
    let points = points_of(metrics, name);
    assert!(points.len() <= 1, "{name}: {points:#?}");
    points
        .first()
        .map(|p| p["asInt"].as_str().unwrap().parse().unwrap())
}

/// What the receiver last got of the tender, after a flush.
async fn sampled(rx: &Receiver, tel: &Telemetry) -> Vec<Option<u64>> {
    flushed(tel).await;
    let m = last_metrics(&rx.got());
    [
        "theseus.index.lag_bytes",
        "theseus.index.lag_ms",
        "theseus.index.documents",
        "theseus.index.rss_bytes",
        "theseus.index.restarts",
    ]
    .iter()
    .map(|n| int_of(&m, n))
    .collect()
}

/// A sample of a running tender records what it answered: its lag in bytes
/// and ms, its documents (not its nodes), and its RSS; no restart yet. Its
/// status is asked first under `index.status`'s deadline, so a sample that a
/// loaded machine makes late records that answer, as a late tender's is.
#[tokio::test]
async fn a_sample_records_the_tenders_numbers() {
    let rx = Receiver::start(vec![]).await;
    let tel = pipeline(&rx.endpoint(), None, tuning());
    let dir = tempfile::tempdir().unwrap();
    stand_in(dir.path(), Arc::default());
    let os = Arc::new(FakeOs::default());
    let (t, _) = supervisor(IndexConfig::default(), dir.path(), os);
    let task = tokio::spawn(t.clone().run());
    settle().await;
    assert_eq!(t.status().unwrap().state, "running");
    assert_eq!(t.health(STATUS_DEADLINE).await.state, "ready");
    t.sample(&tel).await;
    let got = sampled(&rx, &tel).await;
    assert_eq!(
        got,
        [Some(4096), Some(250), Some(3), Some(48 << 20), Some(0)]
    );
    task.abort();
}

/// A kill and the restart after its backoff raise the restarts by one,
/// however many samples see it.
#[tokio::test]
async fn a_kill_and_restart_raise_the_restarts_by_one() {
    let rx = Receiver::start(vec![]).await;
    let tel = pipeline(&rx.endpoint(), None, tuning());
    let dir = tempfile::tempdir().unwrap();
    let os = Arc::new(FakeOs::default());
    let (t, _) = supervisor(IndexConfig::default(), dir.path(), os.clone());
    let task = tokio::spawn(t.clone().run());
    settle().await;
    t.sample(&tel).await;
    assert_eq!(sampled(&rx, &tel).await[4], Some(0));
    let (first, _) = os.last();
    t.exited(first, signal(9));
    let t0 = Instant::now();
    while (
        t.status().unwrap().state.as_str(),
        t.status().unwrap().restarts,
    ) != ("running", 1)
    {
        assert!(t0.elapsed() < Duration::from_secs(10), "no restart");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    for _ in 0..3 {
        t.sample(&tel).await;
    }
    assert_eq!(sampled(&rx, &tel).await[4], Some(1));
    task.abort();
}

/// A tender that hangs keeps each sample within health's deadline, and the
/// gauges keep its last answer.
#[tokio::test]
async fn a_hung_tender_keeps_each_sample_within_the_deadline() {
    let rx = Receiver::start(vec![]).await;
    let tel = pipeline(&rx.endpoint(), None, tuning());
    let dir = tempfile::tempdir().unwrap();
    let hang = Arc::new(AtomicBool::new(false));
    stand_in(dir.path(), hang.clone());
    let os = Arc::new(FakeOs::default());
    let (t, _) = supervisor(IndexConfig::default(), dir.path(), os);
    let task = tokio::spawn(t.clone().run());
    settle().await;
    assert_eq!(t.health(STATUS_DEADLINE).await.state, "ready");
    hang.store(true, Ordering::SeqCst);
    for _ in 0..3 {
        let t0 = Instant::now();
        t.sample(&tel).await;
        let took = t0.elapsed();
        assert!(
            took >= HEALTH_DEADLINE && took < Duration::from_secs(2),
            "{took:?}"
        );
    }
    let got = sampled(&rx, &tel).await;
    assert_eq!(got[2], Some(3), "its last answer");
    task.abort();
}

/// Nothing asks a tender before its supervisor runs one (only its restarts
/// are recorded), and nothing asks it with telemetry off, though it runs;
/// a core without an endpoint, or with `[index]` off, starts no sampler.
#[tokio::test]
async fn nothing_asks_before_the_tender_runs_or_with_telemetry_off() {
    let rx = Receiver::start(vec![]).await;
    let tel = pipeline(&rx.endpoint(), None, tuning());
    let dir = tempfile::tempdir().unwrap();
    let asked = stand_in(dir.path(), Arc::default());
    let os = Arc::new(FakeOs::default());
    let (t, _) = supervisor(IndexConfig::default(), dir.path(), os);
    t.sample(&tel).await;
    assert_eq!(asked.load(Ordering::SeqCst), 0, "asked before it runs");
    assert_eq!(sampled(&rx, &tel).await, [None, None, None, None, Some(0)]);
    let task = tokio::spawn(t.clone().run());
    settle().await;
    assert_eq!(t.status().unwrap().state, "running");
    t.sample(&Telemetry::disabled()).await;
    assert_eq!(asked.load(Ordering::SeqCst), 0, "asked with telemetry off");
    // With telemetry on, a sample asks it (under load, one may give up at
    // the deadline before its request is written: the next asks again).
    for _ in 0..50 {
        if asked.load(Ordering::SeqCst) > 0 {
            break;
        }
        t.sample(&tel).await;
        tokio::task::yield_now().await;
    }
    assert!(
        asked.load(Ordering::SeqCst) > 0,
        "a sample asks a running tender"
    );
    task.abort();

    for (endpoint, index) in [(None, true), (Some(rx.endpoint()), false)] {
        let dir = tempfile::tempdir().unwrap();
        let mut cfg = crate::Config::example();
        cfg.server.state_dir = dir.path().to_string_lossy().into_owned();
        cfg.telemetry.otlp_endpoint = endpoint;
        cfg.index.enabled = index;
        let store = crate::store::Store::open(&dir.path().join("store")).unwrap();
        let fake = Arc::new(crate::provider::FakeProvider::default());
        let core = crate::Core::build(crate::rpc::Parts::for_tests(cfg, fake, store)).unwrap();
        assert!(!core.sample_index_after_serving(), "index {index}");
    }
}

/// A core with an endpoint and `[index]` on samples its tender every
/// `metrics_interval_secs`, after serving: before the tender starts, its
/// restarts alone.
#[tokio::test]
async fn a_core_samples_its_tender_each_interval() {
    let rx = Receiver::start(vec![]).await;
    let dir = tempfile::tempdir().unwrap();
    let mut cfg = crate::Config::example();
    cfg.server.state_dir = dir.path().to_string_lossy().into_owned();
    cfg.telemetry.otlp_endpoint = Some(rx.endpoint());
    cfg.telemetry.metrics_interval_secs = 1;
    cfg.index.enabled = true;
    let store = crate::store::Store::open(&dir.path().join("store")).unwrap();
    let fake = Arc::new(crate::provider::FakeProvider::default());
    let core = crate::Core::build(crate::rpc::Parts {
        telemetry: Some(pipeline(&rx.endpoint(), None, tuning())),
        ..crate::rpc::Parts::for_tests(cfg, fake, store)
    })
    .unwrap();
    assert!(core.sample_index_after_serving());
    let t0 = Instant::now();
    loop {
        tokio::time::sleep(Duration::from_millis(200)).await;
        if sampled(&rx, core.telemetry()).await[4] == Some(0) {
            break;
        }
        assert!(t0.elapsed() < Duration::from_secs(10), "no sample");
    }
}
