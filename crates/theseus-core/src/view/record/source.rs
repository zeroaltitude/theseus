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
//! - A fold node, a fact, a card, a task: no session this build can prove today, so not shown.
//! - A piece whose stored marks say sealed or veiled stays out (marks only grow).

use std::collections::BTreeSet;

use theseus_memory::gate::{PointOfView, Present, Veil};

use super::super::{AtHand, Marks, SourceRange, Space};
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
        out.extend(
            page.into_iter()
                .filter(|t| t.at_ms >= range.first.at_ms)
                .map(|t| t.session_id),
        );
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
            // A named thing's card, what waits and the facts about the person: the owner alone, its
            // stored seal clear (above).
            Space::Card { .. } | Space::Waiting { .. } | Space::Facts { .. } => {
                return self.pov.present == Present::OwnerAlone
            }
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
