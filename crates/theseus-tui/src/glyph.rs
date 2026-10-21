//! Outside text, drawn cell for cell (theseus-ikw4p). A title or a label comes from outside (a task's name begins `⚙️`,
//! U+2699 U+FE0F): the TUI counts the width of such a glyph one way and a terminal, tmux above all, draws another, so
//! every cell after it on the row lands one off and ratatui's diff leaves stale cells, two strings drawn over each
//! other. `tidy` is the one function every draw of outside text calls: it drops the variation selectors and the
//! zero-width joiner, so each glyph is the one cell (or two) `unicode-width` says of the bare character.

use std::borrow::Cow;

/// U+FE0E and U+FE0F select a text or emoji look for the character before them; U+200D joins emoji.
fn drawn_alone(c: char) -> bool {
    matches!(c, '\u{FE0E}' | '\u{FE0F}' | '\u{200D}')
}

/// `text` without the characters terminals disagree on the width of.
pub fn tidy(text: &str) -> Cow<'_, str> {
    if text.chars().any(drawn_alone) {
        Cow::Owned(text.chars().filter(|c| !drawn_alone(*c)).collect())
    } else {
        Cow::Borrowed(text)
    }
}
