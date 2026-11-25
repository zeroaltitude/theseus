//! A test's oracle for where known values lie in a text: the scrubber's own span finder, verbatim and in each
//! encoding it reads, and those encodings. For a test that checks where a cut, or a stored range, falls.

use base64::Engine as _;

use super::{merged, Substitution};

/// The spans `values` take in `text`, verbatim or encoded, merged.
pub(crate) fn spans_of(text: &str, values: &[&str]) -> Vec<(usize, usize)> {
    let sub = Substitution::new(values.iter().map(|v| (*v, "v")).collect());
    merged(text, sub.spans(text))
        .0
        .into_iter()
        .map(|(a, b, _)| (a, b))
        .collect()
}

/// The names of [`encodings`], in its order.
pub(crate) const ENCODINGS: [&str; 6] = [
    "verbatim",
    "base64",
    "base64 with words around it, wrapped",
    "percent-encoded",
    "a JSON string",
    "a YAML double-quoted string folded over a line break",
];

/// `value` in each way the scrubber reads it: as it is; base64; base64 of it with words around it, wrapped at 40
/// columns; percent-encoded; a JSON string; and a YAML double-quoted string folded over a line break, each half
/// escaped as YAML escapes it (a backslash and a quote mark).
pub(crate) fn encodings(value: &str) -> Vec<String> {
    let b64 = |s: &str| base64::engine::general_purpose::STANDARD.encode(s);
    let wrapped = b64(&format!("q{value}{}", "r".repeat(60)))
        .as_bytes()
        .chunks(40)
        .map(|c| std::str::from_utf8(c).unwrap().to_string())
        .collect::<Vec<_>>()
        .join("\n");
    let percent: String = value.bytes().map(|c| format!("%{c:02X}")).collect();
    let json = serde_json::to_string(value).unwrap();
    let mut mid = value.len() / 2;
    while !value.is_char_boundary(mid) {
        mid -= 1;
    }
    let yaml = |s: &str| s.replace('\\', "\\\\").replace('"', "\\\"");
    let folded = format!("\"{}\\\n    {}\"", yaml(&value[..mid]), yaml(&value[mid..]));
    vec![
        value.to_string(),
        b64(value),
        wrapped,
        percent,
        json,
        folded,
    ]
}
