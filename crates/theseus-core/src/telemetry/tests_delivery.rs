//! What the exporter does when a receiver misbehaves, and its other edges
//! (theseus-hee): retries and drops, a receiver that never answers, the
//! headers secret, an endpoint that is no URL, partial successes, the SDK's
//! limits, and health's summaries.

use std::time::{Duration, Instant};

use serde_json::{json, Value};
use theseus_protocol::{Span, TelemetryStatus};

use super::export::{self, Tuning};
use super::tests::{
    cfg, closed_port, flushed, pipeline, result_with, s, sample_trace, tuning, two_loops_result,
    Receiver,
};
use super::Telemetry;
use crate::secrets::Secret;

/// A 503 and then a 200: the batch is posted twice and taken once; nothing
/// is dropped.
#[tokio::test]
async fn a_503_then_a_200_delivers_the_batch_once() {
    let rx = Receiver::start(vec![503, 200]).await;
    let tel = pipeline(&rx.endpoint(), None, tuning());
    tel.record_turn(&result_with(sample_trace()));
    flushed(&tel).await;
    let traces = rx.at("/v1/traces");
    assert_eq!(traces.len(), 2, "the post and its one retry");
    assert_eq!(traces[0].body, traces[1].body, "the same batch");
    let st = tel.status();
    assert_eq!(
        (st.traces_sent, st.metrics_sent, st.dropped),
        (1, 1, 0),
        "{st:?}"
    );
    assert_eq!(st.spans_sent, 3);
    assert_eq!(st.last_error, None);
}

/// A receiver that keeps failing gets the post and one retry; then the batch
/// is dropped, and health counts it and says why.
#[tokio::test]
async fn a_failing_receiver_gets_one_retry_then_the_batch_is_dropped_and_counted() {
    let rx = Receiver::start(vec![503, 503, 429, 429]).await;
    let tel = pipeline(&rx.endpoint(), None, tuning());
    tel.record_turn(&result_with(sample_trace()));
    flushed(&tel).await;
    assert_eq!(rx.at("/v1/traces").len(), 2, "the post and one retry");
    assert_eq!(rx.at("/v1/metrics").len(), 2);
    let st = tel.status();
    assert_eq!(
        (st.traces_sent, st.metrics_sent, st.dropped),
        (0, 0, 2),
        "{st:?}"
    );
    assert_eq!(
        st.last_error.as_deref(),
        Some("metrics: the receiver answered 429 Too Many Requests, after one retry")
    );
    // A 4xx other than 429 is not retried.
    let rx = Receiver::start(vec![401]).await;
    let tel = pipeline(&rx.endpoint(), None, tuning());
    tel.record_turn(&result_with(sample_trace()));
    flushed(&tel).await;
    assert_eq!(rx.at("/v1/traces").len(), 1, "no retry");
    assert_eq!(
        tel.status().last_error.as_deref(),
        Some("traces: the receiver answered 401 Unauthorized")
    );
}

/// A receiver that is down: the batch is dropped after its retry, and health
/// counts it: `theseus health` says so.
#[tokio::test]
async fn a_receiver_that_is_down_is_counted_in_health() {
    let endpoint = closed_port().await;
    let tel = pipeline(&endpoint, None, tuning());
    let t0 = Instant::now();
    tel.record_turn(&result_with(sample_trace()));
    flushed(&tel).await;
    assert!(
        t0.elapsed() >= Duration::from_millis(100),
        "two backoffs: {:?}",
        t0.elapsed()
    );
    let st = tel.status();
    assert_eq!((st.traces_sent, st.dropped), (0, 2), "{st:?}");
    let why = st.last_error.clone().unwrap();
    assert!(
        why.starts_with("metrics: ") && why.ends_with(", after one retry"),
        "{why}"
    );
    assert!(why.contains("Connection refused"), "{why}");
    let line = st.summary(st.last_error_at_ms.unwrap() + 1500);
    assert_eq!(
        line,
        format!("exporting to {endpoint} · sent 0 (0 traces, 0 metrics) · dropped 2 · last error {why} (2 s ago)")
    );
}

/// A receiver that never answers: a turn's hand-over takes microseconds
/// while the sender waits on it, and the queue keeps only the newest traces,
/// counting the rest.
#[tokio::test]
async fn a_hanging_receiver_never_slows_a_turn() {
    let rx = Receiver::start(vec![0; 100]).await;
    let tel = pipeline(
        &rx.endpoint(),
        None,
        Tuning {
            timeout: Duration::from_secs(30),
            queue: 8,
            ..tuning()
        },
    );
    let r = two_loops_result();
    tel.record_turn(&r);
    rx.until("the first post, which hangs", |g| !g.is_empty())
        .await;
    let mut slowest = Duration::ZERO;
    for _ in 0..40 {
        let t = Instant::now();
        tel.record_turn(&r);
        slowest = slowest.max(t.elapsed());
    }
    // Waiting for the receiver would take its 30 s timeout.
    assert!(
        slowest < Duration::from_millis(100),
        "slowest hand-over {slowest:?}"
    );
    let st = tel.status();
    assert_eq!(st.queued, 8, "{st:?}");
    assert_eq!(st.dropped, 32, "the oldest, one by one");
    assert_eq!(
        st.last_error.as_deref(),
        Some("the queue was full, so its oldest trace was dropped")
    );
    assert!(
        !tel.flush(Duration::from_millis(50)).await,
        "a flush is bounded"
    );
    eprintln!("hand-over of a two-loop turn, slowest of 40: {slowest:?}");
}

/// The queue drops its oldest trace, not its newest.
#[test]
fn the_queue_drops_the_oldest() {
    let shared = export::Shared::for_tests(2);
    for i in 0..5u64 {
        let mut t = sample_trace();
        t.attrs["n"] = json!(i);
        shared.push(t);
    }
    let left: Vec<u64> = shared
        .queued()
        .iter()
        .map(|t| t.attrs["n"].as_u64().unwrap())
        .collect();
    assert_eq!(left, [3, 4]);
    assert_eq!(shared.status().dropped, 3);
}

// ---------------------------------------------------------------- headers

/// The headers secret's `Header: value` lines and its `k=v,k2=v2` form are
/// both sent, with every request. No value reaches an error or health.
#[tokio::test]
async fn the_headers_secret_is_sent_in_both_forms_and_never_shown() {
    for (secret, want) in [
        (
            "x-honeycomb-team: hc-value-1\nx-other: other-value-2\n",
            [
                ("x-honeycomb-team", "hc-value-1"),
                ("x-other", "other-value-2"),
            ],
        ),
        (
            "api-key=api-value-3,team=team-value-4",
            [("api-key", "api-value-3"), ("team", "team-value-4")],
        ),
    ] {
        let rx = Receiver::start(vec![401, 401]).await;
        let secret = Secret::new(secret.into());
        let tel = pipeline(&rx.endpoint(), Some(&secret), tuning());
        tel.record_turn(&result_with(sample_trace()));
        flushed(&tel).await;
        let got = rx.got();
        assert_eq!(got.len(), 2, "a trace and the metrics");
        for g in &got {
            for (k, v) in want {
                assert!(
                    g.headers.iter().any(|(hk, hv)| hk == k && hv == v),
                    "{k} on {}",
                    g.path
                );
            }
        }
        let shown = serde_json::to_string(&tel.status()).unwrap();
        for (_, v) in want {
            assert!(!shown.contains(v), "{shown}");
        }
    }
    // A value that is not a header value refuses the pipeline, naming the
    // header and never the value.
    let bad = Secret::new("x-key: secret-value-5\u{7f}".into());
    let err = Telemetry::with_tuning(&cfg("http://127.0.0.1:9"), Some(&bad), tuning())
        .err()
        .expect("refused");
    let err = format!("{err:#}");
    assert!(
        err.contains("x-key") && !err.contains("secret-value-5"),
        "{err}"
    );
}

/// An endpoint that is not an http(s) URL: nothing starts, and health says why.
#[test]
fn an_endpoint_that_is_not_a_url_is_refused() {
    for bad in ["127.0.0.1:4318", "ftp://127.0.0.1:4318"] {
        let e = Telemetry::with_tuning(&cfg(bad), None, tuning())
            .err()
            .expect("refused");
        let st = Telemetry::failed(bad, format!("{e:#}")).status();
        assert_eq!(st.state, "failed");
        assert!(
            st.summary(0)
                .starts_with(&format!("not exporting to {bad}: ")),
            "{}",
            st.summary(0)
        );
    }
}

/// A partial success is taken, and its reason kept for health.
#[test]
fn a_partial_success_is_reported() {
    use super::export::rejected;
    assert_eq!(rejected(b"{}"), None);
    assert_eq!(rejected(b""), None);
    assert_eq!(
        rejected(br#"{"partialSuccess": {"rejectedSpans": "2", "errorMessage": "too old"}}"#),
        Some("the receiver rejected 2 spans: too old".into())
    );
    assert_eq!(
        rejected(br#"{"partialSuccess": {"rejectedDataPoints": 5}}"#),
        Some("the receiver rejected 5 data points".into())
    );
    assert_eq!(rejected(br#"{"partialSuccess": {}}"#), None);
}

/// Spans and events past the SDK's limits (128) are dropped and counted.
#[test]
fn a_span_keeps_the_sdks_limits() {
    let mut attrs = serde_json::Map::new();
    for i in 0..130 {
        attrs.insert(format!("k{i:03}"), json!(i));
    }
    let marks: Vec<Span> = (0..130)
        .map(|i| s(&format!("m{i}"), "mark", i, i, Value::Null, vec![]))
        .collect();
    let root = s("turn", "turn", 0, 1_000, Value::Object(attrs), marks);
    let out = spans::spans(&root);
    assert_eq!(out.len(), 1);
    // 130 attributes and theseus.kind: 131, of which 128 are kept.
    assert_eq!(
        (out[0].attributes.len(), out[0].dropped_attributes_count),
        (128, 3)
    );
    assert_eq!((out[0].events.len(), out[0].dropped_events_count), (128, 2));
    assert_eq!(out[0].events[0].name, "m0");
}

/// Summaries: waiting, and a daemon older than theseus-hee.
#[test]
fn health_says_what_telemetry_is_doing() {
    let waiting = TelemetryStatus {
        otlp_endpoint: Some("http://c:4318".into()),
        state: "waiting".into(),
        detail: Some("its headers secret otlp_headers is resolving".into()),
        ..Default::default()
    };
    assert_eq!(
        waiting.summary(0),
        "waiting to export to http://c:4318: its headers secret otlp_headers is resolving"
    );
    let old = TelemetryStatus {
        enabled: true,
        otlp_endpoint: Some("http://c:4318".into()),
        ..Default::default()
    };
    assert_eq!(old.summary(0), "OTLP/HTTP → http://c:4318");
    let ok = TelemetryStatus {
        otlp_endpoint: Some("http://c:4318".into()),
        state: "exporting".into(),
        traces_sent: 3,
        metrics_sent: 2,
        queued: 1,
        ..Default::default()
    };
    assert_eq!(
        ok.summary(0),
        "exporting to http://c:4318 · sent 5 (3 traces, 2 metrics) · dropped 0 · 1 waiting"
    );
}
