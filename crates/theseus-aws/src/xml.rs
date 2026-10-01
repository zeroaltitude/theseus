//! XML: REST-XML request bodies, and the XML answers of the query, EC2, and
//! REST-XML protocols read through their shapes. AWS's `aws-smithy-xml` does
//! the writing and the reading (a streaming reader, as the SDKs use it).

use aws_smithy_xml::decode::{try_data, Document, ScopedDecoder};
use aws_smithy_xml::encode::{ScopeWriter, XmlWriter};
use serde_json::{Map, Value};
use theseus_aws_catalog::{Kind, Location, MemberRef, ShapeRef, TimestampFormat};

use crate::scalar;
use crate::value::{self, V};

// ---- writing ---------------------------------------------------------------

/// A REST-XML body: the root element `name`, in its namespace, holding a
/// structure's members (attributes first, as botocore writes them).
pub(crate) fn document(
    name: &str,
    ns: Option<(Option<&str>, &str)>,
    members: &[(MemberRef<'_>, V<'_>)],
) -> Vec<u8> {
    let mut out = String::new();
    {
        let mut w = XmlWriter::new(&mut out);
        let mut el = w.start_el(name);
        if let Some((prefix, uri)) = ns {
            el = el.write_ns(uri, prefix);
        }
        for (m, v) in members.iter().filter(|(m, _)| m.is_xml_attribute()) {
            el.write_attribute(value::wire_name(*m), &leaf_text(*m, v));
        }
        let mut scope = el.finish();
        for (m, v) in members.iter().filter(|(m, _)| !m.is_xml_attribute()) {
            element(&mut scope, value::wire_name(*m), *m, v);
        }
        scope.finish();
    }
    out.into_bytes()
}

fn leaf_text(m: MemberRef<'_>, v: &V<'_>) -> String {
    scalar::text(m.shape(), v, TimestampFormat::Iso8601)
}

fn leaf(parent: &mut ScopeWriter<'_, '_>, name: &str, text: &str) {
    let mut s = parent.start_el(name).finish();
    s.data(text);
}

fn element(parent: &mut ScopeWriter<'_, '_>, name: &str, m: MemberRef<'_>, v: &V<'_>) {
    match v {
        V::Struct(ms) => {
            let mut el = parent.start_el(name);
            if let Some((prefix, uri)) = value::xml_namespace(m) {
                el = el.write_ns(uri, prefix);
            }
            for (cm, cv) in ms.iter().filter(|(cm, _)| cm.is_xml_attribute()) {
                el.write_attribute(value::wire_name(*cm), &leaf_text(*cm, cv));
            }
            let mut scope = el.finish();
            for (cm, cv) in ms.iter().filter(|(cm, _)| !cm.is_xml_attribute()) {
                element(&mut scope, value::wire_name(*cm), *cm, cv);
            }
        }
        V::List(items) => {
            let Some(lm) = m.shape().list_member() else {
                return;
            };
            if m.is_flattened() {
                // Each item is an element of the list's own name.
                for item in items {
                    element(parent, name, lm, item);
                }
            } else {
                let mut el = parent.start_el(name);
                if let Some((prefix, uri)) = value::xml_namespace(m) {
                    el = el.write_ns(uri, prefix);
                }
                let mut scope = el.finish();
                let item = value::location_name(lm).unwrap_or("member");
                for i in items {
                    element(&mut scope, item, lm, i);
                }
            }
        }
        V::Map(entries) => {
            let (Some(km), Some(vm)) = (m.shape().map_key(), m.shape().map_value()) else {
                return;
            };
            let kn = value::location_name(km).unwrap_or("key");
            let vn = value::location_name(vm).unwrap_or("value");
            if m.is_flattened() {
                for (k, i) in entries {
                    let mut entry = parent.start_el(name).finish();
                    leaf(&mut entry, kn, k);
                    element(&mut entry, vn, vm, i);
                }
            } else {
                let mut el = parent.start_el(name);
                if let Some((prefix, uri)) = value::xml_namespace(m) {
                    el = el.write_ns(uri, prefix);
                }
                let mut scope = el.finish();
                for (k, i) in entries {
                    let mut entry = scope.start_el("entry").finish();
                    leaf(&mut entry, kn, k);
                    element(&mut entry, vn, vm, i);
                }
            }
        }
        scalar => {
            let mut el = parent.start_el(name);
            if let Some((prefix, uri)) = value::xml_namespace(m) {
                el = el.write_ns(uri, prefix);
            }
            let mut scope = el.finish();
            scope.data(&leaf_text(m, scalar));
        }
    }
}

// ---- reading ---------------------------------------------------------------

fn utf8(body: &[u8]) -> Result<&str, String> {
    std::str::from_utf8(body).map_err(|_| "the XML is not UTF-8".to_owned())
}

/// An element's text (botocore's `node.text`): empty for an empty element.
fn text(dec: &mut ScopedDecoder<'_, '_>) -> String {
    try_data(dec).map(|t| t.into_owned()).unwrap_or_default()
}

/// The tag a structure's member answers to: a flattened list's items carry
/// their member's name, if it has one; anything else carries its own.
fn tag_of(m: MemberRef<'_>) -> &str {
    if m.shape().kind() == Kind::List && m.is_flattened() {
        if let Some(n) = m.shape().list_member().and_then(value::location_name) {
            return n;
        }
    }
    value::wire_name(m)
}

/// Reads the request id where the query protocols put it in a body: a
/// `requestId` or `RequestId` element, or `ResponseMetadata/RequestId`.
fn capture_request_id(tag: &str, child: &mut ScopedDecoder<'_, '_>, slot: &mut Option<String>) {
    match tag {
        "requestId" | "RequestId" | "RequestID" => {
            if slot.is_none() {
                *slot = Some(text(child));
            }
        }
        "ResponseMetadata" => {
            while let Some(mut c) = child.next_tag() {
                if c.start_el().local() == "RequestId" && slot.is_none() {
                    *slot = Some(text(&mut c));
                }
            }
        }
        _ => {}
    }
}

/// A structure's members from the element `dec` is scoped to: its
/// attributes and its children. Members bound to headers or the status are
/// skipped, as are tags no member names.
fn read_struct(
    dec: &mut ScopedDecoder<'_, '_>,
    shape: ShapeRef<'_>,
    mut request_id: Option<&mut Option<String>>,
) -> Result<Map<String, Value>, String> {
    let mut out = Map::new();
    for m in shape.members().filter(|m| m.is_xml_attribute()) {
        if let Some(v) = dec.start_el().attr(value::wire_name(m)) {
            let v = leaf_value(m.shape(), v)?;
            if !v.is_null() {
                out.insert(m.name().to_owned(), v);
            }
        }
    }
    let members: Vec<(MemberRef<'_>, &str)> = shape
        .members()
        .filter(|m| m.location() == Location::Body && !m.is_xml_attribute())
        .map(|m| (m, tag_of(m)))
        .collect();
    while let Some(mut child) = dec.next_tag() {
        let tag = child.start_el().local().to_owned();
        let Some(m) = members.iter().find(|(_, t)| *t == tag).map(|(m, _)| *m) else {
            if let Some(slot) = request_id.as_deref_mut() {
                capture_request_id(&tag, &mut child, slot);
            }
            continue;
        };
        let flat = m.is_flattened();
        match m.shape().kind() {
            Kind::List if flat => {
                let lm = m
                    .shape()
                    .list_member()
                    .ok_or("a list without a member type")?;
                let item = read_value(&mut child, lm)?;
                if let Value::Array(items) = out
                    .entry(m.name())
                    .or_insert_with(|| Value::Array(Vec::new()))
                {
                    items.push(item);
                }
            }
            Kind::Map if flat => {
                let (k, v) = read_entry(&mut child, m.shape())?;
                if let Value::Object(map) = out
                    .entry(m.name())
                    .or_insert_with(|| Value::Object(Map::new()))
                {
                    map.insert(k, v);
                }
            }
            _ => {
                // botocore keeps the first of a repeated element.
                if !out.contains_key(m.name()) {
                    let v = read_value(&mut child, m)?;
                    if !v.is_null() {
                        out.insert(m.name().to_owned(), v);
                    }
                }
            }
        }
    }
    Ok(out)
}

fn read_value(dec: &mut ScopedDecoder<'_, '_>, m: MemberRef<'_>) -> Result<Value, String> {
    let shape = m.shape();
    match shape.kind() {
        Kind::Structure => Ok(Value::Object(read_struct(dec, shape, None)?)),
        Kind::List => {
            let lm = shape.list_member().ok_or("a list without a member type")?;
            let mut items = Vec::new();
            if m.is_flattened() {
                items.push(read_value(dec, lm)?);
            } else {
                // Every child is an item, whatever its tag (botocore's rule).
                while let Some(mut c) = dec.next_tag() {
                    items.push(read_value(&mut c, lm)?);
                }
            }
            Ok(Value::Array(items))
        }
        Kind::Map => {
            let mut map = Map::new();
            if m.is_flattened() {
                let (k, v) = read_entry(dec, shape)?;
                map.insert(k, v);
            } else {
                while let Some(mut entry) = dec.next_tag() {
                    let (k, v) = read_entry(&mut entry, shape)?;
                    map.insert(k, v);
                }
            }
            Ok(Value::Object(map))
        }
        _ => {
            let t = text(dec);
            leaf_value(shape, &t)
        }
    }
}

fn read_entry(
    dec: &mut ScopedDecoder<'_, '_>,
    map: ShapeRef<'_>,
) -> Result<(String, Value), String> {
    let km = map.map_key().ok_or("a map without a key type")?;
    let vm = map.map_value().ok_or("a map without a value type")?;
    let kn = value::location_name(km).unwrap_or("key");
    let vn = value::location_name(vm).unwrap_or("value");
    let (mut key, mut val) = (None, Value::Null);
    while let Some(mut c) = dec.next_tag() {
        let tag = c.start_el().local().to_owned();
        if tag == kn {
            key = Some(text(&mut c));
        } else if tag == vn {
            val = read_value(&mut c, vm)?;
        }
    }
    Ok((key.ok_or("a map entry without a key")?, val))
}

/// A scalar from its text; an empty number or boolean is no value.
pub(crate) fn leaf_value(shape: ShapeRef<'_>, t: &str) -> Result<Value, String> {
    Ok(match shape.kind() {
        Kind::String => Value::String(t.to_owned()),
        _ if t.trim().is_empty() => Value::Null,
        Kind::Boolean => Value::Bool(t.trim() == "true"),
        Kind::Integer | Kind::Long => value::number_out(t, true),
        Kind::Float | Kind::Double => value::number_out(t, false),
        Kind::Timestamp => value::time_out(value::parse_time(t.trim())?),
        Kind::Blob => value::blob_out(&value::unb64(t)?),
        Kind::Structure | Kind::List | Kind::Map => Value::Null,
    })
}

/// The query protocol's answer: the result inside its wrapper element
/// (`<ListRolesResult>`), and the request id in `ResponseMetadata`.
pub(crate) fn read_query(
    body: &[u8],
    shape: Option<ShapeRef<'_>>,
    wrapper: Option<&str>,
) -> Result<(Value, Option<String>), String> {
    let mut doc = Document::new(utf8(body)?);
    let mut root = doc.root_element().map_err(|e| e.to_string())?;
    let mut request_id = None;
    let mut out = Map::new();
    match (shape, wrapper) {
        (Some(shape), Some(wrapper)) => {
            while let Some(mut c) = root.next_tag() {
                let tag = c.start_el().local().to_owned();
                if tag == wrapper {
                    out = read_struct(&mut c, shape, None)?;
                } else {
                    capture_request_id(&tag, &mut c, &mut request_id);
                }
            }
        }
        (Some(shape), None) => out = read_struct(&mut root, shape, Some(&mut request_id))?,
        (None, _) => {
            while let Some(mut c) = root.next_tag() {
                let tag = c.start_el().local().to_owned();
                capture_request_id(&tag, &mut c, &mut request_id);
            }
        }
    }
    Ok((Value::Object(out), request_id))
}

/// EC2's answer: the result's members directly under the root, beside
/// `requestId`.
pub(crate) fn read_ec2(
    body: &[u8],
    shape: Option<ShapeRef<'_>>,
) -> Result<(Value, Option<String>), String> {
    read_query(body, shape, None)
}

/// A REST-XML body as a structure: the root element holds its members.
pub(crate) fn read_rest(body: &[u8], shape: ShapeRef<'_>) -> Result<Value, String> {
    let mut doc = Document::new(utf8(body)?);
    let mut root = doc.root_element().map_err(|e| e.to_string())?;
    Ok(Value::Object(read_struct(&mut root, shape, None)?))
}

/// A REST-XML payload member's value: a structure from the root element, or
/// the root's text for a scalar.
pub(crate) fn read_payload(body: &[u8], m: MemberRef<'_>) -> Result<Value, String> {
    let mut doc = Document::new(utf8(body)?);
    let mut root = doc.root_element().map_err(|e| e.to_string())?;
    read_value(&mut root, m)
}

/// The root element's name and its text, for S3's answers that are one
/// element (`GetBucketLocation`) or an error where a success was expected.
pub(crate) fn root(body: &[u8]) -> Option<(String, String)> {
    let mut doc = Document::new(std::str::from_utf8(body).ok()?);
    let mut root = doc.root_element().ok()?;
    let name = root.start_el().local().to_owned();
    let t = text(&mut root);
    Some((name, t))
}

/// The fields of an XML error, wherever the protocol nests them: query's
/// `ErrorResponse/Error`, EC2's `Response/Errors/Error`, S3's bare `Error`.
#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct ErrorFields {
    pub(crate) code: Option<String>,
    pub(crate) message: Option<String>,
    pub(crate) request_id: Option<String>,
    /// S3's redirect: the bucket's region, or the endpoint to use.
    pub(crate) region: Option<String>,
    pub(crate) endpoint: Option<String>,
}

pub(crate) fn read_error(body: &[u8]) -> Option<ErrorFields> {
    let mut doc = Document::new(std::str::from_utf8(body).ok()?);
    let mut f = ErrorFields::default();
    let mut seen_root = false;
    while let Some(el) = doc.next_start_element() {
        if !seen_root {
            seen_root = true;
            continue;
        }
        let tag = el.local().to_owned();
        let slot = match tag.as_str() {
            "Code" => &mut f.code,
            "Message" => &mut f.message,
            "RequestId" | "RequestID" | "requestId" => &mut f.request_id,
            "Region" | "BucketRegion" => &mut f.region,
            "Endpoint" => &mut f.endpoint,
            _ => continue,
        };
        let mut scoped = doc.scoped_to(el);
        let t = text(&mut scoped);
        if slot.is_none() {
            *slot = Some(t);
        }
    }
    seen_root.then_some(f)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_fields_are_found_at_any_depth() {
        let query = br#"<ErrorResponse xmlns="https://iam.amazonaws.com/doc/2010-05-08/">
          <Error><Type>Sender</Type><Code>NoSuchEntity</Code>
          <Message>The role with name x cannot be found.</Message></Error>
          <RequestId>4ba1-example</RequestId></ErrorResponse>"#;
        let f = read_error(query).unwrap();
        assert_eq!(f.code.as_deref(), Some("NoSuchEntity"));
        assert_eq!(f.request_id.as_deref(), Some("4ba1-example"));

        let ec2 = br#"<Response><Errors><Error><Code>InvalidInstanceID.Malformed</Code>
          <Message>Invalid id: "i-1"</Message></Error></Errors>
          <RequestID>ec2-example</RequestID></Response>"#;
        let f = read_error(ec2).unwrap();
        assert_eq!(f.code.as_deref(), Some("InvalidInstanceID.Malformed"));
        assert_eq!(f.message.as_deref(), Some("Invalid id: \"i-1\""));
        assert_eq!(f.request_id.as_deref(), Some("ec2-example"));

        let s3 = br#"<?xml version="1.0" encoding="UTF-8"?>
          <Error><Code>PermanentRedirect</Code><Message>Use the bucket's endpoint.</Message>
          <Endpoint>example-bucket.s3.us-east-1.amazonaws.com</Endpoint>
          <Bucket>example-bucket</Bucket><RequestId>s3-example</RequestId><HostId>h</HostId></Error>"#;
        let f = read_error(s3).unwrap();
        assert_eq!(f.code.as_deref(), Some("PermanentRedirect"));
        assert_eq!(
            f.endpoint.as_deref(),
            Some("example-bucket.s3.us-east-1.amazonaws.com")
        );
        assert_eq!(read_error(b"not xml at all"), None);
    }

    #[test]
    fn the_root_and_its_text() {
        let b = br#"<?xml version="1.0" encoding="UTF-8"?>
<LocationConstraint xmlns="http://s3.amazonaws.com/doc/2006-03-01/">us-west-2</LocationConstraint>"#;
        assert_eq!(
            root(b),
            Some(("LocationConstraint".into(), "us-west-2".into()))
        );
        let empty = br#"<LocationConstraint xmlns="http://s3.amazonaws.com/doc/2006-03-01/"/>"#;
        assert_eq!(
            root(empty),
            Some(("LocationConstraint".into(), String::new()))
        );
    }
}
