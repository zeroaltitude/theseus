//! The native exporter (theseus-hee): what it writes is OTLP, read back
//! through OTLP's own types (`opentelemetry-proto`, a dev-dependency only);
//! it draws the picture the OpenTelemetry SDK drew (a golden file dumped from
//! the old `otel` exporter at 964411f); and it never makes a turn wait.

use std::collections::{BTreeMap, VecDeque};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde_json::{json, Value};
use theseus_protocol::{Span, TurnSubmitResult, Usage};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

use super::export::{parse_headers, Tuning};
use super::*;

/// OTLP's enum values, written out here so a wrong constant in `otlp` fails.
const SPAN_KIND_INTERNAL: i64 = 1;
const SPAN_KIND_CLIENT: i64 = 3;
pub(super) const STATUS_CODE_ERROR: i64 = 2;
pub(super) const CUMULATIVE: i64 = 2;

// ---------------------------------------------------------------- a receiver

/// One POST the receiver read.
#[derive(Debug, Clone)]
pub(crate) struct Got {
    pub path: String,
    pub headers: Vec<(String, String)>,
    pub body: Value,
}

/// A local OTLP/HTTP receiver. Each POST gets the next scripted status (200
/// once the script runs out); a 0 is never answered.
pub(crate) struct Receiver {
    addr: std::net::SocketAddr,
    got: Arc<Mutex<Vec<Got>>>,
}

impl Receiver {
    pub(crate) async fn start(script: Vec<u16>) -> Self {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let script = Arc::new(Mutex::new(VecDeque::from(script)));
        let got = Arc::new(Mutex::new(Vec::new()));
        let g = got.clone();
        tokio::spawn(async move {
            while let Ok((sock, _)) = listener.accept().await {
                tokio::spawn(answer(sock, script.clone(), g.clone()));
            }
        });
        Self { addr, got }
    }

    pub(crate) fn endpoint(&self) -> String {
        format!("http://{}", self.addr)
    }

    pub(crate) fn got(&self) -> Vec<Got> {
        self.got.lock().unwrap().clone()
    }

    pub(crate) fn at(&self, path: &str) -> Vec<Got> {
        self.got().into_iter().filter(|g| g.path == path).collect()
    }

    /// Until `ok` holds of what came, at most 10 s.
    pub(crate) async fn until(&self, what: &str, ok: impl Fn(&[Got]) -> bool) -> Vec<Got> {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            let got = self.got();
            if ok(&got) {
                return got;
            }
            assert!(Instant::now() < deadline, "no {what} in 10 s: {got:#?}");
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    }
}

async fn answer(
    mut sock: tokio::net::TcpStream,
    script: Arc<Mutex<VecDeque<u16>>>,
    got: Arc<Mutex<Vec<Got>>>,
) {
    let mut buf = Vec::new();
    let mut chunk = [0u8; 16 * 1024];
    let head_end = loop {
        match sock.read(&mut chunk).await {
            Ok(0) | Err(_) => return,
            Ok(n) => buf.extend_from_slice(&chunk[..n]),
        }
        if let Some(i) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
            break i + 4;
        }
    };
    let head = String::from_utf8_lossy(&buf[..head_end]).into_owned();
    let mut lines = head.split("\r\n");
    let path = lines
        .next()
        .and_then(|l| l.split(' ').nth(1))
        .unwrap_or_default()
        .to_string();
    let headers: Vec<(String, String)> = lines
        .filter_map(|l| l.split_once(':'))
        .map(|(k, v)| (k.trim().to_ascii_lowercase(), v.trim().to_string()))
        .collect();
    let len: usize = headers
        .iter()
        .find(|(k, _)| k == "content-length")
        .and_then(|(_, v)| v.parse().ok())
        .unwrap_or(0);
    while buf.len() < head_end + len {
        match sock.read(&mut chunk).await {
            Ok(0) | Err(_) => return,
            Ok(n) => buf.extend_from_slice(&chunk[..n]),
        }
    }
    let body = serde_json::from_slice(&buf[head_end..head_end + len]).unwrap_or(Value::Null);
    let status = script.lock().unwrap().pop_front().unwrap_or(200);
    got.lock().unwrap().push(Got {
        path,
        headers,
        body,
    });
    if status == 0 {
        std::future::pending::<()>().await;
    }
    let reason = match status {
        200 => "OK",
        401 => "Unauthorized",
        429 => "Too Many Requests",
        503 => "Service Unavailable",
        _ => "Status",
    };
    let resp = format!(
        "HTTP/1.1 {status} {reason}\r\ncontent-type: application/json\r\ncontent-length: 2\r\nconnection: close\r\n\r\n{{}}"
    );
    let _ = sock.write_all(resp.as_bytes()).await;
    let _ = sock.shutdown().await;
}

/// A port nothing listens on.
pub(super) async fn closed_port() -> String {
    let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = l.local_addr().unwrap();
    drop(l);
    format!("http://{addr}")
}

pub(super) fn cfg(endpoint: &str) -> TelemetryConfig {
    TelemetryConfig {
        otlp_endpoint: Some(endpoint.to_string()),
        ..Default::default()
    }
}

/// Short times for tests; the interval is long unless a test wants ticks.
pub(super) fn tuning() -> Tuning {
    Tuning {
        interval: Duration::from_secs(3600),
        timeout: Duration::from_secs(2),
        backoff: Duration::from_millis(50),
        queue: 64,
    }
}

pub(super) fn pipeline(endpoint: &str, headers: Option<&Secret>, t: Tuning) -> Telemetry {
    let tel = Telemetry::with_tuning(&cfg(endpoint), headers, t).unwrap();
    assert!(tel.enabled());
    tel
}

pub(super) async fn flushed(tel: &Telemetry) {
    assert!(
        tel.flush(Duration::from_secs(10)).await,
        "flushed within 10 s"
    );
}

// ---------------------------------------------------------------- traces

pub(super) fn s(
    name: &str,
    kind: &str,
    start: u64,
    end: u64,
    attrs: Value,
    children: Vec<Span>,
) -> Span {
    Span {
        name: name.into(),
        kind: kind.into(),
        start_us: start,
        end_us: Some(end),
        attrs,
        children,
    }
}

/// A turn of two loops and a tool call, with fixed times. KEEP IN STEP with
/// the old exporter's dump (`testdata/old-exporter.json`, from 964411f).
#[expect(clippy::too_many_lines, reason = "shape budget: split it")]
fn two_loops_and_a_tool() -> Span {
    s(
        "turn",
        "turn",
        0,
        2_400_000,
        json!({
            "turn_id": "turn_1", "session_id": "ses_1", "profile": "glm", "provider": "zai", "model": "glm-5.1",
            "continuation": false, "started_unix_ms": 1_790_000_000_150u64, "origin_unix_ms": 1_790_000_000_000u64,
            "outcome": "complete", "loops": 2, "stop_reason": "end_turn:no_tool_calls",
            "usage": {"input_tokens": 1200, "output_tokens": 80, "cache_read_input_tokens": 300, "cache_creation_input_tokens": 0}
        }),
        vec![
            s(
                "admission.wait",
                "lock",
                0,
                150,
                json!({"execution_id": "exe_1", "turn": 1, "note": "kernel admission + per-execution turn lock"}),
                vec![],
            ),
            s(
                "loop 0",
                "loop",
                200,
                1_300_000,
                json!({"loop": 0}),
                vec![
                    s(
                        "compile",
                        "compile",
                        210,
                        900,
                        json!({"nodes": 3, "est_tokens": 1100, "roots": ["/w", "/x"], "cached": null}),
                        vec![],
                    ),
                    s(
                        "action.outbox",
                        "store",
                        950,
                        8_000,
                        json!({"correlation_id": "cor_1", "tool": "provider.call", "reserved_usd": 0.0123}),
                        vec![],
                    ),
                    s(
                        "provider.call",
                        "provider",
                        8_100,
                        900_000,
                        json!({"provider": "zai", "model": "glm-5.1", "max_tokens": 8192, "digest": "abc123",
                "request_id": "req_1", "served_model": "glm-5.1", "usage": {"input_tokens": 600, "output_tokens": 40, "cache_read_input_tokens": 300, "cache_creation_input_tokens": 0},
                "stop_reason": "tool_use", "blocks": 2, "output_chars": 12, "rate_limit_tokens_remaining": null}),
                        vec![
                            s("first_byte", "mark", 400_000, 400_000, Value::Null, vec![]),
                            s("first_token", "mark", 450_000, 450_000, Value::Null, vec![]),
                        ],
                    ),
                    s(
                        "action.settle",
                        "store",
                        900_100,
                        907_000,
                        json!({"correlation_id": "cor_1", "outcome": "succeeded", "cost_usd": 0.0007, "node_id": "nod_1"}),
                        vec![],
                    ),
                    s(
                        "tool fs_read",
                        "tool",
                        910_000,
                        1_250_000,
                        json!({"tool_use_id": "tu_1", "outcome": "Done"}),
                        vec![],
                    ),
                    s(
                        "advancer",
                        "advancer",
                        1_250_100,
                        1_250_150,
                        json!({"advancer": "until_no_tool_calls", "decision": "continue"}),
                        vec![],
                    ),
                ],
            ),
            s(
                "loop 1",
                "loop",
                1_300_100,
                2_300_000,
                json!({"loop": 1}),
                vec![
                    s(
                        "compile",
                        "compile",
                        1_300_200,
                        1_301_000,
                        json!({"nodes": 5, "est_tokens": 1300}),
                        vec![],
                    ),
                    s(
                        "provider.call",
                        "provider",
                        1_310_000,
                        2_200_000,
                        json!({"provider": "zai", "model": "glm-5.1", "max_tokens": 8192, "request_id": "req_2",
                "usage": {"input_tokens": 600, "output_tokens": 40}, "stop_reason": "end_turn"}),
                        vec![s(
                            "first_token",
                            "mark",
                            1_700_000,
                            1_700_000,
                            Value::Null,
                            vec![],
                        )],
                    ),
                    s(
                        "advancer",
                        "advancer",
                        2_250_000,
                        2_250_020,
                        json!({"advancer": "until_no_tool_calls", "decision": "end_turn:no_tool_calls"}),
                        vec![],
                    ),
                ],
            ),
            s(
                "session.write",
                "store",
                2_310_000,
                2_390_000,
                Value::Null,
                vec![],
            ),
        ],
    )
}

pub(super) fn two_loops_result() -> TurnSubmitResult {
    TurnSubmitResult {
        session_id: "ses_1".into(),
        turn_id: "turn_1".into(),
        loops: 2,
        output: "done".into(),
        stop_reason: "end_turn:no_tool_calls".into(),
        provider_stop_reason: Some("end_turn".into()),
        model: "glm-5.1".into(),
        provider: "zai".into(),
        profile: "glm".into(),
        usage: Usage {
            input_tokens: 1200,
            output_tokens: 80,
            cache_read_input_tokens: 300,
            cache_creation_input_tokens: 0,
            ..Default::default()
        },
        elapsed_ms: 2400,
        first_token_ms: Some(450),
        request_id: Some("req_2".into()),
        trace: Some(two_loops_and_a_tool()),
        cost_usd: Some(0.0014),
        tool_calls: 1,
        ..Default::default()
    }
}

/// The old dump's failed turn: a 429 on the first provider call.
pub(super) fn failed_trace() -> Span {
    s(
        "turn",
        "turn",
        0,
        800_000,
        json!({"turn_id": "turn_2", "origin_unix_ms": 1_790_000_100_000u64, "outcome": "failed", "class": "rate_limited"}),
        vec![s(
            "loop 0",
            "loop",
            100,
            790_000,
            json!({"loop": 0}),
            vec![s(
                "provider.call",
                "provider",
                1_000,
                780_000,
                json!({"provider": "zai", "model": "glm-5.1", "error": "rate_limited", "message": "429 Too Many Requests"}),
                vec![],
            )],
        )],
    )
}

const NO_USAGE: Usage = Usage {
    input_tokens: 0,
    output_tokens: 0,
    cache_read_input_tokens: 0,
    cache_creation_input_tokens: 0,
    cache_creation_1h_input_tokens: 0,
};

pub(super) fn failed_turn(trace: &Span) -> FailedTurn<'_> {
    FailedTurn {
        profile: "glm",
        provider: "zai",
        model: "glm-5.1",
        class: "rate_limited",
        transient: true,
        elapsed_ms: 800,
        trace: Some(trace),
        usage: &NO_USAGE,
        cost_usd: None,
    }
}

/// The old exporter's own test trace (otel.rs, before theseus-hee).
pub(super) fn sample_trace() -> Span {
    let mut t = crate::trace::Trace::start(
        "turn",
        "turn",
        json!({"turn_id": "t1", "origin_unix_ms": 1_700_000_000_000u64}),
    );
    t.record("lock.wait", "lock", 0, 30, Value::Null);
    t.enter("loop 0", "loop", json!({"loop": 0}));
    t.enter(
        "provider.call",
        "provider",
        json!({"provider": "anthropic", "model": "claude-x"}),
    );
    t.mark_at(1000, "first_byte", "mark", Value::Null);
    t.mark_at(1200, "first_token", "mark", Value::Null);
    t.exit(json!({"request_id": "req_1", "stop_reason": "end_turn", "usage": {"input_tokens": 10, "output_tokens": 3}}));
    t.record(
        "advancer",
        "advancer",
        1300,
        1301,
        json!({"decision": "end_turn:x"}),
    );
    t.exit(Value::Null);
    t.record("session.write", "store", 1400, 1410, Value::Null);
    t.finish(json!({"outcome": "complete"}))
}

pub(super) fn result_with(trace: Span) -> TurnSubmitResult {
    TurnSubmitResult {
        session_id: "s".into(),
        turn_id: "t1".into(),
        loops: 1,
        output: "hi".into(),
        stop_reason: "stop_after_one_loop".into(),
        provider_stop_reason: Some("end_turn".into()),
        model: "claude-x".into(),
        provider: "anthropic".into(),
        profile: "sonnet".into(),
        usage: Usage {
            input_tokens: 10,
            output_tokens: 3,
            ..Default::default()
        },
        elapsed_ms: 2,
        first_token_ms: Some(1),
        request_id: Some("req_1".into()),
        trace: Some(trace),
        ..Default::default()
    }
}

/// The spans of every trace request, in order.
pub(super) fn spans_of(got: &[Got]) -> Vec<Value> {
    got.iter()
        .filter(|g| g.path == "/v1/traces")
        .flat_map(|g| {
            g.body["resourceSpans"][0]["scopeSpans"][0]["spans"]
                .as_array()
                .cloned()
                .unwrap_or_default()
        })
        .collect()
}

/// The metrics of the last metrics request.
pub(super) fn last_metrics(got: &[Got]) -> Vec<Value> {
    got.iter()
        .rev()
        .find(|g| g.path == "/v1/metrics")
        .map(|g| {
            g.body["resourceMetrics"][0]["scopeMetrics"][0]["metrics"]
                .as_array()
                .cloned()
                .unwrap_or_default()
        })
        .unwrap_or_default()
}

fn metric_names(metrics: &[Value]) -> Vec<String> {
    metrics
        .iter()
        .map(|m| m["name"].as_str().unwrap().to_string())
        .collect()
}

pub(super) fn attr<'a>(span: &'a Value, key: &str) -> Option<&'a Value> {
    span["attributes"]
        .as_array()?
        .iter()
        .find(|kv| kv["key"] == key)
        .map(|kv| &kv["value"])
}

pub(super) fn named<'a>(spans: &'a [Value], name: &str) -> &'a Value {
    spans
        .iter()
        .find(|s| s["name"] == name)
        .unwrap_or_else(|| panic!("no span {name}: {spans:#?}"))
}

pub(super) fn u64_of(v: &Value) -> u64 {
    v.as_str().unwrap().parse().unwrap()
}

// ---------------------------------------------------------------- the old tests, ported

/// otel.rs's `exports_turn_as_nested_spans_with_events`, against the wire.
#[tokio::test]
async fn exports_turn_as_nested_spans_with_events() {
    let rx = Receiver::start(vec![]).await;
    let tel = pipeline(&rx.endpoint(), None, tuning());
    tel.record_turn(&result_with(sample_trace()));
    flushed(&tel).await;
    let got = rx.got();
    let spans = spans_of(&got);
    let names: Vec<&str> = spans.iter().map(|s| s["name"].as_str().unwrap()).collect();
    // marks/lock/advancer/store are events, so exactly three spans.
    assert_eq!(spans.len(), 3, "{names:?}");
    let (turn, lp, pc) = (
        named(&spans, "turn"),
        named(&spans, "loop 0"),
        named(&spans, "provider.call"),
    );
    assert_eq!(lp["parentSpanId"], turn["spanId"]);
    assert_eq!(pc["parentSpanId"], lp["spanId"]);
    assert!(turn.get("parentSpanId").is_none(), "the root has no parent");
    assert_eq!(pc["kind"], SPAN_KIND_CLIENT);
    assert_eq!(lp["kind"], SPAN_KIND_INTERNAL);
    assert_eq!(turn["startTimeUnixNano"], "1700000000000000000");
    let ns = |v: &Value| v.as_str().unwrap().parse::<u64>().unwrap();
    assert!(ns(&pc["endTimeUnixNano"]) > ns(&pc["startTimeUnixNano"]));
    let events = |s: &Value| -> Vec<String> {
        s["events"]
            .as_array()
            .map(|e| {
                e.iter()
                    .map(|e| e["name"].as_str().unwrap().to_string())
                    .collect()
            })
            .unwrap_or_default()
    };
    let ev = events(pc);
    assert!(
        ev.contains(&"first_byte".into()) && ev.contains(&"first_token".into()),
        "{ev:?}"
    );
    let turn_ev = events(turn);
    assert!(turn_ev.contains(&"lock.wait".into()) && turn_ev.contains(&"session.write".into()));
    assert_eq!(
        attr(pc, spans::semconv::GEN_AI_REQUEST_MODEL),
        Some(&json!({"stringValue": "claude-x"}))
    );
    assert_eq!(
        attr(pc, spans::semconv::GEN_AI_RESPONSE_ID),
        Some(&json!({"stringValue": "req_1"}))
    );
    assert_eq!(
        attr(pc, spans::semconv::GEN_AI_USAGE_OUTPUT_TOKENS),
        Some(&json!({"intValue": "3"}))
    );
    let metric_names = metric_names(&last_metrics(&got));
    for want in [
        "theseus.turns",
        "theseus.tokens",
        "theseus.turn.duration_ms",
        "theseus.provider.call.duration_ms",
        "theseus.provider.first_token_ms",
    ] {
        assert!(metric_names.contains(&want.to_string()), "{metric_names:?}");
    }
}

/// otel.rs's `dollars_and_tool_calls_are_metrics`.
#[tokio::test]
async fn dollars_and_tool_calls_are_metrics() {
    let rx = Receiver::start(vec![]).await;
    let tel = pipeline(&rx.endpoint(), None, tuning());
    let mut trace = sample_trace();
    trace.children[0].children.push(Span {
        name: "tool fs_read".into(),
        kind: "tool".into(),
        start_us: 10,
        end_us: Some(20),
        ..Default::default()
    });
    let mut r = result_with(trace);
    r.cost_usd = Some(0.0123);
    tel.record_turn(&r);
    flushed(&tel).await;
    let names = metric_names(&last_metrics(&rx.got()));
    assert!(names.contains(&"theseus.cost.usd".to_string()), "{names:?}");
    assert!(
        names.contains(&"theseus.tool.calls".to_string()),
        "{names:?}"
    );
}

/// otel.rs's `failure_sets_error_status_and_counts`.
#[tokio::test]
async fn failure_sets_error_status_and_counts() {
    let rx = Receiver::start(vec![]).await;
    let tel = pipeline(&rx.endpoint(), None, tuning());
    let mut t = crate::trace::Trace::start("turn", "turn", json!({"origin_unix_ms": 1u64}));
    t.enter("loop 0", "loop", Value::Null);
    t.enter(
        "provider.call",
        "provider",
        json!({"provider": "zai", "model": "glm"}),
    );
    t.exit(json!({"error": "rate_limited", "message": "429"}));
    let root = t.finish(json!({"outcome": "failed", "class": "rate_limited"}));
    tel.record_failure(&FailedTurn {
        profile: "glm",
        provider: "zai",
        model: "glm",
        class: "rate_limited",
        transient: true,
        elapsed_ms: 500,
        trace: Some(&root),
        usage: &NO_USAGE,
        cost_usd: None,
    });
    flushed(&tel).await;
    let got = rx.got();
    let spans = spans_of(&got);
    let error = json!(STATUS_CODE_ERROR);
    assert_eq!(named(&spans, "turn")["status"]["code"], error);
    assert_eq!(named(&spans, "turn")["status"]["message"], "rate_limited");
    assert_eq!(named(&spans, "provider.call")["status"]["code"], error);
    assert_eq!(named(&spans, "provider.call")["status"]["message"], "429");
    assert!(named(&spans, "loop 0").get("status").is_none(), "unset");
    let names = metric_names(&last_metrics(&got));
    assert!(names.contains(&"theseus.provider.errors".to_string()));
}

/// otel.rs's `disabled_is_a_no_op`, and no endpoint is off.
#[tokio::test]
async fn disabled_is_a_no_op() {
    for tel in [
        Telemetry::disabled(),
        Telemetry::from_config(&TelemetryConfig::default(), None).unwrap(),
        Telemetry::from_config(&cfg("  "), None).unwrap(),
    ] {
        assert!(!tel.enabled());
        tel.record_turn(&result_with(sample_trace()));
        assert!(tel.flush(Duration::ZERO).await);
        let st = tel.status();
        assert_eq!(st.state, "off");
        assert_eq!(
            st.summary(0),
            "off (no [telemetry].otlp_endpoint)",
            "{st:?}"
        );
    }
}

/// otel.rs's `header_secret_formats`.
#[test]
fn header_secret_formats() {
    let a = parse_headers(&Secret::new("x-honeycomb-team: abc\nx-other: 1\n".into()));
    assert_eq!(
        a,
        [
            ("x-honeycomb-team".to_string(), "abc".to_string()),
            ("x-other".to_string(), "1".to_string())
        ]
    );
    let b = parse_headers(&Secret::new("api-key=zzz,team=t".into()));
    assert_eq!(
        b,
        [
            ("api-key".to_string(), "zzz".to_string()),
            ("team".to_string(), "t".to_string())
        ]
    );
}

// ---------------------------------------------------------------- metrics over time

fn point<'a>(metrics: &'a [Value], name: &str, with: (&str, &str)) -> &'a Value {
    let m = metrics
        .iter()
        .find(|m| m["name"] == name)
        .unwrap_or_else(|| panic!("no {name}"));
    let data = m.get("sum").or_else(|| m.get("histogram")).unwrap();
    data["dataPoints"]
        .as_array()
        .unwrap()
        .iter()
        .find(|p| {
            p["attributes"]
                .as_array()
                .unwrap()
                .iter()
                .any(|kv| kv["key"] == with.0 && kv["value"]["stringValue"] == with.1)
        })
        .unwrap_or_else(|| panic!("no {name} point with {with:?}"))
}

/// Two turns, two export intervals: the first export counts one turn, a
/// later one both, from the same start; cumulative, never reset.
/// The push's metrics (theseus-in3): `theseus.push.events` by method, and
/// the delay from a frame's commit to its notifications being queued.
#[tokio::test]
async fn the_push_counts_its_events_and_times_their_delay() {
    let rx = Receiver::start(vec![]).await;
    let tel = pipeline(
        &rx.endpoint(),
        None,
        Tuning {
            interval: Duration::from_millis(250),
            ..tuning()
        },
    );
    tel.record_push(3, Duration::from_micros(1500));
    tel.record_push(1, Duration::from_micros(500));
    let got = rx
        .until("the push's metrics", |g| {
            last_metrics(g)
                .iter()
                .any(|m| m["name"] == "theseus.push.delay_ms")
        })
        .await;
    let m = last_metrics(&got);
    let method = ("theseus.push.method", "execution.changed");
    assert_eq!(point(&m, "theseus.push.events", method)["asInt"], "4");
    let delay = m
        .iter()
        .find(|x| x["name"] == "theseus.push.delay_ms")
        .unwrap();
    let p = &delay["histogram"]["dataPoints"][0];
    assert_eq!(p["count"], "2", "{delay}");
    assert_eq!(p["sum"], 2.0);
}

#[tokio::test]
async fn metrics_are_cumulative_over_two_intervals() {
    let rx = Receiver::start(vec![]).await;
    let tel = pipeline(
        &rx.endpoint(),
        None,
        Tuning {
            interval: Duration::from_millis(250),
            ..tuning()
        },
    );
    let mut first = result_with(sample_trace());
    first.usage.input_tokens = 100;
    first.cost_usd = Some(0.25);
    tel.record_turn(&first);
    let got = rx
        .until("a first metrics export", |g| {
            g.iter().any(|g| g.path == "/v1/metrics")
        })
        .await;
    let one = last_metrics(&got);
    let mut second = result_with(sample_trace());
    second.usage.input_tokens = 23;
    second.cost_usd = Some(0.5);
    tel.record_turn(&second);
    let got = rx
        .until("an export counting both turns", |g| {
            let m = last_metrics(g);
            m.iter().any(|m| m["name"] == "theseus.turns")
                && point(&m, "theseus.turns", ("theseus.outcome", "complete"))["asInt"] == "2"
        })
        .await;
    let two = last_metrics(&got);
    let exports = got.iter().filter(|g| g.path == "/v1/metrics").count();
    assert!(exports >= 2, "{exports} exports");
    let complete = ("theseus.outcome", "complete");
    assert_eq!(point(&one, "theseus.turns", complete)["asInt"], "1");
    assert_eq!(point(&two, "theseus.turns", complete)["asInt"], "2");
    let input = ("theseus.token.direction", "input");
    assert_eq!(point(&one, "theseus.tokens", input)["asInt"], "100");
    assert_eq!(point(&two, "theseus.tokens", input)["asInt"], "123");
    let output = ("theseus.token.direction", "output");
    assert_eq!(point(&two, "theseus.tokens", output)["asInt"], "6");
    assert_eq!(point(&two, "theseus.cost.usd", complete)["asDouble"], 0.75);
    let d = point(&two, "theseus.turn.duration_ms", complete);
    assert_eq!(d["count"], "2");
    assert_eq!(d["sum"], 4.0);
    let (p1, p2) = (
        point(&one, "theseus.turns", complete),
        point(&two, "theseus.turns", complete),
    );
    assert_eq!(
        p1["startTimeUnixNano"], p2["startTimeUnixNano"],
        "one start"
    );
    assert!(u64_of(&p2["timeUnixNano"]) > u64_of(&p1["timeUnixNano"]));
    for m in two.iter() {
        let data = m.get("sum").or_else(|| m.get("histogram")).unwrap();
        assert_eq!(data["aggregationTemporality"], CUMULATIVE);
    }
    assert_eq!(tel.status().dropped, 0);
}

// ---------------------------------------------------------------- the corrections (theseus-yf1)

/// Every data point of the metric `name`.
pub(super) fn points_of<'a>(metrics: &'a [Value], name: &str) -> Vec<&'a Value> {
    metrics
        .iter()
        .filter(|m| m["name"] == name)
        .flat_map(|m| {
            m.get("sum")
                .or_else(|| m.get("histogram"))
                .and_then(|d| d["dataPoints"].as_array())
                .into_iter()
                .flatten()
        })
        .collect()
}

/// A point's attributes, each value as text.
pub(super) fn attrs_of(p: &Value) -> BTreeMap<String, String> {
    p["attributes"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|kv| {
            let v = &kv["value"];
            let text = match v.get("stringValue").and_then(Value::as_str) {
                Some(s) => s.to_string(),
                None => v.get("boolValue").unwrap_or(v).to_string(),
            };
            (kv["key"].as_str().unwrap().to_string(), text)
        })
        .collect()
}

/// The one point of the metric `name` whose attributes include every pair
/// of `with`.
pub(super) fn point_with<'a>(metrics: &'a [Value], name: &str, with: &[(&str, &str)]) -> &'a Value {
    let all = points_of(metrics, name);
    let found: Vec<&Value> = all
        .iter()
        .copied()
        .filter(|p| {
            let a = attrs_of(p);
            with.iter()
                .all(|(k, v)| a.get(*k).map(String::as_str) == Some(*v))
        })
        .collect();
    assert_eq!(found.len(), 1, "one {name} point with {with:?}: {all:#?}");
    found[0]
}

fn buckets_of(p: &Value) -> Vec<u64> {
    p["bucketCounts"]
        .as_array()
        .unwrap()
        .iter()
        .map(u64_of)
        .collect()
}

/// A turn of 12 s, 45 s, or 12 minutes, a provider call of 11 s, 90 s, or
/// nearly 10 minutes, and a first token of 1.5 s, 25 s, or 400 s each land in
/// a bucket of its own: the bounds go on past 10 s to 10 minutes. With the
/// SDK's alone (0 to 10 s), every one over 10 s fell in the last bucket.
#[tokio::test]
async fn durations_past_ten_seconds_have_buckets_of_their_own() {
    let rx = Receiver::start(vec![]).await;
    let tel = pipeline(&rx.endpoint(), None, tuning());
    for (turn_ms, call_ms, first_ms) in [
        (12_000u64, 11_000u64, 1_500u64),
        (45_000, 90_000, 25_000),
        (720_000, 590_000, 400_000),
    ] {
        let trace = s(
            "turn",
            "turn",
            0,
            turn_ms * 1000,
            json!({"origin_unix_ms": 1_790_000_000_000u64}),
            vec![s(
                "provider.call",
                "provider",
                0,
                call_ms * 1000,
                json!({"provider": "zai", "model": "glm-5.1"}),
                vec![s(
                    "first_token",
                    "mark",
                    first_ms * 1000,
                    first_ms * 1000,
                    Value::Null,
                    vec![],
                )],
            )],
        );
        let mut r = result_with(trace);
        r.elapsed_ms = turn_ms;
        tel.record_turn(&r);
    }
    flushed(&tel).await;
    let metrics = last_metrics(&rx.got());
    let bounds = json!([
        0.0, 5.0, 10.0, 25.0, 50.0, 75.0, 100.0, 250.0, 500.0, 750.0, 1000.0, 2500.0, 5000.0,
        7500.0, 10000.0, 20000.0, 30000.0, 60000.0, 120000.0, 300000.0, 600000.0
    ]);
    // Bucket i counts (bounds[i-1], bounds[i]]; bucket 21, over 10 minutes.
    for (name, at) in [
        ("theseus.turn.duration_ms", [15, 17, 21]),
        ("theseus.provider.call.duration_ms", [15, 18, 20]),
        ("theseus.provider.first_token_ms", [11, 16, 20]),
    ] {
        let p = point_with(&metrics, name, &[]);
        assert_eq!(p["explicitBounds"], bounds, "{name}");
        let b = buckets_of(p);
        assert_eq!(b.len(), 22, "{name}: {b:?}");
        for i in at {
            assert_eq!(b[i], 1, "{name}: bucket {i} of {b:?}");
        }
        assert_eq!(b.iter().sum::<u64>(), 3, "{name}: {b:?}");
    }
}

/// A tool span as the turn records it since theseus-yf1.
pub(super) fn tool_span(wire: &str, start: u64, end: u64, tool: [&str; 4]) -> Span {
    let [name, family, backend, result] = tool;
    s(
        &format!("tool {wire}"),
        "tool",
        start,
        end,
        json!({"tool_use_id": format!("tu_{start}"), "outcome": "as recorded", "tool": name,
            "family": family, "backend": backend, "result": result}),
        vec![],
    )
}
