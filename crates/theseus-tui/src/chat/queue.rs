//! The queue (theseus-wy6v.1): a message typed while a turn runs waits in the
//! dock (`queued › …`), not in the transcript. ↑ with an empty line pulls the
//! last one back to edit, and Ctrl-Enter stops the turn and sends it now.
//! Until the daemon delivers a queued message into the running turn at its
//! next loop boundary (steer-queued), it waits for the turn's end and goes as
//! the next turn, one at a time; then it is echoed into the transcript, where
//! it belongs in the order of the conversation.

use super::dock::DockCtx;
use super::editor::Taken;
use super::state::{Chat, Effect};
use super::style::{span, Line, Sty};

/// Queued messages the dock shows by name; the rest are counted.
const SHOWN: usize = 3;

impl Chat {
    /// Whether a message typed now waits: a turn runs, or the last queued
    /// message has been sent and its turn has not begun.
    pub(super) fn must_queue(&self) -> bool {
        self.live.turn.is_some() || self.released
    }

    /// Hold `text` for the turn's end.
    pub(super) fn enqueue(&mut self, taken: Taken) {
        self.queued.push(taken);
    }

    /// ↑ on an empty line: the last queued message comes back to be edited.
    pub(super) fn unqueue(&mut self) -> bool {
        match self.queued.pop() {
            Some(taken) => {
                self.editor.insert_str(&taken.text);
                true
            }
            None => false,
        }
    }

    /// Ctrl-Enter: the line (or the last queued message) goes first, and the
    /// turn that runs stops, so it goes now.
    pub(super) fn send_now(&mut self) -> Vec<Effect> {
        let typed = self.editor.take();
        if !typed.text.trim().is_empty() {
            self.queued.insert(0, typed);
        } else if let Some(last) = self.queued.pop() {
            self.queued.insert(0, last);
        }
        if self.queued.is_empty() {
            return Vec::new();
        }
        if self.live.turn.is_some() {
            return self.stop_turn(false);
        }
        self.release_queue()
    }

    /// The turn is over: the first queued message goes as a turn of its own.
    pub(super) fn release_queue(&mut self) -> Vec<Effect> {
        if self.live.turn.is_some() || self.released || self.queued.is_empty() {
            return Vec::new();
        }
        let Some(sid) = self.session.clone() else {
            return Vec::new();
        };
        let taken = self.queued.remove(0);
        self.released = true;
        self.echo(&taken.shown);
        vec![self.submit_call(&sid, taken.text)]
    }
}

/// The queue's rows: `queued › …`, the first line of each.
pub fn rows(chat: &Chat, ctx: &DockCtx) -> Vec<Line> {
    let mut out: Vec<Line> = chat
        .queued
        .iter()
        .take(SHOWN)
        .map(|taken| {
            let text = &taken.shown;
            let first = text.lines().next().unwrap_or("");
            let room = ctx.width.saturating_sub(12);
            let mut shown: String = first.chars().take(room).collect();
            if first.chars().count() > room || text.lines().count() > 1 {
                shown.push('…');
            }
            vec![
                span(Sty::Dim, "queued "),
                span(Sty::Cyan, "› "),
                span(Sty::Dim, shown),
            ]
        })
        .collect();
    if chat.queued.len() > SHOWN {
        out.push(vec![span(
            Sty::Dim,
            format!("queued · +{} more", chat.queued.len() - SHOWN),
        )]);
    }
    if !out.is_empty() {
        out.push(vec![span(
            Sty::Dim,
            "  ↑ edit it · ctrl+enter: stop and send now",
        )]);
    }
    out
}
