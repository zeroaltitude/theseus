//! The gate's types and its rule (issue 44, step 2; the design's §4).
//!
//! - **A decision only** ([`check`]): what a model may be shown is a value
//!   of the core's view (`theseus_core::view::gate::Admitted`), made only by
//!   the core's gate; this crate says yes or why not.
//! - **Anything goes** (the owner, 2026-11-22): no presence, place, seal or
//!   veil withholds a piece from any point of view. The one rule left: a
//!   piece whose place cannot be read is not shown, since nothing could say
//!   whose or where it is.
//! - **The marks still decode**: [`Present`], [`Seal`], [`Veil`] and
//!   [`Denied`] keep every value a stored record or a caller names; the rule
//!   reads none of them.

use crate::recall::Place;

/// Who may see this context (§4's `present`). Ordered from the widest to the
/// narrowest: the greater of two is the narrower.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Present {
    /// The terminal, the web page or a direct conversation with the owner,
    /// and this input carries the owner's mark: no agent behind it.
    OwnerAlone,
    /// A private place that is not the owner alone: a channel bound private
    /// (whoever has joined it), an agent's process at the terminal, an input
    /// with no mark.
    PrivateChannel,
    /// A shared place.
    Shared,
    /// The open view (theseus-a1wvq.7).
    Open,
    /// Not known: admits nothing.
    Unknown,
}

impl Present {
    /// The value `as_str` names; none for any other word.
    pub fn named(s: &str) -> Option<Present> {
        [
            Present::OwnerAlone,
            Present::PrivateChannel,
            Present::Shared,
            Present::Open,
            Present::Unknown,
        ]
        .into_iter()
        .find(|p| p.as_str() == s)
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Present::OwnerAlone => "owner_alone",
            Present::PrivateChannel => "private_channel",
            Present::Shared => "shared",
            Present::Open => "open",
            Present::Unknown => "unknown",
        }
    }
}

/// One turn's point of view, as the gate reads it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PointOfView {
    /// The place the turn is anchored at: a binding's target, or the local
    /// place the core names for the terminal and the web page.
    pub anchor: String,
    /// Today's reading of the anchor, for the place rule.
    pub place: Place,
    pub present: Present,
}

/// A piece's seal, as its thread says it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Seal {
    /// No seal, or the seal is of the asking thread itself.
    Clear,
    /// Its thread is sealed, or a seal is proposed for it.
    Held,
    /// Its thread could not be read.
    Unread,
}

/// A piece's veil (an imported episode's sensitivity).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Veil {
    Clear,
    /// `personal` or `partner-confidential`: its text stays veiled.
    Veiled,
    /// Its labels could not be read.
    Unread,
}

/// What the gate reads of one piece.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Marks {
    /// The place it came from, as the anchor is named; none when it cannot
    /// be read.
    pub place: Option<String>,
    /// Today's reading of that place.
    pub class: Place,
    /// When it was written (ms). The rule reads it no more: the open view
    /// and its taint were deleted with the restricted-origin rule.
    pub time_ms: u64,
    pub seal: Seal,
    pub veil: Veil,
}

/// Why a piece was denied.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Denied {
    /// The point of view's presence is not known.
    Unknown,
    /// One of the piece's marks could not be read.
    Unread,
    /// Sealed, or a seal is proposed.
    Sealed,
    Veiled,
    /// Today's place rule refuses it.
    Place,
    /// It is from another place, and the owner is not alone.
    Alone,
}

impl Denied {
    pub fn as_str(self) -> &'static str {
        match self {
            Denied::Unknown => "unknown",
            Denied::Unread => "unread",
            Denied::Sealed => "sealed",
            Denied::Veiled => "veiled",
            Denied::Place => "place",
            Denied::Alone => "alone",
        }
    }
}

/// Whether `pov` may be shown a piece of `marks`: every piece whose place
/// can be read, for every point of view.
pub fn check(_pov: &PointOfView, marks: &Marks) -> Result<(), Denied> {
    match marks.place {
        Some(_) => Ok(()),
        None => Err(Denied::Unread),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn marks(place: Option<&str>) -> Marks {
        Marks {
            place: place.map(str::to_string),
            class: Place::Shared("discord:channel:12".into()),
            time_ms: 1,
            seal: Seal::Held,
            veil: Veil::Veiled,
        }
    }

    /// Anything goes: a sealed, veiled piece of a shared place is shown to an
    /// unknown presence anchored elsewhere; a piece with no place is not.
    #[test]
    fn every_piece_with_a_place_is_shown_to_every_point_of_view() {
        for present in [
            Present::OwnerAlone,
            Present::PrivateChannel,
            Present::Shared,
            Present::Open,
            Present::Unknown,
        ] {
            let pov = PointOfView {
                anchor: "discord:channel:11".into(),
                place: Place::Unknown,
                present,
            };
            assert_eq!(check(&pov, &marks(Some("discord:channel:12"))), Ok(()));
            assert_eq!(check(&pov, &marks(None)), Err(Denied::Unread));
        }
    }
}
