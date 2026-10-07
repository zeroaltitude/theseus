//! `route.decided`'s times (theseus-ddbi): `wait_ms`, the turn's wait for
//! the verdict after its first compile; `late`, the verdict missed it; and
//! `answered_ms`, when `route.v1`'s request came back after the turn's
//! start, if it had by the decision. And `theseus.route.wait`, each live
//! routed turn's wait by `theseus.route.late`. The rig is `tests_route`'s.

use std::time::{Duration, Instant};

use serde_json::Value;
use theseus_judge::fake::{FakeJev, FakeMode};

use crate::telemetry::tests::Receiver;
use crate::telemetry::{Telemetry, TelemetryConfig};
use crate::tests_route::{decided, mode, rig, rig_parts, turn, until_late};

/// A verdict that comes in time routes the turn, and the wait past the
/// compile ends as it lands: never later than it came, and `late` false.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_verdict_in_time_routes_the_turn_and_costs_no_wait_past_its_answer() {
    let jev = FakeJev::start().unwrap();
    mode(&jev, "sophisticated", 0.95);
    let r = rig(Some(&jev), 1, |_| {});
    let res = turn(&r.core, None, "Weigh two designs for the log.", None).await;
    assert_eq!(res.route.as_ref().unwrap().reason, "verdict");
    assert_eq!(res.profile, "opus");
    let d = &decided(&r.core.store)[0];
    assert_eq!(d["late"], false, "{d}");
    let answered = d["answered_ms"].as_u64().expect("answered");
    let waited = d["wait_ms"].as_u64().unwrap();
    assert!(
        waited <= answered,
        "the wait ends as the verdict lands: {d}"
    );
}

/// A held verdict: the turn waits `max_wait_ms` after its compile and no
/// more, runs on the session's own profile, and its row says `late`, with
/// no `answered_ms`; the verdict, released, applies to the next message.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_late_verdict_never_holds_the_turn_past_its_bound_and_is_recorded_late() {
    let jev = FakeJev::start().unwrap();
    mode(&jev, "sophisticated", 0.95);
    jev.set_mode(FakeMode::Held);
    let r = rig(Some(&jev), 2, |c| {
        c.routing.max_wait_ms = 300;
        c.judge.total_secs = 30;
    });
    let t0 = Instant::now();
    let res = turn(&r.core, None, "Weigh two designs for the log.", None).await;
    let took = t0.elapsed();
    assert_eq!(res.route.as_ref().unwrap().reason, "late");
    assert_eq!(res.profile, "sonnet");
    let d = &decided(&r.core.store)[0];
    let waited = d["wait_ms"].as_u64().unwrap();
    assert!(
        (300..1300).contains(&waited),
        "the bound, not the verdict: {d}"
    );
    assert_eq!(
        (d["late"].clone(), d["answered_ms"].clone()),
        (Value::Bool(true), Value::Null)
    );
    assert!(took < Duration::from_secs(10), "{took:?}");
    jev.release();
    until_late(&r.core, &res.session_id).await;
    jev.set_mode(FakeMode::Up);
    let next = turn(&r.core, Some(&res.session_id), "And the index?", None).await;
    assert_eq!(
        next.profile, "opus",
        "the late verdict, on the next message"
    );
    assert_eq!(decided(&r.core.store)[1]["late"], false);
}

/// `theseus.route.wait`'s points: one in time, one late, each a histogram
/// point with `theseus.route.late`.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_route_wait_metric_counts_each_routed_turn_by_late() {
    let rx = Receiver::start(vec![]).await;
    let jev = FakeJev::start().unwrap();
    let tel = TelemetryConfig {
        otlp_endpoint: Some(rx.endpoint()),
        ..Default::default()
    };
    let r = rig_parts(
        Some(&jev),
        2,
        |c| {
            c.routing.max_wait_ms = 300;
            c.judge.total_secs = 30;
        },
        |p| p.telemetry = Some(Telemetry::from_config(&tel, None).unwrap()),
    );
    mode(&jev, "sophisticated", 0.95);
    let first = turn(&r.core, None, "Weigh two designs for the log.", None).await;
    assert_eq!(first.route.as_ref().unwrap().reason, "verdict");
    jev.set_mode(FakeMode::Held);
    let second = turn(&r.core, None, "Weigh two more.", None).await;
    assert_eq!(second.route.as_ref().unwrap().reason, "late");
    jev.release();
    assert!(r.core.telemetry().flush(Duration::from_secs(10)).await);
    let got = rx.at("/v1/metrics");
    let metrics = got.last().expect("a metrics export").body["resourceMetrics"][0]["scopeMetrics"]
        [0]["metrics"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    let m = metrics
        .iter()
        .find(|m| m["name"] == "theseus.route.wait")
        .expect("theseus.route.wait");
    assert_eq!(m["unit"], "ms");
    let mut points: Vec<(bool, u64, f64)> = m["histogram"]["dataPoints"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| {
            let a = &p["attributes"][0];
            assert_eq!(a["key"], "theseus.route.late");
            let count = p["count"]
                .as_str()
                .map_or_else(|| p["count"].as_u64().unwrap(), |s| s.parse().unwrap());
            (
                a["value"]["boolValue"].as_bool().unwrap(),
                count,
                p["sum"].as_f64().unwrap(),
            )
        })
        .collect();
    points.sort_by_key(|a| a.0);
    assert_eq!(points.len(), 2, "{points:?}");
    assert_eq!((points[0].0, points[0].1), (false, 1));
    assert_eq!((points[1].0, points[1].1), (true, 1));
    assert!(
        points[1].2 >= 300.0,
        "the late turn waited the bound: {points:?}"
    );
}
