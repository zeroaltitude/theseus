//! OpenTelemetry as a projection of the record (spec §3.20).
//!
//! The turn trace is already a span tree with absolute timestamps. When a
//! turn ends we walk it and emit OTel spans with those exact start and end
//! times, so the exported picture is the ledger's picture and the hot path
//! pays nothing extra. Metrics are recorded at the same moment. Nothing leaves
//! the process until an OTLP endpoint is configured; the pipeline is always
//! present so enabling it is a config change.

use std::collections::HashMap;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::{Context as _, Result};
use opentelemetry::metrics::{Counter, Histogram, Meter, MeterProvider as _};
use opentelemetry::trace::{SpanKind, Status, TraceContextExt, Tracer, TracerProvider as _};
use opentelemetry::{Context, KeyValue};
use opentelemetry_otlp::{Protocol, WithExportConfig, WithHttpConfig};
use opentelemetry_sdk::metrics::SdkMeterProvider;
use opentelemetry_sdk::trace::{SdkTracer, SdkTracerProvider};
use opentelemetry_sdk::Resource;

/// GenAI semantic-convention attribute names. The upstream crate marks these
/// deprecated because the GenAI conventions moved to their own repository;
/// the strings are the spec's, so we pin them here.
mod semconv {
    pub const GEN_AI_OPERATION_NAME: &str = "gen_ai.operation.name";
    pub const GEN_AI_SYSTEM: &str = "gen_ai.system";
    pub const GEN_AI_PROVIDER_NAME: &str = "gen_ai.provider.name";
    pub const GEN_AI_REQUEST_MODEL: &str = "gen_ai.request.model";
    pub const GEN_AI_RESPONSE_MODEL: &str = "gen_ai.response.model";
    pub const GEN_AI_RESPONSE_ID: &str = "gen_ai.response.id";
    pub const GEN_AI_RESPONSE_FINISH_REASONS: &str = "gen_ai.response.finish_reasons";
    pub const GEN_AI_USAGE_INPUT_TOKENS: &str = "gen_ai.usage.input_tokens";
    pub const GEN_AI_USAGE_OUTPUT_TOKENS: &str = "gen_ai.usage.output_tokens";
}
use serde::{Deserialize, Serialize};
use serde_json::Value;
use theseus_protocol::{Span, TurnSubmitResult, Usage};

use crate::secrets::Secret;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TelemetryConfig {
    /// OTLP/HTTP base URL of a collector, e.g. `http://127.0.0.1:4318`.
    /// `/v1/traces` and `/v1/metrics` are appended. Unset: nothing is sent.
    #[serde(default)]
    pub otlp_endpoint: Option<String>,
    /// Name of a `[secrets]` entry whose value is `Header: value` lines (or a
    /// single `key=value,key2=value2` string) to send with every export, e.g.
    /// a Honeycomb `x-honeycomb-team` key.
    #[serde(default)]
    pub headers_secret: Option<String>,
    #[serde(default = "default_service_name")]
    pub service_name: String,
    /// Export hook sites, compile, store, lock, and advancer as spans rather
    /// than events on their parent. Off: a turn is a handful of spans.
    #[serde(default)]
    pub hook_spans: bool,
    /// Metrics export interval.
    #[serde(default = "default_metrics_interval")]
    pub metrics_interval_secs: u64,
    #[serde(default = "default_export_timeout")]
    pub export_timeout_secs: u64,
}

fn default_service_name() -> String {
    "theseus".into()
}
fn default_metrics_interval() -> u64 {
    15
}
fn default_export_timeout() -> u64 {
    10
}

impl Default for TelemetryConfig {
    fn default() -> Self {
        Self {
            otlp_endpoint: None,
            headers_secret: None,
            service_name: default_service_name(),
            hook_spans: false,
            metrics_interval_secs: default_metrics_interval(),
            export_timeout_secs: default_export_timeout(),
        }
    }
}

/// Kinds that become span events unless `hook_spans` is set.
const EVENT_KINDS: &[&str] = &["hook", "mark", "compile", "store", "lock", "advancer"];

struct Instruments {
    turns: Counter<u64>,
    tokens: Counter<u64>,
    provider_errors: Counter<u64>,
    turn_duration_ms: Histogram<f64>,
    provider_call_ms: Histogram<f64>,
    first_token_ms: Histogram<f64>,
}

impl Instruments {
    fn new(meter: &Meter) -> Self {
        Self {
            turns: meter
                .u64_counter("theseus.turns")
                .with_description("Turns completed, by outcome")
                .build(),
            tokens: meter
                .u64_counter("theseus.tokens")
                .with_description("Tokens by direction (input, output, cache_read, cache_write)")
                .build(),
            provider_errors: meter
                .u64_counter("theseus.provider.errors")
                .with_description("Classified provider failures")
                .build(),
            turn_duration_ms: meter
                .f64_histogram("theseus.turn.duration_ms")
                .with_unit("ms")
                .build(),
            provider_call_ms: meter
                .f64_histogram("theseus.provider.call.duration_ms")
                .with_unit("ms")
                .build(),
            first_token_ms: meter
                .f64_histogram("theseus.provider.first_token_ms")
                .with_unit("ms")
                .build(),
        }
    }
}

struct Inner {
    tracer_provider: SdkTracerProvider,
    tracer: SdkTracer,
    meter_provider: SdkMeterProvider,
    instruments: Instruments,
    hook_spans: bool,
}

/// What a failed turn reports to telemetry.
pub struct FailedTurn<'a> {
    pub profile: &'a str,
    pub provider: &'a str,
    pub model: &'a str,
    pub class: &'a str,
    pub transient: bool,
    pub elapsed_ms: u64,
    pub trace: Option<&'a Span>,
}

/// The telemetry pipeline. Cheap to hold; `None` inside means disabled.
pub struct Telemetry {
    inner: Option<Inner>,
    pub endpoint: Option<String>,
}

impl Telemetry {
    pub fn disabled() -> Self {
        Self {
            inner: None,
            endpoint: None,
        }
    }

    pub fn enabled(&self) -> bool {
        self.inner.is_some()
    }

    /// Build from config. With no endpoint this is `disabled()`.
    pub fn from_config(cfg: &TelemetryConfig, headers: Option<&Secret>) -> Result<Self> {
        let Some(base) = cfg
            .otlp_endpoint
            .as_deref()
            .filter(|s| !s.trim().is_empty())
        else {
            return Ok(Self::disabled());
        };
        let base = base.trim_end_matches('/');
        let headers = headers.map(parse_headers).unwrap_or_default();
        let timeout = Duration::from_secs(cfg.export_timeout_secs);

        let span_exporter = opentelemetry_otlp::SpanExporter::builder()
            .with_http()
            .with_endpoint(format!("{base}/v1/traces"))
            .with_protocol(Protocol::HttpBinary)
            .with_timeout(timeout)
            .with_headers(headers.clone())
            .build()
            .context("building OTLP span exporter")?;
        let metric_exporter = opentelemetry_otlp::MetricExporter::builder()
            .with_http()
            .with_endpoint(format!("{base}/v1/metrics"))
            .with_protocol(Protocol::HttpBinary)
            .with_timeout(timeout)
            .with_headers(headers)
            .build()
            .context("building OTLP metric exporter")?;

        let resource = resource(&cfg.service_name);
        let tracer_provider = SdkTracerProvider::builder()
            .with_resource(resource.clone())
            .with_batch_exporter(span_exporter)
            .build();
        let reader = opentelemetry_sdk::metrics::PeriodicReader::builder(metric_exporter)
            .with_interval(Duration::from_secs(cfg.metrics_interval_secs.max(1)))
            .build();
        let meter_provider = SdkMeterProvider::builder()
            .with_resource(resource)
            .with_reader(reader)
            .build();
        Ok(Self::from_providers(
            tracer_provider,
            meter_provider,
            cfg.hook_spans,
            Some(base.to_string()),
        ))
    }

    /// Build around explicit providers (tests use in-memory exporters).
    pub fn from_providers(
        tracer_provider: SdkTracerProvider,
        meter_provider: SdkMeterProvider,
        hook_spans: bool,
        endpoint: Option<String>,
    ) -> Self {
        let tracer = tracer_provider.tracer("theseus");
        let meter = meter_provider.meter("theseus");
        let instruments = Instruments::new(&meter);
        Self {
            inner: Some(Inner {
                tracer_provider,
                tracer,
                meter_provider,
                instruments,
                hook_spans,
            }),
            endpoint,
        }
    }

    /// Export a finished turn: its trace as spans, its numbers as metrics.
    pub fn record_turn(&self, result: &TurnSubmitResult) {
        let Some(inner) = &self.inner else { return };
        let attrs = [
            KeyValue::new("theseus.profile", result.profile.clone()),
            KeyValue::new(semconv::GEN_AI_PROVIDER_NAME, result.provider.clone()),
            KeyValue::new(semconv::GEN_AI_REQUEST_MODEL, result.model.clone()),
            KeyValue::new("theseus.outcome", "complete"),
        ];
        inner.instruments.turns.add(1, &attrs);
        inner
            .instruments
            .turn_duration_ms
            .record(result.elapsed_ms as f64, &attrs);
        if let Some(ft) = result.first_token_ms {
            inner.instruments.first_token_ms.record(ft as f64, &attrs);
        }
        record_tokens(&inner.instruments.tokens, &result.usage, &attrs);
        if let Some(t) = &result.trace {
            export_tree(inner, t);
        }
    }

    /// Export a failed turn: the partial trace with error status, and the counters.
    pub fn record_failure(&self, f: &FailedTurn<'_>) {
        let Some(inner) = &self.inner else { return };
        let attrs = [
            KeyValue::new("theseus.profile", f.profile.to_string()),
            KeyValue::new(semconv::GEN_AI_PROVIDER_NAME, f.provider.to_string()),
            KeyValue::new(semconv::GEN_AI_REQUEST_MODEL, f.model.to_string()),
            KeyValue::new("theseus.outcome", "failed"),
        ];
        inner.instruments.turns.add(1, &attrs);
        inner
            .instruments
            .turn_duration_ms
            .record(f.elapsed_ms as f64, &attrs);
        inner.instruments.provider_errors.add(
            1,
            &[
                KeyValue::new(semconv::GEN_AI_PROVIDER_NAME, f.provider.to_string()),
                KeyValue::new(semconv::GEN_AI_REQUEST_MODEL, f.model.to_string()),
                KeyValue::new("theseus.error.class", f.class.to_string()),
                KeyValue::new("theseus.error.transient", f.transient),
            ],
        );
        if let Some(t) = f.trace {
            export_tree(inner, t);
        }
    }

    /// Flush everything (tests, and before shutdown).
    pub fn flush(&self) {
        if let Some(inner) = &self.inner {
            let _ = inner.tracer_provider.force_flush();
            let _ = inner.meter_provider.force_flush();
        }
    }

    pub fn shutdown(&self) {
        if let Some(inner) = &self.inner {
            let _ = inner.tracer_provider.shutdown();
            let _ = inner.meter_provider.shutdown();
        }
    }
}

fn resource(service_name: &str) -> Resource {
    Resource::builder()
        .with_service_name(service_name.to_string())
        .with_attributes([
            KeyValue::new("service.version", crate::VERSION),
            KeyValue::new("service.instance.id", crate::new_id("inst")),
        ])
        .build()
}

/// `Header: value` per line, or `k=v,k2=v2`. Values never logged.
fn parse_headers(secret: &Secret) -> HashMap<String, String> {
    let s = secret.expose();
    let mut out = HashMap::new();
    if s.contains('\n') || s.contains(": ") {
        for line in s.lines() {
            if let Some((k, v)) = line.split_once(':') {
                let (k, v) = (k.trim(), v.trim());
                if !k.is_empty() && !v.is_empty() {
                    out.insert(k.to_string(), v.to_string());
                }
            }
        }
    } else {
        for pair in s.split(',') {
            if let Some((k, v)) = pair.split_once('=') {
                let (k, v) = (k.trim(), v.trim());
                if !k.is_empty() && !v.is_empty() {
                    out.insert(k.to_string(), v.to_string());
                }
            }
        }
    }
    out
}

fn record_tokens(counter: &Counter<u64>, u: &Usage, base: &[KeyValue]) {
    let with = |dir: &str, n: u64| {
        if n > 0 {
            let mut a = base.to_vec();
            a.push(KeyValue::new("theseus.token.direction", dir.to_string()));
            counter.add(n, &a);
        }
    };
    with("input", u.input_tokens);
    with("output", u.output_tokens);
    with("cache_read", u.cache_read_input_tokens);
    with("cache_write", u.cache_creation_input_tokens);
}

/// The turn's absolute start, from the trace root's `origin_unix_ms`.
fn origin_of(root: &Span) -> SystemTime {
    let ms = root
        .attrs
        .get("origin_unix_ms")
        .and_then(Value::as_u64)
        .unwrap_or_else(theseus_protocol::now_unix_ms);
    UNIX_EPOCH + Duration::from_millis(ms)
}

fn export_tree(inner: &Inner, root: &Span) {
    let origin = origin_of(root);
    export_span(inner, root, origin, None);
}

fn at(origin: SystemTime, us: u64) -> SystemTime {
    origin + Duration::from_micros(us)
}

fn export_span(inner: &Inner, node: &Span, origin: SystemTime, parent: Option<&Context>) {
    let start = at(origin, node.start_us);
    let end = at(origin, node.end_us.unwrap_or(node.start_us));
    let is_event_kind = EVENT_KINDS.contains(&node.kind.as_str());

    if is_event_kind && !inner.hook_spans {
        if let Some(cx) = parent {
            let mut attrs = flatten(&node.attrs, "");
            attrs.push(KeyValue::new("theseus.kind", node.kind.clone()));
            if end > start {
                attrs.push(KeyValue::new("duration_us", node.duration_us() as i64));
            }
            cx.span()
                .add_event_with_timestamp(node.name.clone(), start, attrs);
        }
        return;
    }

    let mut attrs = flatten(&node.attrs, "");
    attrs.push(KeyValue::new("theseus.kind", node.kind.clone()));
    let mut kind = SpanKind::Internal;
    if node.kind == "provider" {
        kind = SpanKind::Client;
        attrs.push(KeyValue::new(semconv::GEN_AI_OPERATION_NAME, "chat"));
        if let Some(p) = node.attrs.get("provider").and_then(Value::as_str) {
            attrs.push(KeyValue::new(semconv::GEN_AI_SYSTEM, p.to_string()));
            attrs.push(KeyValue::new(semconv::GEN_AI_PROVIDER_NAME, p.to_string()));
        }
        if let Some(m) = node.attrs.get("model").and_then(Value::as_str) {
            attrs.push(KeyValue::new(semconv::GEN_AI_REQUEST_MODEL, m.to_string()));
            attrs.push(KeyValue::new(semconv::GEN_AI_RESPONSE_MODEL, m.to_string()));
        }
        if let Some(id) = node.attrs.get("request_id").and_then(Value::as_str) {
            attrs.push(KeyValue::new(semconv::GEN_AI_RESPONSE_ID, id.to_string()));
        }
        if let Some(sr) = node.attrs.get("stop_reason").and_then(Value::as_str) {
            attrs.push(KeyValue::new(
                semconv::GEN_AI_RESPONSE_FINISH_REASONS,
                opentelemetry::Value::Array(opentelemetry::Array::String(vec![sr
                    .to_string()
                    .into()])),
            ));
        }
        if let Some(u) = node.attrs.get("usage") {
            if let Some(n) = u.get("input_tokens").and_then(Value::as_i64) {
                attrs.push(KeyValue::new(semconv::GEN_AI_USAGE_INPUT_TOKENS, n));
            }
            if let Some(n) = u.get("output_tokens").and_then(Value::as_i64) {
                attrs.push(KeyValue::new(semconv::GEN_AI_USAGE_OUTPUT_TOKENS, n));
            }
        }
        inner
            .instruments
            .provider_call_ms
            .record(node.duration_us() as f64 / 1000.0, &[]);
    }

    let builder = inner
        .tracer
        .span_builder(node.name.clone())
        .with_kind(kind)
        .with_start_time(start)
        .with_attributes(attrs);
    let span = match parent {
        Some(cx) => inner.tracer.build_with_context(builder, cx),
        None => inner.tracer.build_with_context(builder, &Context::new()),
    };
    let cx = Context::current_with_span(span);
    for child in &node.children {
        export_span(inner, child, origin, Some(&cx));
    }
    let failed = node.attrs.get("outcome").and_then(Value::as_str) == Some("failed")
        || node.attrs.get("error").is_some();
    if failed {
        let msg = node
            .attrs
            .get("message")
            .or_else(|| node.attrs.get("class"))
            .or_else(|| node.attrs.get("error"))
            .and_then(Value::as_str)
            .unwrap_or("failed")
            .to_string();
        cx.span().set_status(Status::error(msg));
    }
    cx.span().end_with_timestamp(end);
}

/// JSON attributes → OTel key/values. Nested objects flatten with dots;
/// arrays and anything odd become their JSON text.
fn flatten(v: &Value, prefix: &str) -> Vec<KeyValue> {
    let mut out = Vec::new();
    match v {
        Value::Object(m) => {
            for (k, v) in m {
                let key = if prefix.is_empty() {
                    k.clone()
                } else {
                    format!("{prefix}.{k}")
                };
                match v {
                    Value::Object(_) => out.extend(flatten(v, &key)),
                    Value::String(s) => out.push(KeyValue::new(key, s.clone())),
                    Value::Bool(b) => out.push(KeyValue::new(key, *b)),
                    Value::Number(n) => {
                        if let Some(i) = n.as_i64() {
                            out.push(KeyValue::new(key, i));
                        } else if let Some(f) = n.as_f64() {
                            out.push(KeyValue::new(key, f));
                        }
                    }
                    Value::Null => {}
                    other => out.push(KeyValue::new(key, other.to_string())),
                }
            }
        }
        Value::Null => {}
        other => out.push(KeyValue::new(
            if prefix.is_empty() { "value" } else { prefix }.to_string(),
            other.to_string(),
        )),
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use opentelemetry_sdk::metrics::in_memory_exporter::InMemoryMetricExporter;
    use opentelemetry_sdk::trace::InMemorySpanExporter;
    use serde_json::json;

    fn test_telemetry(
        hook_spans: bool,
    ) -> (Telemetry, InMemorySpanExporter, InMemoryMetricExporter) {
        let spans = InMemorySpanExporter::default();
        let metrics = InMemoryMetricExporter::default();
        let tp = SdkTracerProvider::builder()
            .with_simple_exporter(spans.clone())
            .build();
        let reader = opentelemetry_sdk::metrics::PeriodicReader::builder(metrics.clone()).build();
        let mp = SdkMeterProvider::builder().with_reader(reader).build();
        (
            Telemetry::from_providers(tp, mp, hook_spans, None),
            spans,
            metrics,
        )
    }

    fn sample_trace() -> Span {
        let mut t = crate::trace::Trace::start(
            "turn",
            "turn",
            json!({"turn_id": "t1", "origin_unix_ms": 1_700_000_000_000u64}),
        );
        t.record("lock.wait", "lock", 0, 30, Value::Null);
        t.record("turn.starting", "hook", 40, 52, json!({"handlers": 0}));
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
        }
    }

    #[test]
    fn exports_turn_as_nested_spans_with_events() {
        let (tel, spans, metrics) = test_telemetry(false);
        tel.record_turn(&result_with(sample_trace()));
        tel.flush();
        let finished = spans.get_finished_spans().unwrap();
        let names: Vec<&str> = finished.iter().map(|s| s.name.as_ref()).collect();
        // hooks/marks/lock/advancer are events, so exactly three spans.
        assert_eq!(finished.len(), 3, "{names:?}");
        let turn = finished.iter().find(|s| s.name == "turn").unwrap();
        let lp = finished.iter().find(|s| s.name == "loop 0").unwrap();
        let pc = finished.iter().find(|s| s.name == "provider.call").unwrap();
        assert_eq!(lp.parent_span_id, turn.span_context.span_id());
        assert_eq!(pc.parent_span_id, lp.span_context.span_id());
        assert_eq!(pc.span_kind, SpanKind::Client);
        let origin = UNIX_EPOCH + Duration::from_millis(1_700_000_000_000);
        assert_eq!(turn.start_time, origin);
        assert!(pc.end_time > pc.start_time);
        let ev: Vec<&str> = pc.events.iter().map(|e| e.name.as_ref()).collect();
        assert!(
            ev.contains(&"first_byte") && ev.contains(&"first_token"),
            "{ev:?}"
        );
        let turn_ev: Vec<&str> = turn.events.iter().map(|e| e.name.as_ref()).collect();
        assert!(turn_ev.contains(&"lock.wait") && turn_ev.contains(&"turn.starting"));
        let has = |k: &str, want: &str| {
            pc.attributes
                .iter()
                .any(|kv| kv.key.as_str() == k && kv.value.as_str() == want)
        };
        assert!(has(semconv::GEN_AI_REQUEST_MODEL, "claude-x"));
        assert!(has(semconv::GEN_AI_RESPONSE_ID, "req_1"));
        assert!(pc
            .attributes
            .iter()
            .any(|kv| kv.key.as_str() == semconv::GEN_AI_USAGE_OUTPUT_TOKENS));
        let rm = metrics.get_finished_metrics().unwrap();
        let metric_names: Vec<String> = rm
            .iter()
            .flat_map(|r| r.scope_metrics())
            .flat_map(|s| s.metrics())
            .map(|m| m.name().to_string())
            .collect();
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

    #[test]
    fn hook_spans_mode_exports_every_node_as_a_span() {
        let (tel, spans, _) = test_telemetry(true);
        tel.record_turn(&result_with(sample_trace()));
        tel.flush();
        let finished = spans.get_finished_spans().unwrap();
        // turn, lock.wait, turn.starting, loop 0, provider.call, first_byte, first_token, advancer
        assert_eq!(finished.len(), 8);
    }

    #[test]
    fn failure_sets_error_status_and_counts() {
        let (tel, spans, metrics) = test_telemetry(false);
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
        tel.flush();
        let finished = spans.get_finished_spans().unwrap();
        let turn = finished.iter().find(|s| s.name == "turn").unwrap();
        assert!(matches!(turn.status, Status::Error { .. }));
        let pc = finished.iter().find(|s| s.name == "provider.call").unwrap();
        assert!(matches!(pc.status, Status::Error { .. }));
        let rm = metrics.get_finished_metrics().unwrap();
        let names: Vec<String> = rm
            .iter()
            .flat_map(|r| r.scope_metrics())
            .flat_map(|s| s.metrics())
            .map(|m| m.name().to_string())
            .collect();
        assert!(names.contains(&"theseus.provider.errors".to_string()));
    }

    #[test]
    fn disabled_is_a_no_op() {
        let tel = Telemetry::disabled();
        assert!(!tel.enabled());
        tel.record_turn(&result_with(sample_trace()));
        tel.flush();
        tel.shutdown();
    }

    #[test]
    fn header_secret_formats() {
        let a = parse_headers(&Secret::new("x-honeycomb-team: abc\nx-other: 1\n".into()));
        assert_eq!(a.get("x-honeycomb-team").unwrap(), "abc");
        assert_eq!(a.len(), 2);
        let b = parse_headers(&Secret::new("api-key=zzz,team=t".into()));
        assert_eq!(b.get("api-key").unwrap(), "zzz");
        assert_eq!(b.len(), 2);
    }
}
