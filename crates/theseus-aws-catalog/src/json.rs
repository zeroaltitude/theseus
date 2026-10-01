//! JSON that keeps its keys in document order, for reading the models.
//!
//! A botocore model's member order is its declaration order, and a request is
//! serialized in that order (Route 53's XML is validated against a schema
//! sequence). The workspace's `serde_json::Value` sorts keys, and its
//! `preserve_order` feature would change every crate's maps, so the generator
//! reads into this instead.

use std::fmt;

use serde::de::{self, Deserialize, Deserializer, MapAccess, SeqAccess, Visitor};

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum J {
    Null,
    Bool(bool),
    Int(i64),
    Float(f64),
    Str(String),
    Arr(Vec<J>),
    Obj(Vec<(String, J)>),
}

impl J {
    pub(crate) fn parse(text: &str) -> Result<J, serde_json::Error> {
        serde_json::from_str(text)
    }

    pub(crate) fn get(&self, key: &str) -> Option<&J> {
        match self {
            J::Obj(entries) => entries.iter().find(|(k, _)| k == key).map(|(_, v)| v),
            _ => None,
        }
    }

    pub(crate) fn str(&self) -> Option<&str> {
        match self {
            J::Str(s) => Some(s),
            _ => None,
        }
    }

    pub(crate) fn str_at(&self, key: &str) -> Option<&str> {
        self.get(key).and_then(J::str)
    }

    pub(crate) fn bool_at(&self, key: &str) -> bool {
        matches!(self.get(key), Some(J::Bool(true)))
    }

    pub(crate) fn int(&self) -> Option<i64> {
        match self {
            J::Int(i) => Some(*i),
            J::Float(f) if f.fract() == 0.0 && f.abs() < 9.0e18 => Some(*f as i64),
            _ => None,
        }
    }

    pub(crate) fn obj(&self) -> &[(String, J)] {
        match self {
            J::Obj(entries) => entries,
            _ => &[],
        }
    }

    pub(crate) fn arr(&self) -> &[J] {
        match self {
            J::Arr(items) => items,
            _ => &[],
        }
    }

    /// A string, or a list of strings, as a list (the paginators use both).
    pub(crate) fn strs(&self) -> Vec<&str> {
        match self {
            J::Str(s) => vec![s.as_str()],
            J::Arr(items) => items.iter().filter_map(J::str).collect(),
            _ => Vec::new(),
        }
    }

    /// botocore's `merge_dicts`: objects merge key by key, recursively; any
    /// other value replaces what was there. That is how the loader applies a
    /// model's `sdk-extras` file.
    pub(crate) fn merge(&mut self, extra: &J) {
        match (self, extra) {
            (J::Obj(base), J::Obj(more)) => {
                for (k, v) in more {
                    match base.iter_mut().find(|(bk, _)| bk == k) {
                        Some((_, bv)) => bv.merge(v),
                        None => base.push((k.clone(), v.clone())),
                    }
                }
            }
            (slot, v) => *slot = v.clone(),
        }
    }
}

impl<'de> Deserialize<'de> for J {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<J, D::Error> {
        d.deserialize_any(JVisitor)
    }
}

struct JVisitor;

impl<'de> Visitor<'de> for JVisitor {
    type Value = J;

    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("any JSON value")
    }

    fn visit_unit<E: de::Error>(self) -> Result<J, E> {
        Ok(J::Null)
    }

    fn visit_bool<E: de::Error>(self, v: bool) -> Result<J, E> {
        Ok(J::Bool(v))
    }

    fn visit_i64<E: de::Error>(self, v: i64) -> Result<J, E> {
        Ok(J::Int(v))
    }

    fn visit_u64<E: de::Error>(self, v: u64) -> Result<J, E> {
        Ok(i64::try_from(v).map_or(J::Float(v as f64), J::Int))
    }

    fn visit_f64<E: de::Error>(self, v: f64) -> Result<J, E> {
        Ok(J::Float(v))
    }

    fn visit_str<E: de::Error>(self, v: &str) -> Result<J, E> {
        Ok(J::Str(v.to_owned()))
    }

    fn visit_string<E: de::Error>(self, v: String) -> Result<J, E> {
        Ok(J::Str(v))
    }

    fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<J, A::Error> {
        let mut items = Vec::new();
        while let Some(v) = seq.next_element()? {
            items.push(v);
        }
        Ok(J::Arr(items))
    }

    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<J, A::Error> {
        let mut entries = Vec::new();
        while let Some((k, v)) = map.next_entry::<String, J>()? {
            entries.push((k, v));
        }
        Ok(J::Obj(entries))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_keep_document_order() {
        let j = J::parse(r#"{"z": 1, "a": [true, null, "s"], "m": {"y": 2.5, "b": -3}}"#).unwrap();
        let keys: Vec<&str> = j.obj().iter().map(|(k, _)| k.as_str()).collect();
        assert_eq!(keys, ["z", "a", "m"]);
        let inner: Vec<&str> = j
            .get("m")
            .unwrap()
            .obj()
            .iter()
            .map(|(k, _)| k.as_str())
            .collect();
        assert_eq!(inner, ["y", "b"]);
        assert_eq!(j.get("m").unwrap().get("b").unwrap().int(), Some(-3));
    }

    #[test]
    fn merge_is_botocores_merge_dicts() {
        let mut base = J::parse(
            r#"{"shapes": {"A": {"type": "string"}, "B": {"type": "integer"}}, "v": [1]}"#,
        )
        .unwrap();
        let extra = J::parse(
            r#"{"shapes": {"A": {"type": "timestamp"}, "C": {"type": "blob"}}, "v": [2, 3]}"#,
        )
        .unwrap();
        base.merge(&extra);
        let shapes = base.get("shapes").unwrap();
        assert_eq!(shapes.get("A").unwrap().str_at("type"), Some("timestamp"));
        assert_eq!(shapes.get("B").unwrap().str_at("type"), Some("integer"));
        assert_eq!(shapes.get("C").unwrap().str_at("type"), Some("blob"));
        assert_eq!(base.get("v").unwrap().arr().len(), 2);
    }
}
