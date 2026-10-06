//! A board value printed JSON-escaped is withheld (theseus-ubp7): by
//! serde_json, compact and pretty, and as Python's ASCII-only `json.dumps`
//! writes it, in either case of hex. All values invented.

use super::Scrubber;

/// Each value holds one character a JSON encoder escapes.
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
    values.push(("Inv/ented/Slash7".into(), "slash".into()));
    Scrubber::with_values(values)
}

fn marker(name: &str) -> String {
    format!("[redacted:{name}]")
}

/// An object with the value among other fields, as a tool prints it.
fn object(v: &str) -> serde_json::Value {
    serde_json::json!({ "before": "plain words", "secret": v, "list": [1, "two"], "z": null })
}

#[test]
fn a_value_serde_json_prints_is_withheld_compact_and_pretty() {
    let s = knowing();
    for (v, name) in VALUES {
        for pretty in [false, true] {
            let print = |o: &serde_json::Value| {
                if pretty {
                    serde_json::to_string_pretty(o).unwrap()
                } else {
                    serde_json::to_string(o).unwrap()
                }
            };
            let text = print(&object(v));
            let (out, n) = s.scrub(&text);
            assert_eq!(
                (out.as_str(), n),
                (print(&object(&marker(name))).as_str(), 1),
                "{name}, pretty {pretty}: {text}"
            );
            let back: serde_json::Value = serde_json::from_str(&out).unwrap();
            assert_eq!(
                back["secret"],
                marker(name),
                "the JSON around it stays whole"
            );
        }
    }
}

/// Python's `json.dumps(o)` (ASCII-only by default), written out by hand:
/// lowercase hex as Python writes it, and uppercase as another encoder may.
#[test]
fn a_value_python_prints_ascii_only_is_withheld_in_either_case_of_hex() {
    let s = knowing();
    let printed: &[(&str, &str)] = &[
        (r#"Inv\"ented-Quote1"#, "quote"),
        (r"Inv\\ented\\Back2", "backslash"),
        (r"Inv\tented-Tab33", "tab"),
        (r"Inv\nented-Line4", "newline"),
        (r"Inv\u00e9nt\u00e9d-Accent5", "accent"),
        (r"Inv\u00E9nt\u00E9d-Accent5", "accent"),
        (r"Inv\ud83d\ude00nted-Astral6", "astral"),
        (r"Inv\uD83D\uDE00nted-Astral6", "astral"),
        // Any character may be a `\u` escape: these as an encoder that
        // escapes more than it must writes them.
        (r"Inv\u0022ented-Quote1", "quote"),
        (r"Inv\u005Cented\u005cBack2", "backslash"),
        (r"Inv\u0009ented-Tab33", "tab"),
        (r"Inv\u000Aented-Line4", "newline"),
        (r"\u0049nv\u00e9nt\u00e9d-Accent\u0035", "accent"),
    ];
    for (escaped, name) in printed {
        let text = format!(r#"{{"before": "caf\u00e9", "secret": "{escaped}", "n": 3}}"#);
        let (out, n) = s.scrub(&text);
        assert_eq!(
            (out, n),
            (
                format!(
                    r#"{{"before": "caf\u00e9", "secret": "{}", "n": 3}}"#,
                    marker(name)
                ),
                1
            ),
            "{escaped}"
        );
    }
}

/// PHP and some JavaScript libraries write `/` as `\/`; Go writes `<`, `>`,
/// and `&` as `\u` escapes.
#[test]
fn a_value_with_an_escaped_slash_or_gos_escapes_is_withheld() {
    let s = knowing();
    let (out, n) = s.scrub(r#"{"url":"https:\/\/x.example\/","token":"Inv\/ented\/Slash7"}"#);
    assert_eq!(
        (out.as_str(), n),
        (
            r#"{"url":"https:\/\/x.example\/","token":"[redacted:slash]"}"#,
            1
        )
    );
    let go = Scrubber::with_values(vec![("Inv<ented>&Go8".into(), "go".into())]);
    let (out, n) = go.scrub(r#"{"v":"Inv\u003cented\u003e\u0026Go8"}"#);
    assert_eq!((out.as_str(), n), (r#"{"v":"[redacted:go]"}"#, 1));
}

/// Several escaped occurrences and a verbatim one are each counted, and the
/// escapes around them that hold no value are left as they were.
#[test]
fn every_occurrence_is_counted_and_nothing_else_changes() {
    let s = knowing();
    let text = concat!(
        r#"{"path": "C:\\Users\\inventor\\new", "a": "Inv\"ented-Quote1", "#,
        r#""b": "Inv\u0022ented-Quote1\n", "lone": "\ud800 and \uzzzz and \"}"#,
        "\nraw: Inv\"ented-Quote1\n",
    );
    let (out, n) = s.scrub(text);
    assert_eq!(n, 3, "{out}");
    assert_eq!(
        out,
        concat!(
            r#"{"path": "C:\\Users\\inventor\\new", "a": "[redacted:quote]", "#,
            r#""b": "[redacted:quote]\n", "lone": "\ud800 and \uzzzz and \"}"#,
            "\nraw: [redacted:quote]\n",
        )
    );
    // Backslashes and escapes with no value in them change nothing.
    for kept in [
        r#"{"msg": "line\nnext\ttab \u00e9 \ud83d\ude00 \\ \/ \" end\"}"#,
        r"trailing \",
        r"\u12",
        r"\ud83d\u",
    ] {
        assert_eq!(s.scrub(kept), (kept.to_string(), 0), "{kept}");
    }
}

/// Rust's `{:?}` of a string shares JSON's escapes for a quote, a backslash,
/// a tab, a newline, and a carriage return, and prints a printable non-ASCII
/// character as it is, so it is caught with no rule of its own. Its `\0` and
/// `\u{…}` (other control characters) are not JSON's, and not caught.
#[test]
fn rusts_debug_form_is_caught_where_it_shares_jsons_escapes() {
    let s = knowing();
    for (v, name) in VALUES {
        let printed = format!("Config {{ token: {v:?}, port: 8080 }}");
        let (out, n) = s.scrub(&printed);
        assert_eq!(
            (out, n),
            (
                format!("Config {{ token: \"{}\", port: 8080 }}", marker(name)),
                1
            ),
            "{printed}"
        );
    }
}

/// What `scrub` costs on 64 KB outputs with ten board values, three of which
/// change when escaped (theseus-ubp7): plain text, pretty JSON, ~7,800
/// escapes, a few escapes, JSON inside a JSON string, and base64 wrapped by
/// `\n` escapes (theseus-nlvx, theseus-cjyt). Run in a release build:
/// `cargo test --profile release-thin -p theseus-core --lib scrub_cost -- --ignored --nocapture`.
#[test]
#[ignore = "a timing, run by hand in a release build"]
fn scrub_cost() {
    const SIZE: usize = 64 * 1024;
    const CALLS: u32 = 2_000;
    let mut values: Vec<(String, String)> = (0..7)
        .map(|i| {
            (
                format!("inventedToken{i}ABCDEFGHJKLMNPQRSTUVWX{i}"),
                format!("plain{i}"),
            )
        })
        .collect();
    values.push(("Inv\"ented-Quote1".into(), "quote".into()));
    values.push(("Inv\\ented\\Back2".into(), "backslash".into()));
    values.push(("Inv\u{e9}nt\u{e9}d-Accent5".into(), "accent".into()));
    let s = Scrubber::with_values(values);
    let fill = |piece: &dyn Fn(usize) -> String| {
        let mut t = String::with_capacity(SIZE + 256);
        let mut i = 0;
        while t.len() < SIZE {
            t.push_str(&piece(i));
            i += 1;
        }
        t
    };
    let plain =
        fill(&|i| format!("line {i}: the quick brown fox jumps over the lazy dog, again\n"));
    let pretty = {
        let rows: Vec<serde_json::Value> = (0..700)
            .map(|i| {
                serde_json::json!({
                    "id": i, "name": format!("item-{i}"), "path": format!("/srv/data/{i}/file.txt"),
                    "tags": ["alpha", "bravo"], "ok": true,
                })
            })
            .collect();
        let mut t = serde_json::to_string_pretty(&rows).unwrap();
        t.truncate(t[..SIZE.min(t.len())].rfind('\n').unwrap());
        t
    };
    let slashed = fill(&|i| {
        format!(
            "{{\"path\": \"C:\\\\Users\\\\inventor\\\\{i}\", \"msg\": \"caf\\u00e9\\n\\t\\\"ok\\\"\"}}\n"
        )
    });
    // A few escapes in plain text: one quoted line every 8 KB or so.
    let few = fill(&|i| {
        if i % 128 == 0 {
            format!("line {i}: {{\"msg\": \"a \\\"quoted\\\" word\\tand a tab\"}}\n")
        } else {
            format!("line {i}: the quick brown fox jumps over the lazy dog, again\n")
        }
    });
    // JSON inside a JSON string: every escape is left after the first decode.
    let twice = fill(&|i| {
        let inner = serde_json::json!({
            "path": format!("C:\\Users\\inventor\\{i}"), "msg": "caf\u{e9} \"ok\"\n",
        });
        let outer = serde_json::json!({ "doc": serde_json::to_string(&inner).unwrap() });
        format!("{}\n", serde_json::to_string(&outer).unwrap())
    });
    // A file's base64 as GitHub's contents API returns it: `\n` every 60.
    let wrapped = {
        use base64::Engine as _;
        let bytes: Vec<u8> = (0..48 * 1024u32)
            .map(|i| (i.wrapping_mul(2_654_435_761) >> 24) as u8)
            .collect();
        let enc = base64::engine::general_purpose::STANDARD.encode(bytes);
        let lines: Vec<&str> = enc
            .as_bytes()
            .chunks(60)
            .map(|c| std::str::from_utf8(c).unwrap())
            .collect();
        serde_json::json!({ "name": "blob.bin", "content": lines.join("\n") }).to_string()
    };
    assert!(!pretty.contains('\\') && slashed.matches('\\').count() > 5_000);
    for (what, text) in [
        ("plain text", &plain),
        ("pretty JSON", &pretty),
        ("backslashes", &slashed),
        ("a few escapes", &few),
        ("twice escaped", &twice),
        ("base64 by \\n", &wrapped),
    ] {
        let started = std::time::Instant::now();
        for _ in 0..CALLS {
            std::hint::black_box(s.scrub(std::hint::black_box(text)));
        }
        let per = started.elapsed() / CALLS;
        let scan = std::time::Instant::now();
        for _ in 0..CALLS {
            std::hint::black_box(std::hint::black_box(text).contains('\\'));
        }
        let scan = scan.elapsed() / CALLS;
        println!(
            "scrub_cost {what}: {} bytes, {per:?} a call (a scan for '\\\\': {scan:?})",
            text.len()
        );
    }
}
