//! L1's job start and the limits a job reached as metrics (theseus-zupk):
//! `theseus.job.start_ms` by class and `theseus.sandbox.limit_hits` by limit,
//! through a whole core whose telemetry posts to a receiver, counted where
//! the rows are (`Sandbox::launched`, `Sandbox::limit_hit`).

use std::sync::Arc;

use serde_json::json;

use super::tests::{attrs_of, flushed, last_metrics, pipeline, points_of, tuning, Receiver};
use crate::provider::FakeProvider;
use crate::store::Store;
use crate::{Config, Core};

fn core_in(dir: &std::path::Path, endpoint: &str) -> Arc<Core> {
    let mut cfg = Config::example();
    cfg.server.state_dir = dir.to_string_lossy().into_owned();
    cfg.tools.roots = vec![];
    let store = Store::open(&dir.join("store")).unwrap();
    Core::build(crate::rpc::Parts {
        telemetry: Some(pipeline(endpoint, None, tuning())),
        ..crate::rpc::Parts::for_tests(cfg, Arc::new(FakeProvider::scripted(vec![])), store)
    })
    .unwrap()
}

/// A limit a job reached is one `theseus.sandbox.limit_hits` point per limit,
/// each count the number of jobs, and an L1 job's start is one observation of
/// `theseus.job.start_ms` however many times its result is read.
#[tokio::test]
async fn limit_hits_and_an_l1_start_are_counted_once_per_job() {
    let rx = Receiver::start(vec![]).await;
    let dir = tempfile::tempdir().unwrap();
    let core = core_in(dir.path(), &rx.endpoint());
    let sandbox = &core.runner.tools.sandbox;
    sandbox.limit_hit("output_mb");
    sandbox.limit_hit("output_mb");
    sandbox.limit_hit("scratch_mb");
    // The same completion read three times (a result is built again when it is
    // taken, answered and shown): one start. A later job is a second.
    let detail = json!({"sandbox": {"class": "l1", "start_us": 2500}});
    for _ in 0..3 {
        sandbox.launched(&detail, 1_000);
    }
    sandbox.launched(&detail, 2_000);
    flushed(core.telemetry()).await;

    let metrics = last_metrics(&rx.got());
    let mut hits: Vec<(String, String)> = points_of(&metrics, "theseus.sandbox.limit_hits")
        .into_iter()
        .map(|p| {
            let a = attrs_of(p);
            (
                a["theseus.sandbox.limit"].clone(),
                p["asInt"].as_str().unwrap().to_string(),
            )
        })
        .collect();
    hits.sort();
    assert_eq!(
        hits,
        vec![
            ("output_mb".to_string(), "2".to_string()),
            ("scratch_mb".to_string(), "1".to_string())
        ]
    );
    let starts = points_of(&metrics, "theseus.job.start_ms");
    assert_eq!(starts.len(), 1, "{starts:#?}");
    assert_eq!(attrs_of(starts[0])["theseus.job.class"], "l1");
    assert_eq!(
        (&starts[0]["count"], &starts[0]["sum"]),
        (&json!("2"), &json!(5.0)),
        "two jobs' starts, 2.5 ms each"
    );
}
