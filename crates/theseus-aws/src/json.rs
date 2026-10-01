//! JSON bodies: the JSON protocols' requests and REST-JSON's, and every JSON
//! answer read back through its shape.

use serde_json::{Map, Value};
use theseus_aws_catalog::{Kind, Location, MemberRef, ShapeRef, TimestampFormat};

use crate::scalar;
use crate::value::{self, V};

/// A structure's members as a JSON object, keyed by their wire names. With
/// `body_only`, members bound to the URI, the query, or headers stay out.
pub(crate) fn object(members: &[(MemberRef<'_>, V<'_>)], body_only: bool) -> Value {
    let mut o = Map::new();
    for (m, v) in members {
        if body_only && m.location() != Location::Body {
            continue;
        }
        o.insert(value::wire_name(*m).to_owned(), to_json(m.shape(), v));
    }
    Value::Object(o)
}

/// One value, as the JSON protocols send it.
pub(crate) fn to_json(shape: ShapeRef<'_>, v: &V<'_>) -> Value {
    match v {
        V::Struct(ms) => object(ms, false),
        V::List(items) => match shape.list_member() {
            Some(lm) => Value::Array(items.iter().map(|i| to_json(lm.shape(), i)).collect()),
            None => Value::Array(Vec::new()),
        },
        V::Map(entries) => match shape.map_value() {
            Some(vm) => Value::Object(
                entries
                    .iter()
                    .map(|(k, i)| (k.clone(), to_json(vm.shape(), i)))
                    .collect(),
            ),
            None => Value::Object(Map::new()),
        },
        V::Str(s) => Value::String(s.clone()),
        V::Bool(b) => Value::Bool(*b),
        V::Int(i) => Value::from(*i),
        V::Float(f) => value::float_json(*f),
        V::Time(t) => match shape
            .timestamp_format()
            .unwrap_or(TimestampFormat::UnixTimestamp)
        {
            TimestampFormat::UnixTimestamp => {
                let s = scalar::epoch(*t);
                serde_json::from_str(&s).unwrap_or(Value::String(s))
            }
            other => Value::String(scalar::time(*t, other)),
        },
        V::Blob(b) => Value::String(value::b64(b)),
        V::Doc(d) => d.clone(),
    }
}

/// A JSON answer read through its shape: wire names become member names,
/// timestamps become RFC 3339, and blobs follow the client's convention.
/// With `body_only`, members bound to headers or the status are skipped
/// (REST-JSON reads those from the message).
pub(crate) fn from_json(shape: ShapeRef<'_>, v: &Value, body_only: bool) -> Value {
    if shape.is_document() {
        return v.clone();
    }
    match (shape.kind(), v) {
        (_, Value::Null) => Value::Null,
        (Kind::Structure, Value::Object(o)) => {
            let mut out = Map::new();
            for m in shape.members() {
                if body_only && m.location() != Location::Body {
                    continue;
                }
                if let Some(x) = o.get(value::wire_name(m)) {
                    if !x.is_null() {
                        out.insert(m.name().to_owned(), from_json(m.shape(), x, false));
                    }
                }
            }
            Value::Object(out)
        }
        (Kind::List, Value::Array(items)) => match shape.list_member() {
            Some(lm) => Value::Array(
                items
                    .iter()
                    .map(|i| from_json(lm.shape(), i, false))
                    .collect(),
            ),
            None => v.clone(),
        },
        (Kind::Map, Value::Object(o)) => match shape.map_value() {
            Some(vm) => Value::Object(
                o.iter()
                    .map(|(k, i)| (k.clone(), from_json(vm.shape(), i, false)))
                    .collect(),
            ),
            None => v.clone(),
        },
        (Kind::Timestamp, Value::Number(n)) => n
            .as_f64()
            .map_or(Value::Null, |f| value::time_out(value::from_epoch(f))),
        (Kind::Timestamp, Value::String(s)) => value::parse_time(s)
            .map(value::time_out)
            .unwrap_or_else(|_| v.clone()),
        (Kind::Blob, Value::String(s)) => match value::unb64(s) {
            Ok(bytes) => value::blob_out(&bytes),
            Err(_) => v.clone(),
        },
        _ => v.clone(),
    }
}
