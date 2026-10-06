//! A board value printed in the escapes the first decode leaves, or in
//! another language's (theseus-nlvx): JSON inside a JSON string, as kubectl's
//! `last-applied-configuration`, CloudTrail's `requestParameters`, and
//! `json.dumps` of a structure holding a serialized one print it. All values
//! invented.

use super::Scrubber;

/// Each value holds one character a JSON encoder escapes, as
/// `tests_escaped.rs`'s do.
const VALUES: &[(&str, &str)] = &[
    ("Inv\"ented-Quote1", "quote"),
    ("Inv\\ented\\Back2", "backslash"),
    ("Inv\tented-Tab33", "tab"),
    ("Inv\nented-Line4", "newline"),
    ("Inv\u{e9}nt\u{e9}d-Accent5", "accent"),
    ("Inv\u{1F600}nted-Astral6", "astral"),
];

fn knowing() -> Scrubber {
    let mut values: Vec<(String, String)> = VALUES
        .iter()
        .map(|(v, n)| (v.to_string(), n.to_string()))
        .collect();
    values.push(("Inv\\nented-Literal9".into(), "literal".into()));
    Scrubber::with_values(values)
}

fn marker(name: &str) -> String {
    format!("[redacted:{name}]")
}

/// JSON as serde_json prints it, compact or pretty.
fn print(o: &serde_json::Value, pretty: bool) -> String {
    if pretty {
        serde_json::to_string_pretty(o).unwrap()
    } else {
        serde_json::to_string(o).unwrap()
    }
}

/// JSON as Python's `json.dumps` prints it by default: every non-ASCII
/// character a `\u` escape, a surrogate pair past U+FFFF.
fn ascii(o: &serde_json::Value) -> String {
    let mut out = String::new();
    for c in serde_json::to_string(o).unwrap().chars() {
        if c.is_ascii() {
            out.push(c);
        } else {
            let mut units = [0u16; 2];
            for u in c.encode_utf16(&mut units) {
                out.push_str(&format!("\\u{u:04x}"));
            }
        }
    }
    out
}

/// A record holding another, serialized: the value among the inner one's
/// fields.
fn nested(inner: String) -> serde_json::Value {
    serde_json::json!({ "eventName": "PutParameter", "requestParameters": inner, "n": 3 })
}

fn inner(v: &str) -> serde_json::Value {
    serde_json::json!({ "name": "/demo/db", "value": v, "overwrite": true })
}

#[test]
fn a_value_in_json_inside_a_json_string_is_withheld_compact_and_pretty() {
    let s = knowing();
    for (v, name) in VALUES {
        for pretty in [false, true] {
            let text = print(&nested(print(&inner(v), pretty)), pretty);
            let (out, n) = s.scrub(&text);
            let want = print(&nested(print(&inner(&marker(name)), pretty)), pretty);
            assert_eq!((out.as_str(), n), (want.as_str(), 1), "{name}: {text}");
        }
        // ASCII-only at both levels, as `json.dumps(json.dumps(o))` prints it.
        let text = ascii(&nested(ascii(&inner(v))));
        let (out, n) = s.scrub(&text);
        let want = ascii(&nested(ascii(&inner(&marker(name)))));
        assert_eq!((out.as_str(), n), (want.as_str(), 1), "{name}: {text}");
    }
}

/// kubectl keeps what it applied, serialized, in an annotation: a Secret's
/// `stringData` among it. The `data` beside it is the value's base64, which
/// the base64 pass withholds.
#[test]
fn a_secret_in_kubectls_last_applied_annotation_is_withheld() {
    use base64::Engine as _;
    let s = knowing();
    let secret = |v: &str, data: &str| {
        let applied = serde_json::json!({
            "apiVersion": "v1", "kind": "Secret",
            "metadata": { "name": "invented-db", "namespace": "demo" },
            "stringData": { "password": v }, "type": "Opaque",
        });
        serde_json::json!({
            "apiVersion": "v1", "kind": "Secret",
            "data": { "password": data },
            "metadata": {
                "annotations": {
                    "kubectl.kubernetes.io/last-applied-configuration":
                        format!("{}\n", serde_json::to_string(&applied).unwrap()),
                },
                "name": "invented-db", "namespace": "demo",
            },
            "type": "Opaque",
        })
    };
    for (v, name) in VALUES {
        let data = base64::engine::general_purpose::STANDARD.encode(v);
        let text = print(&secret(v, &data), true);
        let (out, n) = s.scrub(&text);
        let want = print(&secret(&marker(name), &marker(name)), true);
        assert_eq!((out.as_str(), n), (want.as_str(), 2), "{name}: {text}");
    }
}

/// A value holding a literal backslash and `n` is found in the first decode,
/// where the second would read a line break; twice escaped, in the second.
#[test]
fn a_value_holding_a_backslash_matches_at_its_own_level() {
    let s = knowing();
    for text in [
        r#"{"v": "Inv\\nented-Literal9"}"#,
        r#"{"doc": "{\"v\": \"Inv\\\\nented-Literal9\"}"}"#,
    ] {
        let (out, n) = s.scrub(text);
        assert_eq!(n, 1, "{out}");
        assert!(
            out.contains("[redacted:literal]") && !out.contains("Literal9"),
            "{out}"
        );
    }
    // Escapes nested with no value in them change nothing.
    for kept in [
        r#"{"doc": "{\"path\": \"C:\\\\Users\\\\inventor\", \"msg\": \"caf\\u00e9\\n\"}"}"#,
        r#"{"doc": "\\\\\\\\ \\u \\ud83d \\"}"#,
    ] {
        assert_eq!(s.scrub(kept), (kept.to_string(), 0), "{kept}");
    }
}
