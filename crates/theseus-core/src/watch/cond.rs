//! A watch's condition and what it reads (theseus-f2r2b): pure, so every
//! operator is a table of cases.
//!
//! `until` is one operator and its argument: `exit 0` (a command's status;
//! `exit` alone is 0), `equals X`, `contains X`, `matches RE`, `changes` (the
//! value differs from the first one seen), `above N`, `below N`, and
//! `exists` (a file is there; any other value is not empty). A check's value
//! is its output with the ends trimmed, or, with a JSON path, what the path
//! names (`.state`, `.checks[0].status`, `$.a.b`).

use serde_json::Value;

/// What a watch waits for.
#[derive(Debug, Clone, PartialEq)]
pub enum Condition {
    Exit(i32),
    Equals(String),
    Contains(String),
    Matches(String),
    Changes,
    Above(f64),
    Below(f64),
    Exists,
}

/// What one check saw.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Observed {
    /// The value, trimmed (and the JSON path applied).
    pub value: String,
    /// A command's exit status.
    pub code: Option<i32>,
    /// A file is there.
    pub exists: bool,
}

/// `until` as a condition, or why it is not one.
pub fn parse(until: &str) -> Result<Condition, String> {
    let t = until.trim();
    let (op, arg) = match t.split_once(char::is_whitespace) {
        Some((op, rest)) => (op, rest.trim()),
        None => (t, ""),
    };
    let arg = unquote(arg);
    let need = |what: &str| -> Result<String, String> {
        if arg.is_empty() {
            Err(format!("`{op}` needs {what}: `{op} <{what}>`"))
        } else {
            Ok(arg.clone())
        }
    };
    let number = |what: &str| -> Result<f64, String> {
        need(what)?
            .parse::<f64>()
            .map_err(|_| format!("`{op}` needs a number, not `{arg}`"))
    };
    match op.to_ascii_lowercase().as_str() {
        "exit" => match arg.as_str() {
            "" => Ok(Condition::Exit(0)),
            a => a
                .parse()
                .map(Condition::Exit)
                .map_err(|_| format!("`exit` needs a status, not `{a}`")),
        },
        "equals" | "=" | "==" => Ok(Condition::Equals(need("a value")?)),
        "contains" => Ok(Condition::Contains(need("some text")?)),
        "matches" => {
            let re = need("a pattern")?;
            theseus_tools::rx::is_match(&re, "")?;
            Ok(Condition::Matches(re))
        }
        "changes" | "changed" => Ok(Condition::Changes),
        "above" | ">" => Ok(Condition::Above(number("a number")?)),
        "below" | "<" => Ok(Condition::Below(number("a number")?)),
        "exists" => Ok(Condition::Exists),
        _ => Err(format!(
            "`{until}` is not a condition: say `exit 0`, `equals X`, `contains X`, `matches RE`, \
             `changes`, `above N`, `below N`, or `exists`"
        )),
    }
}

fn unquote(s: &str) -> String {
    let b = s.as_bytes();
    if b.len() >= 2 && (b[0] == b'"' || b[0] == b'\'') && b[b.len() - 1] == b[0] {
        s[1..s.len() - 1].to_string()
    } else {
        s.to_string()
    }
}

impl Condition {
    /// Whether `seen` meets it. `first` is the first value the watch ever
    /// saw: `changes` compares with it, and holds only once there is one.
    pub fn met(&self, seen: &Observed, first: Option<&str>) -> bool {
        let v = seen.value.as_str();
        match self {
            Condition::Exit(c) => seen.code == Some(*c),
            Condition::Equals(x) => v == x,
            Condition::Contains(x) => v.contains(x.as_str()),
            Condition::Matches(re) => theseus_tools::rx::is_match(re, v).unwrap_or(false),
            Condition::Changes => first.is_some_and(|f| f != v),
            Condition::Above(n) => number_of(v).is_some_and(|x| x > *n),
            Condition::Below(n) => number_of(v).is_some_and(|x| x < *n),
            Condition::Exists => seen.exists || (seen.code.is_none() && !v.is_empty()),
        }
    }

    /// Whether a command's non-zero exit is its answer (`exit N`) rather than
    /// a failure to get a value.
    pub fn reads_exit(&self) -> bool {
        matches!(self, Condition::Exit(_))
    }
}

fn number_of(v: &str) -> Option<f64> {
    v.trim().parse::<f64>().ok().filter(|x| x.is_finite())
}

/// What `path` names in `text`, as the watch shows a value: a string as it
/// is, a number or a boolean as written, `null`, or an object or array as
/// compact JSON. The path is `.a.b[0]`, with or without a leading `$`.
pub fn json_value(text: &str, path: &str) -> Result<String, String> {
    let doc: Value = serde_json::from_str(text.trim())
        .map_err(|_| format!("the output is not JSON, so `{path}` names nothing in it"))?;
    let mut cur = &doc;
    let p = path.trim().trim_start_matches('$');
    let mut rest = p;
    while !rest.is_empty() {
        if let Some(r) = rest.strip_prefix('.') {
            let end = r.find(['.', '[']).unwrap_or(r.len());
            let key = &r[..end];
            cur = cur
                .get(key)
                .ok_or_else(|| format!("`{path}` names nothing: there is no `{key}`"))?;
            rest = &r[end..];
        } else if let Some(r) = rest.strip_prefix('[') {
            let end = r
                .find(']')
                .ok_or_else(|| format!("`{path}` has a `[` with no `]`"))?;
            let i: usize = r[..end]
                .trim()
                .parse()
                .map_err(|_| format!("`{path}`: `{}` is not an index", &r[..end]))?;
            cur = cur
                .get(i)
                .ok_or_else(|| format!("`{path}` names nothing: there is no item {i}"))?;
            rest = &r[end + 1..];
        } else {
            return Err(format!(
                "`{path}` is not a path: start with `.`, such as `.state`"
            ));
        }
    }
    Ok(match cur {
        Value::String(s) => s.clone(),
        Value::Null => "null".into(),
        other => other.to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seen(v: &str) -> Observed {
        Observed {
            value: v.into(),
            code: None,
            exists: true,
        }
    }

    /// Every operator, met and not.
    #[test]
    fn each_operator_holds_or_does_not() {
        let t =
            |until: &str, v: &str, first: Option<&str>| parse(until).unwrap().met(&seen(v), first);
        assert!(t("equals MERGED", "MERGED", None));
        assert!(!t("equals MERGED", "OPEN", None));
        assert!(
            t("equals 'two words'", "two words", None),
            "quotes come off"
        );
        assert!(t("contains ready", "build is ready now", None));
        assert!(!t("contains ready", "building", None));
        assert!(t("matches ^v[0-9]+$", "v12", None));
        assert!(!t("matches ^v[0-9]+$", "v12b", None));
        assert!(t("above 5", "5.5", None));
        assert!(!t("above 5", "5", None));
        assert!(!t("above 5", "many", None), "not a number is not above");
        assert!(t("below 0.5", "0.25", None));
        assert!(
            !t("changes", "OPEN", None),
            "the first value is the baseline"
        );
        assert!(!t("changes", "OPEN", Some("OPEN")));
        assert!(t("changes", "MERGED", Some("OPEN")));
        let ok = Observed {
            code: Some(0),
            ..Default::default()
        };
        assert!(parse("exit 0").unwrap().met(&ok, None));
        assert!(parse("exit").unwrap().met(&ok, None), "bare exit is 0");
        assert!(!parse("exit 2").unwrap().met(&ok, None));
        assert!(parse("exists").unwrap().met(&seen(""), None));
        let gone = Observed::default();
        assert!(!parse("exists").unwrap().met(&gone, None));
    }

    #[test]
    fn a_condition_that_is_not_one_says_how_to_write_it() {
        for bad in ["", "soon", "above many", "equals", "matches (", "exit x"] {
            let e = parse(bad).unwrap_err();
            assert!(!e.is_empty(), "{bad:?}");
        }
        assert!(parse("soon").unwrap_err().contains("`equals X`"));
    }

    #[test]
    fn a_json_path_names_a_value() {
        let doc =
            r#"{"state": "OPEN", "n": 3, "ok": true, "checks": [{"status": "pass"}], "x": null}"#;
        assert_eq!(json_value(doc, ".state").unwrap(), "OPEN");
        assert_eq!(json_value(doc, "$.n").unwrap(), "3");
        assert_eq!(json_value(doc, ".ok").unwrap(), "true");
        assert_eq!(json_value(doc, ".checks[0].status").unwrap(), "pass");
        assert_eq!(json_value(doc, ".x").unwrap(), "null");
        assert_eq!(
            json_value(doc, ".checks").unwrap(),
            r#"[{"status":"pass"}]"#
        );
        assert!(json_value(doc, ".nope").unwrap_err().contains("no `nope`"));
        assert!(json_value(doc, ".checks[3]")
            .unwrap_err()
            .contains("no item 3"));
        assert!(json_value("not json", ".a")
            .unwrap_err()
            .contains("not JSON"));
        assert!(json_value(doc, "state")
            .unwrap_err()
            .contains("start with `.`"));
    }
}
