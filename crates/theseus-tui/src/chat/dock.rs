//! The dock: the rows at the bottom of the terminal that are redrawn, and
//! nothing else is. From the top: the rows of [`ABOVE_INPUT`] (the live line
//! while a turn runs, a call in flight, a question, a notice), the input, the
//! command list while a `/` is typed, and the status line.
//!
//! A later row adds to the dock by adding a [`DockRows`] function to
//! [`ABOVE_INPUT`] (cli-tasks puts its task rows there) and a [`Segment`] to
//! [`status_segments`]; the dock composes them and knows no row's content.

use super::live::{elapsed, spinner};
use super::state::Chat;
use super::style::{one, span, Line, Sty};

/// What a provider of dock rows is given besides the chat.
pub struct DockCtx {
    pub now_ms: u64,
    pub width: usize,
}

/// A provider of rows for the dock above the input.
pub type DockRows = fn(&Chat, &DockCtx) -> Vec<Line>;

/// Every provider, top row first.
pub const ABOVE_INPUT: &[DockRows] = &[
    super::task_view::header_rows,
    turn_rows,
    tool_rows,
    super::footer::footer_rows,
    super::task_picker::rows,
    super::ask::rows,
    super::queue::rows,
    super::rewind::rows,
    notice_rows,
];

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
    if let Some(words) = chat
        .fill
        .and_then(|(t, w)| theseus_protocol::fill::status_words(Some(t), Some(w)))
    {
        out.push(Segment {
            text: words,
            keep: 4,
        });
    }
    // The approved plan's count, `plan 2/5` (theseus-lyxy).
    if let Some(p) = theseus_protocol::modes::plan_line(&chat.plan) {
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
pub(super) fn status_line(chat: &Chat, width: usize) -> Line {
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

fn notice_rows(chat: &Chat, _ctx: &DockCtx) -> Vec<Line> {
    chat.notice
        .iter()
        .map(|(text, _)| one(Sty::Yellow, text.clone()))
        .collect()
}

/// How many rows a menu (the commands, the `@` files) shows at once.
pub const MENU_ROWS: usize = 6;

/// The slice of a menu of `len` rows that shows with row `sel` chosen: it
/// scrolls just far enough that the chosen row is always one of them.
///
/// `rows` is every row the menu and the count row under it may take
/// together, which is what the pane leaves them: a short pane shows fewer
/// than [`MENU_ROWS`] of them, and none at all when it has no room
/// (theseus-nuqut). With `MENU_ROWS + 1` rows or more the window is what it
/// always was.
pub fn menu_window(len: usize, sel: usize, rows: usize) -> (usize, usize) {
    // A menu that does not fit keeps a row for the count of what it left out.
    let fits = match len > rows {
        true => rows.saturating_sub(1),
        false => len,
    };
    let shown = len.min(MENU_ROWS).min(fits);
    if shown == 0 {
        return (0, 0);
    }
    let sel = sel.min(len.saturating_sub(1));
    let first = (sel + 1).saturating_sub(shown).min(len - shown);
    (first, shown)
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
    if let Some(p) = &chat.picker {
        return super::picker::dock(chat, p, &ctx);
    }
    let mut rows: Vec<Line> = Vec::new();
    for provider in ABOVE_INPUT {
        rows.extend(provider(chat, &ctx));
    }
    let above = rows.len();
    if chat.answering() || chat.task_picker.is_some() {
        // The question stands in the input's place; the cursor rests on its keys.
        rows.push(status_line(chat, width));
        return Dock {
            cursor: (above.saturating_sub(1), 0),
            rows,
        };
    }

    // The input, with room left for the rows under it. The rows under the
    // input never crowd it out: the input's own row and the status line are
    // kept, and the menu and the `@` list share what is left, so in a pane
    // of eight rows the line being typed is still on it (theseus-nuqut).
    let height = usize::from(chat.height);
    let under = height.saturating_sub(above + 2);
    let menu: Vec<_> = chat.suggestions();
    let (mfirst, mshown) = menu_window(menu.len(), chat.menu, under);
    let more = usize::from(mshown > 0 && menu.len() > mshown);
    let at_rows = chat.at_rows(under.saturating_sub(mshown + more));
    let below = mshown + more + at_rows.len() + 1;
    let allow = height.saturating_sub(above + below + 1).max(1);
    let prompt = chat.prompt();
    let pad = prompt.chars().count();
    let layout = chat.editor.layout(chat.input_width());
    let (mut first, mut shown) = (0, layout.rows.len());
    if shown > allow {
        // Keep the cursor's row in view, the tail of a long message.
        first = layout.cursor.0.saturating_sub(allow - 1).min(shown - allow);
        shown = allow;
    }
    for (i, r) in layout.rows.iter().enumerate().skip(first).take(shown) {
        let lead = if i == 0 {
            span(Sty::Cyan, prompt.clone())
        } else {
            span(Sty::Plain, " ".repeat(pad))
        };
        rows.push(vec![lead, span(Sty::Plain, r.text.clone())]);
    }
    let cursor = (
        above + layout.cursor.0.saturating_sub(first),
        pad + layout.cursor.1,
    );

    let wide = menu.iter().map(|c| c.name.len()).max().unwrap_or(0).max(8);
    for (i, c) in menu.iter().enumerate().skip(mfirst).take(mshown) {
        let sty = if i == chat.menu.min(menu.len() - 1) {
            Sty::Cyan
        } else {
            Sty::Dim
        };
        rows.push(one(sty, format!("  /{:<wide$} {}", c.name, c.help)));
    }
    if more > 0 {
        rows.push(one(
            Sty::Dim,
            format!("  {} more · ↑/↓ scroll · esc closes", menu.len() - mshown),
        ));
    }
    rows.extend(at_rows);
    rows.push(status_line(chat, width));
    Dock { rows, cursor }
}
