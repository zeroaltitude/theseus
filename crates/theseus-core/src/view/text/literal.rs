//! Data put into framing (a place, a source, a path, a URL, a command, a tool's name) as a visibly quoted,
//! bounded literal: whatever it holds, it cannot open or close a mark, start a line, or hide a character.
//! The body of a text is never passed through here: the person's words and a result's content stay as they
//! are.

use std::fmt::Write as _;

/// `s` between double quotes, cut at `max` characters with `…` after the closing quote. A quote and a
/// backslash are escaped; a control, format, bidi or zero-width character, any whitespace but the space,
/// and every bracket or quote look-alike are shown as `\u{…}`, so what is in the bytes is what is seen.
pub fn literal(s: &str, max: usize) -> String {
    let mut out = String::from('"');
    let mut chars = s.chars();
    for c in chars.by_ref().take(max) {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            c if hidden(c) || look_alike(c) => {
                let _ = write!(out, "\\u{{{:x}}}", c as u32);
            }
            c => out.push(c),
        }
    }
    out.push('"');
    if chars.next().is_some() {
        out.push('…');
    }
    out
}

/// Whether [`literal`] shows `c` otherwise than as itself (escaped, or as `\u{…}`): a cut before such a
/// rewrite stops before any known value holding it (`scrub::Cuts::unchanged`).
pub fn rewritten(c: char) -> bool {
    matches!(c, '"' | '\\' | '\n') || hidden(c) || look_alike(c)
}

/// A tool's name as framing: as it is when it is a plain name (letters, digits, `_`, `.`, `-`; at most 64),
/// else as a [`literal`].
pub fn name(n: &str) -> String {
    let plain = !n.is_empty()
        && n.len() <= 64
        && n.chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '-'));
    match plain {
        true => n.to_string(),
        false => literal(n, 64),
    }
}

/// A character that is not seen as itself: a control, any whitespace but the space, or a format character
/// (Unicode's Cf: soft hyphen, the bidi marks and isolates, the zero-width ones, the byte-order mark, tags).
fn hidden(c: char) -> bool {
    c.is_control()
        || (c.is_whitespace() && c != ' ')
        || matches!(c as u32,
            0xAD | 0x600..=0x605 | 0x61C | 0x6DD | 0x70F | 0x890..=0x891 | 0x8E2 | 0x180E
            | 0x200B..=0x200F | 0x202A..=0x202E | 0x2060..=0x2064 | 0x2066..=0x206F | 0xFEFF
            | 0xFFF9..=0xFFFB | 0x110BD | 0x110CD | 0x13430..=0x1343F | 0x1BCA0..=0x1BCA3
            | 0x1D173..=0x1D17A | 0xE0001 | 0xE0020..=0xE007F)
}

/// A bracket, or a character drawn like one or like a quote: what a mark is made of.
fn look_alike(c: char) -> bool {
    matches!(
        c,
        '[' | ']'
            | '［'
            | '］'
            | '⟦'
            | '⟧'
            | '【'
            | '】'
            | '〔'
            | '〕'
            | '〖'
            | '〗'
            | '〘'
            | '〙'
            | '〚'
            | '〛'
            | '⁅'
            | '⁆'
            | '﹇'
            | '﹈'
            | '⦋'
            | '⦌'
            | '⦍'
            | '⦎'
            | '⦏'
            | '⦐'
            | '⎡'
            | '⎤'
            | '⎣'
            | '⎦'
            | '⟨'
            | '⟩'
            | '〈'
            | '〉'
            | '「'
            | '」'
            | '『'
            | '』'
            | '“'
            | '”'
            | '„'
            | '‟'
            | '＂'
            | '«'
            | '»'
    )
}
