//! The contract a model menu relies on (theseus-jid7t.2): the refusal
//! `session.profile` makes (`data_mark::profile_refusal`) and the route step's
//! floor (`floor_refuses`) agree for every (mark x origin), so a profile the
//! menu offers is one the turn's floor lets run, and one it greys out is one
//! the floor would move a turn off.

use super::{Floor, TurnRunner};
use crate::catalog::ModelOrigin;
use crate::data_mark::{per_turn_note, profile_refusal};
use crate::tests_route::rig;
use theseus_protocol::ops::DataMark;

const MARKS: [DataMark; 3] = [DataMark::Open, DataMark::Private, DataMark::Sensitive];

/// The floor's answer for a turn of a session marked `mark` whose labels allow
/// a restricted model (the most it lets through): the private marks are the names' floor.
fn route_refuses(r: &TurnRunner, mark: DataMark, profile: &str) -> bool {
    let floor = Floor {
        sensitive: mark >= DataMark::Private,
        cn_ok: true,
    };
    r.floor_refuses(floor, profile).is_some()
}

#[test]
fn the_menus_refusal_and_the_routes_floor_agree_for_every_mark_and_origin() {
    // `glm` is the `zai` provider's: its origin is the rig's to vouch.
    for origin in [
        ModelOrigin::Cn,
        ModelOrigin::Eu,
        ModelOrigin::Unknown,
        ModelOrigin::Us,
    ] {
        let r = rig(None, 1, |c| {
            c.serve_glm_unrestricted();
            c.vouch_zai(origin);
        });
        let (cfg, catalog) = (&r.core.cfg, &r.core.runner.catalog);
        assert_eq!(cfg.profile_origin("glm", catalog), origin);
        assert_eq!(cfg.profile_origin("sonnet", catalog), ModelOrigin::Us);
        for mark in MARKS {
            for profile in ["glm", "sonnet"] {
                let menu = profile_refusal(cfg, catalog, mark, profile).is_some();
                assert_eq!(
                    menu,
                    route_refuses(&r.core.runner, mark, profile),
                    "{origin:?} {mark:?} {profile}: the menu and the floor disagree"
                );
                let us = cfg.profile_origin(profile, catalog) == ModelOrigin::Us;
                assert_eq!(
                    menu,
                    mark >= DataMark::Private && !us,
                    "{origin:?} {mark:?} {profile}"
                );
                // What an open session's restricted profile cannot promise.
                let restricted = cfg.profile_origin(profile, catalog).restricted();
                assert_eq!(
                    per_turn_note(cfg, catalog, mark, profile).is_some(),
                    restricted && mark == DataMark::Open,
                    "{origin:?} {mark:?} {profile}"
                );
            }
        }
    }
}

/// A profile that does not resolve is refused whatever the mark, as the
/// route's names floor (which takes it for uncleared) reads it.
#[test]
fn an_unconfigured_profile_is_refused_at_every_mark() {
    let r = rig(None, 1, |_| {});
    let (cfg, catalog) = (&r.core.cfg, &r.core.runner.catalog);
    for mark in MARKS {
        assert!(profile_refusal(cfg, catalog, mark, "no-such").is_some());
    }
    assert!(route_refuses(&r.core.runner, DataMark::Private, "no-such"));
}
