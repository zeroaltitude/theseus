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
const STATUS_CODE_ERROR: i64 = 2;
const CUMULATIVE: i64 = 2;

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
async fn closed_port() -> String {
    let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = l.local_addr().unwrap();
    drop(l);
    format!("http://{addr}")
}

fn cfg(endpoint: &str) -> TelemetryConfig {
    TelemetryConfig {
        otlp_endpoint: Some(endpoint.to_string()),
        ..Default::default()
    }
}

/// Short times for tests; the interval is long unless a test wants ticks.
fn tuning() -> Tuning {
    Tuning {
        interval: Duration::from_secs(3600),
        timeout: Duration::from_secs(2),
        backoff: Duration::from_millis(50),
        queue: 64,
    }
}

fn pipeline(endpoint: &str, headers: Option<&Secret>, t: Tuning) -> Telemetry {
    let tel = Telemetry::with_tuning(&cfg(endpoint), headers, t).unwrap();
    assert!(tel.enabled());
    tel
}

async fn flushed(tel: &Telemetry) {
    assert!(
        tel.flush(Duration::from_secs(10)).await,
        "flushed within 10 s"
    );
}

// ---------------------------------------------------------------- traces

fn s(name: &str, kind: &str, start: u64, end: u64, attrs: Value, children: Vec<Span>) -> Span {
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

fn two_loops_result() -> TurnSubmitResult {
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
fn failed_trace() -> Span {
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

fn failed_turn(trace: &Span) -> FailedTurn<'_> {
    FailedTurn {
        profile: "glm",
        provider: "zai",
        model: "glm-5.1",
        class: "rate_limited",
        transient: true,
        elapsed_ms: 800,
        trace: Some(trace),
    }
}

/// The old exporter's own test trace (otel.rs, before theseus-hee).
fn sample_trace() -> Span {
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

fn result_with(trace: Span) -> TurnSubmitResult {
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
fn spans_of(got: &[Got]) -> Vec<Value> {
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
fn last_metrics(got: &[Got]) -> Vec<Value> {
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

fn attr<'a>(span: &'a Value, key: &str) -> Option<&'a Value> {
    span["attributes"]
        .as_array()?
        .iter()
        .find(|kv| kv["key"] == key)
        .map(|kv| &kv["value"])
}

fn named<'a>(spans: &'a [Value], name: &str) -> &'a Value {
    spans
        .iter()
        .find(|s| s["name"] == name)
        .unwrap_or_else(|| panic!("no span {name}: {spans:#?}"))
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

// ---------------------------------------------------------------- conformance

/// What `opentelemetry-proto` 0.33's serde reads differently from OTLP JSON:
/// its `NumberDataPoint.asInt` (an sfixed64) takes only a JSON number, where
/// the protobuf JSON mapping writes a decimal string, and every other 64-bit
/// field there takes the string. So this checks that each `asInt` is a
/// decimal string, and gives the crate the number.
fn for_the_proto_crate(v: &mut Value) {
    match v {
        Value::Object(m) => {
            if let Some(n) = m.get_mut("asInt") {
                let s = n.as_str().expect("asInt is a decimal string");
                *n = json!(s.parse::<i64>().expect("asInt is decimal"));
            }
            m.values_mut().for_each(for_the_proto_crate);
        }
        Value::Array(a) => a.iter_mut().for_each(for_the_proto_crate),
        _ => {}
    }
}

fn is_default(v: &Value) -> bool {
    match v {
        Value::Null => true,
        Value::Bool(b) => !b,
        Value::Number(n) => n.as_f64() == Some(0.0),
        Value::String(s) => s.is_empty() || s == "0",
        Value::Array(a) => a.is_empty(),
        Value::Object(m) => m.values().all(is_default),
    }
}

/// Every field we wrote came back from OTLP's own types unchanged, and every
/// field they add is a default: a misnamed field would have been dropped as
/// unknown, a base64 id refused, and a number where OTLP writes a string
/// written back as the string.
fn same_or_default(ours: &Value, theirs: &Value, at: &str) {
    match (ours, theirs) {
        (Value::Object(o), Value::Object(t)) => {
            for (k, v) in o {
                let back = t
                    .get(k)
                    .unwrap_or_else(|| panic!("{at}.{k}: not an OTLP field"));
                same_or_default(v, back, &format!("{at}.{k}"));
            }
            for (k, v) in t {
                assert!(
                    o.contains_key(k) || is_default(v),
                    "{at}.{k}: OTLP has {v}, and we wrote nothing"
                );
            }
        }
        (Value::Array(o), Value::Array(t)) => {
            assert_eq!(o.len(), t.len(), "{at}: length");
            for (i, (a, b)) in o.iter().zip(t).enumerate() {
                same_or_default(a, b, &format!("{at}[{i}]"));
            }
        }
        _ => assert_eq!(ours, theirs, "{at}"),
    }
}

fn round_trip<T: serde::de::DeserializeOwned + serde::Serialize>(ours: &Value) {
    let mut input = ours.clone();
    for_the_proto_crate(&mut input);
    let decoded: T = serde_json::from_value(input.clone())
        .unwrap_or_else(|e| panic!("OTLP's types refuse it: {e}\n{ours:#}"));
    let theirs = serde_json::to_value(&decoded).unwrap();
    same_or_default(&input, &theirs, "$");
}

/// The ids are hex of the right length, the enums integers, and the 64-bit
/// integers decimal strings.
fn check_encodings(v: &Value) {
    match v {
        Value::Object(m) => {
            for (k, v) in m {
                match k.as_str() {
                    "traceId" => assert!(is_hex(v, 32), "traceId {v}"),
                    "spanId" | "parentSpanId" => assert!(is_hex(v, 16), "{k} {v}"),
                    "startTimeUnixNano" | "endTimeUnixNano" | "timeUnixNano" | "count"
                    | "intValue" | "asInt" => assert!(is_decimal(v), "{k} {v}"),
                    "bucketCounts" => {
                        assert!(v.as_array().unwrap().iter().all(is_decimal), "{k} {v}")
                    }
                    "kind" | "code" | "aggregationTemporality" | "flags" => {
                        assert!(v.is_u64(), "{k} is an integer: {v}")
                    }
                    _ => check_encodings(v),
                }
            }
        }
        Value::Array(a) => a.iter().for_each(check_encodings),
        _ => {}
    }
}

fn is_hex(v: &Value, len: usize) -> bool {
    v.as_str().is_some_and(|s| {
        s.len() == len
            && s.bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    })
}

fn is_decimal(v: &Value) -> bool {
    v.as_str()
        .is_some_and(|s| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit()))
}

/// Traces and metrics, as posted, read back through `opentelemetry-proto`:
/// the turn of two loops and a tool call, and a failed turn.
#[tokio::test]
async fn what_is_posted_is_otlp_json() {
    use opentelemetry_proto::tonic::collector::metrics::v1::ExportMetricsServiceRequest;
    use opentelemetry_proto::tonic::collector::trace::v1::ExportTraceServiceRequest;
    let rx = Receiver::start(vec![]).await;
    let tel = pipeline(&rx.endpoint(), None, tuning());
    tel.record_turn(&two_loops_result());
    let failed = failed_trace();
    tel.record_failure(&failed_turn(&failed));
    flushed(&tel).await;
    let got = rx.got();
    let traces = rx.at("/v1/traces");
    let metrics = rx.at("/v1/metrics");
    assert_eq!((traces.len(), metrics.len()), (2, 1), "{got:#?}");
    for g in &got {
        let ct = g.headers.iter().find(|(k, _)| k == "content-type");
        assert_eq!(ct.map(|(_, v)| v.as_str()), Some("application/json"));
        check_encodings(&g.body);
    }
    for t in &traces {
        round_trip::<ExportTraceServiceRequest>(&t.body);
    }
    round_trip::<ExportMetricsServiceRequest>(&metrics[0].body);
    // The decoded types agree on what matters: one resource, one scope, the
    // spans, and every id their own bytes.
    let decoded: ExportTraceServiceRequest =
        serde_json::from_value(traces[0].body.clone()).unwrap();
    let rs = &decoded.resource_spans[0];
    let keys: Vec<&str> = rs
        .resource
        .as_ref()
        .unwrap()
        .attributes
        .iter()
        .map(|kv| kv.key.as_str())
        .collect();
    for k in ["service.name", "service.version", "service.instance.id"] {
        assert!(keys.contains(&k), "{keys:?}");
    }
    let spans = &rs.scope_spans[0].spans;
    assert_eq!(rs.scope_spans[0].scope.as_ref().unwrap().name, "theseus");
    assert_eq!(spans.len(), 6, "turn, 2 loops, 2 provider calls, the tool");
    assert!(spans
        .iter()
        .all(|s| s.trace_id.len() == 16 && s.span_id.len() == 8));
    assert!(spans.iter().all(|s| s.trace_id == spans[0].trace_id));
    let ids: std::collections::BTreeSet<&Vec<u8>> = spans.iter().map(|s| &s.span_id).collect();
    assert_eq!(ids.len(), 6, "distinct span ids");
    let decoded: ExportMetricsServiceRequest = serde_json::from_value({
        let mut m = metrics[0].body.clone();
        for_the_proto_crate(&mut m);
        m
    })
    .unwrap();
    let ms = &decoded.resource_metrics[0].scope_metrics[0].metrics;
    assert_eq!(ms.len(), 9, "every instrument has a point");
}

// ---------------------------------------------------------------- the old picture

/// A typed attribute value, as the old dump wrote it.
fn typed(v: &Value) -> Value {
    let (k, x) = v.as_object().unwrap().iter().next().unwrap();
    match k.as_str() {
        "stringValue" => json!({"s": x}),
        "boolValue" => json!({"b": x}),
        "intValue" => json!({"i": x.as_str().unwrap().parse::<i64>().unwrap()}),
        "doubleValue" => json!({"d": x}),
        "arrayValue" => {
            json!({"as": x["values"].as_array().unwrap().iter().map(|v| v["stringValue"].clone()).collect::<Vec<_>>()})
        }
        other => panic!("attribute type {other}"),
    }
}

fn typed_attrs(v: &Value) -> Value {
    let mut a: Vec<(String, Value)> = v
        .as_array()
        .map(|a| {
            a.iter()
                .map(|kv| (kv["key"].as_str().unwrap().to_string(), typed(&kv["value"])))
                .collect()
        })
        .unwrap_or_default();
    a.sort_by(|x, y| x.0.cmp(&y.0));
    json!(a)
}

fn u64_of(v: &Value) -> u64 {
    v.as_str().unwrap().parse().unwrap()
}

/// Our posted spans in the old dump's normal form.
fn picture_of_spans(spans: &[Value]) -> Vec<Value> {
    let label = |id: &Value| {
        spans.iter().find(|s| &s["spanId"] == id).map(|s| {
            format!(
                "{}@{}",
                s["name"].as_str().unwrap(),
                u64_of(&s["startTimeUnixNano"])
            )
        })
    };
    let first_trace = &spans[0]["traceId"];
    let mut out: Vec<Value> = spans
        .iter()
        .map(|s| {
            json!({
                "name": s["name"],
                "parent": s.get("parentSpanId").and_then(&label),
                "kind": match s["kind"].as_i64() { Some(1) => "Internal", Some(3) => "Client", _ => "?" },
                "start_ns": u64_of(&s["startTimeUnixNano"]),
                "end_ns": u64_of(&s["endTimeUnixNano"]),
                "attributes": typed_attrs(&s["attributes"]),
                "dropped_attributes": s.get("droppedAttributesCount").cloned().unwrap_or(json!(0)),
                "events": s.get("events").and_then(Value::as_array).map(|e| e.iter().map(|e| json!({
                    "name": e["name"],
                    "time_ns": u64_of(&e["timeUnixNano"]),
                    "attributes": typed_attrs(&e["attributes"]),
                    "dropped_attributes": e.get("droppedAttributesCount").cloned().unwrap_or(json!(0)),
                })).collect::<Vec<_>>()).unwrap_or_default(),
                "dropped_events": s.get("droppedEventsCount").cloned().unwrap_or(json!(0)),
                "status": match s.get("status") {
                    None => json!({"code": "unset"}),
                    Some(st) => json!({"code": "error", "message": st["message"]}),
                },
                "trace": if &s["traceId"] == first_trace { "A" } else { "B" },
            })
        })
        .collect();
    out.sort_by_key(|v| {
        (
            v["start_ns"].as_u64().unwrap(),
            v["name"].as_str().unwrap().to_string(),
        )
    });
    out
}

/// Our posted metrics in the old dump's normal form.
fn picture_of_metrics(metrics: &[Value]) -> Vec<Value> {
    let mut out: Vec<Value> = metrics
        .iter()
        .map(|m| {
            let (kind, mono, temporality, points) = if let Some(sum) = m.get("sum") {
                let points: Vec<Value> = sum["dataPoints"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|p| {
                        let value = match (p.get("asInt"), p.get("asDouble")) {
                            (Some(i), _) => json!(u64_of(i)),
                            (_, Some(d)) => d.clone(),
                            _ => Value::Null,
                        };
                        json!({"attributes": typed_attrs(&p["attributes"]), "value": value})
                    })
                    .collect();
                let kind = if points.iter().any(|p| p["value"].is_f64()) {
                    "sum_double"
                } else {
                    "sum_int"
                };
                (kind, json!(sum["isMonotonic"]), &sum["aggregationTemporality"], points)
            } else {
                let h = &m["histogram"];
                let points = h["dataPoints"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|p| {
                        json!({
                            "attributes": typed_attrs(&p["attributes"]),
                            "count": u64_of(&p["count"]),
                            "sum": p["sum"],
                            "min": p["min"],
                            "max": p["max"],
                            "bounds": p["explicitBounds"],
                            "buckets": p["bucketCounts"].as_array().unwrap().iter().map(u64_of).collect::<Vec<_>>(),
                        })
                    })
                    .collect();
                ("histogram", Value::Null, &h["aggregationTemporality"], points)
            };
            let mut points = points;
            points.sort_by_key(|p| p["attributes"].to_string());
            json!({
                "scope": "theseus",
                "name": m["name"],
                "description": m.get("description").cloned().unwrap_or(json!("")),
                "unit": m.get("unit").cloned().unwrap_or(json!("")),
                "kind": kind,
                "monotonic": mono,
                "temporality": if temporality == &json!(CUMULATIVE) { "Cumulative" } else { "?" },
                "points": points,
            })
        })
        .collect();
    out.sort_by_key(|m| m["name"].as_str().unwrap().to_string());
    out
}

/// A turn with two loops and a tool call, and a failed turn, export the same
/// spans (names, parents, kinds, start and end times, attributes, events,
/// and status) and the same metrics (names, units, descriptions, attributes,
/// and values) as the OpenTelemetry SDK exporter did for the same turns: its
/// picture, dumped from the `otel` build at 964411f, is the golden file.
#[tokio::test]
async fn a_turn_exports_the_old_exporters_picture() {
    let old: Value = serde_json::from_str(include_str!("testdata/old-exporter.json")).unwrap();
    let rx = Receiver::start(vec![]).await;
    let tel = pipeline(&rx.endpoint(), None, tuning());
    tel.record_turn(&two_loops_result());
    let failed = failed_trace();
    tel.record_failure(&failed_turn(&failed));
    flushed(&tel).await;
    let got = rx.got();
    let spans = picture_of_spans(&spans_of(&got));
    let old_spans = old["spans"].as_array().unwrap();
    assert_eq!(spans.len(), old_spans.len(), "span count");
    for (new, old) in spans.iter().zip(old_spans) {
        assert_eq!(new, old, "\nnew {new:#}\nold {old:#}");
    }
    let metrics = picture_of_metrics(&last_metrics(&got));
    let old_metrics = old["metrics"].as_array().unwrap();
    assert_eq!(metrics.len(), old_metrics.len(), "metric count");
    for (new, old) in metrics.iter().zip(old_metrics) {
        assert_eq!(new, old, "\nnew {new:#}\nold {old:#}");
    }
    // The resource: the SDK's keys, and the two the old `from_config` added
    // (the dump came through `from_providers`, which added none).
    let res = &got[0].body["resourceSpans"][0]["resource"]["attributes"];
    let mut keys: Vec<&str> = res
        .as_array()
        .unwrap()
        .iter()
        .map(|kv| kv["key"].as_str().unwrap())
        .collect();
    let mut old_keys: Vec<&str> = old["resource_keys"]
        .as_array()
        .unwrap()
        .iter()
        .map(|k| k.as_str().unwrap())
        .chain(["service.version", "service.instance.id"])
        .collect();
    keys.sort();
    old_keys.sort();
    assert_eq!(keys, old_keys);
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

// ---------------------------------------------------------------- failures

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

// ---------------------------------------------------------------- the corrections (theseus-yf1)

/// Every data point of the metric `name`.
fn points_of<'a>(metrics: &'a [Value], name: &str) -> Vec<&'a Value> {
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
fn attrs_of(p: &Value) -> BTreeMap<String, String> {
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
fn point_with<'a>(metrics: &'a [Value], name: &str, with: &[(&str, &str)]) -> &'a Value {
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
                vec![],
            )],
        );
        let mut r = result_with(trace);
        r.elapsed_ms = turn_ms;
        r.first_token_ms = Some(first_ms);
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
fn tool_span(wire: &str, start: u64, end: u64, tool: [&str; 4]) -> Span {
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

/// Every tool call is counted and timed by its tool's name, family, and
/// backend and the call's outcome (one kind of each), with the turn's
/// attributes, in a failed turn too; so the shell-fallback ratio, `proc.run`
/// over every call, is one query (§3.23).
#[tokio::test]
#[expect(clippy::too_many_lines, reason = "shape budget: split it")]
async fn tool_calls_are_counted_and_timed_by_name_family_backend_and_outcome() {
    let rx = Receiver::start(vec![]).await;
    let tel = pipeline(&rx.endpoint(), None, tuning());
    let ms = 1000;
    let trace = s(
        "turn",
        "turn",
        0,
        200_000 * ms,
        json!({"origin_unix_ms": 1_790_000_000_000u64}),
        vec![
            s(
                "loop 0",
                "loop",
                0,
                1_000 * ms,
                json!({"loop": 0}),
                vec![
                    s(
                        "tools",
                        "tools",
                        100,
                        40_100,
                        json!({"calls": 2, "together": true}),
                        vec![
                            tool_span("fs_read", 100, 40_100, ["fs.read", "fs", "inproc", "ok"]),
                            tool_span("fs_read", 200, 2_200, ["fs.read", "fs", "inproc", "error"]),
                        ],
                    ),
                    tool_span(
                        "frob_it",
                        50_000,
                        50_300,
                        ["frob_it", "unknown", "none", "error"],
                    ),
                ],
            ),
            s(
                "loop 1",
                "loop",
                1_000 * ms,
                200_000 * ms,
                json!({"loop": 1}),
                vec![
                    tool_span(
                        "proc_run",
                        1_000 * ms,
                        13_000 * ms,
                        ["proc.run", "proc", "job", "ok"],
                    ),
                    tool_span(
                        "proc_run",
                        13_000 * ms,
                        73_000 * ms,
                        ["proc.run", "proc", "job", "background"],
                    ),
                    tool_span(
                        "proc_run",
                        73_000 * ms,
                        73_001 * ms,
                        ["proc.run", "proc", "job", "awaiting_confirm"],
                    ),
                    tool_span(
                        "proc_run",
                        74_000 * ms,
                        74_002 * ms,
                        ["proc.run", "proc", "job", "cancelled"],
                    ),
                    tool_span(
                        "proc_run",
                        75_000 * ms,
                        75_003 * ms,
                        ["proc.run", "proc", "job", "unknown"],
                    ),
                    tool_span(
                        "http_fetch",
                        76_000 * ms,
                        76_500 * ms,
                        ["http.fetch", "http", "async", "declined"],
                    ),
                ],
            ),
        ],
    );
    tel.record_turn(&result_with(trace));
    // A failed turn's calls are counted too, under its outcome.
    let failed = s(
        "turn",
        "turn",
        0,
        3_000 * ms,
        json!({"origin_unix_ms": 1_790_000_100_000u64, "outcome": "failed", "class": "overloaded"}),
        vec![s(
            "loop 0",
            "loop",
            0,
            3_000 * ms,
            json!({"loop": 0}),
            vec![
                tool_span(
                    "proc_run",
                    10,
                    2_000_010,
                    ["proc.run", "proc", "job", "error"],
                ),
                s(
                    "provider.call",
                    "provider",
                    2_000_100,
                    3_000 * ms,
                    json!({"provider": "zai", "model": "glm-5.1", "error": "overloaded"}),
                    vec![],
                ),
            ],
        )],
    );
    tel.record_failure(&FailedTurn {
        class: "overloaded",
        elapsed_ms: 3000,
        ..failed_turn(&failed)
    });
    flushed(&tel).await;
    let metrics = last_metrics(&rx.got());
    let want = [
        (["fs.read", "fs", "inproc", "ok"], "complete", 40.0),
        (["fs.read", "fs", "inproc", "error"], "complete", 2.0),
        (["frob_it", "unknown", "none", "error"], "complete", 0.3),
        (["proc.run", "proc", "job", "ok"], "complete", 12_000.0),
        (
            ["proc.run", "proc", "job", "background"],
            "complete",
            60_000.0,
        ),
        (
            ["proc.run", "proc", "job", "awaiting_confirm"],
            "complete",
            1.0,
        ),
        (["proc.run", "proc", "job", "cancelled"], "complete", 2.0),
        (["proc.run", "proc", "job", "unknown"], "complete", 3.0),
        (
            ["http.fetch", "http", "async", "declined"],
            "complete",
            500.0,
        ),
        (["proc.run", "proc", "job", "error"], "failed", 2_000.0),
    ];
    let calls = points_of(&metrics, "theseus.tool.calls");
    assert_eq!(calls.len(), want.len(), "{calls:#?}");
    assert_eq!(
        points_of(&metrics, "theseus.tool.duration_ms").len(),
        want.len()
    );
    for ([name, family, backend, outcome], turn, took) in want {
        let with = [
            ("theseus.tool.name", name),
            ("theseus.tool.family", family),
            ("theseus.tool.backend", backend),
            ("theseus.tool.outcome", outcome),
            ("theseus.outcome", turn),
        ];
        let c = point_with(&metrics, "theseus.tool.calls", &with);
        assert_eq!(c["asInt"], "1", "{with:?}");
        let a = attrs_of(c);
        assert_eq!(a.len(), 8, "the turn's four, and the tool's four: {a:?}");
        let model = if turn == "failed" {
            "glm-5.1"
        } else {
            "claude-x"
        };
        assert_eq!(a["gen_ai.request.model"], model);
        let d = point_with(&metrics, "theseus.tool.duration_ms", &with);
        assert_eq!(
            (&d["count"], &d["sum"]),
            (&json!("1"), &json!(took)),
            "{with:?}"
        );
        assert_eq!(attrs_of(d), a, "one attribute set for both");
    }
    // The shell-fallback ratio: the calls named proc.run over all of them.
    let count = |p: &&Value| u64_of(&p["asInt"]);
    let all: u64 = calls.iter().map(count).sum();
    let shell: u64 = calls
        .iter()
        .filter(|p| attrs_of(p)["theseus.tool.name"] == "proc.run")
        .map(count)
        .sum();
    assert_eq!((shell, all), (6, 10));
}

/// What became of each call, as its span's `result` says it: the result
/// node's status, or that it waits for the operator or runs in the
/// background.
#[test]
fn a_calls_result_names_each_outcome() {
    use crate::node::ResultStatus;
    use crate::toolrun::CallOutcome;
    let done = |status| CallOutcome::Done { status };
    for (o, want) in [
        (done(ResultStatus::Ok), "ok"),
        (done(ResultStatus::Error), "error"),
        (done(ResultStatus::Declined), "declined"),
        (done(ResultStatus::Background), "background"),
        (done(ResultStatus::Unknown), "unknown"),
        (done(ResultStatus::Cancelled), "cancelled"),
        (
            CallOutcome::AwaitingConfirm {
                correlation_id: "cor_1".into(),
            },
            "awaiting_confirm",
        ),
        (
            CallOutcome::Background {
                correlation_id: "cor_2".into(),
            },
            "background",
        ),
    ] {
        assert_eq!(crate::turn::call_result(&o), want, "{o:?}");
    }
}

/// `gen_ai.response.model` is the model that answered, as the provider named
/// it, not the one asked for; a call that failed had no answer, so it has
/// none.
#[tokio::test]
async fn the_response_model_is_the_served_model() {
    use spans::semconv::{GEN_AI_REQUEST_MODEL, GEN_AI_RESPONSE_MODEL};
    let rx = Receiver::start(vec![]).await;
    let tel = pipeline(&rx.endpoint(), None, tuning());
    let trace = s(
        "turn",
        "turn",
        0,
        1_000_000,
        json!({"origin_unix_ms": 1_790_000_000_000u64}),
        vec![s(
            "loop 0",
            "loop",
            0,
            1_000_000,
            json!({"loop": 0}),
            vec![s(
                "provider.call",
                "provider",
                10,
                900_000,
                json!({"provider": "zai", "model": "glm-5.3", "served_model": "glm-5.3-0815",
                    "request_id": "req_1", "stop_reason": "end_turn"}),
                vec![],
            )],
        )],
    );
    tel.record_turn(&result_with(trace));
    let failed = failed_trace();
    tel.record_failure(&failed_turn(&failed));
    flushed(&tel).await;
    let spans = spans_of(&rx.got());
    let calls: Vec<&Value> = spans
        .iter()
        .filter(|s| s["name"] == "provider.call")
        .collect();
    assert_eq!(calls.len(), 2);
    let text = |v: &str| Some(json!({ "stringValue": v }));
    let answered = calls.iter().find(|s| s.get("status").is_none()).unwrap();
    assert_eq!(
        attr(answered, GEN_AI_REQUEST_MODEL).cloned(),
        text("glm-5.3")
    );
    assert_eq!(
        attr(answered, GEN_AI_RESPONSE_MODEL).cloned(),
        text("glm-5.3-0815")
    );
    let failed = calls.iter().find(|s| s.get("status").is_some()).unwrap();
    assert_eq!(attr(failed, GEN_AI_REQUEST_MODEL).cloned(), text("glm-5.1"));
    assert_eq!(attr(failed, GEN_AI_RESPONSE_MODEL), None);
}

/// A finished turn's metrics name the model asked for in
/// `gen_ai.request.model`, as its trace's root recorded it, and not the one
/// that answered, which the result names: so a turn on an alias lands in the
/// series its failure would, and agrees with its provider calls' time.
#[tokio::test]
async fn a_turns_metrics_name_the_model_asked_for() {
    let rx = Receiver::start(vec![]).await;
    let tel = pipeline(&rx.endpoint(), None, tuning());
    let trace = s(
        "turn",
        "turn",
        0,
        1_000_000,
        json!({"origin_unix_ms": 1_790_000_000_000u64, "provider": "anthropic",
            "model": "claude-haiku-4-5"}),
        vec![s(
            "provider.call",
            "provider",
            10,
            900_000,
            json!({"provider": "anthropic", "model": "claude-haiku-4-5",
                "served_model": "claude-haiku-4-5-20251001"}),
            vec![],
        )],
    );
    let mut r = result_with(trace);
    r.model = "claude-haiku-4-5-20251001".into();
    tel.record_turn(&r);
    flushed(&tel).await;
    let metrics = last_metrics(&rx.got());
    for name in [
        "theseus.turns",
        "theseus.tokens",
        "theseus.turn.duration_ms",
        "theseus.provider.first_token_ms",
        "theseus.provider.call.duration_ms",
    ] {
        let points = points_of(&metrics, name);
        assert!(!points.is_empty(), "{name}");
        for p in points {
            assert_eq!(
                attrs_of(p)["gen_ai.request.model"],
                "claude-haiku-4-5",
                "{name}"
            );
        }
    }
}

/// Each provider call's time carries the provider and model its span
/// recorded, one series for each pair. It had no attributes, so every
/// model's calls were one series.
#[tokio::test]
async fn a_provider_calls_time_carries_its_provider_and_model() {
    let rx = Receiver::start(vec![]).await;
    let tel = pipeline(&rx.endpoint(), None, tuning());
    tel.record_turn(&two_loops_result());
    tel.record_turn(&result_with(sample_trace()));
    flushed(&tel).await;
    let metrics = last_metrics(&rx.got());
    let name = "theseus.provider.call.duration_ms";
    assert_eq!(points_of(&metrics, name).len(), 2);
    let zai = point_with(&metrics, name, &[("gen_ai.provider.name", "zai")]);
    assert_eq!(
        attrs_of(zai),
        BTreeMap::from([
            ("gen_ai.provider.name".to_string(), "zai".to_string()),
            ("gen_ai.request.model".to_string(), "glm-5.1".to_string()),
        ])
    );
    assert_eq!(
        (&zai["count"], &zai["min"], &zai["max"]),
        (&json!("2"), &json!(890.0), &json!(891.9))
    );
    let anthropic = point_with(&metrics, name, &[("gen_ai.provider.name", "anthropic")]);
    assert_eq!(attrs_of(anthropic)["gen_ai.request.model"], "claude-x");
    assert_eq!(anthropic["count"], "1");
}

// ---------------------------------------------------------------- the corrections, through the whole core

/// A core whose provider follows `script`, whose tools work in a scratch
/// folder holding `harbor.txt`, and whose telemetry posts to `endpoint`.
/// What the template leaves to enforcement (the writers and `proc.run`)
/// waits for the operator.
fn core_with(script: Vec<crate::provider::Scripted>, endpoint: &str) -> CoreRig {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("work");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(root.join("harbor.txt"), "the tide turns at four\n").unwrap();
    let root = root.canonicalize().unwrap();
    let mut c = crate::Config::example();
    c.server.state_dir = dir.path().to_string_lossy().into_owned();
    c.tools.projects_dir = Some(root.to_string_lossy().into_owned());
    c.tools.roots = vec![];
    c.policy.enforcement = crate::policy::Posture::Approve;
    let store = crate::store::Store::open(&dir.path().join("store")).unwrap();
    let fake = Arc::new(crate::provider::FakeProvider::scripted(script));
    let core = crate::Core::build(crate::rpc::Parts {
        telemetry: Some(pipeline(endpoint, None, tuning())),
        ..crate::rpc::Parts::for_tests(c, fake, store)
    })
    .unwrap();
    CoreRig { core, _dir: dir }
}

struct CoreRig {
    core: Arc<crate::Core>,
    _dir: tempfile::TempDir,
}

/// `turn.submit` through the protocol, as a client sends it: its answer.
async fn submit(core: &Arc<crate::Core>, input: &str) -> theseus_protocol::Response {
    use theseus_protocol::{method, Id, Message, Request, TurnSubmitParams};
    use tokio::io::{AsyncBufReadExt, BufReader};
    let (client, server) = tokio::io::duplex(64 * 1024);
    let (sr, sw) = tokio::io::split(server);
    let srv = tokio::spawn(core.clone().serve_connection(sr, sw, "test".into()));
    let (cr, mut cw) = tokio::io::split(client);
    let params = TurnSubmitParams {
        session_id: None,
        input: input.into(),
        profile: None,
        provider: None,
        model: None,
        author: None,
        attachments: vec![],
        reply_to: None,
    };
    let mut line =
        serde_json::to_string(&Request::new(Id::Num(1), method::TURN_SUBMIT, params)).unwrap();
    line.push('\n');
    cw.write_all(line.as_bytes()).await.unwrap();
    let mut lines = BufReader::new(cr).lines();
    let answer = loop {
        let line = lines.next_line().await.unwrap().unwrap();
        if let Message::Response(r) = serde_json::from_str(&line).unwrap() {
            break r;
        }
    };
    cw.shutdown().await.unwrap();
    drop((cw, lines));
    let _ = srv.await;
    answer
}

/// A real turn's calls, through the gate and the tool runtime, reach the
/// tool metrics, and their spans, with the registered tool's name, family,
/// and backend and what became of each: a read that answered, a read that
/// failed, a tool that is not registered, and a program that waits for the
/// operator.
#[tokio::test]
async fn a_turns_calls_reach_the_tool_metrics_as_the_runtime_ran_them() {
    use crate::provider::Scripted;
    let rx = Receiver::start(vec![]).await;
    let r = core_with(
        vec![
            Scripted::tools(
                "Looking.",
                &[
                    ("t1", "fs_read", json!({"path": "harbor.txt"})),
                    ("t2", "fs_read", json!({"path": "missing.txt"})),
                    ("t3", "frob_it", json!({})),
                ],
            ),
            Scripted::tools("", &[("t4", "proc_run", json!({"argv": ["echo", "tide"]}))]),
        ],
        &rx.endpoint(),
    );
    let answer = submit(&r.core, "what does harbor.txt say?").await;
    assert!(answer.error.is_none(), "{answer:?}");
    flushed(r.core.telemetry()).await;
    let got = rx.got();
    let metrics = last_metrics(&got);
    let want = [
        ["fs.read", "fs", "inproc", "ok"],
        ["fs.read", "fs", "inproc", "error"],
        ["frob_it", "unknown", "none", "error"],
        ["proc.run", "proc", "job", "awaiting_confirm"],
    ];
    assert_eq!(
        points_of(&metrics, "theseus.tool.calls").len(),
        want.len(),
        "{metrics:#?}"
    );
    for [name, family, backend, outcome] in want {
        let with = [
            ("theseus.tool.name", name),
            ("theseus.tool.family", family),
            ("theseus.tool.backend", backend),
            ("theseus.tool.outcome", outcome),
            ("theseus.outcome", "complete"),
        ];
        assert_eq!(
            point_with(&metrics, "theseus.tool.calls", &with)["asInt"],
            "1"
        );
        assert_eq!(
            point_with(&metrics, "theseus.tool.duration_ms", &with)["count"],
            "1"
        );
    }
    let spans = spans_of(&got);
    let run = named(&spans, "tool proc_run");
    for (k, v) in [
        ("tool", "proc.run"),
        ("family", "proc"),
        ("backend", "job"),
        ("result", "awaiting_confirm"),
    ] {
        assert_eq!(attr(run, k), Some(&json!({ "stringValue": v })), "{k}");
    }
}

/// A continuation turn that fails (here the driver's retry of a 529) is
/// counted as the client's own failed turn is: in `theseus.turns` as failed
/// and in `theseus.provider.errors`, in the same series, and in health's
/// count. Until theseus-yf1 only `turn.submit` counted a failure. The retry
/// that answers is counted complete.
#[tokio::test]
async fn a_failed_continuation_is_counted_as_a_failed_turn_is() {
    use crate::provider::{ProviderError, Scripted};
    let overloaded = || {
        Scripted::Fail(ProviderError::Overloaded {
            message: "overloaded_error: Overloaded".into(),
        })
    };
    let rx = Receiver::start(vec![]).await;
    let r = core_with(
        vec![
            overloaded(),
            overloaded(),
            Scripted::text("The tide turns at four."),
        ],
        &rx.endpoint(),
    );
    let answer = submit(&r.core, "when does the tide turn?").await;
    let data = answer.error.expect("the provider is overloaded").data;
    assert_eq!(data["class"], "overloaded");
    let session: crate::session::SessionRecord = r
        .core
        .store
        .get_session(data["session_id"].as_str().unwrap())
        .unwrap()
        .unwrap();
    let exec = session.execution_id.clone().unwrap();
    let e = r.core.kernel.execution(&exec).unwrap().unwrap();
    assert!(e.resume_pending, "the retry waits for the driver: {e:?}");
    assert_eq!(r.core.health().provider_errors, 1);

    let failed = r
        .core
        .continue_execution(&exec)
        .await
        .expect_err("the retry fails the same way");
    assert_eq!(
        failed
            .downcast_ref::<crate::turn::TurnError>()
            .unwrap()
            .class,
        "overloaded"
    );
    assert_eq!(r.core.health().provider_errors, 2, "health counts it");
    let answered = r.core.continue_execution(&exec).await.unwrap().unwrap();
    assert_eq!(answered.output, "The tide turns at four.");

    flushed(r.core.telemetry()).await;
    let got = rx.got();
    let metrics = last_metrics(&got);
    let failed = [("theseus.outcome", "failed")];
    assert_eq!(
        point_with(&metrics, "theseus.turns", &failed)["asInt"],
        "2",
        "the client's failed turn and the driver's, in one series"
    );
    assert_eq!(
        point_with(&metrics, "theseus.turn.duration_ms", &failed)["count"],
        "2"
    );
    let errors = points_of(&metrics, "theseus.provider.errors");
    assert_eq!(errors.len(), 1, "{errors:#?}");
    assert_eq!(errors[0]["asInt"], "2");
    let a = attrs_of(errors[0]);
    assert_eq!(
        (
            a["theseus.error.class"].as_str(),
            a["theseus.error.transient"].as_str()
        ),
        ("overloaded", "true")
    );
    let complete = [("theseus.outcome", "complete")];
    assert_eq!(
        point_with(&metrics, "theseus.turns", &complete)["asInt"],
        "1"
    );
    assert_eq!(rx.at("/v1/traces").len(), 3, "each turn's trace");
}
