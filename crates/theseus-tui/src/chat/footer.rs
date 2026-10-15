//! The chat's task footer (theseus-mnd9g): above the input line, one summary
//! line and a line for each task of this conversation that works or asks
//! (five at most, then `+3 more · ctrl-t`). It is a dock provider, so it is
//! repainted when the dock is: on a push, a key, or the minute a row's clock
//! turns over, and never on a timer of its own.
//!
//! It shows only while a task works or asks: a conversation whose tasks have
//! all ended has said so in its transcript, and keeps no row.

use theseus_client::render::task_tree;
use theseus_protocol::work::{QuestionKind, WorkView};

use super::dock::DockCtx;
use super::state::Chat;
use super::style::{one, span, Line, Sty};
use super::tasks::{keys_line, sty_of, wrapped, FOOTER_ROWS};

/// The footer's rows.
pub fn footer_rows(chat: &Chat, ctx: &DockCtx) -> Vec<Line> {
    if !chat.board.live {
        return Vec::new();
    }
    let views: Vec<WorkView> = chat.board.views.values().cloned().collect();
    let f = task_tree::footer(&views, ctx.now_ms, ctx.width, FOOTER_ROWS);
    if f.rows.is_empty() {
        return Vec::new();
    }
    let mut out = vec![one(Sty::Cyan, format!("◐ tasks · {}", f.counts.words()))];
    for row in &f.rows {
        out.push(one(sty_of(row.tag), row.text.clone()));
    }
    if f.more > 0 {
        out.push(one(Sty::Dim, format!("+{} more · ctrl-t", f.more)));
    }
    out
}

/// The question the keys answer, and how.
pub fn question_rows(chat: &Chat, ctx: &DockCtx) -> Vec<Line> {
    let Some(p) = chat.focused() else {
        return Vec::new();
    };
    let mut out = Vec::new();
    if let Some(id) = &chat.board.note {
        let who = chat
            .pending()
            .into_iter()
            .find(|q| &q.id == id)
            .and_then(|q| q.who)
            .unwrap_or_else(|| "this conversation".to_string());
        out.push(vec![
            span(Sty::Yellow, "? "),
            span(Sty::Bold, format!("declining {who}: a note for the model?")),
        ]);
        out.push(one(
            Sty::Yellow,
            "  enter sends it · esc declines without one",
        ));
        return out;
    }
    let title = match (&p.who, p.kind) {
        (None, QuestionKind::Budget) => p.words.clone(),
        (None, _) => format!("{}?", p.words),
        (Some(who), _) => format!("{who} asks: {}?", p.words),
    };
    for (i, row) in wrapped(&title, ctx.width.saturating_sub(2))
        .into_iter()
        .enumerate()
    {
        out.push(vec![
            span(Sty::Yellow, if i == 0 { "? " } else { "  " }),
            span(Sty::Bold, row.trim_start().to_string()),
        ]);
    }
    if p.who.is_none() {
        if let Some(q) = &chat.question {
            for row in wrapped(&format!("why: {}", q.reason), ctx.width.saturating_sub(2)) {
                out.push(one(Sty::Dim, format!("  {}", row.trim_start())));
            }
        }
    }
    let more = chat.pending().len().saturating_sub(1);
    let mut keys = if chat.board.writing {
        "  writing · esc to answer".to_string()
    } else {
        format!("  {}", keys_line(&p))
    };
    if more > 0 && !chat.board.writing {
        keys.push_str(&format!(" · tab next ({more} more)"));
    }
    if !chat.board.writing {
        keys.push_str(" · esc to write");
    }
    if let Some(at) = p.expires_at_ms.filter(|_| p.who.is_some()) {
        let left = at.saturating_sub(ctx.now_ms);
        keys.push_str(&format!(" · {} left", task_tree::clock(left)));
    }
    out.push(one(Sty::Yellow, keys));
    out
}
