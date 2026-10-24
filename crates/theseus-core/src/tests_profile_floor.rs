//! The floor's refusal table, shared with the cockpit's model menu (theseus-jid7t.6): `cockpit/test/fixtures/
//! profile_floor.json` lists every mark × origin × known case and whether it is refused. This test holds
//! `data_mark::refusal_of` to every row, and `profile_list`'s `cleared` to the daemon's own config; the cockpit's `greyOf` is held to the same file.

use serde::Deserialize;
use theseus_protocol::ops::DataMark;

use crate::catalog::ModelOrigin;
use crate::data_mark::{profile_refusal, refusal_of};
use crate::tests_route::rig;

#[derive(Deserialize)]
struct Table {
    rows: Vec<Row>,
    vouched_uncatalogued: Vec<Row>,
}

#[derive(Deserialize)]
struct Row {
    mark: DataMark,
    origin: String,
    known: bool,
    refused: bool,
}

#[test]
fn the_floors_refusals_match_the_shared_table() {
    let table: Table = serde_json::from_str(include_str!(
        "../../../cockpit/test/fixtures/profile_floor.json"
    ))
    .unwrap();
    assert_eq!(table.rows.len(), 24, "3 marks x 4 origins x known or not");
    for r in &table.rows {
        // A profile is cleared for private content when its origin is US.
        let got = refusal_of(r.mark, "some-profile", r.known, r.origin == "us");
        assert_eq!(
            got.is_some(),
            r.refused,
            "{:?} {} known={}: {got:?}",
            r.mark,
            r.origin,
            r.known
        );
    }
}

fn origin_named(word: &str) -> ModelOrigin {
    match word {
        "us" => ModelOrigin::Us,
        "eu" => ModelOrigin::Eu,
        "cn" => ModelOrigin::Cn,
        "unknown" => ModelOrigin::Unknown,
        other => panic!("the table names an origin `{other}` the daemon does not know"),
    }
}

/// Every row of both arrays, through the daemon's real config: the origin the
/// floor reads, the refusal `session.profile` makes, and the `cleared` the
/// wire carries for the model menu. A vouched-`us` provider serving a model
/// the catalog does not know has origin `us` and is not cleared.
#[test]
fn profile_list_says_what_the_floor_refuses_for_every_row() {
    let table: Table = serde_json::from_str(include_str!(
        "../../../cockpit/test/fixtures/profile_floor.json"
    ))
    .unwrap();
    assert_eq!(table.vouched_uncatalogued.len(), 3, "one row per mark");
    let rows = table
        .rows
        .iter()
        .map(|r| (r, false))
        .chain(table.vouched_uncatalogued.iter().map(|r| (r, true)));
    for (r, uncatalogued) in rows {
        let origin = origin_named(&r.origin);
        let rig = rig(None, 1, |c| {
            c.serve_glm_unrestricted();
            c.vouch_zai(origin);
            if uncatalogued {
                c.unprice_profile("glm");
            }
        });
        let (cfg, catalog) = (&rig.core.cfg, &rig.core.runner.catalog);
        let name = if r.known { "glm" } else { "no-such" };
        let what = format!(
            "{:?} {} known={} vouched={uncatalogued}",
            r.mark, r.origin, r.known
        );
        if r.known {
            assert_eq!(cfg.profile_origin(name, catalog), origin, "{what}");
        }
        assert_eq!(
            profile_refusal(cfg, catalog, r.mark, name).is_some(),
            r.refused,
            "{what}"
        );
        let listed = rig.core.profile_list();
        let info = listed.profiles.iter().find(|p| p.name == name);
        assert_eq!(
            info.map(|p| p.cleared),
            r.known
                .then_some(Some(origin == ModelOrigin::Us && !uncatalogued)),
            "{what}: the wire's answer"
        );
    }
}
