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

use std::collections::HashSet;

use theseus_memory::gate as rule;
use theseus_memory::recall::Place;

use super::gate::{Admitted, Gate};
use super::{Anchor, Audience, Piece, PointOfView, Present};
use crate::view_gate::Verified;

/// The gate the view's pieces pass, for one turn; with the exchanges it reads as the turn's own history (an
/// explicit fork's inherited run, `crate::fork::window`), and the exchanges its own session ran at its anchor
/// ([`Permit::speaking`]).
pub struct Permit(Verified, HashSet<String>, HashSet<String>);

impl Permit {
    /// The permit of the turn of `session`, as that turn computed its presence.
    pub fn for_turn(runner: &crate::turn::TurnRunner, session: &str) -> Self {
        Self::of(Verified::of_session(runner, session))
    }

    /// A permit for a point of view already verified.
    pub fn of(verified: Verified) -> Self {
        Permit(verified, HashSet::new(), HashSet::new())
    }

    /// This permit, with `turns` the exchanges the asking turn's own session ran at the turn's anchor
    /// (`view_gate::own_turns`): a turn always sees what its own session said in the same place, whoever is
    /// present, an unknown presence included; their seals, veils and moved messages hold them out as ever.
    /// Nothing of another session, nothing of another place (the conductor's decision for the cut, on the
    /// owner's word that privacy is to be simpler where it stops things working).
    pub fn speaking(mut self, turns: impl IntoIterator<Item = String>) -> Self {
        self.2.extend(turns);
        self
    }

    /// Whether `piece` is an exchange the asking session ran at the turn's anchor.
    fn spoken(&self, pov: &PointOfView, piece: &Piece) -> bool {
        let r = &piece.range;
        let anchor = &self.0.pov().anchor;
        matches!(&r.space, super::Space::Exchanges { .. })
            && matches!(&pov.anchor, Anchor::Place(p) if p == anchor)
            && piece.marks.place.as_deref() == Some(anchor.as_str())
            && self.2.contains(&r.first.id)
            && self.2.contains(&r.last.id)
    }

    /// This permit, reading the exchanges `turns` as the turn's own history where the owner is alone in a
    /// private place: their place and audience pass as the anchor's, their seals and veils as they are.
    /// Anywhere else they pass the rule as any other place's exchange does.
    pub fn owning(mut self, turns: impl IntoIterator<Item = String>) -> Self {
        self.1.extend(turns);
        self
    }

    /// Whether `piece` is an exchange of the turn's own history, and the owner is alone in a private place.
    fn own(&self, pov: &PointOfView, piece: &Piece) -> bool {
        let r = &piece.range;
        let rule = point_of_view(&self.0, pov);
        matches!(r.space, super::Space::Exchanges { .. })
            && self.1.contains(&r.first.id)
            && self.1.contains(&r.last.id)
            && rule.present == rule::Present::OwnerAlone
            && rule.place == Place::Private
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
        if self.spoken(pov, &piece) {
            return super::gate::admit_spoken(&self.0, pov, piece);
        }
        match self.own(pov, &piece) {
            true => super::gate::admit_own(&self.0, pov, piece),
            false => super::gate::admit_checked(&self.0, pov, piece),
        }
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
            Present::Open | Present::Unknown => Place::Unknown,
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

    /// A turn always sees what its own session said at its anchor, whoever is present (the cut's rule): an
    /// unknown presence is shown those exchanges and nothing else; a seal still holds one out.
    #[test]
    fn a_turn_is_shown_what_its_own_session_said_at_its_anchor_whoever_is_present() {
        let unknown = || {
            Permit::of(Verified::for_tests(rule::PointOfView {
                anchor: "local".into(),
                place: Place::Private,
                present: rule::Present::Unknown,
            }))
        };
        let at = pov("local", Present::Unknown);
        let own = || piece(Some("local"), Audience::Private);
        assert!(
            unknown().admit(&at, own()).is_none(),
            "not its own: nothing"
        );
        let p = unknown().speaking(["t1".to_string()]);
        assert!(p.admit(&at, own()).is_some());
        assert!(p
            .admit(&at, piece(Some("discord:dm:7"), Audience::Private))
            .is_none());
        let mut other = own();
        (other.range.first.id, other.range.last.id) = ("t2".into(), "t2".into());
        assert!(
            p.admit(&at, other).is_none(),
            "another turn, another session's"
        );
        let mut sealed = own();
        sealed.marks.sealed = true;
        assert!(p.admit(&at, sealed).is_none());
        assert!(p
            .admit(&pov("discord:dm:7", Present::Unknown), own())
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
