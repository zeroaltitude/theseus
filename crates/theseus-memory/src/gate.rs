//! The gate's type and its rule (issue 44, step 2; the design's §4): what
//! may reach a model for one point of view, decided once, from marks alone.
//!
//! - **A decision only** ([`check`]): what a model may be shown is a value
//!   of the core's view (`theseus_core::view::gate::Admitted`), made only by
//!   the core's gate; this crate says yes or why not. The core's gate
//!   (`theseus_core::view_gate`) is the one caller of [`check`]: it reads the
//!   marks from the store and the point of view from the turn, and its
//!   registry test fails on a second caller.
//! - **The point of view** ([`PointOfView`]): the place a turn is anchored
//!   at, today's reading of that place ([`Place`]), and who is present
//!   ([`Present`]). `OwnerAlone` is the narrowest value: the terminal, the web
//!   page or a direct conversation with the owner, and an input that carries
//!   the owner's mark. A channel is never alone, whoever is in it.
//! - **The rule**, in order: a point of view whose presence is unknown admits
//!   nothing; a piece whose place, seal or veil cannot be read is denied (a
//!   missing or uncertain mark denies); a sealed or veiled piece is denied;
//!   today's place rule ([`Place::may_draw_at`]) must hold; and a piece from
//!   any other place than the anchor needs `OwnerAlone`. No rule here is by
//!   model provider: what a model is shown depends on who is present.
//! - **Marks only grow**: a piece more marked is never admitted where a less
//!   marked one was denied (the property tests below).

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
    /// When it was written (ms), for the open view's taint.
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

/// Whether `pov` may be shown a piece of `marks`: the rule of the module.
pub fn check(pov: &PointOfView, marks: &Marks) -> Result<(), Denied> {
    if pov.present == Present::Unknown || pov.place == Place::Unknown {
        return Err(Denied::Unknown);
    }
    let Some(from) = marks.place.as_deref() else {
        return Err(Denied::Unread);
    };
    if marks.class == Place::Unknown || marks.seal == Seal::Unread || marks.veil == Veil::Unread {
        return Err(Denied::Unread);
    }
    if marks.seal == Seal::Held {
        return Err(Denied::Sealed);
    }
    if marks.veil == Veil::Veiled {
        return Err(Denied::Veiled);
    }
    if !pov.place.may_draw_at(&marks.class, marks.time_ms) {
        return Err(Denied::Place);
    }
    // Alone only where today's rule reads the anchor as private: a
    // presence that says otherwise is not believed.
    let alone = pov.present == Present::OwnerAlone && pov.place == Place::Private;
    if from != pov.anchor && !alone {
        return Err(Denied::Alone);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    const PLACES: [&str; 4] = [
        "local",
        "discord:dm:7",
        "discord:channel:11",
        "discord:channel:12",
    ];

    fn reading(place: &str) -> Place {
        match place {
            "discord:channel:12" => Place::Shared(place.into()),
            _ => Place::Private,
        }
    }

    fn marks(place: &str) -> Marks {
        Marks {
            place: Some(place.into()),
            class: reading(place),
            time_ms: 1,
            seal: Seal::Clear,
            veil: Veil::Clear,
        }
    }

    fn pov(anchor: &str, present: Present) -> PointOfView {
        PointOfView {
            anchor: anchor.into(),
            place: reading(anchor),
            present,
        }
    }

    #[test]
    fn the_owner_alone_draws_on_his_other_private_places() {
        let p = pov("local", Present::OwnerAlone);
        assert!(check(&p, &marks("discord:dm:7")).is_ok());
        assert!(check(&p, &marks("discord:channel:11")).is_ok());
        assert_eq!(check(&p, &marks("discord:dm:7")), Ok(()));
    }

    #[test]
    fn a_private_channel_draws_on_itself_alone() {
        let p = pov("discord:channel:11", Present::PrivateChannel);
        assert!(check(&p, &marks("discord:channel:11")).is_ok());
        assert_eq!(check(&p, &marks("local")).err(), Some(Denied::Alone));
        assert_eq!(check(&p, &marks("discord:dm:7")).err(), Some(Denied::Alone));
    }

    #[test]
    fn a_presence_of_owner_alone_in_a_shared_place_is_not_believed() {
        let p = pov("discord:channel:12", Present::OwnerAlone);
        assert!(check(&p, &marks("discord:channel:12")).is_ok());
        assert_eq!(check(&p, &marks("local")).err(), Some(Denied::Place));
        let mut odd = pov("discord:channel:12", Present::OwnerAlone);
        odd.place = Place::Shared("discord:channel:12".into());
        let mut there = marks("discord:channel:13");
        there.class = Place::Shared("discord:channel:12".into());
        assert_eq!(check(&odd, &there).err(), Some(Denied::Alone));
    }

    #[test]
    fn a_missing_or_uncertain_mark_denies() {
        let p = pov("local", Present::OwnerAlone);
        let mut m = marks("local");
        m.place = None;
        assert_eq!(check(&p, &m), Err(Denied::Unread));
        for f in [
            |m: &mut Marks| m.class = Place::Unknown,
            |m: &mut Marks| m.seal = Seal::Unread,
            |m: &mut Marks| m.veil = Veil::Unread,
        ] {
            let mut m = marks("local");
            f(&mut m);
            assert_eq!(check(&p, &m), Err(Denied::Unread));
        }
        assert_eq!(
            check(&pov("local", Present::Unknown), &marks("local")),
            Err(Denied::Unknown)
        );
    }

    #[test]
    fn a_seal_and_a_veil_deny_even_the_owner_alone_at_home() {
        let p = pov("local", Present::OwnerAlone);
        let mut m = marks("local");
        m.seal = Seal::Held;
        assert_eq!(check(&p, &m), Err(Denied::Sealed));
        let mut m = marks("local");
        m.veil = Veil::Veiled;
        assert_eq!(check(&p, &m), Err(Denied::Veiled));
    }

    fn present() -> impl Strategy<Value = Present> {
        prop_oneof![
            Just(Present::OwnerAlone),
            Just(Present::PrivateChannel),
            Just(Present::Shared),
            Just(Present::Open),
            Just(Present::Unknown),
        ]
    }

    fn piece() -> impl Strategy<Value = Marks> {
        (
            proptest::option::weighted(0.9, 0..PLACES.len()),
            0..6u8,
            0..3u8,
            0..3u8,
            0..4u64,
        )
            .prop_map(|(at, class, seal, veil, time_ms)| {
                let place = at.map(|i| PLACES[i].to_string());
                let class = match class {
                    0 => Place::Unknown,
                    1 => Place::Open(Some(2)),
                    _ => place.as_deref().map_or(Place::Unknown, reading),
                };
                let seal = [Seal::Clear, Seal::Held, Seal::Unread][seal as usize];
                let veil = [Veil::Clear, Veil::Veiled, Veil::Unread][veil as usize];
                Marks {
                    place,
                    class,
                    time_ms,
                    seal,
                    veil,
                }
            })
    }

    fn view() -> impl Strategy<Value = PointOfView> {
        (0..PLACES.len(), present(), 0..5u8).prop_map(|(at, present, open)| {
            let mut p = pov(PLACES[at], present);
            if open == 0 {
                p.place = Place::Open(None);
            }
            p
        })
    }

    proptest! {
        /// For a point of view that is not the owner alone, what is admitted
        /// is the same with every other place's pieces deleted.
        #[test]
        fn other_places_change_nothing_unless_the_owner_is_alone(
            p in view(),
            pieces in proptest::collection::vec(piece(), 0..24),
        ) {
            prop_assume!(p.present != Present::OwnerAlone);
            let admitted = |ps: &[Marks]| -> Vec<Marks> {
                ps.iter().filter(|m| check(&p, m).is_ok()).cloned().collect()
            };
            let own: Vec<Marks> = pieces
                .iter()
                .filter(|m| m.place.as_deref() == Some(p.anchor.as_str()))
                .cloned()
                .collect();
            prop_assert_eq!(admitted(&pieces), admitted(&own));
        }

        /// Marks only grow: sealing or veiling a piece never admits it where
        /// it was denied, and an admitted piece is admitted by today's place
        /// rule too.
        #[test]
        fn marks_only_grow(p in view(), m in piece(), seal in any::<bool>(), veil in any::<bool>()) {
            let mut more = m.clone();
            if seal && more.seal == Seal::Clear {
                more.seal = Seal::Held;
            }
            if veil && more.veil == Veil::Clear {
                more.veil = Veil::Veiled;
            }
            if check(&p, &m).is_err() {
                prop_assert!(check(&p, &more).is_err());
            }
            if check(&p, &more).is_ok() {
                prop_assert!(check(&p, &m).is_ok());
                prop_assert!(p.place.may_draw_at(&m.class, m.time_ms));
            }
        }
    }
}
