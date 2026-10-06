//! A value written with its characters JSON-escaped (theseus-ubp7): `\"`,
//! `\\`, `\/`, `\b`, `\f`, `\n`, `\r`, `\t`, and `\u` with four hex digits in
//! either case, a surrogate pair among them. A tool that prints JSON escapes a
//! quote, a backslash, and a control character; Python's ASCII-only mode every
//! non-ASCII character too; Go `<`, `>`, and `&`; PHP `/`. Which characters an
//! encoder escapes, and in which case of hex, varies, so the text is decoded
//! once rather than each value encoded in a few fixed forms: every value is
//! looked for in the decoded text, and a match is mapped back to the escaped
//! text it came from, whole escapes and all. JSON inside a JSON string is
//! decoded twice, and a match in the second text maps back through both
//! (theseus-nlvx). YAML's double-quoted escapes (`\0`, `\a`, `\e`, `\v`, `\N`,
//! `\_`, `\L`, `\P`, `\ `, `\xNN`, `\UNNNNNNNN`) and Python repr's (`\'`, and
//! `\xNN` for a byte of a bytes repr) are decoded beside JSON's.

/// One escape decoded: where it starts and ends in the text, and where its
/// character starts and ends in the decoded text.
struct Escape {
    src: usize,
    src_end: usize,
    dec: usize,
    dec_end: usize,
}

/// How many times the text is decoded: JSON inside a JSON string leaves `\"`
/// and `\\` after the first (theseus-nlvx).
const LEVELS: usize = 2;

/// Where each value appears in `text` once its escapes are decoded, once or
/// twice, and where a needle of its base64 appears in a decoded run that an
/// escape broke, as byte ranges of the text, each with the value's name. The
/// verbatim pass has already taken every match with no escape in it. An
/// output with no backslash costs one scan for it, and a second decode runs
/// only when the first leaves a backslash.
pub(super) fn spans<'a>(
    text: &str,
    values: &[(&str, &'a str)],
    needles: &[(String, &'a str)],
) -> Vec<(usize, usize, &'a str)> {
    if values.is_empty() || !text.contains('\\') {
        return Vec::new();
    }
    // Each level's decoded text and its escapes, which map it back to the
    // level before it (the first, to the text).
    let mut levels: Vec<(String, Vec<Escape>)> = Vec::new();
    let mut out = Vec::new();
    while levels.len() < LEVELS {
        let from = levels.last().map_or(text, |l| l.0.as_str());
        if !levels.is_empty() && !from.contains('\\') {
            break;
        }
        let (decoded, escapes) = decode(from);
        if escapes.is_empty() {
            break;
        }
        levels.push((decoded, escapes));
        let (decoded, escapes) = &levels[levels.len() - 1];
        // A range of this level's text, as a range of the output.
        let back = |a: usize, b: usize| {
            levels
                .iter()
                .rev()
                .fold((a, b), |(a, b), (_, e)| (start(e, a), end(e, b)))
        };
        // Every value in every level: one holding a literal `\n` matches in
        // the first only.
        for (v, name) in values {
            for (at, _) in decoded.match_indices(v) {
                let (a, b) = back(at, at + v.len());
                out.push((a, b, *name));
            }
        }
        // Base64 wrapped by escaped line breaks, or holding PHP's `\/`
        // (theseus-cjyt): only when an escape joins base64 to base64, and
        // then only a run with one of this level's escapes inside it, since
        // the level before read every other run as it is.
        if !escapes.iter().any(|e| joins(decoded.as_bytes(), e)) {
            continue;
        }
        let inside = |run: std::ops::Range<usize>| {
            let k = escapes.partition_point(|e| e.dec < run.start);
            escapes.get(k).is_some_and(|e| e.dec < run.end)
        };
        for (a, b, name) in super::base64_spans(decoded, needles, inside) {
            let (a, b) = back(a, b);
            out.push((a, b, name));
        }
    }
    out
}

/// Whether an escape stands for a line break or a base64 character between
/// base64 characters, as `\n` does in wrapped base64 in a JSON string and `\/`
/// does in PHP's: a run the raw text broke there may go on in the decoded.
fn joins(decoded: &[u8], e: &Escape) -> bool {
    let run = |c: &u8| super::is_base64(*c) || *c == b'=';
    let c = decoded[e.dec];
    (matches!(c, b'\n' | b'\r') || run(&c))
        && e.dec > 0
        && (run(&decoded[e.dec - 1]) || decoded[e.dec - 1] == b'\r')
        && decoded
            .get(e.dec_end)
            .is_some_and(|n| run(n) || matches!(n, b'\n'))
}

/// Whether the byte at `i` is the letter of an escape: after an odd count of
/// backslashes, the last of which starts one.
pub(super) fn is_escape_letter(b: &[u8], i: usize) -> bool {
    if i == 0 || b[i - 1] != b'\\' {
        return false;
    }
    let slashes = b[..i].iter().rev().take_while(|c| **c == b'\\').count();
    slashes % 2 == 1 && escape_at(b, i - 1).is_some()
}

/// The text with each escape decoded, and the escapes, in order. A backslash
/// that starts no escape, and a lone surrogate, are kept as they are.
fn decode(text: &str) -> (String, Vec<Escape>) {
    let b = text.as_bytes();
    let mut decoded = String::with_capacity(text.len());
    let mut escapes = Vec::new();
    let mut copied = 0;
    let mut i = 0;
    while let Some(rel) = text[i..].find('\\') {
        let at = i + rel;
        let Some((c, len)) = escape_at(b, at) else {
            // A backslash that starts no escape is kept, and the scan goes
            // on from the byte after it.
            i = at + 1;
            continue;
        };
        decoded.push_str(&text[copied..at]);
        let dec = decoded.len();
        decoded.push(c);
        escapes.push(Escape {
            src: at,
            src_end: at + len,
            dec,
            dec_end: decoded.len(),
        });
        copied = at + len;
        i = copied;
    }
    decoded.push_str(&text[copied..]);
    (decoded, escapes)
}

/// The character the escape at `at` stands for, and its length in the text:
/// JSON's escapes, YAML's double-quoted ones, and Python repr's.
fn escape_at(b: &[u8], at: usize) -> Option<(char, usize)> {
    let c = match *b.get(at + 1)? {
        b'"' => '"',
        b'\\' => '\\',
        b'/' => '/',
        b'b' => '\u{8}',
        b'f' => '\u{c}',
        b'n' => '\n',
        b'r' => '\r',
        b't' => '\t',
        b'u' => {
            let hi = hex(b, at + 2, 4)?;
            if (0xD800..0xDC00).contains(&hi) {
                let lo = (b.get(at + 6) == Some(&b'\\') && b.get(at + 7) == Some(&b'u'))
                    .then(|| hex(b, at + 8, 4))
                    .flatten()
                    .filter(|lo| (0xDC00..0xE000).contains(lo))?;
                let c = 0x10000 + ((hi - 0xD800) << 10) + (lo - 0xDC00);
                return char::from_u32(c).map(|c| (c, 12));
            }
            return char::from_u32(hi).map(|c| (c, 6));
        }
        // YAML's, beside JSON's (PyYAML's double-quoted style).
        b'0' => '\0',
        b'a' => '\u{7}',
        b'e' => '\u{1b}',
        b'v' => '\u{b}',
        b'N' => '\u{85}',
        b'_' => '\u{a0}',
        b'L' => '\u{2028}',
        b'P' => '\u{2029}',
        b' ' => ' ',
        b'\t' => '\t',
        b'U' => return char::from_u32(hex(b, at + 2, 8)?).map(|c| (c, 10)),
        b'x' => return hex_byte(b, at),
        // Python repr's, for a value holding both quotes.
        b'\'' => '\'',
        _ => return None,
    };
    Some((c, 2))
}

/// A `\xNN` escape and those after it. A bytes repr writes each byte of a
/// UTF-8 character as one, so a run that is a whole character's UTF-8 is
/// read as that character; any other `\xNN` is the character U+00NN, as
/// YAML's and a string repr's are.
fn hex_byte(b: &[u8], at: usize) -> Option<(char, usize)> {
    let first = hex(b, at + 2, 2)?;
    let width = match first {
        0xC2..=0xDF => 2,
        0xE0..=0xEF => 3,
        0xF0..=0xF4 => 4,
        _ => 1,
    };
    // Each escape of the run is 4 bytes: `\x` and two hex digits.
    let mut bytes = [0u8; 4];
    let mut got = 0;
    for (k, byte) in bytes.iter_mut().enumerate().take(width) {
        let e = at + 4 * k;
        let Some(h) = (b.get(e) == Some(&b'\\') && b.get(e + 1) == Some(&b'x'))
            .then(|| hex(b, e + 2, 2))
            .flatten()
        else {
            break;
        };
        *byte = h as u8;
        got += 1;
    }
    if width > 1 && got == width {
        if let Some(c) = std::str::from_utf8(&bytes[..width])
            .ok()
            .and_then(|s| s.chars().next())
        {
            return Some((c, 4 * width));
        }
    }
    char::from_u32(first).map(|c| (c, 4))
}

/// `n` hex digits at `at`, in either case.
fn hex(b: &[u8], at: usize, n: usize) -> Option<u32> {
    let digits = b.get(at..at + n)?;
    digits
        .iter()
        .try_fold(0u32, |n, d| (*d as char).to_digit(16).map(|d| n * 16 + d))
}

/// The text's offset for a match starting at `p` in the decoded text: an
/// escape's start, or a byte copied as it was.
fn start(escapes: &[Escape], p: usize) -> usize {
    match escapes.partition_point(|e| e.dec <= p).checked_sub(1) {
        None => p,
        Some(k) if escapes[k].dec == p => escapes[k].src,
        Some(k) => escapes[k].src_end + (p - escapes[k].dec_end),
    }
}

/// The text's offset for a match ending at `p` in the decoded text: an
/// escape's end, or a byte copied as it was. A match never ends inside an
/// escape's character, since values and the text are whole characters.
fn end(escapes: &[Escape], p: usize) -> usize {
    match escapes.partition_point(|e| e.dec < p).checked_sub(1) {
        None => p,
        Some(k) => escapes[k].src_end + (p - escapes[k].dec_end),
    }
}
