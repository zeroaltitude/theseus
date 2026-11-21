//! The gate behind the view's seam (issue 44, step 2; `docs/design/view-gate.md`): [`Permit`] is the
//! [`Gate`] every selector's pieces pass. The rule is the one `crate::view_gate` runs for today's readers
//! (`theseus_memory::gate`), so the view and they decide alike: unknown presence admits nothing; a piece with
//! no place, or of an unknown audience, is denied; a seal or a veil denies; today's place rule must hold; and
//! a piece from another place than the anchor needs the owner alone.
//!
//! A permit is made for one turn from its verified point of view (`view_gate::Verified`: the presence the
//! turn computed from its input's mark, the place's class read on its own, the exact arrival place), never
//! from the view's own [`PointOfView`]: that one may only narrow what the permit allows. So an arrival that
//! calls itself owner-marked in a channel passes nothing from another place.

use theseus_memory::gate as rule;
use theseus_memory::recall::Place;

use super::gate::{Admitted, Gate};
use super::{Anchor, Audience, Piece, PointOfView, Present};
use crate::view_gate::Verified;

/// The gate the view's pieces pass, for one turn.
pub struct Permit(Verified);

impl Permit {
    /// The permit of the turn of `session`, as that turn computed its presence.
    pub fn for_turn(runner: &crate::turn::TurnRunner, session: &str) -> Self {
        Permit(Verified::of_session(runner, session))
    }

    /// A permit for a point of view already verified.
    pub fn of(verified: Verified) -> Self {
        Permit(verified)
    }
}

/// The rule's presence for the view's.
fn presence(p: Present) -> rule::Present {
    match p {
        Present::OwnerAlone => rule::Present::OwnerAlone,
        Present::Private => rule::Present::PrivateChannel,
        Present::Shared => rule::Present::Shared,
        Present::Open => rule::Present::Open,
        Present::Unknown => rule::Present::Unknown,
    }
}

/// The rule's point of view: the verified one, narrowed by the view's when the view's is narrower, and
/// nothing when the two name different anchors.
pub fn point_of_view(verified: &Verified, pov: &PointOfView) -> rule::PointOfView {
    let mut out = verified.pov().clone();
    let anchor = match &pov.anchor {
        Anchor::Place(p) => p.clone(),
        Anchor::Task(t) => format!("task:{t}"),
    };
    out.present = out.present.max(presence(pov.present));
    if anchor != out.anchor {
        out.present = rule::Present::Unknown;
    }
    out
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
        crate::view_gate::Gate::admit(&point_of_view(&self.0, pov), &marks(&piece), ())
            .ok()
            .map(|_| Admitted::new(piece))
    }
}

/// A piece of `place`, private, for the core's tests of the permit.
#[cfg(test)]
pub(crate) fn tests_piece(place: &str) -> Piece {
    tests::piece(Some(place), Audience::Private)
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

    pub(super) fn piece(place: Option<&str>, audience: Audience) -> Piece {
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

    /// A permit verified for `anchor` with `present`, as the turn would have
    /// computed it.
    fn permit(anchor: &str, present: Present) -> Permit {
        let place = match present {
            Present::Shared => Place::Shared(anchor.into()),
            Present::Open => Place::Open(None),
            Present::Unknown => Place::Unknown,
            _ => Place::Private,
        };
        Permit::of(Verified::for_tests(rule::PointOfView {
            anchor: anchor.into(),
            place,
            present: presence(present),
        }))
    }

    fn admits(anchor: &str, present: Present, p: Piece) -> bool {
        permit(anchor, present)
            .admit(&pov(anchor, present), p)
            .is_some()
    }

    #[test]
    fn the_owner_alone_is_shown_his_other_places_and_no_one_else_is() {
        let dm = || piece(Some("discord:dm:7"), Audience::Private);
        assert!(admits("local", Present::OwnerAlone, dm()));
        assert!(!admits("local", Present::Private, dm()));
        let lab = "discord:channel:11";
        assert!(!admits(lab, Present::Private, dm()));
        assert!(admits(
            lab,
            Present::Private,
            piece(Some(lab), Audience::Private)
        ));
    }

    /// The view's own point of view only narrows: a channel's arrival that
    /// says it is owner-marked computes `OwnerAlone`, and the permit, made
    /// from the turn's verified presence, still passes nothing from elsewhere.
    #[test]
    fn the_views_owner_alone_is_not_believed_over_the_verified_one() {
        let lab = "discord:channel:11";
        let arrival = Arrival {
            anchor: Anchor::Place(lab.into()),
            person: Some("owner".into()),
            owner_marked: true,
            audience: Audience::Private,
        };
        struct Snap;
        impl Snapshot for Snap {
            fn now_ms(&self) -> u64 {
                9_000
            }
            fn timeline(&self) -> &dyn Timeline {
                unreachable!("not read")
            }
            fn at_hand(&self, _: &Anchor) -> Option<AtHand> {
                None
            }
        }
        let claimed = PointOfView::compute(&arrival, &Snap);
        assert_eq!(claimed.present, Present::OwnerAlone);
        let p = permit(lab, Present::Private);
        assert!(p
            .admit(&claimed, piece(Some("local"), Audience::Private))
            .is_none());
        assert!(p
            .admit(&claimed, piece(Some(lab), Audience::Private))
            .is_some());
        // Unreadable proof: nothing at all.
        let none = permit(lab, Present::Unknown);
        assert!(none
            .admit(&claimed, piece(Some(lab), Audience::Private))
            .is_none());
        // A view anchored elsewhere than the verified turn: nothing.
        let alone = permit("local", Present::OwnerAlone);
        let elsewhere = pov("discord:dm:7", Present::OwnerAlone);
        assert!(alone
            .admit(&elsewhere, piece(Some("local"), Audience::Private))
            .is_none());
    }

    #[test]
    fn a_missing_mark_a_seal_a_veil_or_an_unknown_presence_denies() {
        let alone = |p| admits("local", Present::OwnerAlone, p);
        assert!(!alone(piece(None, Audience::Private)));
        assert!(!alone(piece(Some("local"), Audience::Unknown)));
        let mut sealed = piece(Some("local"), Audience::Private);
        sealed.marks.sealed = true;
        assert!(!alone(sealed));
        let mut veiled = piece(Some("local"), Audience::Private);
        veiled.marks.veiled = true;
        assert!(!alone(veiled));
        assert!(!admits(
            "local",
            Present::Unknown,
            piece(Some("local"), Audience::Private)
        ));
    }

    #[test]
    fn a_shared_place_is_shown_its_own_alone() {
        let pier = "discord:channel:12";
        assert!(admits(
            pier,
            Present::Shared,
            piece(Some(pier), Audience::Shared)
        ));
        assert!(!admits(
            pier,
            Present::Shared,
            piece(Some("local"), Audience::Private)
        ));
        let quay = piece(Some("discord:channel:13"), Audience::Shared);
        assert!(!admits(pier, Present::Shared, quay));
    }
}
