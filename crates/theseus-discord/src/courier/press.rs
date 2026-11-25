//! A press on a fold, at its place's lane (issue 56). The lane is the one
//! writer of a fold's message, for a press as for a live update: a press
//! asks the lane first, and only when the lane will write is the fold's new
//! state made, by the place, and written as any live state is, after
//! whatever the lane already holds of it. So a press and the turn's own
//! updates reach Discord in the place's order, and the place's fold and
//! Discord's message never disagree.
//!
//! The lane answers a press by what it knows: while it waits out Discord, it
//! refuses (the presser is told; nothing changes); the press is honoured only
//! on the message the lane wrote that turn's fold to, from its map or its
//! durable rows (`recall.rs`); a press on any other message (a card, another
//! turn's fold) is refused, and a turn it never folded is too old. A folded
//! note's press (`fold_note.rs`) is checked against its post and written
//! here too.

use tokio::sync::oneshot;

use super::{fold_note, Lane};
use crate::render::Buttons;

/// What a press came to, for its presser.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Told {
    /// Shown as asked: nothing to say.
    Shown,
    /// The lane waits out Discord: nothing changed.
    Away,
    /// The pressed message is not the fold the press names.
    NotHere,
    /// No fold of that turn is known here.
    TooOld,
    /// The place shows neither its tool lines nor its thinking now.
    Hidden,
    /// The note's post cannot be read back for this message.
    NoteGone,
    /// No answer in time.
    Busy,
}

/// A press on the fold under `key`, for the lane to check before the place
/// shows it.
pub(crate) struct Press {
    pub key: String,
    /// Where it was pressed: the channel and the message.
    pub at: (u64, u64),
    /// Called once: `Ok` when the lane will write the fold (the place then
    /// makes its new state and sends it on), or why not.
    pub then: Box<dyn FnOnce(Result<(), Told>) + Send>,
}

/// A press on a folded note, checked and written by the lane.
pub(crate) struct NotePress {
    pub post: String,
    pub open: bool,
    pub at: (u64, u64),
    pub reply: oneshot::Sender<Told>,
}

/// A press the lane holds until its earlier writes are done.
pub(super) enum Pending {
    Fold(Box<Press>),
    Note(Box<NotePress>),
}

impl Pending {
    fn refuse(self, why: Told) {
        match self {
            Self::Fold(p) => (p.then)(Err(why)),
            Self::Note(n) => {
                let _ = n.reply.send(why);
            }
        }
    }
}

impl Lane {
    /// Whether the lane waits out Discord now.
    fn waits_out(&self) -> bool {
        self.retry_at
            .is_some_and(|at| tokio::time::Instant::now() < at)
    }

    /// A press come to the lane: refused at once while it waits out
    /// Discord; else kept, to be answered after the writes it holds.
    pub(super) fn pressed(&mut self, p: Pending) {
        if self.waits_out() {
            p.refuse(Told::Away);
        } else {
            self.presses.push(p);
        }
    }

    /// Discord went away: every press kept is refused, unchanged.
    pub(super) fn refuse_presses(&mut self) {
        for p in std::mem::take(&mut self.presses) {
            p.refuse(Told::Away);
        }
    }

    /// Each kept press, in order.
    pub(super) async fn presses(&mut self) {
        for p in std::mem::take(&mut self.presses) {
            if self.waits_out() {
                p.refuse(Told::Away);
                continue;
            }
            match p {
                Pending::Fold(p) => {
                    let r = self.fold_at(&p.key, p.at).await;
                    (p.then)(r);
                }
                Pending::Note(n) => {
                    let told = self.note(&n).await;
                    let _ = n.reply.send(told);
                }
            }
        }
    }

    /// Whether `at` is the message this lane wrote the fold `key` to.
    async fn fold_at(&mut self, key: &str, at: (u64, u64)) -> Result<(), Told> {
        let known = match self.msgs.get(key).copied() {
            Some(m) => Some(m),
            None => self.recall(key).await,
        };
        match known {
            None => Err(Told::TooOld),
            Some(m) if m == at => Ok(()),
            Some(_) => Err(Told::NotHere),
        }
    }

    /// A folded note pressed: its words, or its line again, on the message
    /// its post was written to, and on no other.
    async fn note(&mut self, n: &NotePress) -> Told {
        let a = self.shared.core.kernel.outbox_action(&n.post).ok().flatten();
        let ours = a
            .as_ref()
            .is_some_and(|a| a.external_op_id.as_deref() == Some(n.at.1.to_string().as_str()));
        let Some((a, text)) = a
            .filter(|_| ours)
            .and_then(|a| fold_note::words(&a).map(|t| (a, t)))
        else {
            return Told::NoteGone;
        };
        let content = if n.open {
            text
        } else {
            fold_note::line(theseus_core::outbox::kind_of(&a))
        };
        // The vault's values never leave, as at every exit (`write`).
        let content = self.shared.core.exits.text(&content).0.into_owned();
        let buttons = Buttons::FoldNote(n.post.clone(), n.open);
        match self.edit(n.at.0, n.at.1, &content, &buttons).await {
            Ok(()) => Told::Shown,
            Err(e) if e.away => {
                self.away(&e);
                Told::Away
            }
            Err(e) => {
                self.shared.board.error("note press", None, e.message);
                Told::NoteGone
            }
        }
    }
}
