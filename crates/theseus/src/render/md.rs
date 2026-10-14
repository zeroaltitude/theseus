//! Markdown to styled lines (theseus-rn7s): the subset a model's reply uses, with
//! no dependency. Headings, emphasis, inline code, fenced code (highlighted by
//! `render::code`), nested lists, block quotes, tables, links and rules, wrapped
//! to a width by display width.
//!
//! It renders by complete lines as deltas arrive: `push` takes text as it
//! streams and returns the lines that are final, `finish` the rest. What a
//! block becomes depends on the lines in order and never on where the deltas
//! were cut, so a reply streamed and the same reply whole render to the same
//! bytes. A newline inside a paragraph is a line break, as typed. A paragraph, a list, a quote and a table are drawn when the line that
//! ends them arrives (or `finish`); a code fence is drawn a line at a time.

mod block;
mod inline;
mod table;

use block::{indent_of, list_marker, Block, Event, Segmenter};
use inline::{inline, wrap};

use super::code::{Highlighter, Lang};
use super::style::{Color, Mode, Span, Style, StyledLine};

/// The renderer of one reply, for a width and a mode.
pub struct Markdown {
    width: usize,
    mode: Mode,
    /// Text after the last newline: the line still arriving.
    pending: String,
    seg: Segmenter,
    draw: Draw,
}

impl Markdown {
    pub fn new(width: usize, mode: Mode) -> Self {
        Self {
            width,
            mode,
            pending: String::new(),
            seg: Segmenter::default(),
            draw: Draw::new(width, mode, 0),
        }
    }

    /// More text of the reply: the lines that are final now.
    pub fn push(&mut self, text: &str) -> Vec<StyledLine> {
        self.pending.push_str(text);
        let mut out = Vec::new();
        while let Some(nl) = self.pending.find('\n') {
            let line: String = self.pending.drain(..=nl).collect();
            self.line(line.trim_end_matches(['\n', '\r']), &mut out);
        }
        self.unstyled(out)
    }

    /// The reply ended: the rest of its lines. The renderer starts over after
    /// it, as for a new reply.
    pub fn finish(&mut self) -> Vec<StyledLine> {
        let mut out = Vec::new();
        if !self.pending.is_empty() {
            let line = std::mem::take(&mut self.pending);
            self.line(line.trim_end_matches('\r'), &mut out);
        }
        for ev in self.seg.finish() {
            self.draw.event(ev, &mut out);
        }
        *self = Self::new(self.width, self.mode);
        self.unstyled(out)
    }

    /// The text rendered whole.
    pub fn render(text: &str, width: usize, mode: Mode) -> Vec<StyledLine> {
        let mut md = Self::new(width, mode);
        let mut out = md.push(text);
        out.extend(md.finish());
        out
    }

    fn line(&mut self, line: &str, out: &mut Vec<StyledLine>) {
        let line = expand_indent(line);
        for ev in self.seg.feed(&line) {
            self.draw.event(ev, out);
        }
    }

    /// In `Plain`, no style is left on any span.
    fn unstyled(&self, mut lines: Vec<StyledLine>) -> Vec<StyledLine> {
        if self.mode == Mode::Plain {
            for s in lines.iter_mut().flat_map(|l| l.spans.iter_mut()) {
                s.style = Style::default();
            }
        }
        lines
    }
}

/// Tabs at a line's start as four spaces.
fn expand_indent(line: &str) -> String {
    let lead = line.chars().take_while(|c| *c == ' ' || *c == '\t').count();
    let (head, rest) = line.split_at(lead);
    format!("{}{rest}", head.replace('\t', "    "))
}

/// What a block was, for the blank line between blocks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Para,
    Other,
}

/// The drawing of events into lines.
struct Draw {
    width: usize,
    mode: Mode,
    /// How deep inside lists: the bullet's glyph, and whether a blank line
    /// separates blocks.
    depth: usize,
    prev: Option<Kind>,
    code: Option<CodeBlock>,
}

struct CodeBlock {
    hl: Option<Highlighter>,
}

const CODE_INDENT: usize = 2;

impl Draw {
    fn new(width: usize, mode: Mode, depth: usize) -> Self {
        Self {
            width: width.max(8),
            mode,
            depth,
            prev: None,
            code: None,
        }
    }

    fn event(&mut self, ev: Event, out: &mut Vec<StyledLine>) {
        match ev {
            Event::Block(b) => self.block(b, out),
            Event::FenceOpen(info) => {
                self.separate(Kind::Other, out);
                self.code = Some(CodeBlock {
                    hl: Lang::from_info(&info).map(Highlighter::new),
                });
            }
            Event::FenceLine(l) => self.code_line(&l, out),
            Event::FenceClose => self.code = None,
        }
    }

    /// A blank line between two blocks: always at the top level; inside a list
    /// item only between two paragraphs.
    fn separate(&mut self, kind: Kind, out: &mut Vec<StyledLine>) {
        let blank = match self.prev {
            None => false,
            Some(prev) => self.depth == 0 || (prev == Kind::Para && kind == Kind::Para),
        };
        if blank {
            out.push(StyledLine::blank());
        }
        self.prev = Some(kind);
    }

    fn block(&mut self, b: Block, out: &mut Vec<StyledLine>) {
        match b {
            Block::Heading(level, text) => {
                self.separate(Kind::Other, out);
                self.heading(level, &text, out);
            }
            Block::Para(lines) => {
                self.separate(Kind::Para, out);
                out.extend(self.paragraph(&lines));
            }
            Block::Quote(lines) => {
                self.separate(Kind::Other, out);
                self.quote(&lines, out);
            }
            Block::List(lines) => {
                self.separate(Kind::Other, out);
                self.list(&lines, out);
            }
            Block::Table(rows) => {
                self.separate(Kind::Other, out);
                out.extend(table::render(&rows, self.width, self.mode));
            }
            Block::Rule => {
                self.separate(Kind::Other, out);
                out.push(StyledLine::from_text(
                    "─".repeat(self.width.min(40)),
                    Style::fg(Color::Gray),
                ));
            }
        }
    }

    /// A paragraph's lines joined by line breaks, as the model wrote them: a
    /// newline in a reply is meant (a list with no markers, an address), and
    /// the width, not the source, is what wraps a long line. (CommonMark makes
    /// a lone newline a space; a reply is read the way a chat is.)
    fn paragraph(&self, lines: &[String]) -> Vec<StyledLine> {
        let text = lines
            .iter()
            .map(|l| l.trim_end_matches('\\').trim_end())
            .collect::<Vec<_>>()
            .join("\n");
        wrap(&inline(&text, &Style::default(), self.mode), self.width)
    }

    fn heading(&self, level: usize, text: &str, out: &mut Vec<StyledLine>) {
        let style = match level {
            1 => Style {
                bold: true,
                underline: true,
                ..Style::default()
            },
            2 => Style::bold(),
            _ => Style {
                bold: true,
                italic: true,
                ..Style::default()
            },
        };
        let mark = if self.mode == Mode::Plain {
            format!("{} ", "#".repeat(level))
        } else {
            String::new()
        };
        let spans = inline(&format!("{mark}{text}"), &style, self.mode);
        out.extend(wrap(&spans, self.width));
    }

    /// A quote: its content drawn narrower, each line behind a bar.
    fn quote(&self, lines: &[String], out: &mut Vec<StyledLine>) {
        let (bar, style) = match self.mode {
            Mode::Ansi => ("▎ ", Style::fg(Color::Gray)),
            Mode::Plain => ("> ", Style::default()),
        };
        let inner = nested(lines, self.width.saturating_sub(2), self.mode, self.depth);
        let prefix = Span::new(bar, style);
        for l in inner {
            out.push(if l.is_blank() {
                StyledLine::from_text(bar.trim_end(), prefix.style.clone())
            } else {
                l.prefixed(&prefix)
            });
        }
    }

    /// A list: each item's content drawn narrower behind its marker.
    fn list(&self, lines: &[String], out: &mut Vec<StyledLine>) {
        for item in items(lines) {
            let bullet = self.bullet(&item);
            let pad = bullet.chars().count();
            let inner = nested(
                &item.lines,
                self.width.saturating_sub(pad),
                self.mode,
                self.depth + 1,
            );
            let mark = Span::new(bullet, Style::fg(Color::Gray));
            let blank = Span::plain(" ".repeat(pad));
            if inner.is_empty() {
                out.push(StyledLine::from_text(
                    mark.text.trim_end(),
                    mark.style.clone(),
                ));
            }
            for (i, l) in inner.into_iter().enumerate() {
                out.push(if l.is_blank() {
                    StyledLine::blank()
                } else {
                    l.prefixed(if i == 0 { &mark } else { &blank })
                });
            }
        }
    }

    /// An item's marker as drawn, with the space after it.
    fn bullet(&self, item: &Item) -> String {
        if item.ordered {
            return format!("{} ", item.mark);
        }
        let glyph = match (item.task, self.depth % 3) {
            (Some(' '), _) => "☐",
            (Some(_), _) => "☑",
            (None, 0) => "•",
            (None, 1) => "◦",
            (None, _) => "▪",
        };
        format!("{glyph} ")
    }

    /// One line of a fence: highlighted when its language is known, indented,
    /// and folded behind a `↪` where it is wider than the block.
    fn code_line(&mut self, text: &str, out: &mut Vec<StyledLine>) {
        let text = text.replace('\t', "    ");
        let line = match self.code.as_mut().and_then(|c| c.hl.as_mut()) {
            Some(hl) => hl.line(&text),
            None => StyledLine::from_text(text.clone(), Style::default()),
        };
        if text.is_empty() {
            out.push(StyledLine::blank());
            return;
        }
        let room = self.width.saturating_sub(CODE_INDENT).max(4);
        let indent = Span::plain(" ".repeat(CODE_INDENT));
        let mark = Span::new("↪ ", Style::fg(Color::Gray));
        for (i, part) in fold(&line, room).into_iter().enumerate() {
            let mut l = StyledLine::new(vec![indent.clone()]);
            if i > 0 {
                l.push_span(mark.clone());
            }
            l.extend(part);
            out.push(l);
        }
    }
}

/// `lines` drawn as blocks of their own at `width`.
fn nested(lines: &[String], width: usize, mode: Mode, depth: usize) -> Vec<StyledLine> {
    let mut seg = Segmenter::default();
    let mut draw = Draw::new(width, mode, depth);
    let mut out = Vec::new();
    for l in lines {
        for ev in seg.feed(l) {
            draw.event(ev, &mut out);
        }
    }
    for ev in seg.finish() {
        draw.event(ev, &mut out);
    }
    out
}

/// A list item: its marker and its content, dedented to the content's column.
struct Item {
    ordered: bool,
    mark: String,
    /// A task item's box: `x` for a checked one, a space for an open one.
    task: Option<char>,
    lines: Vec<String>,
}

/// The items of a list's lines. An item starts at a marker no deeper than one
/// column past the first line's; everything else belongs to the item before.
fn items(lines: &[String]) -> Vec<Item> {
    let base = indent_of(&lines[0]);
    let mut out: Vec<Item> = Vec::new();
    let mut content = 0;
    for l in lines {
        let starts = list_marker(l).filter(|_| indent_of(l) <= base + 1);
        if let Some(m) = starts {
            content = m.content;
            let first: String = l.chars().skip(m.content).collect();
            let (task, first) = task_box(&first);
            out.push(Item {
                ordered: m.ordered,
                mark: m.mark,
                task,
                lines: vec![first],
            });
        } else if let Some(item) = out.last_mut() {
            let strip = indent_of(l).min(content);
            item.lines.push(l.chars().skip(strip).collect());
        }
    }
    out
}

/// A task item's box (`[ ] `, `[x] `) at the start of its text, and the text
/// without it.
fn task_box(text: &str) -> (Option<char>, String) {
    let mut chars = text.chars();
    match (chars.next(), chars.next(), chars.next(), chars.next()) {
        (Some('['), Some(c @ (' ' | 'x' | 'X')), Some(']'), Some(' ')) => {
            (Some(c), chars.collect())
        }
        _ => (None, text.to_string()),
    }
}

/// A styled line folded into parts of at most `first` columns; every part
/// after the first has `↪ ` ahead of it, which takes two of its room.
fn fold(line: &StyledLine, room: usize) -> Vec<StyledLine> {
    if line.width() <= room {
        return vec![line.clone()];
    }
    let mut parts = vec![StyledLine::default()];
    let mut used = 0;
    let mut cap = room;
    for s in &line.spans {
        for ch in s.text.chars() {
            let w = super::style::char_width(ch);
            if used + w > cap && used > 0 {
                parts.push(StyledLine::default());
                used = 0;
                cap = room.saturating_sub(2).max(1);
            }
            let mut buf = [0u8; 4];
            if let Some(cur) = parts.last_mut() {
                cur.push(ch.encode_utf8(&mut buf), &s.style);
            }
            used += w;
        }
    }
    parts
}

#[cfg(test)]
mod tests;
