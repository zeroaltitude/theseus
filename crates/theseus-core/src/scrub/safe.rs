//! Cuts of text that is then stored or sent. Each one stops at the edge of a
//! value a secret board knows, verbatim or in any encoding the scrubber
//! reads, so what is kept holds each value whole or not at all, and the mask
//! at an exit, which matches whole values only, still finds it.
//!
//! The plain forms read [`Cuts::live`], the values of every board alive in
//! the process; with none alive a cut falls where it is asked, as a plain
//! cut does. While a value is still loading a cut also falls at whitespace.
//! A caller that already holds a [`Cuts`] passes it to the `_with` form.
//!
//! A unit test that asserts an exact cut position calls the `_with` form
//! with `&Cuts::none()` (or a scrubber's own `cuts()`): a plain form reads
//! every board in the process, and under threaded `cargo test` another
//! test's board with a value still loading moves its cut to whitespace.

use super::Cuts;

/// `text`'s first `max_chars` characters, ending before any known value they
/// would split, and whether anything was cut.
pub fn head(text: &str, max_chars: usize) -> (&str, bool) {
    if text.len() <= max_chars {
        return (text, false);
    }
    head_with(&Cuts::live(), text, max_chars)
}

/// [`head`] with the values `cuts` knows.
pub fn head_with<'t>(cuts: &Cuts, text: &'t str, max_chars: usize) -> (&'t str, bool) {
    cuts.head(text, max_chars)
}

/// [`head`], then `…` when it cut: at most `max_chars + 1` characters.
pub fn head_marked(text: &str, max_chars: usize) -> String {
    marked(head(text, max_chars))
}

/// [`head_marked`] with the values `cuts` knows.
pub fn head_marked_with(cuts: &Cuts, text: &str, max_chars: usize) -> String {
    marked(head_with(cuts, text, max_chars))
}

fn marked((kept, cut): (&str, bool)) -> String {
    if cut {
        format!("{kept}…")
    } else {
        kept.to_string()
    }
}

/// `text`'s last `max_chars` characters, starting after any known value they
/// would split, and whether anything was cut.
pub fn tail(text: &str, max_chars: usize) -> (&str, bool) {
    if text.len() <= max_chars {
        return (text, false);
    }
    tail_with(&Cuts::live(), text, max_chars)
}

/// [`tail`] with the values `cuts` knows.
pub fn tail_with<'t>(cuts: &Cuts, text: &'t str, max_chars: usize) -> (&'t str, bool) {
    let n = text.chars().count();
    if n <= max_chars {
        return (text, false);
    }
    let at = char_at(text, n - max_chars);
    (&text[cuts.after(text, at)..], true)
}

/// `text`'s first `head_chars` and last `tail_chars` characters, the head
/// ending before and the tail starting after any known value either would
/// split, the text read once for both; and whether anything between them was
/// left out. With no more than `head_chars + tail_chars` characters it is
/// `(text, "", false)`.
pub fn ends(text: &str, head_chars: usize, tail_chars: usize) -> (&str, &str, bool) {
    if text.len() <= head_chars.saturating_add(tail_chars) {
        return (text, "", false);
    }
    ends_with(&Cuts::live(), text, head_chars, tail_chars)
}

/// [`ends`] with the values `cuts` knows.
pub fn ends_with<'t>(
    cuts: &Cuts,
    text: &'t str,
    head_chars: usize,
    tail_chars: usize,
) -> (&'t str, &'t str, bool) {
    let n = text.chars().count();
    if n <= head_chars.saturating_add(tail_chars) {
        return (text, "", false);
    }
    let (head, tail) = cuts.around(
        text,
        char_at(text, head_chars),
        char_at(text, n - tail_chars),
    );
    (&text[..head], &text[tail..], true)
}

/// `text`'s head of at most `max_bytes` bytes, ending on a character
/// boundary and before any known value it would split, and whether anything
/// was cut. For a byte budget; it never panics.
pub fn head_bytes(text: &str, max_bytes: usize) -> (&str, bool) {
    if text.len() <= max_bytes {
        return (text, false);
    }
    head_bytes_with(&Cuts::live(), text, max_bytes)
}

/// [`head_bytes`] with the values `cuts` knows.
pub fn head_bytes_with<'t>(cuts: &Cuts, text: &'t str, max_bytes: usize) -> (&'t str, bool) {
    if text.len() <= max_bytes {
        return (text, false);
    }
    (&text[..cuts.before(text, max_bytes)], true)
}

/// The text's first line that is not blank, trimmed: whole when it has at
/// most `max_chars` characters and no known value runs on past its end, else
/// its head of at most `keep_chars` characters ending before any known
/// value, perhaps empty; and whether it was cut.
pub fn first_line(text: &str, max_chars: usize, keep_chars: usize) -> (&str, bool) {
    first_line_with(&Cuts::live(), text, max_chars, keep_chars)
}

/// [`first_line`] with the values `cuts` knows.
pub fn first_line_with<'t>(
    cuts: &Cuts,
    text: &'t str,
    max_chars: usize,
    keep_chars: usize,
) -> (&'t str, bool) {
    cuts.first_line(text, max_chars, keep_chars)
}

/// The byte where `text`'s character number `n` starts; its end past the
/// last.
fn char_at(text: &str, n: usize) -> usize {
    text.char_indices().nth(n).map_or(text.len(), |(i, _)| i)
}
