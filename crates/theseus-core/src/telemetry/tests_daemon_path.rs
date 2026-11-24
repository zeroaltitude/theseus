//! The judge's feeds on the daemon's path (theseus-qqhd): a core
//! whose parts bring no pipeline and whose config names the receiver's
//! endpoint, so `install_telemetry` builds the exporter after serving
//! (`Core::build_telemetry`) and hands it to each of them there. A test core
//! built with a pipeline never reaches those lines, but `Core::build` hands its
//! pipeline to the same feeds itself (theseus-xd6l).

use std::sync::Arc;
use std::time::{Duration, Instant};

use serde_json::Value;
use theseus_judge::fake::{FakeJev, Scripted as Jev};

use super::tests::{flushed, last_metrics, point_with, points_of, Receiver};
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
        ("theseus.judge.pack", "loop.v2"),
        ("theseus.judge.mode", "shadow"),
        ("theseus.judge.band", "act"),
        ("theseus.judge.class", "reply"),
    ];
    assert_eq!(
        point_with(&m, "theseus.judge.calls", &answered)["asInt"],
        "1"
    );
}
