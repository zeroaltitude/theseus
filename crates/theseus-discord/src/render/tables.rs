//! Markdown tables, made readable in Discord (issue 28). Discord does not draw a markdown table: the
//! pipes and the rule of dashes show as text. A reply's words and a subagent's both reach the place
//! as written, and a subagent's are not written under Theseus's own rules for the place (the asked
//! model sees only the question and the profile's own system text), so a table in either is drawn
//! here as a fenced monospace block with its columns padded to align: the header, a rule, the rows,
//! no pipes. `split_text` keeps the fence whole across a split (it closes the fence at the end of a
//! part and reopens it at the next, cutting at a row boundary when it can).
//!
//! Only a real table is touched: a header row with a pipe, directly followed by a delimiter row of
//! dashes (and optional colons) with the same number of cells, and the rows after it that have a
//! pipe. Text that only contains a pipe is left alone, and so is a table inside a code fence.
//! Widths count characters (there is no width table in this crate), so a wide character can misalign
//! its column by a cell; the words are all there either way. The terminal and the cockpit draw tables
//! themselves and are given the text as it was.

use std::borrow::Cow;

/// How a column's cells are padded.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Align {
    Left,
    Right,
    Center,
}

/// `text` with each markdown table drawn as a fenced block; `text` itself when it holds none.
pub fn fence_tables(text: &str) -> Cow<'_, str> {
    if !text.contains('|') {
        return Cow::Borrowed(text);
    }
    let lines: Vec<&str> = text.split('\n').collect();
    let mut out: Vec<String> = Vec::with_capacity(lines.len());
    let (mut i, mut fence, mut changed) = (0, false, false);
    while i < lines.len() {
        let line = lines[i];
        if line.trim_start().starts_with("```") {
            fence = !fence;
        }
        let head = (!fence).then(|| header_and_rule(&lines, i)).flatten();
        let Some((cells, aligns)) = head else {
            out.push(line.to_string());
            i += 1;
            continue;
        };
        let rows: Vec<Vec<String>> = lines[i + 2..]
            .iter()
            .take_while(|l| is_row(l))
            .map(|l| row_cells(l))
            .collect();
        let used = 2 + rows.len();
        out.extend(block(&cells, &aligns, &rows));
        changed = true;
        i += used;
    }
    match changed {
        true => Cow::Owned(out.join("\n")),
        false => Cow::Borrowed(text),
    }
}

/// The header's cells and the columns' alignment, when line `i` is a header row and line `i + 1`
/// its delimiter row with as many cells.
fn header_and_rule(lines: &[&str], i: usize) -> Option<(Vec<String>, Vec<Align>)> {
    let (head, rule) = (lines.get(i)?, lines.get(i + 1)?);
    if !is_row(head) || !rule.contains('|') {
        return None;
    }
    let cells = row_cells(head);
    let aligns: Vec<Align> = row_cells(rule)
        .iter()
        .map(|c| rule_cell(c))
        .collect::<Option<_>>()?;
    (aligns.len() == cells.len()).then_some((cells, aligns))
}

/// A line that can be a table's row: it has a pipe and some text.
fn is_row(line: &str) -> bool {
    line.contains('|') && !line.trim().is_empty()
}

/// A delimiter cell: dashes with optional colons at either end, and its alignment.
fn rule_cell(cell: &str) -> Option<Align> {
    let c = cell.trim();
    let (left, right) = (c.starts_with(':'), c.ends_with(':'));
    let dashes = c.trim_matches(':');
    if dashes.is_empty() || !dashes.bytes().all(|b| b == b'-') {
        return None;
    }
    Some(match (left, right) {
        (true, true) => Align::Center,
        (false, true) => Align::Right,
        _ => Align::Left,
    })
}

/// A row's cells: split at the pipes that are not escaped, the outer pipes dropped, each cell
/// trimmed and without the marks a fence would show as noise (`**`, `__`, backticks).
fn row_cells(line: &str) -> Vec<String> {
    let t = line.trim();
    let t = t.strip_prefix('|').unwrap_or(t);
    let t = match t.ends_with('|') && !t.ends_with("\\|") {
        true => &t[..t.len() - 1],
        false => t,
    };
    let (mut cells, mut cur, mut chars) = (Vec::new(), String::new(), t.chars().peekable());
    while let Some(c) = chars.next() {
        match (c, chars.peek()) {
            ('\\', Some('|')) => {
                cur.push('|');
                chars.next();
            }
            ('|', _) => cells.push(clean(&std::mem::take(&mut cur))),
            _ => cur.push(c),
        }
    }
    cells.push(clean(&cur));
    cells
}

/// A cell as plain words.
fn clean(cell: &str) -> String {
    cell.trim()
        .replace("**", "")
        .replace("__", "")
        .replace('`', "")
}

/// The fenced block: the header, a rule, the rows, each column as wide as its widest cell. A row
/// with fewer cells than the header is padded with empty ones, one with more is cut to it.
fn block(head: &[String], aligns: &[Align], rows: &[Vec<String>]) -> Vec<String> {
    let n = head.len();
    let cell = |row: &[String], c: usize| row.get(c).cloned().unwrap_or_default();
    let width: Vec<usize> = (0..n)
        .map(|c| {
            std::iter::once(&head.to_vec())
                .chain(rows)
                .map(|r| cell(r, c).chars().count())
                .max()
                .unwrap_or(0)
                .max(1)
        })
        .collect();
    let draw = |row: &[String]| {
        let cells: Vec<String> = (0..n)
            .map(|c| pad(&cell(row, c), width[c], aligns[c]))
            .collect();
        cells.join("  ").trim_end().to_string()
    };
    let rule: Vec<String> = width.iter().map(|w| "-".repeat(*w)).collect();
    let mut out = vec!["```".to_string(), draw(head), rule.join("  ")];
    out.extend(rows.iter().map(|r| draw(r)));
    out.push("```".into());
    out
}

fn pad(s: &str, width: usize, align: Align) -> String {
    let gap = width.saturating_sub(s.chars().count());
    match align {
        Align::Left => format!("{s}{}", " ".repeat(gap)),
        Align::Right => format!("{}{s}", " ".repeat(gap)),
        Align::Center => format!("{}{s}{}", " ".repeat(gap / 2), " ".repeat(gap - gap / 2)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TABLE: &str = "Here are the files:\n\n| # | File | Size | Bytes |\n|---|------|------|-------|\n| 1 | a.rs | 2 KB | 2048 |\n| 2 | longer_name.rs | 10 KB | 10240 |\n\nThat is all.";

    /// A table is a fenced block with aligned columns: the header, a rule, the rows, no pipes; the
    /// words around it are as they were.
    #[test]
    fn a_table_is_a_fenced_block_with_aligned_columns() {
        let got = fence_tables(TABLE);
        assert_eq!(
            got,
            "Here are the files:\n\n```\n# \x20File            Size   Bytes\n-  --------------  -----  -----\n1  a.rs            2 KB   2048\n2  longer_name.rs  10 KB  10240\n```\n\nThat is all."
        );
        assert!(!got.contains('|'), "{got}");
    }

    /// Alignment colons are kept: right-aligned numbers, centred words; a short row is padded and a
    /// long one cut to the header; escaped pipes and emphasis marks in a cell are plain words.
    #[test]
    fn alignment_short_rows_escapes_and_marks() {
        let t = "| name | n | mid |\n|:-----|--:|:---:|\n| **bold** | 7 | x |\n| a \\| b | 12 |\n| c | 3 | y | extra |";
        assert_eq!(
            fence_tables(t),
            "```\nname    n  mid\n-----  --  ---\nbold    7   x\na | b  12\nc       3   y\n```"
        );
    }

    /// A table with no outer pipes, and one in the middle of a paragraph, is still a table.
    #[test]
    fn a_table_without_outer_pipes_still_draws() {
        let t = "Sizes\na | b\n--|--\n1 | 2";
        assert_eq!(fence_tables(t), "Sizes\n```\na  b\n-  -\n1  2\n```");
    }

    /// Text that only holds a pipe is left alone: a pipe in a sentence, a header with no rule under
    /// it, a rule whose cells do not match the header's, a setext underline, a shell pipeline.
    #[test]
    fn pipes_that_are_not_a_table_are_left_alone() {
        for t in [
            "use a | b to choose",
            "| a | b |\nno rule here",
            "| a | b |\n|---|\n| 1 | 2 |",
            "| a | b |\n| x | y |",
            "Title\n-----",
            "run `ls | wc -l` now\nand `cat x | head`",
            "no pipe at all",
            "| a | b |\n|---|--x|\n| 1 | 2 |",
        ] {
            assert_eq!(fence_tables(t), t, "{t}");
        }
    }

    /// A table already inside a code fence is the code's, and is left as it is.
    #[test]
    fn a_table_inside_a_code_fence_is_left_alone() {
        let t = "```\n| a | b |\n|---|---|\n| 1 | 2 |\n```";
        assert_eq!(fence_tables(t), t);
        // One before the fence draws, one after it too; the one inside does not.
        let both = format!("| a | b |\n|---|---|\n| 1 | 2 |\n{t}\n| c | d |\n|---|---|\n| 3 | 4 |");
        let got = fence_tables(&both);
        assert_eq!(got.matches("| 1 | 2 |").count(), 1, "{got}");
        assert!(
            got.starts_with("```\na  b\n-  -\n1  2\n```\n```\n| a | b |"),
            "{got}"
        );
        assert!(got.ends_with("```\nc  d\n-  -\n3  4\n```"), "{got}");
    }

    /// Two tables in one text are two blocks; text with none comes back as the same borrow.
    #[test]
    fn two_tables_are_two_blocks_and_none_is_none() {
        let two = "| a |\n|---|\n| 1 |\n\ntext\n\n| b |\n|---|\n| 2 |";
        assert_eq!(fence_tables(two).matches("```").count(), 4);
        assert!(matches!(fence_tables("plain text"), Cow::Borrowed(_)));
    }

    /// Theseus's own reply is drawn the same way, in its post and in its stream: nothing in the
    /// binding kept a table out of a reply, so the same function serves both.
    #[test]
    fn a_reply_with_a_table_is_drawn_as_a_block_in_its_post_and_its_stream() {
        let parts = crate::render::reply_parts("t1", &[(0, TABLE.to_string())], None);
        assert_eq!(parts.len(), 1);
        assert_eq!(parts[0].1, fence_tables(TABLE));
        let mut r = crate::render::Renderer::default();
        r.on_notification(
            "turn.started",
            &serde_json::json!({"session_id": "s", "turn_id": "t1"}),
        );
        let mut ops = r.on_notification(
            "model.delta",
            &serde_json::json!({"turn_id": "t1", "loop_index": 0, "text": TABLE}),
        );
        ops.extend(r.tick());
        let live: Vec<String> = ops
            .into_iter()
            .filter_map(|o| match o {
                crate::render::Op::Upsert { content, .. } => Some(content),
                _ => None,
            })
            .collect();
        assert_eq!(live, vec![fence_tables(TABLE).into_owned()]);
    }
}
