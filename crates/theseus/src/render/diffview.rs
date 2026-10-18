//! A diff and a result's first lines as rows (theseus-z6au): the question's
//! preview, a finished edit's diff, and the `⎿` gutter under a tool line. Pure:
//! a row is a `Line` whose tag says removed (`Del`), added (`Add`) or the rest,
//! and whose text holds `MARK_ON`/`MARK_OFF` around the changed words of a
//! paired −/+ line. A printer paints them (`paint`, with colour on a terminal,
//! none on a pipe or under `NO_COLOR`); the TUI draws them its own way.

use theseus_protocol::diff::{first_changed, rows, Kind, Row};
use theseus_protocol::term::words::clip;
use theseus_protocol::{ConfirmPreview, ConfirmRequest, ToolEnded};

use super::{Line, Tag};

/// Around the changed words of a line.
pub const MARK_ON: char = '\u{1}';
pub const MARK_OFF: char = '\u{2}';

/// Lines of a result under its tool line.
const GUTTER_HEAD: usize = 3;
/// Lines of a failed result's end, where its error is.
const GUTTER_FAIL: usize = 8;
/// Rows of a diff a question or a finished call shows.
const DIFF_ROWS: usize = 24;
/// A row is clipped here.
const WIDTH: usize = 160;

/// `text` with its marks as sentinels.
fn marked(text: &str, marks: &[(usize, usize)]) -> String {
    let mut out = String::new();
    let mut at = 0;
    for &(a, b) in marks {
        out.push_str(&text[at..a]);
        out.push(MARK_ON);
        out.push_str(&text[a..b]);
        out.push(MARK_OFF);
        at = b;
    }
    out.push_str(&text[at..]);
    out
}

/// `text` without its sentinels.
pub fn plain(text: &str) -> String {
    text.chars()
        .filter(|c| *c != MARK_ON && *c != MARK_OFF)
        .collect()
}

/// A line for a terminal: with colour, the tag's colour and the marked words
/// reversed; without, the text alone.
pub fn paint(line: &Line, colour: bool) -> String {
    if !colour || !matches!(line.tag, Tag::Add | Tag::Del) {
        return plain(&line.text);
    }
    let (base, mark) = match line.tag {
        Tag::Add => ("\x1b[32m", "\x1b[1;97;42m"),
        _ => ("\x1b[31m", "\x1b[1;97;41m"),
    };
    let text = line
        .text
        .replace(MARK_ON, mark)
        .replace(MARK_OFF, &format!("\x1b[0m{base}"));
    format!("{base}{text}\x1b[0m")
}

/// A unified diff as rows: `file:line (+n −m)`, then each hunk's lines,
/// numbered, the changed words marked. At most `DIFF_ROWS` rows, then a line
/// saying how many more.
pub fn diff_lines(unified: &str, pad: &str) -> Vec<Line> {
    let parsed = rows(unified);
    let file = parsed
        .iter()
        .rev()
        .find(|r| r.kind == Kind::File && r.text.starts_with("+++ "))
        .map(|r| r.text.trim_start_matches("+++ ").trim().to_string())
        .filter(|f| f != "/dev/null")
        .or_else(|| {
            parsed
                .iter()
                .find(|r| r.kind == Kind::File)
                .map(|r| r.text.trim_start_matches("--- ").trim().to_string())
        });
    let count = |k: Kind| parsed.iter().filter(|r| r.kind == k).count();
    let mut out = Vec::new();
    if let (Some(f), Some(n)) = (file, first_changed(&parsed)) {
        out.push(Line::new(
            Tag::Dim,
            format!(
                "{pad}{f}:{n}  (+{} −{})",
                count(Kind::Added),
                count(Kind::Removed)
            ),
        ));
    }
    let body: Vec<&Row> = parsed
        .iter()
        .filter(|r| r.kind != Kind::File && r.kind != Kind::Hunk)
        .collect();
    let mut shown = 0;
    for (i, r) in parsed.iter().enumerate() {
        if r.kind == Kind::File {
            continue;
        }
        if shown == DIFF_ROWS {
            let left = body.len().saturating_sub(
                parsed[..i]
                    .iter()
                    .filter(|r| !matches!(r.kind, Kind::File | Kind::Hunk))
                    .count(),
            );
            out.push(Line::new(Tag::Dim, format!("{pad}… {left} more lines")));
            break;
        }
        let (tag, sign, no) = match r.kind {
            Kind::Removed => (Tag::Del, '-', r.old),
            Kind::Added => (Tag::Add, '+', r.new),
            Kind::Hunk => {
                out.push(Line::new(
                    Tag::Dim,
                    format!("{pad}{}", clip(&r.text, WIDTH)),
                ));
                continue;
            }
            Kind::Note => {
                out.push(Line::new(Tag::Dim, format!("{pad}     {}", r.text)));
                continue;
            }
            _ => (Tag::Plain, ' ', r.new),
        };
        let no = no.map(|n| n.to_string()).unwrap_or_default();
        let text = marked(&clip(&r.text, WIDTH), &r.marks);
        out.push(Line::new(tag, format!("{pad}{no:>4} {sign} {text}")));
        shown += 1;
    }
    out
}

/// What a question shows of its call: an edit's hunk, or a command with its
/// directory. Empty when the daemon sent no preview.
pub fn preview_lines(c: &ConfirmRequest) -> Vec<Line> {
    let Some(p) = &c.preview else {
        return Vec::new();
    };
    match p.kind.as_str() {
        ConfirmPreview::DIFF => diff_lines(&p.body, "      "),
        ConfirmPreview::COMMAND => p
            .body
            .lines()
            .enumerate()
            .map(|(i, l)| {
                Line::new(
                    Tag::Dim,
                    format!("      {}{l}", if i == 0 { "$ " } else { "" }),
                )
            })
            .collect(),
        _ => p
            .body
            .lines()
            .map(|l| Line::new(Tag::Dim, format!("      {}", clip(l, WIDTH))))
            .collect(),
    }
}

/// What a finished call shows under its end line: a file tool's diff, else
/// the first lines of its result (a failure's last lines, where its error is),
/// behind a `⎿` gutter.
pub fn ended_lines(t: &ToolEnded) -> Vec<Line> {
    if let Some(d) = &t.diff {
        let mut out = vec![Line::new(
            Tag::Dim,
            format!(
                "  ⎿ {}  +{} −{}{}",
                d.path,
                d.added,
                d.removed,
                if d.truncated { " (cut)" } else { "" }
            ),
        )];
        out.extend(diff_lines(&d.unified, "    ").into_iter().skip(1));
        return out;
    }
    if matches!(
        t.tool.as_str(),
        "fs.read" | "fs.write" | "fs.edit" | "fs.patch"
    ) && t.status == "ok"
    {
        return Vec::new();
    }
    // The tool-input-fit line (`[exit code 0 · in /app]`) is the end line's.
    let text: Vec<&str> = t
        .preview
        .lines()
        .skip_while(|l| l.starts_with("[exit code"))
        .skip_while(|l| l.trim().is_empty())
        .collect();
    let failed = t.status != "ok" || t.exit_code.is_some_and(|c| c != 0);
    let (shown, all): (Vec<&str>, bool) = if failed && text.len() > GUTTER_FAIL {
        (text[text.len() - GUTTER_FAIL..].to_vec(), false)
    } else {
        let n = if failed { GUTTER_FAIL } else { GUTTER_HEAD };
        (text.iter().take(n).copied().collect(), text.len() <= n)
    };
    let mut out: Vec<Line> = shown
        .iter()
        .enumerate()
        .map(|(i, l)| {
            Line::new(
                Tag::Dim,
                format!("{}{}", if i == 0 { "  ⎿ " } else { "    " }, clip(l, WIDTH)),
            )
        })
        .collect();
    if !all && !out.is_empty() {
        let cut = t.truncated || t.preview.chars().count() >= 2000;
        out.push(Line::new(
            Tag::Dim,
            format!(
                "    … all {}{} lines",
                text.len(),
                if cut { "+" } else { "" }
            ),
        ));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use theseus_protocol::ToolDiff;

    const EDIT: &str = "--- tides.py\n+++ tides.py\n@@ -48,3 +48,3 @@\n     highest = max(heights)\n-    return lowest - highest\n+    return highest - lowest\n \n";

    fn texts(l: &[Line]) -> Vec<String> {
        l.iter().map(|l| plain(&l.text)).collect()
    }

    #[test]
    fn a_diff_is_rows_with_a_file_line_numbers_and_marked_words() {
        let l = diff_lines(EDIT, "  ");
        assert_eq!(
            texts(&l),
            [
                "  tides.py:49  (+1 −1)",
                "  @@ -48,3 +48,3 @@",
                "    48       highest = max(heights)",
                "    49 -     return lowest - highest",
                "    49 +     return highest - lowest",
                "    50   ",
            ]
        );
        assert_eq!((l[3].tag, l[4].tag), (Tag::Del, Tag::Add));
        assert!(l[3].text.contains(&format!("{MARK_ON}lowest{MARK_OFF}")));
        assert!(l[4].text.contains(&format!("{MARK_ON}highest{MARK_OFF}")));
    }

    #[test]
    fn colour_paints_the_rows_and_a_pipe_gets_no_escape() {
        let l = diff_lines(EDIT, "");
        let del = &l[3];
        let coloured = paint(del, true);
        assert!(coloured.starts_with("\x1b[31m") && coloured.contains("\x1b[1;97;41m"));
        assert!(!paint(del, false).contains('\x1b'));
        assert!(!paint(del, false).contains(MARK_ON));
        // A row of another tag is its text alone.
        assert!(!paint(&l[0], true).contains('\x1b'));
    }

    #[test]
    fn a_failed_result_shows_its_end_and_a_finished_edit_its_diff() {
        let failed = ToolEnded {
            tool: "proc.run".into(),
            status: "ok".into(),
            exit_code: Some(1),
            preview: format!(
                "[exit code 1 · in /app]\n{}FAIL: test_tidal_range\nAssertionError: -2.8 != 2.8\n",
                "ok line\n".repeat(10)
            ),
            ..ToolEnded::default()
        };
        let l = texts(&ended_lines(&failed));
        assert_eq!(l.len(), 9);
        assert!(l[0].starts_with("  ⎿ "));
        assert_eq!(l[7], "    AssertionError: -2.8 != 2.8");
        assert_eq!(l[8], "    … all 12 lines");
        let edit = ToolEnded {
            tool: "fs.edit".into(),
            status: "ok".into(),
            diff: Some(ToolDiff {
                path: "tides.py".into(),
                added: 1,
                removed: 1,
                unified: EDIT.into(),
                truncated: false,
            }),
            ..ToolEnded::default()
        };
        let l = texts(&ended_lines(&edit));
        assert_eq!(l[0], "  ⎿ tides.py  +1 −1");
        assert!(l.iter().any(|x| x.contains("49 + ")));
        // A short result needs no "all N lines".
        let short = ToolEnded {
            tool: "fs.glob".into(),
            status: "ok".into(),
            preview: "a\nb".into(),
            ..ToolEnded::default()
        };
        assert_eq!(texts(&ended_lines(&short)), ["  ⎿ a", "    b"]);
    }
}
