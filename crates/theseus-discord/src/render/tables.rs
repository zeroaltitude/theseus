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
//! Widths are display columns (`unicode-width`, already used by the CLI and the terminal UI), so
//! wide characters (CJK, emoji) keep their column. The terminal and the cockpit draw tables
//! themselves and are given the text as it was.

use std::borrow::Cow;

use unicode_width::UnicodeWidthStr;

/// The longest a drawn line may be, in bytes: under half a part, so that a cut at a newline always
/// falls on a row and never inside one. A row that would be longer is left as plain text, outside
/// the fence (which closes before it and reopens after it); a table whose header cannot fit is left
/// as it was.
const MAX_ROW: usize = 900;

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
        let Some((mut cells, mut aligns)) = head else {
            out.push(line.to_string());
            i += 1;
            continue;
        };
        let rows: Vec<(Vec<String>, &str)> = lines[i + 2..]
            .iter()
            .take_while(|l| is_row(l))
            .map(|l| (row_cells(l), l.trim()))
            .collect();
        let used = 2 + rows.len();
        // Joining code spans moved a row's cells away from the header's where the plain split by
        // pipes matches it: the table is ambiguous (the two backticks of ``| `a | b` |``), so it
        // stays as written, whole.
        if rows
            .iter()
            .any(|(r, src)| r.len() != cells.len() && plain_count(src) == cells.len())
        {
            out.extend(lines[i..i + used].iter().map(|l| l.to_string()));
            i += used;
            continue;
        }
        // Drawing a table loses nothing: a row with more cells than the header widens the table
        // (the header gets empty cells), where a row with fewer is padded.
        let n = rows
            .iter()
            .map(|(r, _)| r.len())
            .fold(cells.len(), usize::max);
        cells.resize(n, String::new());
        aligns.resize(n, Align::Left);
        match block(&cells, &aligns, &rows) {
            Some(drawn) => {
                out.extend(drawn);
                changed = true;
            }
            // Too wide to draw: the table is the text it was.
            None => out.extend(lines[i..i + used].iter().map(|l| l.to_string())),
        }
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

/// A row's cells: split at the pipes that are not escaped and not inside a code span, the outer
/// pipes dropped, each cell trimmed and cleaned (`clean`). A pipe inside a pair of backticks is
/// text, not a column break (`ls | wc -l` in a command table): that goes beyond GFM, which splits
/// there, because drawing a table loses nothing.
fn row_cells(line: &str) -> Vec<String> {
    let t = line.trim();
    let t = t.strip_prefix('|').unwrap_or(t);
    let t = match t.ends_with('|') && !t.ends_with("\\|") {
        true => &t[..t.len() - 1],
        false => t,
    };
    let chars: Vec<char> = t.chars().collect();
    let (mut cells, mut cur, mut i) = (Vec::new(), String::new(), 0);
    while i < chars.len() {
        match chars[i] {
            '\\' if chars.get(i + 1) == Some(&'|') => {
                cur.push('|');
                i += 2;
            }
            '|' => {
                cells.push(clean(&std::mem::take(&mut cur)));
                i += 1;
            }
            '`' => {
                let run = chars[i..].iter().take_while(|c| **c == '`').count();
                // A later run of as many backticks closes the span, pipes and all.
                let end = code_span_end(&chars, i + run, run).unwrap_or(i + run);
                let mut span = chars[i..end].iter().peekable();
                while let Some(c) = span.next() {
                    match (c, span.peek()) {
                        ('\\', Some('|')) => {}
                        _ => cur.push(*c),
                    }
                }
                i = end;
            }
            c => {
                cur.push(c);
                i += 1;
            }
        }
    }
    cells.push(clean(&cur));
    cells
}

/// How many cells a row has when every unescaped pipe splits, code spans or not (GFM's own split):
/// what `row_cells` would say without its reading of code spans.
fn plain_count(line: &str) -> usize {
    let t = line.trim();
    let t = t.strip_prefix('|').unwrap_or(t);
    let t = match t.ends_with('|') && !t.ends_with("\\|") {
        true => &t[..t.len() - 1],
        false => t,
    };
    let (mut count, mut prev) = (1, ' ');
    for c in t.chars() {
        if c == '|' && prev != '\\' {
            count += 1;
        }
        prev = c;
    }
    count
}

/// Where a code span opened by `run` backticks ends (just past its closing run), searching from
/// `from`: the next run of exactly `run` backticks.
fn code_span_end(chars: &[char], from: usize, run: usize) -> Option<usize> {
    let mut j = from;
    while j < chars.len() {
        if chars[j] != '`' {
            j += 1;
            continue;
        }
        let len = chars[j..].iter().take_while(|c| **c == '`').count();
        if len == run {
            return Some(j + len);
        }
        j += len;
    }
    None
}

/// A cell as it is drawn: the words the model wrote, and nothing less. Only a mark that is one
/// pair around the whole cell is dropped (`**x**`, `__x__`, `*x*`, `_x_`, and a backtick pair),
/// since the block shows no emphasis; `__` and `**` inside a word or a cell stay as written
/// (`__init__.py`, `2**10`), and so does a pair of underscores around one word, which may be a name
/// (`__init__`): a pair of underscores is dropped only around words with a space in them.
fn clean(cell: &str) -> String {
    let t = cell.trim();
    for (mark, words_only) in [
        ("**", false),
        ("__", true),
        ("*", false),
        ("_", true),
        ("`", false),
    ] {
        let Some(inner) = wrapped(t, mark) else {
            continue;
        };
        if !words_only || inner.contains(char::is_whitespace) {
            return inner.to_string();
        }
    }
    t.to_string()
}

/// What `mark` wraps when it is one pair around all of `t`: the inside holds no mark of its own and
/// does not begin or end in a space (`* x *` is no emphasis).
fn wrapped<'a>(t: &'a str, mark: &str) -> Option<&'a str> {
    let inner = t.strip_prefix(mark)?.strip_suffix(mark)?;
    let bare = !inner.is_empty()
        && !inner.contains(&mark[..1])
        && !inner.starts_with(char::is_whitespace)
        && !inner.ends_with(char::is_whitespace);
    bare.then_some(inner)
}

/// The fenced block: the header, a rule, the rows, each column as wide as its widest cell, in
/// display columns. A row with fewer cells than the header is padded with empty ones (`fence_tables`
/// has widened the header to the widest row, so no cell is cut). A row that would draw longer than [`MAX_ROW`] is left as the text it was, outside
/// the fence, which closes before it and opens again for the rows after it. `None` when the header
/// itself cannot fit, or no row can: the table is then left as it was.
fn block(head: &[String], aligns: &[Align], rows: &[(Vec<String>, &str)]) -> Option<Vec<String>> {
    // A cell with three backticks would open or close the block's own fence: the table stays as
    // written.
    let fenced = |c: &String| c.contains("```");
    if head.iter().any(fenced) || rows.iter().any(|(r, _)| r.iter().any(fenced)) {
        return None;
    }
    let n = head.len();
    let cell = |row: &[String], c: usize| row.get(c).cloned().unwrap_or_default();
    let draw = |row: &[String], width: &[usize]| {
        let cells: Vec<String> = (0..n)
            .map(|c| pad(&cell(row, c), width[c], aligns[c]))
            .collect();
        cells.join("  ").trim_end().to_string()
    };
    let mut plain = vec![false; rows.len()];
    let width = loop {
        let width: Vec<usize> = (0..n)
            .map(|c| {
                std::iter::once(head)
                    .chain(
                        rows.iter()
                            .zip(&plain)
                            .filter(|(_, p)| !**p)
                            .map(|((r, _), _)| r.as_slice()),
                    )
                    .map(|r| cell(r, c).width())
                    .max()
                    .unwrap_or(0)
                    .max(1)
            })
            .collect();
        let rule: usize = width.iter().sum::<usize>() + 2 * (n - 1);
        // Too wide as a whole (a cell somewhere is long): the row with the most in it goes out of
        // the table and the widths are read again. Else only the rows that still draw too long.
        let too_wide = rule > MAX_ROW || draw(head, &width).len() > MAX_ROW;
        let victim = rows
            .iter()
            .enumerate()
            .filter(|(i, (r, _))| !plain[*i] && (too_wide || draw(r, &width).len() > MAX_ROW))
            .max_by_key(|(_, (r, _))| r.iter().map(String::len).sum::<usize>())
            .map(|(i, _)| i);
        match victim {
            Some(i) => plain[i] = true,
            // The header alone is too wide to draw.
            None if too_wide => return None,
            None => break width,
        }
    };
    if !rows.is_empty() && plain.iter().all(|p| *p) {
        return None;
    }
    let rule: Vec<String> = width.iter().map(|w| "-".repeat(*w)).collect();
    let mut out = vec!["```".to_string(), draw(head, &width), rule.join("  ")];
    let mut open = true;
    for ((row, source), plain) in rows.iter().zip(&plain) {
        match (plain, open) {
            (true, true) => {
                out.push("```".into());
                open = false;
            }
            (false, false) => {
                out.push("```".into());
                open = true;
            }
            _ => {}
        }
        out.push(match plain {
            true => source.to_string(),
            false => draw(row, &width),
        });
    }
    if open {
        out.push("```".into());
    }
    Some(out)
}

fn pad(s: &str, width: usize, align: Align) -> String {
    let gap = width.saturating_sub(s.width());
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
    /// long one widens the table (the header gets an empty cell: nothing a row says is cut); an
    /// escaped pipe is a pipe, and a pair of marks around a whole cell is dropped.
    #[test]
    fn alignment_short_rows_escapes_and_marks() {
        let t = "| name | n | mid |\n|:-----|--:|:---:|\n| **bold** | 7 | x |\n| a \\| b | 12 |\n| c | 3 | y | extra |";
        assert_eq!(
            fence_tables(t),
            "```\nname    n  mid\n-----  --  ---  -----\nbold    7   x\na | b  12\nc       3   y   extra\n```"
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

    /// Wide characters keep their column: with the last column right-aligned every drawn line ends
    /// at the same display column (CJK and emoji count two), where a count of characters would leave
    /// the lines with them a column or more short.
    #[test]
    fn wide_characters_keep_their_columns() {
        let t = "| 名前 | size |\n|---|--:|\n| ファイル名 | 10 |\n| a | 200 |\n| ✅ ok | 3 |";
        let got = fence_tables(t).into_owned();
        let lines: Vec<&str> = got.lines().collect();
        let body = &lines[1..lines.len() - 1];
        // The widest first cell is the five wide characters (ten columns); `size` is four.
        let total = 10 + 2 + 4;
        for l in body {
            assert_eq!(UnicodeWidthStr::width(*l), total, "{l:?} in\n{got}");
        }
        assert!(lines[3].starts_with("ファイル名  "), "{got}");
    }

    fn long(n: usize) -> String {
        "x".repeat(n)
    }

    /// A row longer than a part is not drawn: it is the text it was, outside the fence, which closes
    /// before it and opens again after it; the other rows are drawn as they would be without it, and
    /// no fence is left open.
    #[test]
    fn a_row_longer_than_a_part_is_left_as_plain_text_and_no_fence_is_left_open() {
        let t = format!(
            "| a | b |\n|---|---|\n| 1 | 2 |\n| 3 | {} |\n| 5 | 6 |",
            long(3_000)
        );
        let got = fence_tables(&t);
        let lines: Vec<&str> = got.lines().collect();
        assert_eq!(
            lines[..7],
            [
                "```",
                "a  b",
                "-  -",
                "1  2",
                "```",
                &format!("| 3 | {} |", long(3_000)),
                "```"
            ]
        );
        assert_eq!(&lines[7..], ["5  6", "```"]);
        assert_eq!(got.matches("```").count() % 2, 0);
        // Through the split, no part holds an open fence either.
        for part in crate::render::split_text(&got, crate::render::PART_LIMIT) {
            assert_eq!(part.matches("```").count() % 2, 0, "{part}");
        }
    }

    /// A table whose every row, or whose header, is too long to draw is the text it was.
    #[test]
    fn a_table_that_cannot_fit_is_left_as_it_was() {
        let all = format!("| a |\n|---|\n| {} |", long(2_000));
        assert_eq!(fence_tables(&all), all);
        let head = format!("| {} |\n|---|\n| 1 |", long(2_000));
        assert_eq!(fence_tables(&head), head);
    }

    /// Rows that are each short enough but together too wide (two long cells in different columns)
    /// draw once the worst of them is out.
    #[test]
    fn rows_that_are_too_wide_together_lose_the_worst_one() {
        let t = format!(
            "| a | b |\n|---|---|\n| {} | 1 |\n| 2 | {} |\n| 3 | 4 |",
            long(600),
            long(600)
        );
        let got = fence_tables(&t);
        let lines: Vec<&str> = got.lines().collect();
        assert_eq!(got.matches("```").count() % 2, 0);
        // One row is the text it was; the rest are drawn, none longer than a row may be.
        assert_eq!(lines.iter().filter(|l| l.starts_with("| ")).count(), 1);
        assert!(lines
            .iter()
            .any(|l| l.starts_with("3 ") && l.ends_with("  4")));
        assert!(
            lines
                .iter()
                .all(|l| l.len() <= MAX_ROW || l.starts_with('|')),
            "a drawn line is too long"
        );
    }

    /// Drawing a table loses nothing: names with underscores and asterisks inside them, an extra
    /// cell, a pair of underscores around one word (a name), all as the model wrote them.
    #[test]
    fn a_table_draws_the_words_as_written() {
        let t = "| file | note |\n|---|---|\n| __init__.py | 2**10 |\n| __pycache__ | **bold** |\n| __init__ | _two words_ |\n| a*b*c | `code` | extra |";
        let got = fence_tables(t).into_owned();
        for kept in [
            "__init__.py",
            "2**10",
            "__pycache__",
            "__init__",
            "a*b*c",
            "extra",
        ] {
            assert!(got.contains(kept), "{kept} in\n{got}");
        }
        // A pair around the whole cell is the block's to drop; a pair inside is the words'.
        assert!(got.contains("bold") && !got.contains("**bold**"), "{got}");
        assert!(
            got.contains("two words") && !got.contains("_two words_"),
            "{got}"
        );
        assert!(got.contains("code") && !got.contains("`code`"), "{got}");
    }

    /// The cells of a drawn block, row by row, read back by the column widths its rule line gives.
    fn cells_drawn(block: &str) -> Vec<Vec<String>> {
        let lines: Vec<&str> = block.lines().collect();
        let widths: Vec<usize> = lines[2].split("  ").map(str::len).collect();
        let body = lines[1..lines.len() - 1].iter().enumerate();
        body.filter(|(i, _)| *i != 1)
            .map(|(_, line)| {
                let mut chars = line.chars().peekable();
                widths
                    .iter()
                    .map(|w| {
                        let mut cell = String::new();
                        let mut used = 0;
                        while used < *w {
                            let Some(c) = chars.next() else { break };
                            used += unicode_width::UnicodeWidthChar::width(c).unwrap_or(0);
                            cell.push(c);
                        }
                        chars.by_ref().take(2).for_each(drop);
                        cell.trim().to_string()
                    })
                    .collect()
            })
            .collect()
    }

    /// What a cell is drawn as, written out apart from `clean`: the text the model wrote (an escaped
    /// pipe a pipe), with one pair of marks off the whole of it when the pair wraps it alone and
    /// its inside holds none of that mark and no space at either end; and a pair of underscores
    /// only when the inside has a space in it (`__init__` is a name and stays).
    fn drawn_as(raw: &str) -> String {
        for mark in ["**", "__", "*", "_", "`"] {
            let Some(inner) = raw.strip_prefix(mark).and_then(|r| r.strip_suffix(mark)) else {
                continue;
            };
            let bare = !inner.is_empty()
                && !inner.contains(&mark[..1])
                && !inner.starts_with(char::is_whitespace)
                && !inner.ends_with(char::is_whitespace);
            let name = mark.starts_with('_') && !inner.contains(char::is_whitespace);
            if bare && !name {
                return inner.to_string();
            }
        }
        raw.to_string()
    }

    /// Generated tables (a fixed seed): cells with underscores, asterisks, backticks, escaped pipes,
    /// wide characters, spaces and nothing at all, rows with fewer cells and with more than the
    /// header, each alignment. Every cell of every row is in the drawn block, in its row and its
    /// column, as written or with one pair of marks off the whole of it: nothing else is changed.
    #[test]
    fn a_generated_table_loses_nothing_it_says() {
        let pool = [
            "__init__.py",
            "__pycache__",
            "2**10",
            "**bold**",
            "__x__ y",
            "_a b_",
            "*it*",
            "`code`",
            "a \\| b",
            "名前",
            "✅",
            "plain",
            "*",
            "**",
            "_",
            "snake_case_name",
            "a*b*c",
            "",
            "x y",
            "* star *",
            "__init__",
            "_x_y_",
            "**a** b",
            "ファイル名.rs",
            "a_b",
            "*a*b*",
            "`ls | wc -l`",
            "`a \\| b`",
            "__a b__",
            "a `b | c` d",
        ];
        let rules = ["---", ":--", "--:", ":-:"];
        let mut seed = 0x2545_f491_4f6c_dd1d_u64;
        let mut next = move |n: usize| {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            (seed % n as u64) as usize
        };
        let (mut tables, mut checked, mut left) = (0, 0, 0);
        for _ in 0..400 {
            tables += 1;
            let cols = 1 + next(5);
            let rows: Vec<Vec<&str>> = (0..next(7))
                .map(|_| {
                    (0..1 + next(cols + 2))
                        .map(|_| pool[next(pool.len())])
                        .collect()
                })
                .collect();
            let head: Vec<&str> = (0..cols).map(|_| pool[next(pool.len())]).collect();
            let line = |cells: &[&str]| format!("| {} |", cells.join(" | "));
            let rule: Vec<&str> = (0..cols).map(|_| rules[next(4)]).collect();
            let mut t = format!("{}\n{}", line(&head), line(&rule));
            for r in &rows {
                t.push('\n');
                t.push_str(&line(r));
            }
            let got = fence_tables(&t).into_owned();
            // A row the code spans make ambiguous (fewer or more cells than the header's, where the
            // plain split by pipes matches it) leaves the table as written, whole.
            if rows
                .iter()
                .any(|r| r.len() != cols && plain_count(&line(r)) == cols)
            {
                assert_eq!(got, t);
                left += 1;
                continue;
            }
            let drawn = cells_drawn(&got);
            let width = rows.iter().map(Vec::len).fold(cols, usize::max);
            let wrote: Vec<&Vec<&str>> = std::iter::once(&head).chain(&rows).collect();
            assert_eq!(drawn.len(), wrote.len(), "{t}\n->\n{got}");
            for (r, (cells, said)) in drawn.iter().zip(&wrote).enumerate() {
                assert_eq!(cells.len(), width, "row {r} of\n{got}");
                for (c, shown) in cells.iter().enumerate() {
                    let raw = said
                        .get(c)
                        .map_or(String::new(), |x| x.replace("\\|", "|").trim().to_string());
                    assert_eq!(
                        shown,
                        &drawn_as(&raw),
                        "row {r} column {c}: wrote {raw:?}\nin\n{got}\nfrom\n{t}"
                    );
                    checked += 1;
                }
            }
        }
        assert_eq!(tables, 400);
        assert!(left > 0, "the generator never made an ambiguous table");
        assert!(checked > 3_000, "{checked} cells checked");
        eprintln!("generated tables: {tables} ({left} ambiguous, left as written), cells checked: {checked}");
    }

    /// One-word underscores are a name and stay as written; a pair around words is dropped. The
    /// drawn cells are read back and compared exactly.
    #[test]
    fn a_pair_of_underscores_around_one_word_stays_and_around_words_goes() {
        let t = "| a | b | c |\n|---|---|---|\n| __init__ | _x_ | __two words__ |\n| __pycache__ | _a b_ | __init__.py |";
        let got = fence_tables(t).into_owned();
        assert_eq!(
            cells_drawn(&got)[1..],
            [
                ["__init__", "_x_", "two words"],
                ["__pycache__", "a b", "__init__.py"]
            ]
        );
    }

    /// A pipe inside a code span is text, not a column break: a command table survives, and an
    /// escaped pipe inside one is a pipe.
    #[test]
    fn a_pipe_inside_backticks_is_text() {
        let t = "| command | what |\n|---|---|\n| `ls | wc -l` | counts |\n| `a \\| b` | escaped |\n| ``x | y`` | double |\n| a ` b | c |";
        let got = fence_tables(t).into_owned();
        assert_eq!(
            cells_drawn(&got)[1..],
            [
                ["ls | wc -l", "counts"],
                ["a | b", "escaped"],
                ["``x | y``", "double"],
                ["a ` b", "c"]
            ]
        );
    }

    /// A cell with three backticks would open or close the block's own fence: the table is left as
    /// written, whole.
    #[test]
    fn a_table_with_three_backticks_in_a_cell_is_left_as_written() {
        let t = "| a | b |\n|---|---|\n| ```x``` | 1 |\n| 2 | 3 |";
        assert_eq!(fence_tables(t), t);
        let head = "| ```a``` | b |\n|---|---|\n| 1 | 2 |";
        assert_eq!(fence_tables(head), head);
    }

    /// Joining code spans that moves a row's cell count away from the header's, where the plain split
    /// by pipes matches it, is ambiguous: the table is left as written, whole. A command table whose
    /// spans are closed inside one cell, and whose plain split does not match the header, is drawn.
    #[test]
    fn a_table_the_code_spans_make_ambiguous_is_left_as_written() {
        // The reviewer's: two backticks that pair across a pipe, and the plain split fits the header.
        let t = "| a | b |\n|---|---|\n| `a | b` |";
        assert_eq!(fence_tables(t), t);
        // Three columns, a span joining two of them: the plain split still fits.
        let three = "| a | b | c |\n|---|---|---|\n| `x | y` | z |\n| 1 | 2 | 3 |";
        assert_eq!(fence_tables(three), three);
        // Two lone backticks in different cells of a row pair across the pipe: the same.
        let lone = "| a | b | c |\n|---|---|---|\n| ` | x | ` |";
        assert_eq!(fence_tables(lone), lone);
        // Lone backticks where the plain split does not fit the header (a longer row) still pair
        // across the pipes, and the cells merge: the table is drawn and no word of it is lost. This
        // is why lone backticks are not among the generated cells of the property test.
        let ragged = "| a | b |\n|---|---|\n| ` | x | y | ` |";
        let got = fence_tables(ragged).into_owned();
        assert_eq!(cells_drawn(&got)[1], ["` | x | y | `", ""]);
        // A command table: the span is closed inside its cell, the plain split (three) is not the
        // header's (two): drawn, and the pipe is kept.
        let cmd = "| command | what |\n|---|---|\n| `ls | wc -l` | counts |\n| `cat x` | prints |";
        let got = fence_tables(cmd).into_owned();
        assert!(got.starts_with("```\ncommand"), "{got}");
        assert_eq!(cells_drawn(&got)[1][0], "ls | wc -l");
        // A short row whose span closes in its cell, and whose plain split (two) is not the
        // header's (three): drawn, padded.
        let short = "| a | b | c |\n|---|---|---|\n| `x | y` |";
        let got = fence_tables(short).into_owned();
        assert_eq!(cells_drawn(&got)[1], ["x | y", "", ""]);
    }

    #[test]
    fn plain_count_is_the_split_by_every_unescaped_pipe() {
        assert_eq!(plain_count("| a | b |"), 2);
        assert_eq!(plain_count("a | b | c"), 3);
        assert_eq!(plain_count("| `a | b` |"), 2);
        assert_eq!(plain_count("| a \\| b |"), 1);
        assert_eq!(plain_count("| `x | y` | z |"), 3);
    }
}
