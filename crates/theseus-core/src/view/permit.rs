//! The gate behind the view's seam (issue 44; `docs/design/view-gate.md`): [`Permit`] is the [`Gate`] every
//! selector's pieces pass. Anything goes (2026-11-22): it admits every piece that names its source place.
//! Who is present, where the turn is, a seal, a veil or a move withholds nothing; the only check left is
//! the seam's own ([`super::gate::admit_checked`]: a piece with no source place is not admitted, and only
//! [`super::gate::Admitted::code`] lays code's own text).

use super::gate::{Admitted, Gate};
use super::{Piece, PointOfView};
use crate::view_gate::Verified;

/// The gate the view's pieces pass, for one turn.
pub struct Permit;

impl Permit {
    /// The permit of the turn of `session`.
    pub fn for_turn(_runner: &crate::turn::TurnRunner, _session: &str) -> Self {
        Permit
    }

    /// A permit for a point of view already verified.
    pub fn of(_verified: Verified) -> Self {
        Permit
    }
}

impl Gate for Permit {
    fn admit(&self, _pov: &PointOfView, piece: Piece) -> Option<Admitted<Piece>> {
        super::gate::admit_checked(piece)
    }
}
