//! What a cut kept of the text it was given, as the bytes of that text: a cutter that rewrites as it cuts
//! (one line of data, with each run of blanks one space) says where in the ORIGINAL the words it shows begin
//! and end, so that its rewrite of those bytes, plus its own additions (an ellipsis, a mark), is what it showed.
//!
//! A blank here is a control character or whitespace: what such a rewrite makes one space.

use std::ops::Range;

/// Whether `c` is blank: a control character or whitespace.
pub(crate) fn blank(c: char) -> bool {
    c.is_control() || c.is_whitespace()
}

/// `text` as one line: each character as `map` makes it, each run of blank characters one space, nothing
/// trimmed. (`map` never makes a character blank that was not, nor one blank that was.)
pub fn one_line(text: &str, map: impl Fn(char) -> char) -> String {
    let mut out = String::with_capacity(text.len());
    let mut run = false;
    for c in text.chars() {
        if blank(c) {
            run = true;
            continue;
        }
        if run {
            out.push(' ');
            run = false;
        }
        out.push(map(c));
    }
    if run {
        out.push(' ');
    }
    out
}

/// The bytes of `text` whose [`one_line`] is exactly the first `p` bytes of `one_line(text, map)` trimmed:
/// they begin at its first character that is not blank, and end where those `p` bytes end, after one blank
/// character when the head ends in a space. `p` is a character boundary of that line; past its end, the
/// bytes end after the last character that is not blank.
pub fn source(text: &str, p: usize, map: impl Fn(char) -> char) -> Range<usize> {
    let from = text.find(|c: char| !blank(c)).unwrap_or(text.len());
    let mut o = 0usize;
    let mut run: Option<usize> = None;
    for (i, c) in text[from..].char_indices() {
        let i = from + i;
        if blank(c) {
            run.get_or_insert(i);
            continue;
        }
        if let Some(r) = run.take() {
            if o == p {
                return from..r;
            }
            o += 1;
            if o == p {
                let first = text[r..].chars().next().map_or(0, char::len_utf8);
                return from..r + first;
            }
        }
        if o == p {
            return from..i;
        }
        o += map(c).len_utf8();
    }
    from..run.unwrap_or(text.len())
}

#[cfg(test)]
pub(crate) mod sample;

#[cfg(test)]
mod tests;
