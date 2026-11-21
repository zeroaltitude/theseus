//! The gate's seam: only content that passed the permission check reaches a model, and the type says so.
//! [`Admitted`] is made only in this module: by [`admit_checked`], after the gate's rule
//! (`crate::view_gate::Gate::check`) for the turn's verified point of view, and by the checked
//! [`Admitted::code`]. Its constructor is private (the contract's amendment of issue 44, step 2), so a
//! selector or a renderer cannot wrap a piece itself. The gate's logic (the place rule, `owner alone` for every cross-place kind,
//! the seal and the veil; a missing mark denies) is the gate work's, behind [`Gate`].

use super::{Piece, PointOfView, Space};
use crate::view_gate::Verified;

/// `piece`, admitted for the turn `verified` when the gate's rule lets it
/// through for the view's `pov` (which may only narrow the verified one).
pub(in crate::view) fn admit_checked(
    verified: &Verified,
    pov: &PointOfView,
    piece: Piece,
) -> Option<Admitted<Piece>> {
    let rule = super::permit::point_of_view(verified, pov);
    crate::view_gate::Gate::check(&rule, &super::permit::marks(&piece))
        .ok()
        .map(|()| Admitted::new(piece))
}

/// A value the gate admitted for one point of view.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Admitted<T>(T);

impl<T> Admitted<T> {
    /// Only this module makes one.
    fn new(value: T) -> Self {
        Self(value)
    }

    /// A value admitted by a test's own fake gate.
    #[cfg(test)]
    pub(in crate::view) fn for_tests(value: T) -> Self {
        Self(value)
    }

    pub fn get(&self) -> &T {
        &self.0
    }

    pub fn into_inner(self) -> T {
        self.0
    }
}

impl Admitted<Piece> {
    /// A piece laid by code with no source in the store ([`Space::Code`]: the frame's parts, the clock,
    /// residue lines) has nothing to permit; any other piece goes through a [`Gate`].
    /// A piece that names a source place, or carries a seal, a veil or outside text, is not code's own,
    /// whatever its space says: relabelling a stored piece as code does not get it past the gate.
    pub fn code(piece: Piece) -> Option<Self> {
        let m = &piece.marks;
        let source_free = m.place.is_none() && !m.sealed && !m.veiled && !m.outside;
        (matches!(piece.range.space, Space::Code { .. }) && source_free).then(|| Self::new(piece))
    }
}

/// The permission check, run before ranking and before any model reads a candidate, and again wherever text
/// reaches a model later (a page of history, a tool's result). What it denies is not counted or mentioned.
pub trait Gate {
    /// `None` denies.
    fn admit(&self, pov: &PointOfView, piece: Piece) -> Option<Admitted<Piece>>;
}
