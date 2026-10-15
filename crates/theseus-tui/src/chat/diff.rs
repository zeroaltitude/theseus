//! A diff and a result's lines, as the chat draws them (theseus-wy6v.1): the
//! CLI's rows (`diffview`: numbered, the changed words of a paired −/+ line
//! between `MARK_ON` and `MARK_OFF`) become styled lines, each cut by display
//! width. A row longer than the terminal continues on the next line behind
//! `↪`, as a code block does, and a changed word keeps its mark across the
//! cut. Nothing is clipped with an ellipsis.

use theseus_client::render::diffview::{MARK_OFF, MARK_ON};
use theseus_client::render::{Line as Row, Tag};
use unicode_width::UnicodeWidthChar;

use super::style::{span, Line, Sty};

/// A diff row's numbers and sign: `{no:>4} {sign} `.
const GUTTER: usize = 7;

/// The rows as styled lines of at most `width - 1` columns. `pad` is the
/// indent the rows were made with (`diff_lines(.., pad)`), which a diff row's
/// continuation keeps.
pub fn lines(rows: &[Row], pad: usize, width: usize) -> Vec<Line> {
    let mut out = Vec::new();
    for r in rows {
        let (head, base, mark) = match r.tag {
            Tag::Add => (pad + GUTTER, Sty::Green, Sty::AddMark),
            Tag::Del => (pad + GUTTER, Sty::Red, Sty::DelMark),
            Tag::Plain => (pad + GUTTER, Sty::Plain, Sty::Plain),
            _ => (leading_spaces(&r.text), Sty::Dim, Sty::Dim),
        };
        out.extend(wrap(&r.text, head, width, base, mark));
    }
    out
}

fn leading_spaces(text: &str) -> usize {
    text.chars().take_while(|c| *c == ' ').count()
}

/// One row, cut to `width - 1` columns. The first `head` characters (its
/// indent, numbers and sign) lead the first line only; a continuation leads
/// with the same room and `↪`.
fn wrap(text: &str, head: usize, width: usize, base: Sty, mark: Sty) -> Vec<Line> {
    let room = width.saturating_sub(1).max(12);
    let head = head.min(room.saturating_sub(8));
    let mut chars = text.chars().peekable();
    let lead: String = (0..head).filter_map(|_| next_plain(&mut chars)).collect();
    let cont = format!("{}↪ ", " ".repeat(head.saturating_sub(2)));
    let mut marked = false;
    let mut out: Vec<Line> = Vec::new();
    let mut line: Line = vec![span(base, lead)];
    let mut used = head;
    let mut piece = String::new();
    let flush = |line: &mut Line, piece: &mut String, marked: bool| {
        if !piece.is_empty() {
            line.push(span(
                if marked { mark } else { base },
                std::mem::take(piece),
            ));
        }
    };
    for c in chars {
        match c {
            MARK_ON | MARK_OFF => {
                flush(&mut line, &mut piece, marked);
                marked = c == MARK_ON;
                continue;
            }
            _ => {}
        }
        let w = c.width().unwrap_or(0);
        if used + w > room {
            flush(&mut line, &mut piece, marked);
            out.push(std::mem::take(&mut line));
            line = vec![span(Sty::Dim, cont.clone())];
            used = head;
        }
        piece.push(c);
        used += w;
    }
    flush(&mut line, &mut piece, marked);
    out.push(line);
    out
}

/// The next character that is not a mark.
fn next_plain(it: &mut std::iter::Peekable<std::str::Chars<'_>>) -> Option<char> {
    loop {
        match it.next()? {
            MARK_ON | MARK_OFF => {}
            c => return Some(c),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use theseus_client::render::diffview::diff_lines;

    const EDIT: &str = "--- tides.py\n+++ tides.py\n@@ -48,3 +48,3 @@\n     highest = max(heights)\n-    return round(lowest - highest, 2)\n+    return round(highest - lowest, 2)\n \n";

    fn text(l: &Line) -> String {
        l.iter().map(|(_, t)| t.as_str()).collect()
    }

    #[test]
    fn a_diff_row_is_numbered_and_its_changed_words_are_a_style_of_their_own() {
        let rows = diff_lines(EDIT, "  ");
        let l = lines(&rows, 2, 80);
        let t: Vec<String> = l.iter().map(text).collect();
        assert_eq!(t[0], "  tides.py:49  (+1 −1)");
        assert_eq!(t[3], "    49 -     return round(lowest - highest, 2)");
        assert_eq!(t[4], "    49 +     return round(highest - lowest, 2)");
        let removed = &l[3];
        assert!(
            removed.contains(&(Sty::DelMark, "lowest".into())),
            "{removed:?}"
        );
        assert!(removed.contains(&(Sty::DelMark, "highest".into())));
        assert!(l[4].contains(&(Sty::AddMark, "highest".into())));
        assert!(removed.iter().any(|(s, _)| *s == Sty::Red));
    }

    #[test]
    fn a_long_row_continues_behind_an_arrow_and_loses_nothing() {
        let long = format!(
            "--- a.py\n+++ a.py\n@@ -1 +1 @@\n-x = 1\n+x = \"{}\"\n",
            "tidal ".repeat(14)
        );
        let rows = diff_lines(&long, "  ");
        let l = lines(&rows, 2, 40);
        let t: Vec<String> = l.iter().map(text).collect();
        assert!(
            t.iter()
                .all(|r| unicode_width::UnicodeWidthStr::width(r.as_str()) <= 39),
            "{t:?}"
        );
        assert!(t.iter().any(|r| r.contains("↪ ")), "{t:?}");
        let at = t.iter().position(|r| r.contains(" + x")).unwrap();
        let joined: String = t[at..]
            .iter()
            .map(|r| r.chars().skip(9).collect::<String>())
            .collect();
        assert_eq!(joined.matches("tidal").count(), 14, "{t:?}");
        assert!(!t.iter().any(|r| r.contains('…')), "no ellipsis cut: {t:?}");
    }

    #[test]
    fn a_mark_survives_the_cut() {
        let rows = vec![Row::new(
            Tag::Add,
            format!(
                "  {:>4} + {MARK_ON}aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa{MARK_OFF}z",
                3
            ),
        )];
        let l = lines(&rows, 2, 30);
        assert_eq!(l.len(), 3, "{l:?}");
        assert!(
            l[..2]
                .iter()
                .all(|r| r.iter().any(|(s, _)| *s == Sty::AddMark)),
            "{l:?}"
        );
        assert!(
            !l[2].iter().any(|(s, _)| *s == Sty::AddMark),
            "the mark ended before z: {l:?}"
        );
    }
}
