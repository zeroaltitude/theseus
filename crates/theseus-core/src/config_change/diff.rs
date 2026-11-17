//! What a change really changes: the settings that differ between two parsed
//! configs, each with its value before and after (issue 22).
//!
//! The card and the sensitive check both read this, never the proposal: a
//! table written whole, an array, or a key renamed by a loader all reach the
//! same settings here, and each is shown and classified as the setting it is.

use std::collections::BTreeMap;

use serde_json::Value;

/// One setting that differs: its path, and its value on each side (`None`:
/// not set, so its default).
#[derive(Debug, Clone, PartialEq)]
pub struct Setting {
    pub path: Vec<String>,
    pub before: Option<Value>,
    pub after: Option<Value>,
}

fn flatten(v: &Value, at: &mut Vec<String>, out: &mut BTreeMap<Vec<String>, Value>) {
    match v {
        Value::Object(m) if !m.is_empty() => {
            for (k, v) in m {
                at.push(k.clone());
                flatten(v, at, out);
                at.pop();
            }
        }
        _ if at.is_empty() => {}
        _ => {
            out.insert(at.clone(), v.clone());
        }
    }
}

fn leaves(text: &str) -> Result<BTreeMap<Vec<String>, Value>, String> {
    let doc: toml::Value =
        toml::from_str(text).map_err(|e| format!("the settings could not be read back: {e}"))?;
    let v = serde_json::to_value(doc)
        .map_err(|e| format!("the settings could not be read back: {e}"))?;
    let mut out = BTreeMap::new();
    flatten(&v, &mut Vec::new(), &mut out);
    Ok(out)
}

/// The settings that differ between the settings file's text `before` and
/// `after`, in path order. Each side is the parsed document, so a setting
/// the file does not name is `None` (its default), and one the loader would
/// drop from a default table is still seen.
pub fn between(before: &str, after: &str) -> Result<Vec<Setting>, String> {
    let (b, a) = (leaves(before)?, leaves(after)?);
    let mut paths: Vec<&Vec<String>> = b.keys().chain(a.keys()).collect();
    paths.sort();
    paths.dedup();
    Ok(paths
        .into_iter()
        .filter(|p| b.get(*p) != a.get(*p))
        .map(|p| Setting {
            path: p.clone(),
            before: b.get(p).cloned(),
            after: a.get(p).cloned(),
        })
        .collect())
}

/// The longest a value is shown for.
const SHOWN: usize = 80;

fn clip(s: &str) -> String {
    match s.chars().count() > SHOWN {
        true => format!("{}…", s.chars().take(SHOWN).collect::<String>()),
        false => s.to_string(),
    }
}

/// A value in plain words: on and off for a switch, quotes for text, a short
/// list for a list. Never a diff of text.
pub fn words(v: Option<&Value>) -> String {
    match v {
        None | Some(Value::Null) => "not set (its default)".into(),
        Some(Value::Bool(true)) => "on".into(),
        Some(Value::Bool(false)) => "off".into(),
        Some(Value::Number(n)) => n.to_string(),
        Some(Value::String(s)) => format!("\"{}\"", clip(s)),
        Some(Value::Array(a)) if a.is_empty() => "an empty list".into(),
        Some(Value::Array(a)) => {
            let items: Vec<String> = a.iter().map(|x| words(Some(x))).collect();
            clip(&format!(
                "a list of {}: {}",
                crate::narrative::count(a.len() as u64, "item", "items"),
                items.join(", ")
            ))
        }
        Some(Value::Object(_)) => "a table".into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn the_settings_that_differ_are_the_ones_that_changed() {
        let a = "narrative = false\n";
        let b = "narrative = true\n[kernel]\nconfirm_ttl_secs = 77\n";
        let d = between(a, b).unwrap();
        let paths: Vec<String> = d.iter().map(|s| s.path.join(".")).collect();
        assert_eq!(paths, ["kernel.confirm_ttl_secs", "narrative"], "{d:?}");
        let n = d.iter().find(|s| s.path == ["narrative"]).unwrap();
        assert_eq!(
            (&n.before, &n.after),
            (&Some(json!(false)), &Some(json!(true)))
        );
    }

    #[test]
    fn no_difference_is_none() {
        assert!(between("narrative = false\n", "narrative = false\n")
            .unwrap()
            .is_empty());
    }

    /// A setting turned to its own default is still a change: the loader
    /// leaves a default table out of what it serializes, the file does not.
    #[test]
    fn a_value_turned_to_its_default_is_still_seen() {
        let d = between(
            "[import.notes]\nenabled = true\n",
            "[import.notes]\nenabled = false\n",
        )
        .unwrap();
        assert_eq!(d.len(), 1, "{d:?}");
        assert_eq!(
            (&d[0].before, &d[0].after),
            (&Some(json!(true)), &Some(json!(false)))
        );
    }

    #[test]
    fn values_are_worded_for_a_person() {
        assert_eq!(words(Some(&json!(true))), "on");
        assert_eq!(words(Some(&json!(false))), "off");
        assert_eq!(words(Some(&json!(300))), "300");
        assert_eq!(words(Some(&json!("x"))), "\"x\"");
        assert_eq!(words(None), "not set (its default)");
        assert_eq!(words(Some(&json!([]))), "an empty list");
        assert!(words(Some(&json!(["a", "b"]))).starts_with("a list of 2 items: \"a\", \"b\""));
        assert!(words(Some(&json!("y".repeat(300)))).chars().count() < 100);
    }
}
