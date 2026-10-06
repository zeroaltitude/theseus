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

/// PyYAML's double-quoted style, ASCII-only: a character under U+0100 as
/// `\xNN`, past U+FFFF as `\UNNNNNNNN`, in either case of hex, and YAML's own
/// `\e` for an ESC.
#[test]
fn a_value_in_yamls_escapes_is_withheld() {
    let mut values: Vec<(String, String)> = VALUES
        .iter()
        .map(|(v, n)| (v.to_string(), n.to_string()))
        .collect();
    values.push(("Inv\u{1b}[0mented-Esc10".into(), "esc".into()));
    values.push(("Inv\u{85}ented\u{a0}Yaml11\u{2028}".into(), "yaml".into()));
    let s = Scrubber::with_values(values);
    for (printed, name) in [
        (r"Inv\xE9nt\xE9d-Accent5", "accent"),
        (r"Inv\xe9nt\xe9d-Accent5", "accent"),
        (r"Inv\U0001F600nted-Astral6", "astral"),
        (r"Inv\U0001f600nted-Astral6", "astral"),
        (r"Inv\e[0mented-Esc10", "esc"),
        (r"Inv\x1B[0mented-Esc10", "esc"),
        (r"Inv\Nented\_Yaml11\L", "yaml"),
        (r#"Inv\"ented-Quote1"#, "quote"),
        (r"Inv\x09ented-Tab33", "tab"),
    ] {
        let text =
            format!("db:\n  host: invented.example\n  password: \"{printed}\"\n  port: 5432\n");
        let (out, n) = s.scrub(&text);
        assert_eq!(
            (out, n),
            (
                format!(
                    "db:\n  host: invented.example\n  password: \"{}\"\n  port: 5432\n",
                    marker(name)
                ),
                1
            ),
            "{printed}"
        );
    }
}

/// Python's repr of a string: `\'` in a value holding both quotes, `\xNN`
/// for a control character, `\uNNNN` and `\UNNNNNNNN` for an unprintable
/// one; and of bytes, each byte of a UTF-8 character a `\xNN`.
#[test]
fn a_value_in_pythons_repr_is_withheld() {
    let mut values: Vec<(String, String)> = VALUES
        .iter()
        .map(|(v, n)| (v.to_string(), n.to_string()))
        .collect();
    values.push(("Inv'ent\"ed-Both12".into(), "both".into()));
    values.push(("Inv\u{0}ented\u{7f}Ctl13".into(), "control".into()));
    let s = Scrubber::with_values(values);
    for (printed, name) in [
        // repr(v), and repr(v.encode()), of each.
        (r#"'Inv\'ent"ed-Both12'"#, "both"),
        (r#"b'Inv\'ent"ed-Both12'"#, "both"),
        (r"'Inv\x00ented\x7fCtl13'", "control"),
        (r"'Inv\tented-Tab33'", "tab"),
        (r"b'Inv\xc3\xa9nt\xc3\xa9d-Accent5'", "accent"),
        (r"b'Inv\xC3\xA9nt\xC3\xA9d-Accent5'", "accent"),
        (r"b'Inv\xf0\x9f\x98\x80nted-Astral6'", "astral"),
        (r"b'Inv\\ented\\Back2'", "backslash"),
    ] {
        let text = format!("{{'user': 'inventor', 'token': {printed}, 'n': 3}}");
        let (out, n) = s.scrub(&text);
        let quote = if printed.starts_with('b') { "b'" } else { "'" };
        assert_eq!(
            (out, n),
            (
                format!(
                    "{{'user': 'inventor', 'token': {quote}{}', 'n': 3}}",
                    marker(name)
                ),
                1
            ),
            "{printed}"
        );
    }
    // A run of `\xNN` that is no character's UTF-8 is read byte by byte, as
    // YAML's characters: `\xc3` then `\x28` is `Ã(`.
    let latin = Scrubber::with_values(vec![("Inv\u{c3}(ented14".into(), "latin".into())]);
    let (out, n) = latin.scrub(r"'Inv\xc3\x28ented14' and a cut \xc3\xa and \x and \U0001F6");
    assert_eq!(
        (out.as_str(), n),
        (
            r"'[redacted:latin]' and a cut \xc3\xa and \x and \U0001F6",
            1
        )
    );
}

/// GitHub's contents API returns a file as base64 with a line break every 60
/// characters, and JSON writes each as `\n` (theseus-cjyt): a value's base64
/// inside it, at each of the three offsets, in one line and across a wrap,
/// withheld with the lines it touches. So with `\r\n`, and with PHP's `\/`.
#[test]
fn a_values_base64_wrapped_by_escaped_line_breaks_is_withheld() {
    use base64::Engine as _;
    const PLAIN: &str = "Inv3nted/Value+For~Tests?x=1";
    let s = Scrubber::with_values(vec![
        (PLAIN.into(), "plain".into()),
        (VALUES[0].0.into(), VALUES[0].1.into()),
    ]);
    let file = |content: &str| serde_json::json!({ "name": "notes.txt", "content": content, "encoding": "base64" });
    for (v, name) in [(PLAIN, "plain"), VALUES[0]] {
        // Before it: 3 to 5 bytes, so it sits in the first line; 30 to 32, so
        // it crosses the first wrap; 95 to 97, so it sits in the third line,
        // after an escape. Each set of three is every offset.
        for before in [3, 4, 5, 30, 31, 32, 95, 96, 97] {
            let bytes = format!("{}{v}{}", "x".repeat(before), "y".repeat(70));
            let enc = base64::engine::general_purpose::STANDARD.encode(&bytes);
            let lines: Vec<&str> = enc
                .as_bytes()
                .chunks(60)
                .map(|c| std::str::from_utf8(c).unwrap())
                .collect();
            // The characters that hold the value's own bits, and their lines.
            let first = (8 * before).div_ceil(6) / 60;
            let last = (8 * (before + v.len()) / 6 - 1) / 60;
            let mut kept: Vec<String> = lines[..first].iter().map(|l| l.to_string()).collect();
            kept.push(marker(name));
            kept.extend(lines[last + 1..].iter().map(|l| l.to_string()));
            for (lf, php) in [("\n", false), ("\r\n", false), ("\n", true)] {
                let show = |lines: &[String]| {
                    let t = serde_json::to_string(&file(&format!("{}\n", lines.join(lf)))).unwrap();
                    if php {
                        t.replace('/', "\\/")
                    } else {
                        t
                    }
                };
                let all: Vec<String> = lines.iter().map(|l| l.to_string()).collect();
                let text = show(&all);
                let (out, n) = s.scrub(&text);
                assert_eq!(
                    (out.as_str(), n),
                    (show(&kept).as_str(), 1),
                    "{name}, {before} before, {lf:?}, php {php}: {text}"
                );
            }
        }
    }
}

/// `json.dumps` of `base64.encodebytes`, a line break every 76 characters,
/// and a PEM held in a JSON string, a line break every 64.
#[test]
fn a_values_base64_in_encodebytes_or_a_pem_in_json_is_withheld() {
    use base64::Engine as _;
    let s = Scrubber::with_values(vec![("Inv3nted/Value+For~Tests?x=1".into(), "demo".into())]);
    for width in [76, 64] {
        let enc = base64::engine::general_purpose::STANDARD.encode(format!(
            "{}Inv3nted/Value+For~Tests?x=1{}",
            "x".repeat(120),
            "y".repeat(60)
        ));
        let body: Vec<&str> = enc
            .as_bytes()
            .chunks(width)
            .map(|c| std::str::from_utf8(c).unwrap())
            .collect();
        let text = serde_json::json!({ "cert": format!("-----BEGIN CERTIFICATE-----\n{}\n-----END CERTIFICATE-----\n", body.join("\n")) }).to_string();
        let (out, n) = s.scrub(&text);
        assert_eq!(n, 1, "{out}");
        assert!(
            out.contains("[redacted:demo]") && out.contains(body[0]),
            "{out}"
        );
        assert!(out.contains(body[body.len() - 1]), "{out}");
        let back: serde_json::Value = serde_json::from_str(&out).unwrap();
        assert!(
            back["cert"]
                .as_str()
                .unwrap()
                .contains("\n[redacted:demo]\n"),
            "{out}"
        );
    }
}
