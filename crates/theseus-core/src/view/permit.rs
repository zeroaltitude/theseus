//! The gate behind the view's seam (issue 44, step 2; `docs/design/view-gate.md`): [`Permit`] is the
//! [`Gate`] every selector's pieces pass. The rule is the one `crate::view_gate` runs for today's readers
//! (`theseus_memory::gate`), so the view and they decide alike: unknown presence admits nothing; a piece with
//! no place, or of an unknown audience, is denied; a seal or a veil denies; today's place rule must hold; and
//! a piece from another place than the anchor needs the owner alone. A task's anchor is `task:<id>`.

use theseus_memory::gate as rule;
use theseus_memory::recall::Place;

use super::gate::{Admitted, Gate};
use super::{Anchor, Audience, Piece, PointOfView, Present};

/// The gate the view's pieces pass.
pub struct Permit;

/// The rule's point of view for the view's.
pub fn point_of_view(pov: &PointOfView) -> rule::PointOfView {
    let anchor = match &pov.anchor {
        Anchor::Place(p) => p.clone(),
        Anchor::Task(t) => format!("task:{t}"),
    };
    let (present, place) = match pov.present {
        Present::OwnerAlone => (rule::Present::OwnerAlone, Place::Private),
        Present::Private => (rule::Present::PrivateChannel, Place::Private),
        Present::Shared => (rule::Present::Shared, Place::Shared(anchor.clone())),
        Present::Open => (rule::Present::Open, Place::Open(None)),
        Present::Unknown => (rule::Present::Unknown, Place::Unknown),
    };
    rule::PointOfView {
        anchor,
        place,
        present,
    }
}

/// The rule's marks for a piece.
pub fn marks(piece: &Piece) -> rule::Marks {
    let m = &piece.marks;
    let class = match (m.audience, m.place.as_deref()) {
        (Audience::Private, _) => Place::Private,
        (Audience::Shared, Some(p)) => Place::Shared(p.to_string()),
        (Audience::Open, _) => Place::Open(None),
        _ => Place::Unknown,
    };
    rule::Marks {
        place: m.place.clone(),
        class,
        time_ms: piece.range.last.at_ms,
        seal: match m.sealed {
            true => rule::Seal::Held,
            false => rule::Seal::Clear,
        },
        veil: match m.veiled {
            true => rule::Veil::Veiled,
            false => rule::Veil::Clear,
        },
    }
}

impl Gate for Permit {
    fn admit(&self, pov: &PointOfView, piece: Piece) -> Option<Admitted<Piece>> {
        crate::view_gate::Gate::admit(&point_of_view(pov), &marks(&piece), ())
            .ok()
            .map(|_| Admitted::new(piece))
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::super::select::Band;
    use super::super::*;
    use super::*;

    fn pov(anchor: &str, present: Present) -> PointOfView {
        PointOfView {
            anchor: Anchor::Place(anchor.into()),
            person: None,
            present,
            at_hand: None,
            now_ms: 9_000,
        }
    }

    fn piece(place: Option<&str>, audience: Audience) -> Piece {
        let pos = Pos {
            at_ms: 1_000,
            id: "t1".into(),
        };
        Piece {
            range: SourceRange {
                space: Space::Exchanges {
                    place: place.unwrap_or("local").into(),
                },
                first: pos.clone(),
                last: pos,
            },
            band: Band::Recall,
            slot: Band::Recall.row().slot,
            reasons: BTreeSet::from([Reason::Recalled]),
            form: Form::Whole,
            marks: Marks {
                place: place.map(str::to_string),
                audience,
                author: Author::His {
                    may_authorise: true,
                },
                outside: false,
                sealed: false,
                veiled: false,
            },
            sizes: Sizes {
                whole: 100,
                spoken: 40,
                line: 10,
            },
            handle: Handle("x:t1".into()),
            unit: None,
        }
    }

    #[test]
    fn the_owner_alone_is_shown_his_other_places_and_no_one_else_is() {
        let dm = || piece(Some("discord:dm:7"), Audience::Private);
        assert!(Permit
            .admit(&pov("local", Present::OwnerAlone), dm())
            .is_some());
        assert!(Permit
            .admit(&pov("local", Present::Private), dm())
            .is_none());
        let lab = "discord:channel:11";
        assert!(Permit.admit(&pov(lab, Present::Private), dm()).is_none());
        let own = piece(Some(lab), Audience::Private);
        assert!(Permit.admit(&pov(lab, Present::Private), own).is_some());
    }

    #[test]
    fn a_missing_mark_a_seal_a_veil_or_an_unknown_presence_denies() {
        let alone = pov("local", Present::OwnerAlone);
        assert!(Permit
            .admit(&alone, piece(None, Audience::Private))
            .is_none());
        assert!(Permit
            .admit(&alone, piece(Some("local"), Audience::Unknown))
            .is_none());
        let mut sealed = piece(Some("local"), Audience::Private);
        sealed.marks.sealed = true;
        assert!(Permit.admit(&alone, sealed).is_none());
        let mut veiled = piece(Some("local"), Audience::Private);
        veiled.marks.veiled = true;
        assert!(Permit.admit(&alone, veiled).is_none());
        let unknown = pov("local", Present::Unknown);
        assert!(Permit
            .admit(&unknown, piece(Some("local"), Audience::Private))
            .is_none());
    }

    #[test]
    fn a_shared_place_is_shown_its_own_alone() {
        let pier = "discord:channel:12";
        let here = pov(pier, Present::Shared);
        assert!(Permit
            .admit(&here, piece(Some(pier), Audience::Shared))
            .is_some());
        assert!(Permit
            .admit(&here, piece(Some("local"), Audience::Private))
            .is_none());
        let quay = piece(Some("discord:channel:13"), Audience::Shared);
        assert!(Permit.admit(&here, quay).is_none());
    }
}
