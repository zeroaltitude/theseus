//! The contract a model menu relies on (theseus-jid7t.2): the refusal
//! `session.profile` makes (`TurnRunner::profile_refusal`) and the route step's
//! floor (`floor_refuses`) agree for every (mark x origin), so a profile the
//! menu offers is one the turn's floor lets run, and one it greys out is one
//! the floor would move a turn off. A mark is what a session's labels show
//! (`ContentFlags::mark`): a label keeps a model of restricted origin off, and
//! nothing else.

use super::{Floor, TurnRunner};
use crate::catalog::ModelOrigin;
use crate::tests_route::rig;
use theseus_protocol::ops::DataMark;

const MARKS: [DataMark; 3] = [DataMark::Open, DataMark::Private, DataMark::Sensitive];

/// The floor's answer for a turn of a session marked `mark`: a labelled
/// session's turns may not use a restricted profile.
fn route_refuses(r: &TurnRunner, mark: DataMark, profile: &str) -> bool {
    let floor = Floor {
        cn_ok: mark == DataMark::Open,
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
                let menu = r.core.runner.profile_refusal(mark, profile).is_some();
                assert_eq!(
                    menu,
                    route_refuses(&r.core.runner, mark, profile),
                    "{origin:?} {mark:?} {profile}: the menu and the floor disagree"
                );
                // What an open session's restricted profile cannot promise.
                let restricted = cfg.profile_origin(profile, catalog).restricted();
                assert_eq!(
                    r.core.runner.per_turn_note(mark, profile).is_some(),
                    restricted && mark == DataMark::Open,
                    "{origin:?} {mark:?} {profile}"
                );
            }
        }
    }
}

/// A profile that does not resolve is refused whatever the mark.
#[test]
fn an_unconfigured_profile_is_refused_at_every_mark() {
    let r = rig(None, 1, |_| {});
    for mark in MARKS {
        assert!(r.core.runner.profile_refusal(mark, "no-such").is_some());
    }
    assert!(route_refuses(&r.core.runner, DataMark::Private, "no-such"));
}
