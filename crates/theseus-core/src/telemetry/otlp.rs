//! The OTLP/HTTP JSON encoding of what Theseus exports: the protobuf messages
//! `ExportTraceServiceRequest` and `ExportMetricsServiceRequest`, under
//! protobuf's JSON mapping as OTLP amends it. Field names are lowerCamelCase,
//! trace and span ids are hex (not base64), enums are integers, and 64-bit
//! integers are decimal strings. A field left at its default is omitted,
//! which the mapping allows. The conformance tests read what is written here
//! back through `opentelemetry-proto`'s own types.

use serde::{Serialize, Serializer};

/// `SpanKind`: `SPAN_KIND_INTERNAL`, `SPAN_KIND_CLIENT`.
pub(super) const KIND_INTERNAL: i32 = 1;
pub(super) const KIND_CLIENT: i32 = 3;
/// `StatusCode`: `STATUS_CODE_ERROR`.
pub(super) const STATUS_ERROR: i32 = 2;
/// `AggregationTemporality`: `AGGREGATION_TEMPORALITY_CUMULATIVE`.
pub(super) const CUMULATIVE: i32 = 2;
/// The span's flags as the OpenTelemetry SDK set them: sampled (the W3C
/// trace flags' low byte), and "has is_remote" with is_remote clear.
pub(super) const SPAN_FLAGS: u32 = 0x01 | 0x100;

/// A 64-bit integer, as a decimal string.
fn decimal<S: Serializer>(n: &u64, s: S) -> Result<S::Ok, S::Error> {
    s.collect_str(n)
}

fn is_zero(n: &u32) -> bool {
    *n == 0
}

/// `{"stringValue": …}`, and so on.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub(super) enum AnyValue {
    #[serde(rename = "stringValue")]
    String(String),
    #[serde(rename = "boolValue")]
    Bool(bool),
    /// An int64, as a decimal string.
    #[serde(rename = "intValue")]
    Int(String),
    #[serde(rename = "doubleValue")]
    Double(f64),
    #[serde(rename = "arrayValue")]
    Array(ArrayValue),
}

impl AnyValue {
    pub(super) fn int(n: i64) -> Self {
        Self::Int(n.to_string())
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub(super) struct ArrayValue {
    pub values: Vec<AnyValue>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub(super) struct KeyValue {
    pub key: String,
    pub value: AnyValue,
}

impl KeyValue {
    pub(super) fn new(key: impl Into<String>, value: AnyValue) -> Self {
        Self {
            key: key.into(),
            value,
        }
    }

    pub(super) fn string(key: impl Into<String>, value: impl Into<String>) -> Self {
        Self::new(key, AnyValue::String(value.into()))
    }
}

#[derive(Debug, Clone, Serialize)]
pub(super) struct Resource {
    pub attributes: Vec<KeyValue>,
}

#[derive(Debug, Clone, Serialize)]
pub(super) struct Scope {
    pub name: String,
}

// ---------------------------------------------------------------- traces

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct ExportTraceServiceRequest {
    pub resource_spans: Vec<ResourceSpans>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct ResourceSpans {
    pub resource: Resource,
    pub scope_spans: Vec<ScopeSpans>,
}

#[derive(Debug, Serialize)]
pub(super) struct ScopeSpans {
    pub scope: Scope,
    pub spans: Vec<Span>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct Span {
    /// 16 bytes, hex.
    pub trace_id: String,
    /// 8 bytes, hex.
    pub span_id: String,
    /// Empty for the root.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub parent_span_id: String,
    pub flags: u32,
    pub name: String,
    pub kind: i32,
    #[serde(serialize_with = "decimal")]
    pub start_time_unix_nano: u64,
    #[serde(serialize_with = "decimal")]
    pub end_time_unix_nano: u64,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub attributes: Vec<KeyValue>,
    #[serde(skip_serializing_if = "is_zero")]
    pub dropped_attributes_count: u32,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub events: Vec<Event>,
    #[serde(skip_serializing_if = "is_zero")]
    pub dropped_events_count: u32,
    /// Absent: unset.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<Status>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct Event {
    #[serde(serialize_with = "decimal")]
    pub time_unix_nano: u64,
    pub name: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub attributes: Vec<KeyValue>,
    #[serde(skip_serializing_if = "is_zero")]
    pub dropped_attributes_count: u32,
}

#[derive(Debug, Clone, Serialize)]
pub(super) struct Status {
    #[serde(skip_serializing_if = "String::is_empty")]
    pub message: String,
    pub code: i32,
}

// ---------------------------------------------------------------- metrics

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct ExportMetricsServiceRequest {
    pub resource_metrics: Vec<ResourceMetrics>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct ResourceMetrics {
    pub resource: Resource,
    pub scope_metrics: Vec<ScopeMetrics>,
}

#[derive(Debug, Serialize)]
pub(super) struct ScopeMetrics {
    pub scope: Scope,
    pub metrics: Vec<Metric>,
}

#[derive(Debug, Serialize)]
pub(super) struct Metric {
    pub name: String,
    #[serde(skip_serializing_if = "str::is_empty")]
    pub description: &'static str,
    #[serde(skip_serializing_if = "str::is_empty")]
    pub unit: &'static str,
    /// `"sum": {…}` or `"histogram": {…}`.
    #[serde(flatten)]
    pub data: Data,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) enum Data {
    Sum(Sum),
    Histogram(Histogram),
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct Sum {
    pub data_points: Vec<NumberDataPoint>,
    pub aggregation_temporality: i32,
    pub is_monotonic: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct NumberDataPoint {
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub attributes: Vec<KeyValue>,
    #[serde(serialize_with = "decimal")]
    pub start_time_unix_nano: u64,
    #[serde(serialize_with = "decimal")]
    pub time_unix_nano: u64,
    /// `"asInt": "3"` or `"asDouble": 0.5`.
    #[serde(flatten)]
    pub value: Number,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) enum Number {
    /// An sfixed64, as a decimal string.
    AsInt(String),
    AsDouble(f64),
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct Histogram {
    pub data_points: Vec<HistogramDataPoint>,
    pub aggregation_temporality: i32,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct HistogramDataPoint {
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub attributes: Vec<KeyValue>,
    #[serde(serialize_with = "decimal")]
    pub start_time_unix_nano: u64,
    #[serde(serialize_with = "decimal")]
    pub time_unix_nano: u64,
    #[serde(serialize_with = "decimal")]
    pub count: u64,
    pub sum: f64,
    /// fixed64s, as decimal strings: one more than the bounds.
    pub bucket_counts: Vec<String>,
    pub explicit_bounds: Vec<f64>,
    pub min: f64,
    pub max: f64,
}
