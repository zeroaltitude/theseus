//! herdr's API contract, read from a checked-in snapshot of `herdr api schema --json` (herdr 0.9.1,
//! protocol 22: `tests/fixtures/herdr-0.9.1-api-schema.json`), and a small structural checker for the
//! JSON Schema subset that file uses (theseus-6ifz1). Shared by `#[path]` include: the fake herdrs of
//! `tests/herdr_*.rs` check every request they get with it and every result they give, and the unit
//! tests in the binary (`herdr_contract_tests.rs`) check each request the code builds.
//!
//! The checker is **stricter than herdr**: an object whose schema lists `properties` and says nothing of
//! `additionalProperties` rejects a key the schema does not name. herdr's serde ignores such a key, which is
//! how an invented request shape once passed every test and failed live, so this is the point.
//! No new dependency: the keywords are `$ref`, `oneOf`, `anyOf`, `type`, `const`, `enum`, `properties`,
//! `required`, `additionalProperties`, `items`, `minimum`, `maximum`, `maxProperties`, and the one
//! `propertyNames.pattern` herdr uses. A keyword the checker does not know is a panic, never a pass.
//!
//! Each includer uses only some of the checks, so the checker's own tests (below) use all of them: no item
//! is dead in any test binary, and no lint is allowed away.

use serde_json::Value;

const SNAPSHOT: &str = include_str!("../fixtures/herdr-0.9.1-api-schema.json");

/// The snapshot, ready to check against.
pub struct Schema(Value);

impl Schema {
    pub fn load() -> Self {
        Self(serde_json::from_str(SNAPSHOT).expect("the herdr schema snapshot is JSON"))
    }

    /// The params of `method` against its request schema. `Err` says why, with the path of the part at fault.
    pub fn check_request(&self, method: &str, params: &Value) -> Result<(), String> {
        let entry = self.0["schemas"]["request"]["oneOf"]
            .as_array()
            .and_then(|all| {
                all.iter()
                    .find(|o| o["properties"]["method"]["const"] == method)
            })
            .ok_or_else(|| format!("herdr has no method `{method}`"))?;
        let mut errs = Vec::new();
        self.walk(&entry["properties"]["params"], params, method, &mut errs);
        done(errs)
    }

    /// A result (the object under `result`, its `type` tag included) against the success schema.
    pub fn check_result(&self, result: &Value) -> Result<(), String> {
        let tag = result["type"].as_str().unwrap_or_default();
        let entry = self.0["schemas"]["success_response"]["$defs"]["ResponseResult"]["oneOf"]
            .as_array()
            .and_then(|all| all.iter().find(|o| o["properties"]["type"]["const"] == tag))
            .ok_or_else(|| format!("herdr has no result type `{tag}`"))?;
        let mut errs = Vec::new();
        self.walk(entry, result, tag, &mut errs);
        done(errs)
    }

    /// An event line (`{"event": …, "data": …}`) against the event schema.
    pub fn check_event(&self, event: &Value) -> Result<(), String> {
        let mut errs = Vec::new();
        self.walk(&self.0["schemas"]["event"], event, "event", &mut errs);
        done(errs)
    }

    /// Whether `herdr` knows the method.
    pub fn knows(&self, method: &str) -> bool {
        self.0["schemas"]["request"]["oneOf"]
            .as_array()
            .is_some_and(|all| {
                all.iter()
                    .any(|o| o["properties"]["method"]["const"] == method)
            })
    }

    fn resolve<'a>(&'a self, r: &str) -> &'a Value {
        let path = r.strip_prefix('#').unwrap_or_else(|| panic!("ref {r}"));
        self.0
            .pointer(path)
            .unwrap_or_else(|| panic!("the snapshot has no {r}"))
    }

    fn ok(&self, schema: &Value, v: &Value) -> bool {
        let mut errs = Vec::new();
        self.walk(schema, v, "", &mut errs);
        errs.is_empty()
    }

    fn walk(&self, schema: &Value, v: &Value, at: &str, errs: &mut Vec<String>) {
        let Some(s) = schema.as_object() else {
            return;
        };
        if let Some(r) = s.get("$ref") {
            return self.walk(self.resolve(r.as_str().unwrap()), v, at, errs);
        }
        for k in s.keys() {
            assert!(KNOWN.contains(&k.as_str()), "unknown schema keyword `{k}`");
        }
        if let Some(alts) = s.get("oneOf").and_then(Value::as_array) {
            let n = alts.iter().filter(|a| self.ok(a, v)).count();
            if n != 1 {
                errs.push(format!("{at}: matches {n} of {} alternatives", alts.len()));
                // The alternative its tag names says what is wrong with it.
                for a in alts.iter().filter(|a| tag_matches(self, a, v)) {
                    self.walk(a, v, at, errs);
                }
            }
        }
        if let Some(alts) = s.get("anyOf").and_then(Value::as_array) {
            if !alts.iter().any(|a| self.ok(a, v)) {
                errs.push(format!("{at}: matches none of {} alternatives", alts.len()));
            }
        }
        if let Some(t) = s.get("type") {
            let names: Vec<&str> = match t {
                Value::String(s) => vec![s],
                other => other
                    .as_array()
                    .unwrap()
                    .iter()
                    .filter_map(Value::as_str)
                    .collect(),
            };
            if !names.iter().any(|n| is_type(n, v)) {
                errs.push(format!("{at}: is {}, wants {names:?}", show(v)));
                return;
            }
        }
        if let Some(c) = s.get("const") {
            if c != v {
                errs.push(format!("{at}: must be {c}, is {}", show(v)));
            }
        }
        if let Some(e) = s.get("enum").and_then(Value::as_array) {
            if !e.contains(v) {
                errs.push(format!("{at}: {} is not one of {e:?}", show(v)));
            }
        }
        if let Some(n) = v.as_f64() {
            if s.get("minimum")
                .and_then(Value::as_f64)
                .is_some_and(|m| n < m)
            {
                errs.push(format!("{at}: {n} is under the minimum"));
            }
            if s.get("maximum")
                .and_then(Value::as_f64)
                .is_some_and(|m| n > m)
            {
                errs.push(format!("{at}: {n} is over the maximum"));
            }
        }
        match v {
            Value::Object(o) => self.object(s, o, at, errs),
            Value::Array(a) => {
                if let Some(items) = s.get("items") {
                    for (i, x) in a.iter().enumerate() {
                        self.walk(items, x, &format!("{at}[{i}]"), errs);
                    }
                }
            }
            _ => {}
        }
    }

    fn object(
        &self,
        s: &serde_json::Map<String, Value>,
        o: &serde_json::Map<String, Value>,
        at: &str,
        errs: &mut Vec<String>,
    ) {
        for r in s
            .get("required")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            let key = r.as_str().unwrap();
            if !o.contains_key(key) {
                errs.push(format!("{at}: missing field `{key}`"));
            }
        }
        if let Some(max) = s.get("maxProperties").and_then(Value::as_u64) {
            if o.len() as u64 > max {
                errs.push(format!("{at}: {} properties, at most {max}", o.len()));
            }
        }
        if let Some(p) = s.get("propertyNames").and_then(|n| n["pattern"].as_str()) {
            for k in o.keys().filter(|k| !pattern_ok(p, k)) {
                errs.push(format!("{at}: the name `{k}` does not match {p}"));
            }
        }
        let props = s.get("properties").and_then(Value::as_object);
        for (k, x) in o {
            let here = format!("{at}.{k}");
            match (props.and_then(|p| p.get(k)), s.get("additionalProperties")) {
                (Some(schema), _) => self.walk(schema, x, &here, errs),
                (None, Some(Value::Bool(false))) => errs.push(format!("{here}: not allowed")),
                (None, Some(extra @ Value::Object(_))) => self.walk(extra, x, &here, errs),
                (None, _) if props.is_some() => {
                    errs.push(format!("{here}: not in the schema (herdr would ignore it)"));
                }
                (None, _) => {}
            }
        }
    }
}

const KNOWN: &[&str] = &[
    "$ref",
    "$defs",
    "$schema",
    "oneOf",
    "anyOf",
    "type",
    "const",
    "enum",
    "properties",
    "required",
    "additionalProperties",
    "items",
    "minimum",
    "maximum",
    "maxProperties",
    "propertyNames",
    "description",
    "default",
    "format",
    "title",
];

fn done(errs: Vec<String>) -> Result<(), String> {
    if errs.is_empty() {
        Ok(())
    } else {
        Err(errs.join("; "))
    }
}

fn show(v: &Value) -> String {
    let s = v.to_string();
    if s.len() > 60 {
        format!("{}…", s.chars().take(60).collect::<String>())
    } else {
        s
    }
}

fn is_type(name: &str, v: &Value) -> bool {
    match name {
        "null" => v.is_null(),
        "string" => v.is_string(),
        "boolean" => v.is_boolean(),
        "number" => v.is_number(),
        "integer" => v.is_i64() || v.is_u64(),
        "array" => v.is_array(),
        "object" => v.is_object(),
        other => panic!("unknown schema type `{other}`"),
    }
}

/// An alternative whose `type` or `op` tag is the value's own (the one worth reporting).
fn tag_matches(schema: &Schema, alt: &Value, v: &Value) -> bool {
    let alt = match alt["$ref"].as_str() {
        Some(r) => schema.resolve(r),
        None => alt,
    };
    ["type", "op"]
        .iter()
        .any(|t| !v[*t].is_null() && alt["properties"][*t]["const"] == v[*t])
}

/// The one pattern herdr's token and label names use: `^[A-Za-z0-9_-]{lo,hi}$`. Any other is a panic.
fn pattern_ok(pattern: &str, name: &str) -> bool {
    assert_eq!(
        pattern, "^[A-Za-z0-9_-]{1,32}$",
        "extend pattern_ok for this pattern"
    );
    (1..=32).contains(&name.chars().count())
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    /// The shape that failed live is refused, and herdr's own is accepted.
    #[test]
    fn the_checker_refuses_the_invented_layout_and_takes_the_real_one() {
        let s = Schema::load();
        let invented = json!({"workspace": {"label": "theseus", "create": true}, "focus": false,
            "tabs": [{"label": "board", "focus": false, "panes": [{"argv": ["x"], "tokens": {}}]}]});
        let e = s.check_request("layout.apply", &invented).unwrap_err();
        assert!(e.contains("missing field `root`"), "{e}");
        assert!(e.contains("not in the schema"), "{e}");
        let real = json!({"workspace_id": "w1", "tab_label": "board", "focus": false,
            "root": {"type": "pane", "command": ["theseus", "chat"]}});
        s.check_request("layout.apply", &real).unwrap();
        let bad_node =
            json!({"workspace_id": "w1", "root": {"type": "pane", "command": "theseus"}});
        assert!(s.check_request("layout.apply", &bad_node).is_err());
        assert!(!s.knows("no.such_method"));
        assert!(s.knows("layout.apply"));
        let e = s.check_request("no.such_method", &json!({})).unwrap_err();
        assert!(e.contains("no method"), "{e}");
    }

    /// Results and events are checked too: a missing field, an unknown type, a wrong enum.
    #[test]
    fn the_checker_reads_results_and_events() {
        let s = Schema::load();
        s.check_result(&json!({"type": "ok"})).unwrap();
        let e = s
            .check_result(&json!({"type": "layout_apply", "layout": {}}))
            .unwrap_err();
        assert!(e.contains("missing field"), "{e}");
        assert!(s.check_result(&json!({"type": "invented"})).is_err());
        let exited = json!({"event": "pane_exited",
            "data": {"type": "pane_exited", "pane_id": "w1:p2", "workspace_id": "w1"}});
        s.check_event(&exited).unwrap();
        let wrong = json!({"event": "pane_exited", "data": {"type": "pane_exited"}});
        assert!(s.check_event(&wrong).is_err());
        let name = json!({"event": "pane.exited", "data": exited["data"]});
        assert!(
            s.check_event(&name).is_err(),
            "the event's name has underscores"
        );
    }
}
