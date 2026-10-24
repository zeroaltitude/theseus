//! The floor's refusal table, shared with the cockpit's model menu (theseus-jid7t.6): `cockpit/test/fixtures/
//! profile_floor.json` lists every mark × origin × known case and whether it is refused. This test holds
//! `data_mark::refusal_of` to every row; the cockpit's `greyOf` is held to the same file.

use serde::Deserialize;
use theseus_protocol::ops::DataMark;

use crate::data_mark::refusal_of;

#[derive(Deserialize)]
struct Table {
    rows: Vec<Row>,
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
