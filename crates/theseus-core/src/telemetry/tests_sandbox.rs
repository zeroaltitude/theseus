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

/// Each result an L1 job leaves is counted once, whatever order results are
/// read in: the later-started job read first, and two jobs started in the same
/// millisecond, are all counted (the start is not behind health's "newest
/// launch"). Limits are one `theseus.sandbox.limit_hits` point per limit, each
/// count the number of jobs; an L0 job's detail counts nothing.
#[tokio::test]
async fn every_l1_result_counts_its_start_and_its_limits_once() {
    let rx = Receiver::start(vec![]).await;
    let dir = tempfile::tempdir().unwrap();
    let core = core_in(dir.path(), &rx.endpoint());
    let sandbox = &core.runner.tools.sandbox;
    let l1 = |us: u64, hits: serde_json::Value| json!({"sandbox": {"class": "l1", "start_us": us, "limit_hits": hits}});
    let out = json!([{"limit": "output_mb", "max_mb": 64}]);
    let both = json!([{"limit": "output_mb", "max_mb": 64}, {"limit": "scratch_mb", "max_mb": 1}]);
    sandbox.counted(&l1(2500, out.clone()));
    sandbox.counted(&l1(2500, both));
    // Job B started before job A and is read after it; C started with A.
    sandbox.counted(&l1(1500, json!([])));
    sandbox.counted(&json!({"exit_code": 0}));
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
        (&json!("3"), &json!(6.5)),
        "three L1 jobs, in any order and with equal start times; the L0 detail counts none"
    );
}
