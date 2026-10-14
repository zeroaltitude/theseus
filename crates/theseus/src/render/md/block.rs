//! Markdown's blocks, found line by line (theseus-rn7s). The segmenter is given
//! complete lines, in order, and says what each makes: a heading, a paragraph
//! once it is known to end, a code fence's lines as they come. Whether a block
//! has ended depends on the lines before it and the line that follows, never on
//! how the text was cut into deltas, so a reply streamed and the same reply whole
//! come out as the same events.
//!
//! The subset: ATX headings, fenced code, block quotes, bullet and numbered
//! lists (nested by indentation), pipe tables, rules, paragraphs. Indented code
//! and setext headings are not read: their lines are a paragraph's.

/// What the segmenter found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    Block(Block),
    /// A fence opened, with its info string.
    FenceOpen(String),
    /// One line of the open fence's code, as written less the fence's indent.
    FenceLine(String),
    FenceClose,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Block {
    Heading(usize, String),
    /// Lines of one paragraph, each with its trailing spaces.
    Para(Vec<String>),
    /// A quote's lines with the marks taken off.
    Quote(Vec<String>),
    /// A list's lines as written, nested items and their content included.
    List(Vec<String>),
    /// A table's rows: the header, the delimiter row, then the body.
    Table(Vec<String>),
    Rule,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Fence {
    ch: char,
    len: usize,
    indent: usize,
}

#[derive(Debug, Default)]
struct ListState {
    /// What the first item is: numbered or not, and its bullet or delimiter.
    /// Another kind at the same level starts another list.
    kind: (bool, char),
    lines: Vec<String>,
    /// Blank lines seen since the last line of the list: part of it only if
    /// the list goes on.
    blanks: usize,
    fence: Option<Fence>,
}

#[derive(Debug, Default)]
enum Open {
    #[default]
    None,
    Para(Vec<String>),
    Quote(Vec<String>),
    List(ListState),
    Table(Vec<String>),
    Fence(Fence),
}

#[derive(Debug, Default)]
pub struct Segmenter {
    open: Open,
}

impl Segmenter {
    /// One more complete line (no newline).
    pub fn feed(&mut self, line: &str) -> Vec<Event> {
        let mut ev = Vec::new();
        match std::mem::take(&mut self.open) {
            Open::None => self.start(line, &mut ev),
            Open::Para(p) => self.in_para(p, line, &mut ev),
            Open::Quote(q) => self.in_quote(q, line, &mut ev),
            Open::List(l) => self.in_list(l, line, &mut ev),
            Open::Table(t) => self.in_table(t, line, &mut ev),
            Open::Fence(f) => self.in_fence(f, line, &mut ev),
        }
        ev
    }

    /// The text ended: whatever is open ends with it.
    pub fn finish(&mut self) -> Vec<Event> {
        let mut ev = Vec::new();
        flush(std::mem::take(&mut self.open), &mut ev);
        ev
    }

    fn start(&mut self, line: &str, ev: &mut Vec<Event>) {
        if line.trim().is_empty() {
            return;
        }
        if let Some((fence, info)) = fence_open(line) {
            ev.push(Event::FenceOpen(info));
            self.open = Open::Fence(fence);
        } else if let Some((level, text)) = heading(line) {
            ev.push(Event::Block(Block::Heading(level, text)));
        } else if is_rule(line) {
            ev.push(Event::Block(Block::Rule));
        } else if let Some(rest) = quote_content(line) {
            self.open = Open::Quote(vec![rest]);
        } else if let Some(m) = list_marker(line) {
            self.open = Open::List(ListState {
                kind: kind_of(&m),
                lines: vec![line.to_string()],
                ..ListState::default()
            });
        } else {
            self.open = Open::Para(vec![line.trim_start().to_string()]);
        }
    }

    fn in_para(&mut self, mut p: Vec<String>, line: &str, ev: &mut Vec<Event>) {
        if line.trim().is_empty() {
            flush(Open::Para(p), ev);
        } else if let Some(header) = p.last().filter(|h| table_starts(h, line)).cloned() {
            p.pop();
            flush(Open::Para(p), ev);
            self.open = Open::Table(vec![header, line.to_string()]);
        } else if interrupts_paragraph(line) {
            flush(Open::Para(p), ev);
            self.start(line, ev);
        } else {
            p.push(line.trim_start().to_string());
            self.open = Open::Para(p);
        }
    }

    fn in_quote(&mut self, mut q: Vec<String>, line: &str, ev: &mut Vec<Event>) {
        if let Some(rest) = quote_content(line) {
            q.push(rest);
            self.open = Open::Quote(q);
        } else if line.trim().is_empty() {
            flush(Open::Quote(q), ev);
        } else if !interrupts_paragraph(line) {
            q.push(line.trim_start().to_string());
            self.open = Open::Quote(q);
        } else {
            flush(Open::Quote(q), ev);
            self.start(line, ev);
        }
    }

    fn in_table(&mut self, mut t: Vec<String>, line: &str, ev: &mut Vec<Event>) {
        if line.trim().is_empty() || !line.contains('|') {
            flush(Open::Table(t), ev);
            self.start(line, ev);
        } else {
            t.push(line.to_string());
            self.open = Open::Table(t);
        }
    }

    fn in_fence(&mut self, f: Fence, line: &str, ev: &mut Vec<Event>) {
        if closes(line, f) {
            ev.push(Event::FenceClose);
            return;
        }
        let strip = line
            .chars()
            .take(f.indent)
            .take_while(|c| *c == ' ')
            .count();
        ev.push(Event::FenceLine(line[strip..].to_string()));
        self.open = Open::Fence(f);
    }

    fn in_list(&mut self, mut l: ListState, line: &str, ev: &mut Vec<Event>) {
        if let Some(f) = l.fence {
            if closes(line.trim_start(), f) {
                l.fence = None;
            }
            l.lines.push(line.to_string());
            self.open = Open::List(l);
        } else if line.trim().is_empty() {
            l.blanks += 1;
            self.open = Open::List(l);
        } else if continues_list(&l, line) {
            l.lines.extend(std::iter::repeat_n(String::new(), l.blanks));
            l.blanks = 0;
            l.fence = fence_open(line.trim_start()).map(|(f, _)| f);
            l.lines.push(line.to_string());
            self.open = Open::List(l);
        } else {
            flush(Open::List(l), ev);
            self.start(line, ev);
        }
    }
}

/// Whether a line belongs to the open list: indented, another item of the
/// same kind, or a plain line straight after one (a lazy continuation).
fn continues_list(l: &ListState, line: &str) -> bool {
    if let Some(m) = list_marker(line).filter(|_| indent_of(line) < 2) {
        return kind_of(&m) == l.kind;
    }
    indent_of(line) >= 2 || (l.blanks == 0 && !interrupts_paragraph(line))
}

fn kind_of(m: &Marker) -> (bool, char) {
    (m.ordered, m.mark.chars().last().unwrap_or('-'))
}

fn flush(open: Open, ev: &mut Vec<Event>) {
    let block = match open {
        Open::None => return,
        Open::Para(p) if p.is_empty() => return,
        Open::Para(p) => Block::Para(p),
        Open::Quote(q) => Block::Quote(q),
        Open::List(l) => Block::List(l.lines),
        Open::Table(t) => Block::Table(t),
        Open::Fence(_) => {
            ev.push(Event::FenceClose);
            return;
        }
    };
    ev.push(Event::Block(block));
}

/// The leading spaces of a line.
pub fn indent_of(line: &str) -> usize {
    line.chars().take_while(|c| *c == ' ').count()
}

/// A line that ends a paragraph or a quote without a blank line before it.
fn interrupts_paragraph(line: &str) -> bool {
    fence_open(line).is_some()
        || heading(line).is_some()
        || is_rule(line)
        || quote_content(line).is_some()
        || list_marker(line).is_some_and(|m| !m.ordered || m.number == "1")
}

/// A fence's opening line: three backticks or tildes at most three spaces in.
fn fence_open(line: &str) -> Option<(Fence, String)> {
    let indent = indent_of(line);
    let rest = &line[indent..];
    let ch = rest.chars().next().filter(|c| *c == '`' || *c == '~')?;
    let len = rest.chars().take_while(|c| *c == ch).count();
    let info = rest[len..].trim();
    if len < 3 || indent > 3 || (ch == '`' && info.contains('`')) {
        return None;
    }
    Some((Fence { ch, len, indent }, info.to_string()))
}

fn closes(line: &str, f: Fence) -> bool {
    let t = line.trim();
    t.len() >= f.len && t.chars().all(|c| c == f.ch)
}

/// `#` to `######`, a space, the text without a closing run of `#`.
fn heading(line: &str) -> Option<(usize, String)> {
    let indent = indent_of(line);
    let rest = &line[indent..];
    let level = rest.chars().take_while(|c| *c == '#').count();
    if indent > 3 || !(1..=6).contains(&level) {
        return None;
    }
    let text = &rest[level..];
    if !text.is_empty() && !text.starts_with(' ') {
        return None;
    }
    let text = text.trim();
    let closed = text.trim_end_matches('#');
    let text = if closed.is_empty() || closed.ends_with(' ') {
        closed.trim_end()
    } else {
        text
    };
    Some((level, text.to_string()))
}

/// Three or more of `-`, `*` or `_`, spaces between allowed, nothing else.
fn is_rule(line: &str) -> bool {
    let t = line.trim();
    let Some(ch) = t.chars().next().filter(|c| matches!(c, '-' | '*' | '_')) else {
        return false;
    };
    indent_of(line) <= 3
        && t.chars().filter(|c| *c == ch).count() >= 3
        && t.chars().all(|c| c == ch || c == ' ')
}

/// A `>` quote line's content, the mark and one space after it off.
fn quote_content(line: &str) -> Option<String> {
    let rest = line.strip_prefix(&" ".repeat(indent_of(line).min(3)))?;
    let rest = rest.strip_prefix('>')?;
    Some(rest.strip_prefix(' ').unwrap_or(rest).to_string())
}

/// A list item's marker.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Marker {
    pub ordered: bool,
    /// The digits of a numbered item.
    pub number: String,
    /// The mark as written, with its delimiter: `-`, `*`, `+`, `3.`, `3)`.
    pub mark: String,
    /// Columns from the line's start to its content.
    pub content: usize,
}

/// The marker a line opens with, if it is an item: a bullet or up to nine
/// digits and a `.` or `)`, then a space or the line's end.
pub fn list_marker(line: &str) -> Option<Marker> {
    let indent = indent_of(line);
    let rest = &line[indent..];
    let first = rest.chars().next()?;
    let (ordered, number, mark) = if matches!(first, '-' | '*' | '+') {
        (false, String::new(), first.to_string())
    } else {
        let digits: String = rest.chars().take_while(char::is_ascii_digit).collect();
        let delim = rest[digits.len()..].chars().next()?;
        if digits.is_empty() || digits.len() > 9 || !matches!(delim, '.' | ')') {
            return None;
        }
        let mark = format!("{digits}{delim}");
        (true, digits, mark)
    };
    let after = &rest[mark.len()..];
    if !after.is_empty() && !after.starts_with(' ') {
        return None;
    }
    let spaces = after.chars().take_while(|c| *c == ' ').count();
    // Five spaces or more after a mark is a marker and an indented line.
    let gap = if spaces == 0 || spaces > 4 { 1 } else { spaces };
    Some(Marker {
        ordered,
        number,
        content: indent + mark.len() + gap,
        mark,
    })
}

/// Cells of a table row, the pipes at its ends off and `\|` kept.
pub fn cells(row: &str) -> Vec<String> {
    let t = row.trim();
    let t = t.strip_prefix('|').unwrap_or(t);
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut chars = t.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\\' if chars.peek() == Some(&'|') => {
                cur.push_str("\\|");
                chars.next();
            }
            '|' => out.push(std::mem::take(&mut cur).trim().to_string()),
            _ => cur.push(c),
        }
    }
    let last = cur.trim();
    if !last.is_empty() || !t.ends_with('|') {
        out.push(last.to_string());
    }
    out
}

/// A delimiter row's alignments (`:--`, `:-:`, `--:`), or none when the line
/// is no delimiter row.
pub fn delimiter(row: &str) -> Option<Vec<Align>> {
    if !row.contains('-') {
        return None;
    }
    cells(row)
        .iter()
        .map(|c| {
            let core = c.trim_matches(':');
            if core.is_empty() || !core.chars().all(|ch| ch == '-') {
                return None;
            }
            Some(match (c.starts_with(':'), c.ends_with(':')) {
                (true, true) => Align::Center,
                (false, true) => Align::Right,
                _ => Align::Left,
            })
        })
        .collect()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Align {
    Left,
    Center,
    Right,
}

/// Whether `delim` follows `header` as a table's delimiter row.
fn table_starts(header: &str, delim: &str) -> bool {
    header.contains('|')
        && delim.contains('|')
        && delimiter(delim).is_some_and(|d| d.len() == cells(header).len() && !d.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(text: &str) -> Vec<Event> {
        let mut s = Segmenter::default();
        let mut ev: Vec<Event> = text.lines().flat_map(|l| s.feed(l)).collect();
        ev.extend(s.finish());
        ev
    }

    fn blocks(text: &str) -> Vec<Block> {
        run(text)
            .into_iter()
            .filter_map(|e| match e {
                Event::Block(b) => Some(b),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn blocks_end_where_the_next_line_says() {
        let b = blocks("# T\npara a\npara b\n\n- x\n  - y\n\n- z\nafter\n> q\nlazy\n---\n");
        assert_eq!(b[0], Block::Heading(1, "T".into()));
        assert_eq!(b[1], Block::Para(vec!["para a".into(), "para b".into()]));
        assert_eq!(
            b[2],
            Block::List(vec![
                "- x".into(),
                "  - y".into(),
                String::new(),
                "- z".into(),
                "after".into()
            ])
        );
        assert_eq!(b[3], Block::Quote(vec!["q".into(), "lazy".into()]));
        assert_eq!(b[4], Block::Rule);
    }

    #[test]
    fn a_fence_is_lines_as_they_come_and_runs_over_blank_lines() {
        let ev = run("```rust\nfn a() {}\n\n  b\n```\ntext");
        assert_eq!(ev[0], Event::FenceOpen("rust".into()));
        assert_eq!(ev[1], Event::FenceLine("fn a() {}".into()));
        assert_eq!(ev[2], Event::FenceLine(String::new()));
        assert_eq!(ev[3], Event::FenceLine("  b".into()));
        assert_eq!(ev[4], Event::FenceClose);
        assert_eq!(ev[5], Event::Block(Block::Para(vec!["text".into()])));
        // An unclosed fence ends with the text.
        assert_eq!(run("~~~\nx").last(), Some(&Event::FenceClose));
    }

    #[test]
    fn a_table_starts_at_its_delimiter_row_even_after_text() {
        let b = blocks("intro\n| a | b |\n|---|:-:|\n| 1 | 2 |\n\nnot | table\n---|---");
        assert_eq!(b[0], Block::Para(vec!["intro".into()]));
        assert!(matches!(&b[1], Block::Table(t) if t.len() == 3));
        // A table needs no pipes at its edges, only a delimiter row of as many cells.
        assert!(matches!(&b[2], Block::Table(t) if t.len() == 2));
        assert!(blocks("a | b\n---")
            .iter()
            .all(|b| !matches!(b, Block::Table(_))));
        assert_eq!(cells("| a \\| b | c |"), ["a \\| b", "c"]);
        assert_eq!(
            delimiter("|:--|:-:|--:|"),
            Some(vec![Align::Left, Align::Center, Align::Right])
        );
        assert_eq!(delimiter("| a | b |"), None);
    }

    #[test]
    fn a_marker_needs_its_space_and_a_number_its_delimiter() {
        assert!(list_marker("-x").is_none());
        assert!(list_marker("**bold**").is_none());
        assert!(list_marker("2024. was").is_some());
        assert!(list_marker("10) ten").is_some());
        assert_eq!(list_marker("  - a").unwrap().content, 4);
        assert_eq!(list_marker("1.  a").unwrap().content, 4);
        assert!(is_rule("* * *") && is_rule("---") && !is_rule("--") && !is_rule("***a***"));
        assert_eq!(heading("## Two ##"), Some((2, "Two".into())));
        assert_eq!(heading("#nospace"), None);
        assert_eq!(heading("####### seven"), None);
    }

    #[test]
    fn a_paragraph_is_interrupted_by_a_block_but_not_by_a_year() {
        let b = blocks("text\n2. not a list\nmore\n1. a list\n");
        assert_eq!(
            b[0],
            Block::Para(vec!["text".into(), "2. not a list".into(), "more".into()])
        );
        assert!(matches!(&b[1], Block::List(_)));
    }
}
