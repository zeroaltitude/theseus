//! The exchange report counts a turn's things by the kinds this extractor makes, and any other key
//! under one fixed word (issue 44). Every `kind:` the extractor writes must be one the report lists,
//! or a new kind would be counted as unknown without a test saying so.

use std::collections::BTreeSet;

use regex::Regex;
use theseus_core::exchange::ledger::THING_KINDS;

#[test]
fn every_kind_the_extractor_makes_is_one_the_exchange_report_lists() {
    let source = include_str!("../src/entity.rs");
    let term = Regex::new(r#"format!\("([a-z_]+):"#).unwrap();
    let made: BTreeSet<&str> = term
        .captures_iter(source)
        .map(|c| c.get(1).unwrap().as_str())
        .collect();
    assert!(
        made.len() >= 7,
        "the extractor's kinds were found: {made:?}"
    );
    let listed: BTreeSet<&str> = THING_KINDS.into_iter().collect();
    assert_eq!(made, listed);
    // And what it makes of a text naming each of them.
    let text = "see crates/theseus-core/src/turn.rs and https://example.org, ask @kelp, \
                cargo add serde, theseus-a1b2 and 3f9c2ab7";
    for t in theseus_index::entity::entities(text) {
        let (kind, _) = t.split_once(':').unwrap();
        assert!(listed.contains(kind), "{t}");
    }
}
