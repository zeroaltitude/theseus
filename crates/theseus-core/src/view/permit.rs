//! The gate behind the view's seam (issue 44; `docs/design/view-gate.md`): [`Permit`] is the [`Gate`] every
//! selector's pieces pass. Anything goes (2026-11-22): it admits every piece that names its source place.
//! Who is present, where the turn is, a seal, a veil or a move withholds nothing; the seam's own check stays
//! ([`super::gate::admit_checked`]: a piece with no source place is not admitted, and only
//! [`super::gate::Admitted::code`] lays code's own text).
//!
//! One scope rule, not a withholding rule: a turn of a self-loop session that does not build (a reviewer, an
//! approver, a mint's writer, one whose role is not recorded) lays its own place only
//! (`view_gate::Verified::own_place_only`): its own session's exchanges at its anchor, and, for a fork of such
//! a session, the run the fork was handed (`fork::window::inherited`, laid through the fork's cut); nothing
//! else a selector reads ([`Permit::scoped`]). A builder and a fixer are laid the widest, as every other turn
//! is.

use std::collections::HashSet;

use super::gate::{Admitted, Gate};
use super::{Piece, PointOfView, Space};
use crate::view_gate::Verified;

/// The gate the view's pieces pass, for one turn: with, for the turn of a self-loop session that does not
/// build, the exchanges its own session ran at its anchor (the only pieces it admits).
pub struct Permit(Option<HashSet<String>>);

impl Permit {
    /// A permit with no scope (tests and readers that are no turn's view; a turn's is [`Self::scoped`]).
    pub fn of(_verified: Verified) -> Self {
        Permit(None)
    }

    /// The permit of `session`'s turn `turn` for `verified`: for the turn of a self-loop session that does not
    /// build, its own place (`view_gate::own_turns` at the anchor) and the turns of the run it was `handed` as
    /// a fork (none for a session that is no fork); an unread scope, nothing; every other turn, as
    /// [`Self::of`].
    pub fn scoped(
        store: &crate::store::Store,
        (session, turn): (&str, &str),
        verified: &Verified,
        handed: &[&str],
    ) -> Self {
        if verified.scope_unread() {
            // The session's scope cannot be read: nothing is admitted (a turn's view
            // fails before this, `view_built`; this holds for every other reader of the scope).
            return Permit(Some(HashSet::new()));
        }
        match verified.own_place_only() {
            true => Permit(Some(
                crate::view_gate::own_turns(store, session, turn, &verified.pov().anchor)
                    .into_iter()
                    .chain(handed.iter().map(|t| t.to_string()))
                    .collect(),
            )),
            false => Permit(None),
        }
    }

    /// Whether `piece` is in this turn's scope: every piece, or, scoped, an exchange its own session ran or
    /// it was handed.
    fn in_scope(&self, piece: &Piece) -> bool {
        let Some(own) = &self.0 else {
            return true;
        };
        let r = &piece.range;
        matches!(r.space, Space::Exchanges { .. })
            && own.contains(&r.first.id)
            && own.contains(&r.last.id)
    }
}

impl Gate for Permit {
    fn own_place_only(&self) -> bool {
        self.0.is_some()
    }

    fn admit(&self, _pov: &PointOfView, piece: Piece) -> Option<Admitted<Piece>> {
        if !self.in_scope(&piece) {
            return None;
        }
        super::gate::admit_checked(piece)
    }
}
