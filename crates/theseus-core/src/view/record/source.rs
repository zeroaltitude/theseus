//! A stored piece's source, read again today ([`super::show::Source`]): a view record is a map of what WAS
//! admitted, and a seal set since, a seal proposed since, or provenance that cannot be read any more keeps a
//! piece out of every reader. The check is the gate's own: the source session's marks
//! (`view_gate::session_marks`: its place, today's reading of it, its seal, failing closed) against a point of
//! view through `view_gate::Gate::check`.
//!
//! - An exchange range: each exchange's session, through its exchange record (`exchange.<turn>`) for a
//!   range of one, and through the place's touch index for a longer one, read back at most
//!   [`RANGE_MAX`] exchanges; a range that does not resolve whole is not shown.
//! - Code's own text (the frame, the clock): every reader's; it has no source.
//! - A named thing's card and a waiting record: the owner alone, and every session it was read from (its
//!   `from`, ids only) readable and clear today; the facts about the person: the owner alone.
//! - A fold node, a task: no session this build can prove today, so not shown.
//! - A piece whose stored marks say sealed or veiled stays out (marks only grow).

use std::collections::BTreeSet;

use theseus_memory::gate::{PointOfView, Present, Veil};

use super::super::{AtHand, Marks, Provenance, SourceRange, Space};
use super::show::Source;
use crate::exchange::{self, Root};
use crate::turn::TurnRunner;
use crate::view_gate::{self, Gate};

/// The most exchanges of one range read back to prove its sources.
pub const RANGE_MAX: usize = 200;

/// Today's sources, for one reader.
pub struct GateSource<'a> {
    pub runner: &'a TurnRunner,
    /// The reader's point of view: the turn's verified one for the model, the owner alone for his own
    /// inspection.
    pub pov: PointOfView,
    /// The reader's own session, which a seal of its own thread does not hold out; none for the owner's
    /// inspection, so no seal is lifted by inspecting.
    pub asker: Option<&'a str>,
}

impl GateSource<'_> {
    /// Whether a card's or a waiting line's provenance holds today: every session it read readable and clear
    /// for this reader, and every exchange it read readable, its session clear, and none of its messages
    /// moved since into a thread not proven clear (`view_gate::Moved`, read now).
    fn proven(&self, p: &Provenance) -> bool {
        let clear = |s: &str| {
            let m = view_gate::session_marks(self.runner, self.asker, s, Veil::Clear);
            crate::view::sources_names::place_reads(m.place.as_deref())
                && Gate::check(&self.pov, &m).is_ok()
        };
        let moved = view_gate::Moved::read(self.runner, self.asker);
        p.sessions.iter().all(|s| clear(s))
            && p.exchanges
                .iter()
                .all(|id| match exchange::exchange(&self.runner.store, id) {
                    Ok(Some(e)) => clear(&e.session_id) && !moved.holds(&e),
                    _ => false,
                })
    }

    /// The sessions behind an exchange range of `place`; none when they cannot all be proven, or when a
    /// message or reply of an exchange in it was moved into a thread not proven clear for this reader
    /// (`view_gate::Moved`: the owner's inspection lifts no seal).
    fn sessions(&self, place: &str, range: &SourceRange) -> Option<BTreeSet<String>> {
        let store = &self.runner.store;
        let moved = view_gate::Moved::read(self.runner, self.asker);
        let one = |turn: &str| -> Option<String> {
            let e = exchange::exchange(store, turn).ok()??;
            (e.place == place && !moved.holds(&e)).then_some(e.session_id)
        };
        let mut out = BTreeSet::from([one(&range.first.id)?, one(&range.last.id)?]);
        if range.first.id == range.last.id {
            return Some(out);
        }
        let before = range.last.at_ms.checked_add(1)?;
        let page = exchange::newest_before(store, Root::Place(place), before, RANGE_MAX).ok()?;
        let reached = page.iter().any(|t| t.at_ms < range.first.at_ms) || page.len() < RANGE_MAX;
        if !reached {
            return None;
        }
        for t in page.into_iter().filter(|t| t.at_ms >= range.first.at_ms) {
            if !matches!(exchange::exchange(store, &t.exchange), Ok(Some(e)) if !moved.holds(&e)) {
                return None;
            }
            out.insert(t.session_id);
        }
        Some(out)
    }
}

impl Source for GateSource<'_> {
    fn may_show(&self, range: &SourceRange, marks: Option<&Marks>) -> bool {
        if marks.is_some_and(|m| m.sealed || m.veiled) || self.pov.present == Present::Unknown {
            return false;
        }
        let place = match &range.space {
            Space::Code { .. } => return true,
            Space::Exchanges { place } => place,
            // A named thing's card and what waits: the owner alone, and its provenance proven and clear today;
            // the facts about the person (the owner's filing): the owner alone.
            Space::Card { from, .. } | Space::Waiting { from, .. } => {
                return self.pov.present == Present::OwnerAlone
                    && from.as_ref().is_some_and(|p| self.proven(p))
            }
            Space::Facts { .. } => return self.pov.present == Present::OwnerAlone,
            _ => return false,
        };
        let Some(sessions) = self.sessions(place, range) else {
            return false;
        };
        sessions.iter().all(|s| {
            let m = view_gate::session_marks(self.runner, self.asker, s, Veil::Clear);
            Gate::check(&self.pov, &m).is_ok()
        })
    }

    /// A line of work: the owner alone, and its thread neither sealed nor seal-pending (unreadable is no). A
    /// task: the owner alone.
    fn at_hand(&self, at_hand: &AtHand) -> bool {
        if self.pov.present != Present::OwnerAlone {
            return false;
        }
        match at_hand {
            AtHand::Task(_) => true,
            AtHand::LineOfWork(slug) => match self.runner.threads.all(&self.runner.store) {
                Ok(board) => board
                    .get(slug)
                    .is_none_or(|t| t.sealed.is_none() && t.seal_pending.is_none()),
                Err(_) => false,
            },
        }
    }
}
