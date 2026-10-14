//! The detail pane's rows (theseus-rn7s): a run of the model's reply lines is
//! drawn as markdown with the CLI's renderer (`theseus_client::render::md`),
//! and every other line is wrapped as it always was. The CLI crate does not
//! link ratatui, so the conversion from its styled spans to ratatui's lives
//! here.

use ratatui::style::{Color, Modifier, Style};
use theseus_client::render::md::Markdown;
use theseus_client::render::style::{self, Mode};
use theseus_client::render::Tag;

use crate::ui::{tag_style, wrap};

/// One screen row of the pane.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PaneRow {
    /// A line of the CLI's words, in its tag's style.
    Plain(Tag, String),
    /// A row of the reply, drawn: spans, each with its own style.
    Drawn(Vec<(String, Style)>),
}

/// ratatui's style for a span of the CLI's.
pub fn convert(s: &style::Style, base: Style) -> Style {
    let mut out = base;
    for (on, m) in [
        (s.bold, Modifier::BOLD),
        (s.italic, Modifier::ITALIC),
        (s.dim, Modifier::DIM),
        (s.underline, Modifier::UNDERLINED),
        (s.strike, Modifier::CROSSED_OUT),
    ] {
        if on {
            out = out.add_modifier(m);
        }
    }
    match s.fg {
        Some(style::Color::Red) => out.fg(Color::Red),
        Some(style::Color::Green) => out.fg(Color::Green),
        Some(style::Color::Yellow) => out.fg(Color::Yellow),
        Some(style::Color::Blue) => out.fg(Color::Blue),
        Some(style::Color::Magenta) => out.fg(Color::Magenta),
        Some(style::Color::Cyan) => out.fg(Color::Cyan),
        Some(style::Color::Gray) => out.fg(Color::DarkGray),
        None => out,
    }
}

/// The newest rows of `lines` at `width`, newest first, until `want` are
/// held: wrapped from the end only as far as the pane needs. A run of `Reply`
/// lines is one reply, drawn whole as markdown.
pub fn rows_from_end(lines: &[(Tag, String)], width: usize, want: usize) -> Vec<PaneRow> {
    let mut rows: Vec<PaneRow> = Vec::new();
    let mut end = lines.len();
    while end > 0 && rows.len() < want {
        let tag = lines[end - 1].0;
        if tag != Tag::Reply {
            let mut wrapped = wrap(&lines[end - 1].1, width);
            while let Some(r) = wrapped.pop() {
                rows.push(PaneRow::Plain(tag, r));
            }
            end -= 1;
            continue;
        }
        let start = lines[..end]
            .iter()
            .rposition(|(t, _)| *t != Tag::Reply)
            .map_or(0, |i| i + 1);
        rows.extend(reply_rows(&lines[start..end], width).into_iter().rev());
        end = start;
    }
    rows
}

/// A reply's lines drawn, oldest first.
fn reply_rows(lines: &[(Tag, String)], width: usize) -> Vec<PaneRow> {
    let text = lines
        .iter()
        .map(|(_, t)| t.as_str())
        .collect::<Vec<_>>()
        .join("\n");
    let base = tag_style(Tag::Reply);
    Markdown::render(&text, width, Mode::Ansi)
        .into_iter()
        .map(|l| {
            PaneRow::Drawn(
                l.spans
                    .iter()
                    .map(|s| (s.text.clone(), convert(&s.style, base)))
                    .collect(),
            )
        })
        .collect()
}
