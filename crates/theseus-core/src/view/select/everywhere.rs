//! EVERYWHERE (the owner's shape of a turn's context, 2026-11-24; slot F's place): the newest
//! [`EVERYWHERE_EXCHANGES`] exchanges from every place this conversation does not already show, one line each
//! with its place, its time and whose words they are, laid beside the message so that activity elsewhere never
//! rewrites the cached front. The renderer ends them with where the every-place pager resumes
//! (`context.history` with `place: "*"`).
//!
//! Every exchange a person opened is on some person's timeline, so the lines are found by walking every
//! person's timeline as one ([`Walk::every_person`]), within [`EVERYWHERE_READ_MOST`] references read; a wake,
//! which no person opened, is on none. Left out: every exchange of this place (its window and ladder show those
//! since its `/new` mark, and his words from before it are his past, never laid as anything else), a task's own
//! workings (`task:` places), the exchanges of a person present but the owner (the people block lays their
//! newest, `super::people`), and any the gate does not admit. Whose lines may be laid on whose turn is one
//! decision: [`may_lay`].
//!
//! A turn with no speaker (a wake, an agent's process) is told none of it, as slot F was.

use std::collections::BTreeSet;

use super::super::timeline::ExchangeRef;
use super::super::{Form, Piece, PointOfView, Reason};
use super::here::anchor_place;
use super::make::{self, Cut};
use super::people::{company, RECENT_LINE_TOKENS, RECENT_READ_MOST};
use super::sources::{ExchangeFacts, Sources};
use super::walk::Walk;
use super::{Band, Selector, Snapshot};

/// The newest exchanges from every place laid beside the message.
pub const EVERYWHERE_EXCHANGES: usize = 15;
/// The most references of every person's timelines read for them, a touch met twice counted twice.
pub const EVERYWHERE_READ_MOST: usize = RECENT_READ_MOST;
/// One line's words and `did` line: beyond it, the line is cut where it is laid, with its handle
/// (`forms::fit`), as a person's recent line is.
pub const EVERYWHERE_LINE_TOKENS: u64 = RECENT_LINE_TOKENS;

/// THE ONE SWITCH (the owner's ruling, 2026-11-24, "no conversations are private"): on a turn another person
/// speaks, the owner's own newest exchanges from elsewhere are laid as on his own turn. `false` would leave
/// them out of a guest's turn, and change nothing else.
pub const THE_OWNERS_LINES_ON_A_GUESTS_TURN: bool = true;

/// Whether a line may be laid beside the message: the one decision, asked of every line. `owner_speaks`: the
/// turn's speaker is the owner; `owners_line`: the owner opened the line's exchange.
pub fn may_lay(owner_speaks: bool, owners_line: bool) -> bool {
    owner_speaks || !owners_line || THE_OWNERS_LINES_ON_A_GUESTS_TURN
}

/// What the look across every place found: its pieces, newest first, and whether its walk stopped at its
/// bound with older exchanges not read.
pub struct Look {
    pub pieces: Vec<Piece>,
    pub stopped: bool,
}

/// The selector of [`Band::Everywhere`].
pub struct Everywhere<'a> {
    src: Sources<'a>,
}

impl<'a> Everywhere<'a> {
    pub fn new(src: Sources<'a>) -> Self {
        Self { src }
    }

    /// The newest lines from every place this conversation does not show, at most [`EVERYWHERE_EXCHANGES`],
    /// chosen over those the gate admits.
    pub fn look(&self, pov: &PointOfView, snap: &dyn Snapshot) -> anyhow::Result<Look> {
        let mut out = Vec::new();
        let Some(speaker) = pov.person.as_deref() else {
            return Ok(Look {
                pieces: out,
                stopped: false,
            });
        };
        let owner_speaks = self.src.people.owner(speaker);
        let here = anchor_place(pov);
        // The people present but the owner, by every handle: the people block lays their newest elsewhere.
        let present: BTreeSet<String> = (company(&self.src, pov)?.into_iter())
            .filter(|p| !p.owner)
            .flat_map(|p| p.handles.into_iter().chain([p.person]))
            .collect();
        let mut walk = Walk::every_person(snap, EVERYWHERE_READ_MOST)?;
        while out.len() < EVERYWHERE_EXCHANGES {
            let Some(touch) = walk.next_touch()? else {
                break;
            };
            let Some(e) = touch else { continue };
            let Some(f) = self.src.exchanges.facts(&e.turn_id)? else {
                continue;
            };
            let Some(person) = f.person.as_deref() else {
                continue;
            };
            let task = crate::task_graph::place::task_of(&f.place).is_some();
            let owners_line = self.src.people.owner(person);
            if here == Some(f.place.as_str())
                || task
                || present.contains(person)
                || !may_lay(owner_speaks, owners_line)
            {
                continue;
            }
            let piece = line(&e, &f);
            if !make::admits(&self.src, pov, &piece) {
                continue;
            }
            if !owners_line {
                // Asked so that the person's name, as the owner's records hold it, heads their words where
                // they are laid (`Sessions::names`).
                self.src.people.held(person)?;
            }
            out.push(piece);
        }
        Ok(Look {
            pieces: out,
            stopped: walk.stopped(),
        })
    }
}

/// One exchange as a line: its words and `did` line, within [`EVERYWHERE_LINE_TOKENS`].
fn line(e: &ExchangeRef, f: &ExchangeFacts) -> Piece {
    let mut p = make::exchange(
        e,
        f,
        Band::Everywhere,
        Reason::Elsewhere,
        Form::Line,
        Cut::Window,
    );
    p.sizes.line = p.sizes.line.min(EVERYWHERE_LINE_TOKENS);
    p
}

impl Selector for Everywhere<'_> {
    fn band(&self) -> Band {
        Band::Everywhere
    }

    fn select(&self, pov: &PointOfView, snap: &dyn Snapshot) -> anyhow::Result<Vec<Piece>> {
        let look = self.look(pov, snap)?;
        if look.stopped {
            tracing::debug!(
                lines = look.pieces.len(),
                "the look across every place stopped at its bound of references read"
            );
        }
        Ok(look.pieces)
    }
}
