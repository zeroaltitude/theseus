//! Pagination from `paginators-1.json`: where a page's tokens come from,
//! when the pages end, and how their results join.
//!
//! The models' expressions are a small subset of JMESPath: a member, a
//! dotted path (`DistributionList.NextMarker`), an index (`Contents[-1].Key`),
//! and alternatives (`NextMarker || Contents[-1].Key`). Nothing else appears
//! in the 3,162 paginators of the 2.34.15 models.

use serde_json::{Map, Value};
use theseus_aws_catalog::Paginator;

/// JMESPath's truth: null, false, and empty strings, lists, and objects are
/// false.
fn truthy(v: &Value) -> bool {
    match v {
        Value::Null => false,
        Value::Bool(b) => *b,
        Value::String(s) => !s.is_empty(),
        Value::Array(a) => !a.is_empty(),
        Value::Object(o) => !o.is_empty(),
        Value::Number(_) => true,
    }
}

/// One step of a path: a member, then any indexes (`Items[0][-1]`).
fn step<'v>(v: &'v Value, part: &str) -> Option<&'v Value> {
    let (name, mut rest) = match part.find('[') {
        Some(i) => (&part[..i], &part[i..]),
        None => (part, ""),
    };
    let mut cur = if name.is_empty() { v } else { v.get(name)? };
    while let Some(r) = rest.strip_prefix('[') {
        let end = r.find(']')?;
        let i: i64 = r[..end].trim().parse().ok()?;
        let items = cur.as_array()?;
        let at = if i < 0 { items.len() as i64 + i } else { i };
        cur = items.get(usize::try_from(at).ok()?)?;
        rest = &r[end + 1..];
    }
    Some(cur)
}

/// Evaluates an expression against a page's output (keyed by member names).
pub(crate) fn eval<'v>(expr: &str, v: &'v Value) -> Option<&'v Value> {
    let alternatives: Vec<&str> = expr.split("||").map(str::trim).collect();
    let last = alternatives.len().saturating_sub(1);
    for (i, alt) in alternatives.iter().enumerate() {
        let mut cur = Some(v);
        for part in alt.split('.') {
            cur = cur.and_then(|c| step(c, part.trim()));
        }
        match cur {
            Some(x) if truthy(x) || i == last => return Some(x),
            _ => {}
        }
    }
    None
}

/// The input members to set for the page after `output`, or `None` when the
/// pages are done: `more_results` says no, every token is empty, or the
/// tokens did not move.
pub(crate) fn next_input(
    p: Paginator<'_>,
    output: &Value,
    input: &Value,
) -> Option<Map<String, Value>> {
    if let Some(more) = p.more_results() {
        if !eval(more, output).is_some_and(truthy) {
            return None;
        }
    }
    let inputs = p.input_tokens();
    let outputs = p.output_tokens();
    let mut next = Map::new();
    for (name, expr) in inputs.iter().zip(outputs.iter()) {
        if let Some(v) = eval(expr, output).filter(|v| truthy(v)) {
            next.insert((*name).to_owned(), v.clone());
        }
    }
    if next.is_empty() {
        return None;
    }
    let same = next.iter().all(|(k, v)| input.get(k) == Some(v));
    (!same).then_some(next)
}

/// Sets `value` at a dotted path, making objects as needed.
fn set_path(target: &mut Value, path: &str, value: Option<Value>) {
    let parts: Vec<&str> = path.split('.').map(str::trim).collect();
    let mut cur = target;
    for (i, part) in parts.iter().enumerate() {
        let Value::Object(o) = cur else {
            return;
        };
        if i == parts.len() - 1 {
            match value {
                Some(v) => {
                    o.insert((*part).to_owned(), v);
                }
                None => {
                    o.remove(*part);
                }
            }
            return;
        }
        cur = o
            .entry((*part).to_owned())
            .or_insert_with(|| Value::Object(Map::new()));
    }
}

/// Joins the pages: the first page, its result lists made the lists of
/// every page, and its tokens made the last page's (so the body says where
/// the next page starts, as AWS's own last page would).
pub(crate) fn merge(p: Paginator<'_>, pages: &[Value]) -> Value {
    let Some((first, rest)) = pages.split_first() else {
        return Value::Object(Map::new());
    };
    if rest.is_empty() {
        return first.clone();
    }
    let mut merged = first.clone();
    let results = p.result_keys();
    for key in &results {
        let mut all = Vec::new();
        for page in pages {
            if let Some(Value::Array(items)) = eval(key, page) {
                all.extend(items.iter().cloned());
            }
        }
        set_path(&mut merged, key, Some(Value::Array(all)));
    }
    let last = &pages[pages.len() - 1];
    let mut tokens: Vec<&str> = p.output_tokens();
    tokens.extend(p.more_results());
    for expr in tokens {
        for term in expr.split("||").map(str::trim) {
            if term.contains('[') || results.contains(&term) {
                continue;
            }
            set_path(&mut merged, term, eval(term, last).cloned());
        }
    }
    merged
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn expressions_of_the_models() {
        let page = json!({
            "IsTruncated": true,
            "Contents": [{"Key": "a"}, {"Key": "b"}],
            "DistributionList": {"NextMarker": "m2", "Items": [1, 2]},
            "Empty": ""
        });
        assert_eq!(eval("Contents[-1].Key", &page), Some(&json!("b")));
        assert_eq!(eval("Contents[0].Key", &page), Some(&json!("a")));
        assert_eq!(
            eval("DistributionList.NextMarker", &page),
            Some(&json!("m2"))
        );
        assert_eq!(
            eval("NextMarker || Contents[-1].Key", &page),
            Some(&json!("b"))
        );
        assert_eq!(eval("Empty || Contents[0].Key", &page), Some(&json!("a")));
        assert_eq!(eval("Missing.Deeper", &page), None);
        assert_eq!(eval("Contents[9].Key", &page), None);
    }

    #[test]
    fn set_path_makes_room() {
        let mut v = json!({"A": {"B": 1}});
        set_path(&mut v, "A.C", Some(json!([1])));
        set_path(&mut v, "D.E", Some(json!(true)));
        set_path(&mut v, "A.B", None);
        assert_eq!(v, json!({"A": {"C": [1]}, "D": {"E": true}}));
    }
}
