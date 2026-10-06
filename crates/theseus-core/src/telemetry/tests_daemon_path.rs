//! The judge's and memory's feeds on the daemon's path (theseus-qqhd): a core
//! whose parts bring no pipeline and whose config names the receiver's
//! endpoint, so `install_telemetry` builds the exporter after serving
//! (`Core::build_telemetry`) and hands it to each of them there. A test core
//! built with a pipeline never reaches those lines, but `Core::build` hands its
//! pipeline to the same feeds itself (theseus-xd6l).

use std::sync::Arc;
use std::time::{Duration, Instant};

use serde_json::Value;
use theseus_judge::fake::{FakeJev, Scripted as Jev};

use super::tests::{flushed, last_metrics, pipeline, point_with, points_of, tuning, Receiver};
use crate::config::memory::MemoryArm;
use crate::config::MemoryMode;
use crate::recall::retention::Phase;
use crate::rpc::{Core, Parts};
use crate::store::Store;
use crate::tests_judge::{board, config, texts, turn, until_judged};

/// Flush until the receiver's last metrics hold what `ok` asks of them, at
/// most 10 s: a metric recorded just after its row is written reaches the
/// next flush.
async fn metrics_until(
    core: &Core,
    rx: &Receiver,
    what: &str,
    ok: impl Fn(&[Value]) -> bool,
) -> Vec<Value> {
    let t0 = Instant::now();
    loop {
        flushed(core.telemetry()).await;
        let m = last_metrics(&rx.got());
        if ok(&m) {
            return m;
        }
        assert!(t0.elapsed() < Duration::from_secs(10), "no {what}: {m:#?}");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

/// A judged call counts in `theseus.judge.calls`, through the judge's own
/// `export_to` in `build_telemetry`.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_judgment_on_the_daemons_path_is_counted() {
    let rx = Receiver::start(vec![]).await;
    let jev = FakeJev::start().unwrap();
    jev.script(
        "work_state",
        Jev::Choice {
            option: "progressing".into(),
            confidence: 0.95,
        },
    );
    jev.script("announced_unfinished", Jev::Noul(0.97));
    let dir = tempfile::tempdir().unwrap();
    let mut cfg = config(dir.path(), Some(&jev));
    cfg.telemetry.otlp_endpoint = Some(rx.endpoint());
    cfg.validate().unwrap();
    let store = Store::open(&dir.path().join("store")).unwrap();
    let fake = Arc::new(crate::provider::FakeProvider::scripted(texts(1)));
    let mut p = Parts::for_tests(cfg, fake, store);
    p.secrets = board();
    p.telemetry = None;
    let core = Core::build(p).unwrap();
    core.clone().install_telemetry().await;
    assert!(core.telemetry().enabled(), "the exporter is built");

    turn(&core, None, "Say done.").await;
    until_judged(&core.store, 1).await;
    let m = metrics_until(&core, &rx, "the judge's calls", |m| {
        !points_of(m, "theseus.judge.calls").is_empty()
    })
    .await;
    let answered = [
        ("theseus.judge.pack", "loop.v1"),
        ("theseus.judge.mode", "shadow"),
        ("theseus.judge.band", "act"),
        ("theseus.judge.class", "reply"),
    ];
    assert_eq!(
        point_with(&m, "theseus.judge.calls", &answered)["asInt"],
        "1"
    );
}

/// With `+retention`, the projection's size is `theseus.memory.retention.nodes`:
/// what `export_to` measures at the build, and what each frame handed to the
/// projection after it measures again.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_retention_gauge_on_the_daemons_path_is_the_projections_size() {
    let rx = Receiver::start(vec![]).await;
    let dir = tempfile::tempdir().unwrap();
    let mut cfg = config(dir.path(), None);
    cfg.memory.mode = MemoryMode::Live;
    cfg.memory.arm = MemoryArm::Retention;
    cfg.telemetry.otlp_endpoint = Some(rx.endpoint());
    cfg.validate().unwrap();
    let store = Store::open(&dir.path().join("store")).unwrap();
    let fake = Arc::new(crate::provider::FakeProvider::default());
    let mut p = Parts::for_tests(cfg, fake, store);
    p.telemetry = None;
    let core = Core::build(p).unwrap();

    // Built before the exporter: only `export_to` can measure it.
    let t = crate::tests_retention::t0();
    core.store
        .append(&[
            crate::tests_retention::labeled("nod_wren", t, "high"),
            crate::tests_retention::labeled("nod_ash", t, "medium"),
        ])
        .unwrap();
    crate::tests_retention::built(&core).await;
    assert_eq!(core.runner.memory.retention().shape().nodes, 2);
    core.clone().install_telemetry().await;
    assert!(core.telemetry().enabled(), "the exporter is built");
    let nodes = |m: &[Value]| -> Option<String> {
        points_of(m, "theseus.memory.retention.nodes")
            .first()
            .map(|p| p["asInt"].as_str().unwrap().to_string())
    };
    let m = metrics_until(&core, &rx, "the retention gauge", |m| nodes(m).is_some()).await;
    assert_eq!(nodes(&m).as_deref(), Some("2"));

    // Grown after it: the gauge follows the projection.
    crate::tests_retention::frame(
        &core,
        &[crate::tests_retention::labeled("nod_reed", t, "floor")],
    );
    let p = core.runner.memory.retention();
    assert_eq!((p.phase(), p.shape().nodes), (Phase::Ready, 3));
    let m = metrics_until(&core, &rx, "the grown gauge", |m| {
        nodes(m).as_deref() == Some("3")
    })
    .await;
    assert_eq!(nodes(&m).as_deref(), Some("3"));
}

/// A core built with a pipeline (every test core) feeds the retention gauge
/// as the daemon's path does: `Core::build` hands the exporter to memory
/// beside the judge and the stops (theseus-xd6l).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_core_built_with_a_pipeline_records_the_retention_gauge() {
    let rx = Receiver::start(vec![]).await;
    let dir = tempfile::tempdir().unwrap();
    let mut cfg = config(dir.path(), None);
    cfg.memory.mode = MemoryMode::Live;
    cfg.memory.arm = MemoryArm::Retention;
    cfg.validate().unwrap();
    let store = Store::open(&dir.path().join("store")).unwrap();
    let fake = Arc::new(crate::provider::FakeProvider::default());
    let mut p = Parts::for_tests(cfg, fake, store);
    p.telemetry = Some(pipeline(&rx.endpoint(), None, tuning()));
    let core = Core::build(p).unwrap();
    assert!(core.telemetry().enabled(), "the pipeline is the core's");

    let t = crate::tests_retention::t0();
    core.store
        .append(&[
            crate::tests_retention::labeled("nod_wren", t, "high"),
            crate::tests_retention::labeled("nod_ash", t, "medium"),
        ])
        .unwrap();
    crate::tests_retention::built(&core).await;
    let nodes = |m: &[Value]| -> Option<String> {
        points_of(m, "theseus.memory.retention.nodes")
            .first()
            .map(|p| p["asInt"].as_str().unwrap().to_string())
    };
    let m = metrics_until(&core, &rx, "the retention gauge", |m| nodes(m).is_some()).await;
    assert_eq!(nodes(&m).as_deref(), Some("2"));

    crate::tests_retention::frame(
        &core,
        &[crate::tests_retention::labeled("nod_reed", t, "floor")],
    );
    let m = metrics_until(&core, &rx, "the grown gauge", |m| {
        nodes(m).as_deref() == Some("3")
    })
    .await;
    assert_eq!(nodes(&m).as_deref(), Some("3"));
}
