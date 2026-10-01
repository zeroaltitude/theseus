//! The REST protocols' HTTP bindings (REST-JSON and REST-XML): members bound
//! to the URI, the query string, headers, and the body, both ways, as
//! botocore binds them.

use std::collections::HashMap;

use base64::Engine as _;
use serde_json::{Map, Value};
use theseus_aws_catalog::{Kind, Location, MemberRef, OperationRef, TimestampFormat};

use crate::json;
use crate::request::{encode, encode_path};
use crate::scalar;
use crate::value::{self, V};
use crate::xml;

/// The parts of a REST request the bindings fill.
#[derive(Debug, Default)]
pub(crate) struct Parts {
    pub(crate) path: String,
    pub(crate) query: Vec<(String, Option<String>)>,
    pub(crate) headers: Vec<(String, String)>,
    pub(crate) body: Vec<u8>,
}

/// A header's text: a list joined with commas (an item holding a comma or a
/// quote is quoted), a timestamp as an HTTP date unless the shape says
/// otherwise, a JSON value as base64 of its compact JSON.
fn header_text(m: MemberRef<'_>, v: &V<'_>) -> Option<String> {
    match v {
        V::List(items) => {
            if items.is_empty() {
                return None;
            }
            let lm = m.shape().list_member()?;
            let parts: Vec<String> = items
                .iter()
                .map(|i| match i {
                    V::Str(s) if s.contains(',') || s.contains('"') => {
                        format!("\"{}\"", s.replace('"', "\\\""))
                    }
                    other => scalar::text(lm.shape(), other, TimestampFormat::Rfc822),
                })
                .collect();
            Some(parts.join(","))
        }
        V::Doc(d) => Some(value::b64(d.to_string().as_bytes())),
        other => Some(scalar::text(m.shape(), other, TimestampFormat::Rfc822)),
    }
}

fn query_text(m: MemberRef<'_>, v: &V<'_>) -> String {
    scalar::text(m.shape(), v, TimestampFormat::Iso8601)
}

/// Fills a URI template's labels (`/{Bucket}/{Key+}`): a greedy label keeps
/// its slashes. A label with no value is invalid input.
fn render(template: &str, labels: &HashMap<&str, String>) -> Result<String, String> {
    let mut out = String::with_capacity(template.len() + 32);
    let mut rest = template;
    while let Some(i) = rest.find('{') {
        out.push_str(&rest[..i]);
        let end = rest[i..]
            .find('}')
            .map(|e| e + i)
            .ok_or("the request URI has an unclosed label")?;
        let label = &rest[i + 1..end];
        let (name, greedy) = match label.strip_suffix('+') {
            Some(n) => (n, true),
            None => (label, false),
        };
        let v = labels
            .get(name)
            .ok_or_else(|| format!("missing the URI member {name:?}"))?;
        if v.is_empty() {
            return Err(format!("the URI member {name:?} must not be empty"));
        }
        out.push_str(&if greedy { encode_path(v) } else { encode(v) });
        rest = &rest[end + 1..];
    }
    out.push_str(rest);
    Ok(out)
}

/// Binds an operation's input to an HTTP request.
pub(crate) fn serialize(
    op: OperationRef<'_>,
    input: Option<&V<'_>>,
    xml_body: bool,
) -> Result<Parts, String> {
    let mut parts = Parts::default();
    let (path_t, literal_query) = match op.request_uri().split_once('?') {
        Some((p, q)) => (p, Some(q)),
        None => (op.request_uri(), None),
    };
    if let Some(q) = literal_query {
        for pair in q.split('&').filter(|p| !p.is_empty()) {
            match pair.split_once('=') {
                Some((k, v)) => parts.query.push((k.to_owned(), Some(v.to_owned()))),
                None => parts.query.push((pair.to_owned(), None)),
            }
        }
    }
    let empty = Vec::new();
    let members = match input {
        Some(V::Struct(ms)) => ms,
        _ => &empty,
    };
    let mut labels: HashMap<&str, String> = HashMap::new();
    let mut body_members = Vec::new();
    for (m, v) in members {
        let wire = value::wire_name(*m);
        match m.location() {
            Location::Uri => {
                labels.insert(wire, query_text(*m, v));
            }
            Location::Querystring => match v {
                V::Map(entries) => {
                    let vm = m.shape().map_value();
                    for (k, item) in entries {
                        match (item, vm) {
                            (V::List(items), Some(vm)) => {
                                let lm = vm.shape().list_member();
                                for i in items {
                                    let t = lm.map_or_else(String::new, |lm| query_text(lm, i));
                                    parts.query.push((k.clone(), Some(t)));
                                }
                            }
                            (other, Some(vm)) => {
                                parts.query.push((k.clone(), Some(query_text(vm, other))));
                            }
                            (_, None) => {}
                        }
                    }
                }
                V::List(items) => {
                    if let Some(lm) = m.shape().list_member() {
                        for i in items {
                            parts.query.push((wire.to_owned(), Some(query_text(lm, i))));
                        }
                    }
                }
                other => parts
                    .query
                    .push((wire.to_owned(), Some(query_text(*m, other)))),
            },
            Location::Header => {
                if let Some(t) = header_text(*m, v) {
                    parts.headers.push((wire.to_owned(), t));
                }
            }
            Location::Headers => {
                if let (V::Map(entries), Some(vm)) = (v, m.shape().map_value()) {
                    for (k, item) in entries {
                        parts.headers.push((
                            format!("{wire}{k}"),
                            scalar::text(vm.shape(), item, TimestampFormat::Rfc822),
                        ));
                    }
                }
            }
            Location::StatusCode => {}
            Location::Body => body_members.push((*m, v.clone())),
        }
    }
    parts.path = render(path_t, &labels)?;

    let input_shape = op.input();
    let payload = input_shape.and_then(|s| s.payload());
    match payload {
        Some(p) if matches!(p.shape().kind(), Kind::Blob | Kind::String) => {
            // The body is the member's bytes, sent as they are.
            match members.iter().find(|(m, _)| m.name() == p.name()) {
                Some((_, V::Blob(b))) => parts.body = b.clone(),
                Some((_, V::Str(s))) => parts.body = s.as_bytes().to_vec(),
                _ => {}
            }
        }
        Some(p) => match members.iter().find(|(m, _)| m.name() == p.name()) {
            Some((_, v)) => {
                parts.body = if xml_body {
                    let name = value::location_name(p).unwrap_or_else(|| p.name());
                    match v {
                        V::Struct(ms) => xml::document(name, value::xml_namespace(p), ms),
                        _ => Vec::new(),
                    }
                } else {
                    serde_json::to_vec(&json::to_json(p.shape(), v)).unwrap_or_default()
                };
            }
            None if !xml_body => parts.body = b"{}".to_vec(),
            None => {}
        },
        None => {
            if !body_members.is_empty() {
                parts.body = if xml_body {
                    let name = op
                        .input_location_name()
                        .or_else(|| input_shape.map(|s| s.name()))
                        .unwrap_or("Request");
                    xml::document(name, op.input_xml_namespace(), &body_members)
                } else {
                    serde_json::to_vec(&json::object(&body_members, true)).unwrap_or_default()
                };
            } else if !xml_body
                && input_shape.is_some_and(|s| s.members().any(|m| m.location() == Location::Body))
            {
                parts.body = b"{}".to_vec();
            }
        }
    }
    let streaming = payload.is_some_and(|p| matches!(p.shape().kind(), Kind::Blob | Kind::String));
    if !xml_body
        && !streaming
        && !parts.body.is_empty()
        && !parts
            .headers
            .iter()
            .any(|(k, _)| k.eq_ignore_ascii_case("content-type"))
    {
        parts
            .headers
            .push(("Content-Type".to_owned(), "application/json".to_owned()));
    }
    Ok(parts)
}

fn header<'h>(headers: &'h [(String, String)], name: &str) -> Option<&'h str> {
    headers
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case(name))
        .map(|(_, v)| v.as_str())
}

/// A header's value read through its member's shape.
fn header_value(m: MemberRef<'_>, t: &str) -> Value {
    let shape = m.shape();
    match shape.kind() {
        Kind::String if m.is_jsonvalue() => base64::engine::general_purpose::STANDARD
            .decode(t.trim())
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_else(|| Value::String(t.to_owned())),
        Kind::List => match shape.list_member() {
            Some(lm) => Value::Array(
                t.split(',')
                    .map(|i| {
                        xml::leaf_value(lm.shape(), i.trim())
                            .unwrap_or_else(|_| Value::String(i.trim().to_owned()))
                    })
                    .collect(),
            ),
            None => Value::String(t.to_owned()),
        },
        _ => xml::leaf_value(shape, t).unwrap_or_else(|_| Value::String(t.to_owned())),
    }
}

/// Reads a REST answer: the members bound to the status and headers, then
/// the body (the payload member's, or the structure's other members).
pub(crate) fn parse(
    op: OperationRef<'_>,
    status: u16,
    headers: &[(String, String)],
    body: &[u8],
    xml_body: bool,
) -> Result<Value, String> {
    let Some(shape) = op.output() else {
        return Ok(Value::Object(Map::new()));
    };
    let mut out = Map::new();
    for m in shape.members() {
        match m.location() {
            Location::StatusCode => {
                out.insert(m.name().to_owned(), Value::from(status));
            }
            Location::Header => {
                if let Some(t) = header(headers, value::wire_name(m)) {
                    out.insert(m.name().to_owned(), header_value(m, t));
                }
            }
            Location::Headers => {
                let prefix = value::wire_name(m).to_ascii_lowercase();
                let found: Map<String, Value> = headers
                    .iter()
                    .filter(|(k, _)| k.to_ascii_lowercase().starts_with(&prefix))
                    .map(|(k, v)| (k[prefix.len()..].to_owned(), Value::String(v.clone())))
                    .collect();
                if !found.is_empty() {
                    out.insert(m.name().to_owned(), Value::Object(found));
                }
            }
            _ => {}
        }
    }
    match shape.payload() {
        Some(p) if p.shape().kind() == Kind::Blob => {
            out.insert(p.name().to_owned(), value::blob_out(body));
        }
        Some(p) if p.shape().kind() == Kind::String => {
            out.insert(
                p.name().to_owned(),
                Value::String(String::from_utf8_lossy(body).into_owned()),
            );
        }
        Some(p) => {
            if !body.is_empty() {
                let v = if xml_body {
                    xml::read_payload(body, p)?
                } else {
                    let j: Value = serde_json::from_slice(body).map_err(|e| e.to_string())?;
                    json::from_json(p.shape(), &j, false)
                };
                if !v.is_null() {
                    out.insert(p.name().to_owned(), v);
                }
            }
        }
        None => {
            if !body.iter().all(u8::is_ascii_whitespace) {
                let v = if xml_body {
                    xml::read_rest(body, shape)?
                } else {
                    let j: Value = serde_json::from_slice(body).map_err(|e| e.to_string())?;
                    json::from_json(shape, &j, true)
                };
                if let Value::Object(o) = v {
                    out.extend(o);
                }
            }
        }
    }
    Ok(Value::Object(out))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uri_templates_render_and_encode() {
        let mut labels = HashMap::new();
        labels.insert("Bucket", "example-bucket".to_owned());
        labels.insert("Key", "photos/2026 trip/a+b.jpg".to_owned());
        assert_eq!(
            render("/{Bucket}/{Key+}", &labels).unwrap(),
            "/example-bucket/photos/2026%20trip/a%2Bb.jpg"
        );
        assert_eq!(
            render("/{Bucket}/{Key}", &labels).unwrap(),
            "/example-bucket/photos%2F2026%20trip%2Fa%2Bb.jpg"
        );
        assert!(render("/{Missing}", &labels).is_err());
        labels.insert("Empty", String::new());
        assert!(render("/{Empty}", &labels).is_err());
    }
}
