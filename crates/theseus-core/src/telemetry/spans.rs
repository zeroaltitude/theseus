//! A finished turn's trace as OTLP spans: the picture the OpenTelemetry SDK
//! drew before theseus-hee, span for span. Each span of the trace becomes one
//! with its recorded start and end, its attributes flattened, and its parent
//! link; marks, compile, store, lock, and advancer become events on their
//! parent span, so a turn is a handful of spans; a provider call is a client
//! span with the GenAI attributes; a failed span has error status.

use serde_json::Value;
use theseus_protocol::Span;

use super::otlp::{self, AnyValue, KeyValue};

/// GenAI semantic-convention attribute names, pinned: the strings are the
/// spec's (§3.20).
pub(super) mod semconv {
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

/// Kinds that become events on their parent span, so a turn is a handful of spans.
const EVENT_KINDS: &[&str] = &["mark", "compile", "store", "lock", "advancer"];

/// The SDK's default limits, kept: attributes per span and per event, and
/// events per span. What is over is counted as dropped.
const MAX_ATTRIBUTES: usize = 128;
const MAX_EVENTS: usize = 128;

/// A trace id: 16 random bytes (a v4 UUID: 122 of them random, and never zero).
fn trace_id() -> String {
    hex::encode(uuid::Uuid::new_v4().as_bytes())
}

/// A span id: the first 8 bytes of a v4 UUID, whose version nibble keeps them
/// from ever being zero.
fn span_id() -> String {
    hex::encode(&uuid::Uuid::new_v4().as_bytes()[..8])
}

/// The spans of one turn's trace, in one fresh trace: each parent before its
/// children.
pub(super) fn spans(root: &Span) -> Vec<otlp::Span> {
    let walk = Walk {
        trace_id: trace_id(),
        origin_ns: origin_ms(root) * 1_000_000,
    };
    let mut out = Vec::new();
    walk.span_or_event(root, None, &mut out);
    out
}

/// The turn's absolute start, from the trace root's `origin_unix_ms`.
fn origin_ms(root: &Span) -> u64 {
    root.attrs
        .get("origin_unix_ms")
        .and_then(Value::as_u64)
        .unwrap_or_else(theseus_protocol::now_unix_ms)
}

struct Walk {
    trace_id: String,
    origin_ns: u64,
}

impl Walk {
    fn at(&self, us: u64) -> u64 {
        self.origin_ns + us * 1_000
    }

    /// A span, pushed onto `out` with its children after it; or, for an
    /// event kind, the event it makes on its parent (none without one).
    fn span_or_event(
        &self,
        node: &Span,
        parent: Option<&str>,
        out: &mut Vec<otlp::Span>,
    ) -> Option<otlp::Event> {
        let start = self.at(node.start_us);
        let end = self.at(node.end_us.unwrap_or(node.start_us));
        if EVENT_KINDS.contains(&node.kind.as_str()) {
            parent?;
            let mut attrs = flatten(&node.attrs, "");
            attrs.push(KeyValue::string("theseus.kind", node.kind.clone()));
            if end > start {
                attrs.push(KeyValue::new(
                    "duration_us",
                    AnyValue::int(node.duration_us() as i64),
                ));
            }
            let (attributes, dropped_attributes_count) = capped(attrs, MAX_ATTRIBUTES);
            return Some(otlp::Event {
                time_unix_nano: start,
                name: node.name.clone(),
                attributes,
                dropped_attributes_count,
            });
        }

        let mut attrs = flatten(&node.attrs, "");
        attrs.push(KeyValue::string("theseus.kind", node.kind.clone()));
        let mut kind = otlp::KIND_INTERNAL;
        if node.kind == "provider" {
            kind = otlp::KIND_CLIENT;
            attrs.extend(gen_ai(&node.attrs));
        }
        // An AWS request (row 29, C1) is a client span, its attributes in
        // OpenTelemetry's AWS names (`rpc.system`, `aws.request_id`, …).
        if node.kind == "aws" {
            kind = otlp::KIND_CLIENT;
        }
        let (attributes, dropped_attributes_count) = capped(attrs, MAX_ATTRIBUTES);
        let id = span_id();
        let at = out.len();
        out.push(otlp::Span {
            trace_id: self.trace_id.clone(),
            span_id: id.clone(),
            parent_span_id: parent.unwrap_or_default().to_string(),
            flags: otlp::SPAN_FLAGS,
            name: node.name.clone(),
            kind,
            start_time_unix_nano: start,
            end_time_unix_nano: end,
            attributes,
            dropped_attributes_count,
            events: Vec::new(),
            dropped_events_count: 0,
            status: failed(&node.attrs),
        });
        let events: Vec<otlp::Event> = node
            .children
            .iter()
            .filter_map(|child| self.span_or_event(child, Some(&id), out))
            .collect();
        let (events, dropped) = capped(events, MAX_EVENTS);
        out[at].events = events;
        out[at].dropped_events_count = dropped;
        None
    }
}

/// The first `max`, and how many were dropped.
fn capped<T>(mut v: Vec<T>, max: usize) -> (Vec<T>, u32) {
    let dropped = v.len().saturating_sub(max) as u32;
    v.truncate(max);
    (v, dropped)
}

/// A provider call's GenAI attributes, from what the trace recorded.
fn gen_ai(attrs: &Value) -> Vec<KeyValue> {
    use semconv::*;
    let text = |k: &str| attrs.get(k).and_then(Value::as_str);
    let mut out = vec![KeyValue::string(GEN_AI_OPERATION_NAME, "chat")];
    if let Some(p) = text("provider") {
        out.push(KeyValue::string(GEN_AI_SYSTEM, p));
        out.push(KeyValue::string(GEN_AI_PROVIDER_NAME, p));
    }
    if let Some(m) = text("model") {
        out.push(KeyValue::string(GEN_AI_REQUEST_MODEL, m));
    }
    // The model that answered, as the provider named it, which a fallback
    // makes differ from the one asked for; a call that failed had no answer,
    // so it has none (theseus-yf1: it was the requested model).
    if let Some(m) = text("served_model").filter(|m| !m.is_empty()) {
        out.push(KeyValue::string(GEN_AI_RESPONSE_MODEL, m));
    }
    if let Some(id) = text("request_id") {
        out.push(KeyValue::string(GEN_AI_RESPONSE_ID, id));
    }
    if let Some(sr) = text("stop_reason") {
        out.push(KeyValue::new(
            GEN_AI_RESPONSE_FINISH_REASONS,
            AnyValue::Array(otlp::ArrayValue {
                values: vec![AnyValue::String(sr.to_string())],
            }),
        ));
    }
    if let Some(u) = attrs.get("usage") {
        if let Some(n) = u.get("input_tokens").and_then(Value::as_i64) {
            out.push(KeyValue::new(GEN_AI_USAGE_INPUT_TOKENS, AnyValue::int(n)));
        }
        if let Some(n) = u.get("output_tokens").and_then(Value::as_i64) {
            out.push(KeyValue::new(GEN_AI_USAGE_OUTPUT_TOKENS, AnyValue::int(n)));
        }
    }
    out
}

/// Error status, with a message, for a span that failed.
fn failed(attrs: &Value) -> Option<otlp::Status> {
    let failed = attrs.get("outcome").and_then(Value::as_str) == Some("failed")
        || attrs.get("error").is_some();
    failed.then(|| otlp::Status {
        message: attrs
            .get("message")
            .or_else(|| attrs.get("class"))
            .or_else(|| attrs.get("error"))
            .and_then(Value::as_str)
            .unwrap_or("failed")
            .to_string(),
        code: otlp::STATUS_ERROR,
    })
}

/// JSON attributes → OTLP key/values. Nested objects flatten with dots;
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
                    Value::String(s) => out.push(KeyValue::string(key, s.clone())),
                    Value::Bool(b) => out.push(KeyValue::new(key, AnyValue::Bool(*b))),
                    Value::Number(n) => {
                        if let Some(i) = n.as_i64() {
                            out.push(KeyValue::new(key, AnyValue::int(i)));
                        } else if let Some(f) = n.as_f64() {
                            out.push(KeyValue::new(key, AnyValue::Double(f)));
                        }
                    }
                    Value::Null => {}
                    other => out.push(KeyValue::string(key, other.to_string())),
                }
            }
        }
        Value::Null => {}
        other => out.push(KeyValue::string(
            if prefix.is_empty() { "value" } else { prefix },
            other.to_string(),
        )),
    }
    out
}

/// A span's text attribute `key`.
fn text(node: &Span, key: &str) -> Option<String> {
    node.attrs
        .get(key)
        .and_then(Value::as_str)
        .map(str::to_string)
}

/// A provider call, as its span recorded it.
pub(super) struct ProviderCall {
    pub provider: Option<String>,
    pub model: Option<String>,
    pub ms: f64,
}

/// Each provider call, as the export walk meets them: an event kind's
/// subtree holds none.
pub(super) fn provider_calls(node: &Span, out: &mut Vec<ProviderCall>) {
    if EVENT_KINDS.contains(&node.kind.as_str()) {
        return;
    }
    if node.kind == "provider" {
        out.push(ProviderCall {
            provider: text(node, "provider"),
            model: text(node, "model"),
            ms: node.duration_us() as f64 / 1000.0,
        });
    }
    for c in &node.children {
        provider_calls(c, out);
    }
}

/// A tool call, as its span recorded it.
pub(super) struct ToolCall {
    /// The canonical name (`fs.read`).
    pub name: String,
    pub family: String,
    pub backend: String,
    pub outcome: String,
    pub ms: f64,
}

/// One entry per `tool <wire name>` span, with the tool's name, family, and
/// backend and the call's result, as the turn recorded them (since
/// theseus-yf1). A span from before has only its wire name, which gives the
/// name (`fs_read` → `fs.read`) and the family; its backend and outcome are
/// `unknown`.
pub(super) fn tool_calls(node: &Span, out: &mut Vec<ToolCall>) {
    if node.kind == "tool" {
        if let Some(wire) = node.name.strip_prefix("tool ") {
            let name = text(node, "tool").unwrap_or_else(|| wire.replacen('_', ".", 1));
            let family = text(node, "family")
                .unwrap_or_else(|| name.split('.').next().unwrap_or_default().to_string());
            out.push(ToolCall {
                family,
                backend: text(node, "backend").unwrap_or_else(|| "unknown".into()),
                outcome: text(node, "result").unwrap_or_else(|| "unknown".into()),
                ms: node.duration_us() as f64 / 1000.0,
                name,
            });
        }
    }
    for c in &node.children {
        tool_calls(c, out);
    }
}
