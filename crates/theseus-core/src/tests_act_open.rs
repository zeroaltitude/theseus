//! route.v10's shape acting in a test (theseus-h66v4.11). Where `ROUTE_PACK` still names route.v8
//! the fixture shaped like route.v10 stands in for it (`JudgeService::stand_in`, which keeps every
//! name the daemon compares meaning what it means); once `ROUTE_PACK` names route.v10 the acting
//! pack is the embedded one and nothing stands in. The tests that use it build the same on either
//! side of the flip.

use crate::judge::inbound::ROUTE_PACK;
use crate::tests_route::Rig;

/// `r`'s route pack asks `model` and `effort` as route.v10 does.
pub(crate) fn act_open(r: &Rig) {
    if ROUTE_PACK == "route.v8" {
        let mut p = theseus_judge::Pack::parse(include_str!("../fixtures/packs/route.open.toml"))
            .expect("the fixture parses");
        assert_eq!(p.name(), "route.v10", "the fixture is shaped like v10");
        p.version = 8;
        r.core.runner.judge.stand_in(ROUTE_PACK, p);
    }
}
