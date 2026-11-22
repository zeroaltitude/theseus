//! A stored piece's source, read again today ([`super::show::Source`]): a view record is a map of what WAS
//! admitted, and provenance that cannot be read any more keeps a piece out of every reader. The check is the
//! gate's own: the source session's marks (`view_gate::session_marks`: its place and today's reading of it)
//! against a point of view through `view_gate::Gate::check`. Anything goes (2026-11-22): who is present, a
//! seal, a move or a veil holds nothing.
//!
//! - An exchange range: each exchange's session, through its exchange record (`exchange.<turn>`) for a
//!   range of one, and through the place's touch index for a longer one, read back at most
//!   [`RANGE_MAX`] exchanges; a range that does not resolve whole is not shown.
//! - Code's own text (the frame, the clock): every reader's; it has no source.
//! - A named thing's card and a waiting record: every session it was read from (its `from`, ids only)
//!   readable today; the facts about the person: shown.
//! - A recalled item: its session's marks through the gate.
//! - A fold node: no session this build can prove today, so not shown.

use std::collections::BTreeSet;

use theseus_memory::gate::{PointOfView, Veil};

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
    /// Whether `session`'s marks pass the gate for this reader: its place reads, and today's reading of it.
    fn clear(&self, session: &str) -> bool {
        let m = view_gate::session_marks(self.runner, self.asker, session, Veil::Clear);
        crate::view::sources_names::place_reads(m.place.as_deref())
            && Gate::check(&self.pov, &m).is_ok()
    }

    /// Whether a card's or a waiting line's provenance holds today: every session it read, and every
    /// exchange it read, readable.
    fn proven(&self, p: &Provenance) -> bool {
        p.sessions.iter().all(|s| self.clear(s))
            && p.exchanges
                .iter()
                .all(|id| match exchange::exchange(&self.runner.store, id) {
                    Ok(Some(e)) => self.clear(&e.session_id),
                    _ => false,
                })
    }

    /// The sessions behind an exchange range of `place`; none when they cannot all be proven.
    fn sessions(&self, place: &str, range: &SourceRange) -> Option<BTreeSet<String>> {
        let store = &self.runner.store;
        let one = |turn: &str| -> Option<String> {
            let e = exchange::exchange(store, turn).ok()??;
            (e.place == place).then_some(e.session_id)
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
            if !matches!(exchange::exchange(store, &t.exchange), Ok(Some(_))) {
                return None;
            }
            out.insert(t.session_id);
        }
        Some(out)
    }
}

impl Source for GateSource<'_> {
    fn may_show(&self, range: &SourceRange, _marks: Option<&Marks>) -> bool {
        let place = match &range.space {
            Space::Code { .. } | Space::Facts { .. } => return true,
            Space::Exchanges { place } => place,
            // A named thing's card and what waits: its provenance proven today.
            Space::Card { from, .. } | Space::Waiting { from, .. } => {
                return from.as_ref().is_some_and(|p| self.proven(p))
            }
            Space::Recalled { session, .. } => return self.clear(session),
            _ => return false,
        };
        let Some(sessions) = self.sessions(place, range) else {
            return false;
        };
        sessions.iter().all(|s| self.clear(s))
    }

    /// A line of work, a task: shown.
    fn at_hand(&self, _at_hand: &AtHand) -> bool {
        true
    }
}
