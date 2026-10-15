//! The chat's task footer (theseus-mnd9g): above the input line, one summary
//! line and a line for each task of this conversation that works or asks
//! (five at most, then `+3 more · ctrl-t`). It is a dock provider, so it is
//! repainted when the dock is: on a push, a key, or the minute a row's clock
//! turns over, and never on a timer of its own.
//!
//! It shows only while a task works or asks: a conversation whose tasks have
//! all ended has said so in its transcript, and keeps no row.

use theseus_client::render::task_tree;
use theseus_protocol::work::WorkView;

use super::dock::DockCtx;
use super::state::Chat;
use super::style::{one, Line, Sty};
use super::tasks::{sty_of, FOOTER_ROWS};

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
