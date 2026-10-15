//! Styled text for the chat: a line is spans, each a style and its text, so a
//! line can be fitted to the terminal's width by what it shows, not by its
//! bytes, and read back as plain text in a test. Colour is off under
//! `NO_COLOR` (no-color.org) and when the output is not a terminal.

use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Sty {
    Plain,
    Dim,
    Bold,
    Cyan,
    Green,
    Red,
    Yellow,
    /// The changed words of an added line, and of a removed one.
    AddMark,
    DelMark,
}

impl Sty {
    fn code(self) -> &'static str {
        match self {
            Sty::Plain => "",
            Sty::Dim => "2",
            Sty::Bold => "1",
            Sty::Cyan => "36",
            Sty::Green => "32",
            Sty::Red => "31",
            Sty::Yellow => "33",
            Sty::AddMark => "1;97;42",
            Sty::DelMark => "1;97;41",
        }
    }
}

/// One line: spans in order.
pub type Line = Vec<(Sty, String)>;

pub fn span(sty: Sty, text: impl Into<String>) -> (Sty, String) {
    (sty, text.into())
}

/// The line's display width.
pub fn width(line: &Line) -> usize {
    line.iter().map(|(_, t)| t.width()).sum()
}

/// Whether the terminal gets colour: not under `NO_COLOR`, and not on a
/// dumb terminal.
pub fn color_wanted() -> bool {
    std::env::var_os("NO_COLOR").is_none_or(|v| v.is_empty())
        && std::env::var("TERM").map_or(true, |t| t != "dumb")
}

/// The line cut to `cols` display columns (an ellipsis at the cut), as a
/// string with its styles written as SGR escapes when `color`.
pub fn render(line: &Line, cols: usize, color: bool) -> String {
    let total = width(line);
    let keep = if total > cols {
        cols.saturating_sub(1)
    } else {
        cols
    };
    let mut out = String::new();
    let mut used = 0;
    let mut cut = false;
    'spans: for (sty, text) in line {
        let mut piece = String::new();
        for c in text.chars() {
            let w = c.width().unwrap_or(0);
            if used + w > keep && total > cols {
                cut = true;
                flush(&mut out, *sty, &piece, color);
                break 'spans;
            }
            used += w;
            piece.push(c);
        }
        flush(&mut out, *sty, &piece, color);
    }
    if cut {
        flush(&mut out, Sty::Dim, "…", color);
    }
    out
}

fn flush(out: &mut String, sty: Sty, text: &str, color: bool) {
    if text.is_empty() {
        return;
    }
    if color && sty != Sty::Plain {
        out.push_str("\x1b[");
        out.push_str(sty.code());
        out.push('m');
        out.push_str(text);
        out.push_str("\x1b[0m");
    } else {
        out.push_str(text);
    }
}

/// `text` styled whole, as a one-span line.
pub fn one(sty: Sty, text: impl Into<String>) -> Line {
    vec![span(sty, text)]
}

/// A line cut at word boundaries to `cols`, each continuation indented by
/// `indent` spaces: how the chat wraps what it writes itself (a notice, a
/// question). Words longer than a line are cut.
pub fn wrap_words(text: &str, cols: usize, indent: usize) -> Vec<String> {
    let cols = cols.max(8);
    let pad = " ".repeat(indent);
    let mut out = Vec::new();
    let mut line = String::new();
    let (mut w, mut words) = (0, 0);
    for word in text.split_whitespace() {
        let ww = word.width();
        if words > 0 && w + 1 + ww > cols {
            out.push(std::mem::replace(&mut line, pad.clone()));
            (w, words) = (indent, 0);
        }
        if words == 0 && w + ww > cols {
            for c in word.chars() {
                let cw = c.width().unwrap_or(0);
                if w + cw > cols {
                    out.push(std::mem::replace(&mut line, pad.clone()));
                    w = indent;
                }
                line.push(c);
                w += cw;
            }
            words = 1;
            continue;
        }
        if words > 0 {
            line.push(' ');
            w += 1;
        }
        line.push_str(word);
        w += ww;
        words += 1;
    }
    if words > 0 || out.is_empty() {
        out.push(line);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_line_is_cut_by_what_it_shows_and_ends_in_an_ellipsis() {
        let l = vec![span(Sty::Dim, "abcdef"), span(Sty::Plain, "ghij")];
        assert_eq!(render(&l, 20, false), "abcdefghij");
        assert_eq!(render(&l, 8, false), "abcdefg…");
        assert_eq!(render(&l, 8, true), "\x1b[2mabcdef\x1b[0mg\x1b[2m…\x1b[0m");
    }

    #[test]
    fn words_wrap_without_cutting_one_and_indent_the_rest() {
        let w = wrap_words("one two three four five", 9, 2);
        assert_eq!(w, vec!["one two", "  three", "  four", "  five"]);
        let long = wrap_words("abcdefghijkl", 8, 0);
        assert_eq!(long, vec!["abcdefgh", "ijkl"]);
    }
}
