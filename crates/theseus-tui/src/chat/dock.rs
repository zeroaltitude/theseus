//! The dock: the rows at the bottom of the terminal that are redrawn, and
//! nothing else is. From the top: the rows of [`ABOVE_INPUT`] (the live line
//! while a turn runs, a call in flight, a question, a notice), the input, the
//! command list while a `/` is typed, and the status line.
//!
//! A later row adds to the dock by adding a [`DockRows`] function to
//! [`ABOVE_INPUT`] (cli-tasks puts its task rows there) and a [`Segment`] to
//! [`status_segments`]; the dock composes them and knows no row's content.

use super::editor::PREFIX;
use super::live::{elapsed, spinner};
use super::state::Chat;
use super::style::{one, span, wrap_words, Line, Sty};

/// What a provider of dock rows is given besides the chat.
pub struct DockCtx {
    pub now_ms: u64,
    pub width: usize,
}

/// A provider of rows for the dock above the input.
pub type DockRows = fn(&Chat, &DockCtx) -> Vec<Line>;

/// Every provider, top row first.
pub const ABOVE_INPUT: &[DockRows] = &[turn_rows, tool_rows, question_rows, notice_rows];

/// The most rows of a question's text preview the dock shows.
const MAX_PREVIEW: usize = 12;

/// One part of the status line. When the line is too wide the segments of
/// the lowest `keep` go first.
pub struct Segment {
    pub text: String,
    pub keep: u8,
}

/// The status line's parts, left to right: `<mode> · <directory> · <model> ·
/// $<session cost> · ◐ N tasks`.
pub fn status_segments(chat: &Chat) -> Vec<Segment> {
    let mut out = vec![
        Segment {
            text: chat.mode.status(),
            keep: 3,
        },
        Segment {
            text: chat.cfg.shown_dir.clone(),
            keep: 2,
        },
    ];
    if let Some(m) = &chat.model {
        out.push(Segment {
            text: super::banner::model_words(m),
            keep: 4,
        });
    }
    out.push(Segment {
        text: cost_words(chat.cost),
        keep: 5,
    });
    // The approved plan's count, `plan 2/5` (theseus-lyxy).
    if let Some(p) = theseus_protocol::modes::plan_line(&chat.steps) {
        out.push(Segment { text: p, keep: 3 });
    }
    if chat.tasks > 0 {
        out.push(Segment {
            text: format!(
                "◐ {} task{}",
                chat.tasks,
                if chat.tasks == 1 { "" } else { "s" }
            ),
            keep: 1,
        });
    }
    if chat.offline {
        out.push(Segment {
            text: "offline".to_string(),
            keep: 6,
        });
    }
    out
}

fn cost_words(x: f64) -> String {
    if x <= 0.0 {
        "$0.00".to_string()
    } else {
        theseus_protocol::usd(x)
    }
}

/// The status line, fitted to `width`.
fn status_line(chat: &Chat, width: usize) -> Line {
    let mut segs = status_segments(chat);
    loop {
        let text = segs
            .iter()
            .map(|s| s.text.as_str())
            .collect::<Vec<_>>()
            .join(" · ");
        if text.chars().count() <= width || segs.len() <= 1 {
            return one(Sty::Dim, text);
        }
        let lowest = segs.iter().map(|s| s.keep).min().unwrap_or(0);
        let at = segs.iter().position(|s| s.keep == lowest).unwrap_or(0);
        segs.remove(at);
    }
}

fn turn_rows(chat: &Chat, _ctx: &DockCtx) -> Vec<Line> {
    // While a call runs its own row says what happens; the turn's line says
    // so otherwise.
    if chat.live.tools.iter().any(|t| !t.background) && chat.question.is_none() {
        return Vec::new();
    }
    chat.live_line().into_iter().collect()
}

fn tool_rows(chat: &Chat, ctx: &DockCtx) -> Vec<Line> {
    let mut out = Vec::new();
    for t in &chat.live.tools {
        let words = chat.tool_words(&t.tool, &t.input);
        let room = ctx.width.saturating_sub(14);
        let mut shown: String = words.chars().take(room).collect();
        if words.chars().count() > room {
            shown.pop();
            shown.push('…');
        }
        out.push(vec![
            span(Sty::Cyan, format!("{} ", spinner(ctx.now_ms))),
            span(Sty::Plain, shown),
            span(
                Sty::Dim,
                format!(
                    " · {}{}",
                    elapsed(ctx.now_ms.saturating_sub(t.since_ms)),
                    if t.background { " · background" } else { "" }
                ),
            ),
        ]);
        if let Some(last) = &t.last {
            let row: String = last.chars().take(ctx.width.saturating_sub(4)).collect();
            out.push(one(Sty::Dim, format!("  └ {row}")));
        }
    }
    out
}

fn question_rows(chat: &Chat, ctx: &DockCtx) -> Vec<Line> {
    let Some(q) = &chat.question else {
        return Vec::new();
    };
    let mut out = Vec::new();
    let title = if q.budget.is_some() {
        "the spend limit is reached".to_string()
    } else {
        format!("{}?", chat.tool_words(&q.tool, &q.input))
    };
    for (i, row) in wrap_words(&title, ctx.width.saturating_sub(2), 2)
        .into_iter()
        .enumerate()
    {
        out.push(vec![
            span(Sty::Yellow, if i == 0 { "? " } else { "  " }),
            span(Sty::Bold, row.trim_start().to_string()),
        ]);
    }
    for row in wrap_words(
        &format!("why: {}", q.reason),
        ctx.width.saturating_sub(2),
        2,
    ) {
        out.push(one(Sty::Dim, format!("  {}", row.trim_start())));
    }
    // A plan's steps, numbered, are its question (theseus-lyxy).
    if let Some(p) = q.preview.as_ref().filter(|p| p.kind == "text") {
        for line in p.body.lines().take(MAX_PREVIEW) {
            out.push(one(Sty::Plain, format!("  {line}")));
        }
    }
    let yes = q
        .yes_option()
        .map_or("approve", |o| o.label.as_str())
        .to_lowercase();
    let mut keys = format!("  [y] {yes} · [n] decline");
    if let Some(o) = &q.grant_offer {
        keys.push_str(&format!(" · [{}] {}", o.key(), o.button().to_lowercase()));
    }
    if q.external_text.is_some() {
        keys.push_str(" · [t] approve and trust");
    }
    out.push(one(Sty::Yellow, keys));
    out
}

fn notice_rows(chat: &Chat, _ctx: &DockCtx) -> Vec<Line> {
    chat.notice
        .iter()
        .map(|(text, _)| one(Sty::Yellow, text.clone()))
        .collect()
}

/// The rows, and where the cursor sits among them.
pub struct Dock {
    pub rows: Vec<Line>,
    /// Row and column of the cursor.
    pub cursor: (usize, usize),
}

pub fn build(chat: &Chat) -> Dock {
    let width = usize::from(chat.width);
    let ctx = DockCtx {
        now_ms: chat.now_ms,
        width,
    };
    let mut rows: Vec<Line> = Vec::new();
    for provider in ABOVE_INPUT {
        rows.extend(provider(chat, &ctx));
    }
    let above = rows.len();

    // The input, with room left for the rows under it.
    let menu: Vec<_> = chat.suggestions();
    let below = menu.len().min(6) + 1;
    let height = usize::from(chat.height);
    let allow = height.saturating_sub(above + below + 1).max(1);
    let layout = chat.editor.layout(width);
    let (mut first, mut shown) = (0, layout.rows.len());
    if shown > allow {
        // Keep the cursor's row in view, the tail of a long message.
        first = layout.cursor.0.saturating_sub(allow - 1).min(shown - allow);
        shown = allow;
    }
    for (i, r) in layout.rows.iter().enumerate().skip(first).take(shown) {
        let lead = if i == 0 {
            span(Sty::Cyan, "› ")
        } else {
            span(Sty::Plain, "  ")
        };
        rows.push(vec![lead, span(Sty::Plain, r.text.clone())]);
    }
    let cursor = (
        above + layout.cursor.0.saturating_sub(first),
        PREFIX + layout.cursor.1,
    );

    for (i, c) in menu.iter().take(6).enumerate() {
        let sty = if i == chat.menu.min(menu.len() - 1) {
            Sty::Cyan
        } else {
            Sty::Dim
        };
        rows.push(one(sty, format!("  /{:<8} {}", c.name, c.help)));
    }
    rows.push(status_line(chat, width));
    Dock { rows, cursor }
}
