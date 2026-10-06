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
//! (theseus-nlvx).

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
/// twice, as byte ranges of the text, each with the value's name. The
/// verbatim pass has already taken every match with no escape in it. An
/// output with no backslash costs one scan for it, and a second decode runs
/// only when the first leaves a backslash.
pub(super) fn spans<'a>(text: &str, values: &[(&str, &'a str)]) -> Vec<(usize, usize, &'a str)> {
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
        let decoded = &levels[levels.len() - 1].0;
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
    }
    out
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

/// The character the escape at `at` stands for, and its length in the text.
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
            let hi = hex4(b, at + 2)?;
            if (0xD800..0xDC00).contains(&hi) {
                let lo = (b.get(at + 6) == Some(&b'\\') && b.get(at + 7) == Some(&b'u'))
                    .then(|| hex4(b, at + 8))
                    .flatten()
                    .filter(|lo| (0xDC00..0xE000).contains(lo))?;
                let c = 0x10000 + ((hi - 0xD800) << 10) + (lo - 0xDC00);
                return char::from_u32(c).map(|c| (c, 12));
            }
            return char::from_u32(hi).map(|c| (c, 6));
        }
        _ => return None,
    };
    Some((c, 2))
}

fn hex4(b: &[u8], at: usize) -> Option<u32> {
    let digits = b.get(at..at + 4)?;
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
