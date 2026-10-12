//! The shapes of secrets in the encodings a board value is looked for in
//! (theseus-oyrt): a shape the verbatim pass could not see because an
//! encoding hid it. Each decoding is read for every shape, and a match is
//! mapped back to the text it came from:
//!
//! - base64, standard or URL-safe, wrapped across lines or not, at any of the
//!   three offsets a secret can start at inside it (a Kubernetes secret's
//!   `data`, a Basic `Authorization` header): each run of the text, decoded,
//!   and a match withholds the run's lines it touches, as a value's does;
//! - percent-encoding, wholly or in part (`%2F` or `%2f`);
//! - JSON's, YAML's and Python repr's escapes, once or twice, and YAML's
//!   folded line break (`escaped.rs`), with base64 inside them read too.
//!
//! Every pass runs only where its encoding could be: a text with no `%`
//! followed by a hex digit, no backslash, or no run of base64 long enough to
//! hold a shape costs one scan.

use base64::Engine as _;

use super::{escaped, shapes, Span};

/// The shortest base64 run read for a shape: 15 bytes' worth, shorter than
/// any shape's shortest form.
const MIN_RUN: usize = 20;

/// The shortest stretch of a run's decoded bytes that is printable text and
/// so read for a shape.
const MIN_TEXT: usize = 12;

/// Standard base64 with no padding, taking the bits a cut run's last
/// character holds past its last whole byte.
const LENIENT: base64::engine::GeneralPurpose = base64::engine::GeneralPurpose::new(
    &base64::alphabet::STANDARD,
    base64::engine::GeneralPurposeConfig::new()
        .with_decode_allow_trailing_bits(true)
        .with_decode_padding_mode(base64::engine::DecodePaddingMode::RequireNone),
);

/// Every shape the text's decodings hold, as byte ranges of the text.
pub(super) fn spans(text: &str) -> Vec<Span> {
    let mut spans = in_base64(text, shapes::all);
    spans.extend(escaped::found(text, |t| {
        let mut found = shapes::all(t);
        found.extend(in_base64(t, shapes::all));
        found
    }));
    spans.extend(in_percent(text, |t| {
        let mut found = shapes::all(t);
        found.extend(in_base64(t, shapes::all));
        found
    }));
    spans
}

/// What `find` finds in each base64 run of the text once decoded, at each of
/// the four places an encoding could have begun, as the byte range of the
/// run's lines a match touches, with its stand-in. A run whose lines are not
/// all one width is read cut at each change of width too: the run may have
/// joined the end of an encoding to a word on the line after it, whose
/// letters decode to text that would glue itself to a shape.
fn in_base64(text: &str, find: fn(&str) -> Vec<Span>) -> Vec<Span> {
    let mut spans = Vec::new();
    let (mut joined, mut starts, mut bytes) = (Vec::new(), Vec::new(), Vec::new());
    let mut cuts = Vec::new();
    for lines in super::base64_runs(text).iter() {
        let len: usize = lines.iter().map(ExactSizeIterator::len).sum();
        if len < MIN_RUN
            || !lines
                .iter()
                .any(|l| has_capital(&text.as_bytes()[l.clone()]))
        {
            continue;
        }
        // The run without its line breaks, in the standard alphabet, and
        // where each line starts in it.
        joined.clear();
        starts.clear();
        for l in lines {
            starts.push(joined.len());
            joined.extend(text.as_bytes()[l.clone()].iter().map(|c| match c {
                b'-' => b'+',
                b'_' => b'/',
                c => *c,
            }));
        }
        let line_of = |at: usize| &lines[starts.partition_point(|s| *s <= at) - 1];
        // Where to cut: either side of a change of width.
        let width = lines[0].len();
        cuts.clear();
        for (k, l) in lines.iter().enumerate().skip(1) {
            if l.len() != width {
                cuts.extend([starts[k], starts[k] + l.len()]);
            }
        }
        let mut whole = &joined[..];
        while let Some((b'=', rest)) = whole.split_last() {
            whole = rest;
        }
        for skip in 0..4 {
            let Some(chars) = whole.get(skip..) else {
                break;
            };
            // A last character alone holds no whole byte.
            let chars = &chars[..chars.len() - usize::from(chars.len() % 4 == 1)];
            bytes.clear();
            if LENIENT.decode_vec(chars, &mut bytes).is_err() {
                continue;
            }
            // A byte depends on the characters before it alone, so the bytes
            // before a cut are what the run cut there decodes to.
            let ends: Vec<usize> = cuts
                .iter()
                .filter(|c| **c > skip)
                .map(|c| (c - skip) * 6 / 8)
                .collect();
            for (from, text) in printable(&bytes) {
                let mut found = find(text);
                for &e in &ends {
                    if e > from && e < from + text.len() && text.is_char_boundary(e - from) {
                        found.extend(find(&text[..e - from]));
                    }
                }
                for (a, b, with) in found {
                    // Byte k of the decoding is bits 8k to 8k+8 of the
                    // characters after `skip`, character j bits 6j to 6j+6.
                    let first = skip + 8 * (from + a) / 6;
                    let last = skip + (8 * (from + b) - 1) / 6;
                    spans.push((line_of(first).start, line_of(last).end, with));
                }
            }
        }
    }
    spans
}

/// Whether a run holds a capital, as the base64 of any text that holds a
/// shape does: every fourth character is the top six bits of a byte, and for
/// a byte below `h` (a digit, a capital, punctuation, `a` to `g`) that is a
/// capital. A run of small letters and digits alone (a hex hash, a UUID, a
/// name in snake case) is not decoded.
fn has_capital(run: &[u8]) -> bool {
    run.iter().any(u8::is_ascii_uppercase)
}

/// The stretches of `bytes` that are text (printable ASCII, tabs, line
/// breaks, and whole UTF-8 characters past ASCII), long enough to hold a
/// shape, each with where it starts. One pass, a character at a time.
fn printable(bytes: &[u8]) -> Vec<(usize, &str)> {
    let mut out = Vec::new();
    let (mut from, mut i) = (0, 0);
    while i <= bytes.len() {
        let len = bytes.get(i).map_or(0, |c| match c {
            b' '..=b'~' | b'\t' | b'\n' | b'\r' => 1,
            0xC2..=0xF4 => {
                let n = match c {
                    0xC2..=0xDF => 2,
                    0xE0..=0xEF => 3,
                    _ => 4,
                };
                let whole = bytes
                    .get(i..i + n)
                    .is_some_and(|s| std::str::from_utf8(s).is_ok());
                if whole {
                    n
                } else {
                    0
                }
            }
            _ => 0,
        });
        if len > 0 {
            i += len;
            continue;
        }
        if i - from >= MIN_TEXT {
            if let Ok(s) = std::str::from_utf8(&bytes[from..i]) {
                out.push((from, s));
            }
        }
        i += 1;
        from = i;
    }
    out
}

/// What `find` finds in the text with each `%XX` of an ASCII character
/// decoded, as byte ranges of the text. A `%` that starts no such escape, and
/// an escape of a byte past ASCII, are kept as they are: no shape holds one.
fn in_percent(text: &str, find: impl Fn(&str) -> Vec<Span>) -> Vec<Span> {
    let b = text.as_bytes();
    let hex = |c: Option<&u8>| c.and_then(|c| (*c as char).to_digit(16));
    let mut decoded = String::new();
    // (where an escape starts in the text, where its character is in the
    // decoded text)
    let mut escapes: Vec<(usize, usize)> = Vec::new();
    let mut copied = 0;
    let mut i = 0;
    while let Some(rel) = text[i..].find('%') {
        let at = i + rel;
        i = at + 1;
        let (Some(h), Some(l)) = (hex(b.get(at + 1)), hex(b.get(at + 2))) else {
            continue;
        };
        if h >= 8 {
            continue;
        }
        decoded.push_str(&text[copied..at]);
        escapes.push((at, decoded.len()));
        decoded.push(char::from((h * 16 + l) as u8));
        copied = at + 3;
        i = copied;
    }
    if escapes.is_empty() {
        return Vec::new();
    }
    decoded.push_str(&text[copied..]);
    // A decoded offset as the text's: each escape before it is two bytes
    // longer in the text; a range ending just past an escape's character
    // ends past its third byte.
    let back = |p: usize, end: bool| {
        let k = escapes.partition_point(|e| if end { e.1 < p } else { e.1 <= p });
        match k.checked_sub(1) {
            None => p,
            Some(k) if !end && escapes[k].1 == p => escapes[k].0,
            Some(k) => escapes[k].0 + 3 + (p - escapes[k].1 - 1),
        }
    };
    // Only the lines of the text an escape is on can hold what the text did
    // not; each as the decoded text has it. A decoded offset is the text's,
    // less two for each escape before it.
    let forward = |p: usize| p - 2 * escapes.partition_point(|e| e.0 < p);
    let mut spans = Vec::new();
    let mut read_to = 0;
    for &(src, _) in &escapes {
        if src < read_to {
            continue;
        }
        let from = text[..src].rfind('\n').map_or(0, |p| p + 1);
        let to = text[src..].find('\n').map_or(text.len(), |p| src + p);
        let (dec_from, dec_to) = (forward(from), forward(to));
        for (a, b, with) in find(&decoded[dec_from..dec_to]) {
            spans.push((back(dec_from + a, false), back(dec_from + b, true), with));
        }
        read_to = to;
    }
    spans
}
