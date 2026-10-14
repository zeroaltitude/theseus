//! A request's digest without a copy of it (theseus-0l3g): `body`'s JSON,
//! keys sorted as `serde_json`'s map sorts them, written straight into the
//! hash. `body` copied every message to hash it, and a compile takes the
//! digest of a request the length of its session.

use std::collections::BTreeMap;

use serde::ser::{Serialize, SerializeMap, Serializer};
use serde_json::Value;
use sha2::{Digest, Sha256};

use super::ProviderRequest;

/// One of the body's values, borrowed.
enum Field<'a> {
    Str(&'a str),
    Num(u32),
    List(&'a [Value]),
    Val(&'a Value),
}

impl Serialize for Field<'_> {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        match self {
            Field::Str(v) => v.serialize(s),
            Field::Num(v) => v.serialize(s),
            Field::List(v) => v.serialize(s),
            Field::Val(v) => v.serialize(s),
        }
    }
}

/// The body's fields as `body` puts them, a later one over an earlier of
/// its key, in key order.
struct Body<'a>(BTreeMap<&'a str, Field<'a>>);

impl Serialize for Body<'_> {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        let mut m = s.serialize_map(Some(self.0.len()))?;
        for (k, v) in &self.0 {
            m.serialize_entry(k, v)?;
        }
        m.end()
    }
}

/// The hash as a writer.
struct Hashing(Sha256);

impl std::io::Write for Hashing {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0.update(buf);
        Ok(buf.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// sha256 of `r.body()`'s JSON, keys sorted, in hex.
pub(super) fn of(r: &ProviderRequest) -> String {
    let mut h = Hashing(Sha256::new());
    // Writing into a hash does not fail, nor does a JSON value.
    let _ = serde_json::to_writer(&mut h, &fields(r, false));
    hex::encode(h.0.finalize())
}

/// `r.body()`'s JSON, with `"stream": true` when `stream`: the bytes sent,
/// as `body` with the flag would serialize them, with no copy of the
/// request between.
pub(super) fn bytes(r: &ProviderRequest, stream: bool) -> Vec<u8> {
    serde_json::to_vec(&fields(r, stream)).unwrap_or_default()
}

static TRUE: Value = Value::Bool(true);

fn fields(r: &ProviderRequest, stream: bool) -> Body<'_> {
    let mut f: BTreeMap<&str, Field> = BTreeMap::new();
    f.insert("model", Field::Str(&r.model));
    f.insert("max_tokens", Field::Num(r.max_tokens));
    if !r.system.is_empty() {
        f.insert("system", Field::List(&r.system));
    }
    f.insert("messages", Field::List(&r.messages));
    if !r.tools.is_empty() {
        f.insert("tools", Field::List(&r.tools));
    }
    if let Some(t) = &r.thinking {
        f.insert("thinking", Field::Val(t));
    }
    if let Some(o) = &r.output_config {
        f.insert("output_config", Field::Val(o));
    }
    if let Some(c) = &r.cache_control {
        f.insert("cache_control", Field::Val(c));
    }
    for (k, v) in &r.extra {
        f.insert(k, Field::Val(v));
    }
    if stream {
        f.insert("stream", Field::Val(&TRUE));
    }
    Body(f)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// The digest is `body`'s, byte for byte, whatever the request holds.
    #[test]
    fn the_digest_is_the_bodys_without_copying_it() {
        let mut r = ProviderRequest::simple("m", 10, "hi");
        assert_eq!(of(&r), theseus_kernel::digest_json(&r.body()));
        r.system =
            vec![json!({"type": "text", "text": "S", "cache_control": {"type": "ephemeral"}})];
        r.tools = vec![json!({"name": "t", "input_schema": {"type": "object"}})];
        r.thinking = Some(json!({"type": "adaptive", "display": "summarized"}));
        r.output_config = Some(json!({"effort": "high"}));
        r.cache_control = Some(json!({"type": "ephemeral"}));
        r.extra.insert("fallbacks".into(), json!("default"));
        r.extra.insert("zz".into(), json!([1, {"b": 2, "a": 1}]));
        r.messages
            .push(json!({"role": "assistant", "content": [{"type": "text", "text": "é ✓ \"q\""}]}));
        assert_eq!(of(&r), theseus_kernel::digest_json(&r.body()));
        // An extra of a body key's name stands in its place, as in `body`.
        r.extra.insert("model".into(), json!("other"));
        assert_eq!(of(&r), theseus_kernel::digest_json(&r.body()));
        // What is sent: the body and the stream flag.
        let mut body = r.body();
        body["stream"] = json!(true);
        assert_eq!(bytes(&r, true), serde_json::to_vec(&body).unwrap());
    }
}
