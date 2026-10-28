//! The exporter's wire format (theseus-hee): what it posts is OTLP JSON, read
//! back through OTLP's own types, and it draws the picture the old SDK
//! exporter drew.

use serde_json::{json, Value};

use super::tests::{
    failed_trace, failed_turn, flushed, last_metrics, pipeline, spans_of, tuning, two_loops_result,
    u64_of, Receiver, CUMULATIVE,
};

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

/// The one attribute the old exporter never wrote: a failed provider call's
/// `error.type`, which the GenAI conventions add (theseus-lmhp). The dump
/// stays as it was; its picture is compared without it.
fn without_error_type(attrs: Value) -> Value {
    let mut a = attrs;
    if let Some(list) = a.as_array_mut() {
        list.retain(|kv| kv[0] != "error.type");
    }
    a
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
                "attributes": without_error_type(typed_attrs(&s["attributes"])),
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
