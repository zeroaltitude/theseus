//! A call's input, checked against its shape and typed, and the conventions
//! for values that JSON has no type for.
//!
//! The model writes input as JSON keyed by member names (`{"Bucket": "x"}`).
//! [`normalize`] walks it against the operation's input shape once: unknown
//! members, missing required members, and values of the wrong type are
//! invalid input, named by their path. What comes out is a typed tree, in the
//! shape's member order, that every protocol's serializer walks.
//!
//! The conventions (for the tool's description, AWS design §3.1):
//! - **Timestamps** in: an RFC 3339 string (`2026-10-01T12:00:00Z`, or with an
//!   offset), a date (`2026-10-01`), an HTTP date, or a number of epoch
//!   seconds. Out: an RFC 3339 string in UTC.
//! - **Blobs** in: a string is its UTF-8 bytes (botocore's rule, so a Lambda
//!   payload is plain JSON text); `{"base64": "…"}` is binary; any other
//!   object or array is sent as its JSON text. Out: a string when the bytes
//!   are printable UTF-8, else `{"base64": "…"}`.
//! - **Numbers** may also be given as numeric strings; floats take `"NaN"`,
//!   `"Infinity"`, and `"-Infinity"`. Booleans may be `"true"` or `"false"`.

use aws_smithy_types::date_time::Format;
use aws_smithy_types::DateTime;
use base64::Engine as _;
use serde_json::{Map, Value};
use theseus_aws_catalog::{Kind, Location, MemberRef, ShapeRef};

/// A value, typed by its shape.
#[derive(Clone, Debug)]
pub(crate) enum V<'a> {
    /// A structure's members that are set, in the shape's declaration order.
    Struct(Vec<(MemberRef<'a>, V<'a>)>),
    List(Vec<V<'a>>),
    /// Keys in the order given.
    Map(Vec<(String, V<'a>)>),
    Str(String),
    Bool(bool),
    Int(i64),
    Float(f64),
    Time(DateTime),
    Blob(Vec<u8>),
    /// A free-form document, sent as given.
    Doc(Value),
}

impl<'a> V<'a> {
    /// A structure's member, by name.
    pub(crate) fn member(&self, name: &str) -> Option<&V<'a>> {
        match self {
            V::Struct(ms) => ms.iter().find(|(m, _)| m.name() == name).map(|(_, v)| v),
            _ => None,
        }
    }

    pub(crate) fn as_str(&self) -> Option<&str> {
        match self {
            V::Str(s) => Some(s),
            _ => None,
        }
    }
}

/// Why an input is invalid, with the path to the value (`Filters[0].Values`).
pub(crate) fn invalid(path: &str, what: impl std::fmt::Display) -> String {
    if path.is_empty() {
        what.to_string()
    } else {
        format!("{path}: {what}")
    }
}

fn join(path: &str, name: &str) -> String {
    if path.is_empty() {
        name.to_owned()
    } else {
        format!("{path}.{name}")
    }
}

/// Checks `input` against an operation's input shape. `skip` names a
/// required member the client fills itself (the idempotency token).
pub(crate) fn normalize<'a>(
    shape: Option<ShapeRef<'a>>,
    input: &Value,
    skip: Option<&str>,
) -> Result<Option<V<'a>>, String> {
    let empty = matches!(input, Value::Null) || input.as_object().is_some_and(|o| o.is_empty());
    let Some(shape) = shape else {
        return if empty {
            Ok(None)
        } else {
            Err("this operation takes no input".into())
        };
    };
    let none = Map::new();
    let obj = match input {
        Value::Null => &none,
        Value::Object(o) => o,
        other => {
            return Err(format!(
                "the input must be an object, not {}",
                type_name(other)
            ))
        }
    };
    structure(shape, obj, "", skip).map(Some)
}

fn type_name(v: &Value) -> &'static str {
    match v {
        Value::Null => "null",
        Value::Bool(_) => "a boolean",
        Value::Number(_) => "a number",
        Value::String(_) => "a string",
        Value::Array(_) => "an array",
        Value::Object(_) => "an object",
    }
}

fn structure<'a>(
    shape: ShapeRef<'a>,
    obj: &Map<String, Value>,
    path: &str,
    skip: Option<&str>,
) -> Result<V<'a>, String> {
    for key in obj.keys() {
        if shape.member(key).is_none() {
            let names: Vec<&str> = shape.members().map(|m| m.name()).take(40).collect();
            let more = shape.member_count().saturating_sub(names.len());
            let tail = if more > 0 {
                format!(", and {more} more")
            } else {
                String::new()
            };
            return Err(invalid(
                path,
                format!(
                    "unknown member {key:?} of {}; its members are {}{tail}",
                    shape.name(),
                    names.join(", ")
                ),
            ));
        }
    }
    let mut out = Vec::new();
    for m in shape.members() {
        match obj.get(m.name()) {
            None | Some(Value::Null) => {
                if m.is_required() && Some(m.name()) != skip {
                    return Err(invalid(
                        path,
                        format!("missing required member {:?} of {}", m.name(), shape.name()),
                    ));
                }
            }
            Some(v) => {
                let p = join(path, m.name());
                let typed = if m.is_jsonvalue() {
                    // A JSON value: any JSON in a header (sent as base64),
                    // or JSON text in a body.
                    match (m.location(), v) {
                        (Location::Header, _) => V::Doc(v.clone()),
                        (_, Value::String(s)) => V::Str(s.clone()),
                        (_, other) => V::Str(other.to_string()),
                    }
                } else {
                    value(m.shape(), v, &p)?
                };
                out.push((m, typed));
            }
        }
    }
    if shape.is_union() && out.len() != 1 {
        return Err(invalid(
            path,
            format!(
                "{} is a union: set exactly one of its members, not {}",
                shape.name(),
                out.len()
            ),
        ));
    }
    Ok(V::Struct(out))
}

fn value<'a>(shape: ShapeRef<'a>, v: &Value, path: &str) -> Result<V<'a>, String> {
    if shape.is_document() {
        return Ok(V::Doc(v.clone()));
    }
    let wrong = |want: &str| invalid(path, format!("expected {want}, got {}", type_name(v)));
    Ok(match shape.kind() {
        Kind::Structure => match v {
            Value::Object(o) => structure(shape, o, path, None)?,
            _ => return Err(wrong("an object")),
        },
        Kind::List => {
            let Value::Array(items) = v else {
                return Err(wrong("an array"));
            };
            let member = shape
                .list_member()
                .ok_or_else(|| invalid(path, "a list without a member type"))?;
            let mut out = Vec::with_capacity(items.len());
            for (i, item) in items.iter().enumerate() {
                let p = format!("{path}[{i}]");
                if item.is_null() && !shape.is_sparse() {
                    return Err(invalid(&p, "null is not allowed in this list"));
                }
                out.push(value(member.shape(), item, &p)?);
            }
            V::List(out)
        }
        Kind::Map => {
            let Value::Object(o) = v else {
                return Err(wrong("an object"));
            };
            let val = shape
                .map_value()
                .ok_or_else(|| invalid(path, "a map without a value type"))?;
            let mut out = Vec::with_capacity(o.len());
            for (k, item) in o {
                let p = format!("{path}[{k:?}]");
                out.push((k.clone(), value(val.shape(), item, &p)?));
            }
            V::Map(out)
        }
        Kind::String => match v {
            Value::String(s) => V::Str(s.clone()),
            Value::Number(n) => V::Str(n.to_string()),
            Value::Bool(b) => V::Str(b.to_string()),
            _ => return Err(wrong("a string")),
        },
        Kind::Boolean => match v {
            Value::Bool(b) => V::Bool(*b),
            Value::String(s) if s == "true" || s == "false" => V::Bool(s == "true"),
            _ => return Err(wrong("a boolean")),
        },
        Kind::Integer | Kind::Long => match v {
            Value::Number(n) => match n.as_i64() {
                Some(i) => V::Int(i),
                None => match n.as_f64() {
                    Some(f) if f.fract() == 0.0 && f.abs() < 9.0e15 => V::Int(f as i64),
                    _ => return Err(wrong("an integer")),
                },
            },
            Value::String(s) => V::Int(s.trim().parse().map_err(|_| wrong("an integer"))?),
            _ => return Err(wrong("an integer")),
        },
        Kind::Float | Kind::Double => match v {
            Value::Number(n) => V::Float(n.as_f64().ok_or_else(|| wrong("a number"))?),
            Value::String(s) => V::Float(match s.trim() {
                "NaN" => f64::NAN,
                "Infinity" => f64::INFINITY,
                "-Infinity" => f64::NEG_INFINITY,
                t => t.parse().map_err(|_| wrong("a number"))?,
            }),
            _ => return Err(wrong("a number")),
        },
        Kind::Timestamp => V::Time(timestamp_in(v).map_err(|e| invalid(path, e))?),
        Kind::Blob => V::Blob(blob_in(v).map_err(|e| invalid(path, e))?),
    })
}

/// Epoch seconds, to the microsecond (as botocore rounds them): a float's
/// `.123` is 123 ms, not 122.999906 ms.
pub(crate) fn from_epoch(f: f64) -> DateTime {
    let whole = f.floor();
    let micros = ((f - whole) * 1e6).round();
    if micros >= 1e6 {
        DateTime::from_secs(whole as i64 + 1)
    } else {
        DateTime::from_secs_and_nanos(whole as i64, micros as u32 * 1000)
    }
}

/// A timestamp from the caller's JSON.
pub(crate) fn timestamp_in(v: &Value) -> Result<DateTime, String> {
    match v {
        Value::Number(n) => n
            .as_f64()
            .filter(|f| f.is_finite())
            .map(from_epoch)
            .ok_or_else(|| "a timestamp out of range".to_owned()),
        Value::String(s) => parse_time(s.trim()),
        other => Err(format!(
            "expected a timestamp (an RFC 3339 string or epoch seconds), got {}",
            type_name(other)
        )),
    }
}

/// Any of the protocols' timestamp forms: RFC 3339 (with or without an
/// offset), a bare date, an HTTP date, or epoch seconds.
pub(crate) fn parse_time(s: &str) -> Result<DateTime, String> {
    if let Ok(t) = DateTime::from_str(s, Format::DateTime) {
        return Ok(t);
    }
    if let Ok(t) = DateTime::from_str(s, Format::DateTimeWithOffset) {
        return Ok(t);
    }
    // botocore also takes a space for the `T`, and a lowercase `z`.
    let fixed = s.replacen(' ', "T", 1).replace('z', "Z");
    if fixed != s {
        if let Ok(t) = DateTime::from_str(&fixed, Format::DateTimeWithOffset) {
            return Ok(t);
        }
    }
    if s.len() == 10 && s.as_bytes().get(4) == Some(&b'-') {
        if let Ok(t) = DateTime::from_str(&format!("{s}T00:00:00Z"), Format::DateTime) {
            return Ok(t);
        }
    }
    if let Ok(t) = DateTime::from_str(s, Format::HttpDate) {
        return Ok(t);
    }
    if let Ok(f) = s.parse::<f64>() {
        if f.is_finite() {
            return Ok(from_epoch(f));
        }
    }
    Err(format!(
        "{s:?} is not a timestamp (use RFC 3339, such as 2026-10-01T12:00:00Z)"
    ))
}

/// A timestamp for the caller: RFC 3339, UTC.
pub(crate) fn time_out(t: DateTime) -> Value {
    match t.fmt(Format::DateTime) {
        Ok(s) => Value::String(s),
        Err(_) => Value::from(t.secs()),
    }
}

pub(crate) fn b64(bytes: &[u8]) -> String {
    base64::engine::general_purpose::STANDARD.encode(bytes)
}

pub(crate) fn unb64(text: &str) -> Result<Vec<u8>, String> {
    let t: String = text.split_ascii_whitespace().collect();
    base64::engine::general_purpose::STANDARD
        .decode(t.as_bytes())
        .map_err(|e| format!("invalid base64: {e}"))
}

/// A blob from the caller's JSON (see the module's conventions).
pub(crate) fn blob_in(v: &Value) -> Result<Vec<u8>, String> {
    match v {
        Value::String(s) => Ok(s.as_bytes().to_vec()),
        Value::Object(o) if o.len() == 1 && o.contains_key("base64") => match &o["base64"] {
            Value::String(s) => unb64(s),
            _ => Err("{\"base64\": …} takes a string".into()),
        },
        Value::Object(_) | Value::Array(_) => Ok(v.to_string().into_bytes()),
        other => Err(format!(
            "expected a blob (a string, or {{\"base64\": \"…\"}}), got {}",
            type_name(other)
        )),
    }
}

/// A blob for the caller: its text when it is printable UTF-8, else base64.
pub(crate) fn blob_out(bytes: &[u8]) -> Value {
    match std::str::from_utf8(bytes) {
        Ok(s)
            if s.chars()
                .all(|c| !c.is_control() || matches!(c, '\n' | '\r' | '\t')) =>
        {
            Value::String(s.to_owned())
        }
        _ => serde_json::json!({ "base64": b64(bytes) }),
    }
}

/// A float the way the protocols spell it: a number, or `NaN` and the
/// infinities as strings.
pub(crate) fn float_text(f: f64) -> String {
    if f.is_nan() {
        "NaN".into()
    } else if f == f64::INFINITY {
        "Infinity".into()
    } else if f == f64::NEG_INFINITY {
        "-Infinity".into()
    } else {
        f.to_string()
    }
}

pub(crate) fn float_json(f: f64) -> Value {
    serde_json::Number::from_f64(f).map_or_else(|| Value::String(float_text(f)), Value::Number)
}

/// A number the caller reads: the integer when it is whole and in range.
pub(crate) fn number_out(text: &str, integer: bool) -> Value {
    let t = text.trim();
    if integer {
        if let Ok(i) = t.parse::<i64>() {
            return Value::from(i);
        }
    }
    match t {
        "NaN" | "Infinity" | "-Infinity" => Value::String(t.to_owned()),
        _ => t
            .parse::<f64>()
            .ok()
            .map_or_else(|| Value::String(t.to_owned()), float_json),
    }
}

/// The name a member travels under: its own `locationName`, else its
/// shape's, else the member's name (botocore merges a member's traits over
/// its shape's).
pub(crate) fn wire_name<'a>(m: MemberRef<'a>) -> &'a str {
    m.location_name()
        .or_else(|| m.shape().location_name())
        .unwrap_or_else(|| m.name())
}

/// A member's `locationName`, its own or its shape's, if it has one.
pub(crate) fn location_name<'a>(m: MemberRef<'a>) -> Option<&'a str> {
    m.location_name().or_else(|| m.shape().location_name())
}

/// A member's XML namespace, its own or its shape's.
pub(crate) fn xml_namespace<'a>(m: MemberRef<'a>) -> Option<(Option<&'a str>, &'a str)> {
    m.xml_namespace().or_else(|| m.shape().xml_namespace())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timestamps_take_the_usual_forms() {
        let want = DateTime::from_secs(1_790_000_000);
        for s in [
            "2026-09-21T14:13:20Z",
            "2026-09-21T14:13:20+00:00",
            "2026-09-21T16:13:20+02:00",
            "2026-09-21 14:13:20Z",
            "Mon, 21 Sep 2026 14:13:20 GMT",
            "1790000000",
        ] {
            assert_eq!(parse_time(s).unwrap(), want, "{s}");
        }
        assert_eq!(
            parse_time("2026-09-21").unwrap(),
            DateTime::from_secs(1_789_948_800)
        );
        assert_eq!(
            timestamp_in(&serde_json::json!(1_790_000_000)).unwrap(),
            want
        );
        assert!(parse_time("next tuesday").is_err());
        assert_eq!(time_out(want), "2026-09-21T14:13:20Z");
    }

    #[test]
    fn blobs_are_text_or_base64() {
        assert_eq!(blob_in(&serde_json::json!("hi")).unwrap(), b"hi");
        assert_eq!(
            blob_in(&serde_json::json!({"base64": "AAEC"})).unwrap(),
            [0, 1, 2]
        );
        assert_eq!(
            blob_in(&serde_json::json!({"key": 1})).unwrap(),
            br#"{"key":1}"#
        );
        assert!(blob_in(&serde_json::json!(5)).is_err());
        assert_eq!(blob_out(b"{\"a\": 1}\n"), serde_json::json!("{\"a\": 1}\n"));
        assert_eq!(blob_out(&[0, 1, 2]), serde_json::json!({"base64": "AAEC"}));
    }

    #[test]
    fn floats_spell_their_specials() {
        assert_eq!(float_text(1.5), "1.5");
        assert_eq!(float_text(f64::NAN), "NaN");
        assert_eq!(
            float_json(f64::NEG_INFINITY),
            serde_json::json!("-Infinity")
        );
        assert_eq!(number_out("42", true), serde_json::json!(42));
        assert_eq!(number_out("4.5", false), serde_json::json!(4.5));
    }
}
