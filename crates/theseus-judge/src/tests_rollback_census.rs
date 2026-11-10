//! The rollback census: every rollback rule an embedded pack declares that no
//! production code can ever feed.
//!
//! A `RollbackRule` is counted only from a `CanaryEvent` handed to
//! `Ladder::land`. Four `CanaryEvent` kinds (`TaskSpend`, `OperatorStop`,
//! `OnPath`, `RoleSwitch`) have no production constructor, so the four rules
//! they feed (`spend_ratio`, `operator_stop_within_nudge`, `on_path_p95`,
//! `switches_per_exchange`) are declared and can never fire. This test holds
//! that list, positively: a pack that gains or loses such a declaration, or a
//! producer that gets wired, moves it and is seen rather than silent.
//! No clock is read.

use crate::pack::embedded;

/// The rules a production site can feed, each with its producer.
const FEEDABLE: [&str; 5] = [
    // `NudgedTurnEnded`: theseus-core `turn/nudge_step.rs:204`.
    "nudge_loop",
    // `Notice`: theseus-core `judge/notice.rs:184` and `:461`.
    "notices_per_day",
    // `Label`: `judge/notice.rs:185`, `judge/inbound.rs:660`, `judge/mod.rs:430`.
    "labels_per_day",
    // `Pinned`: `judge/inbound.rs:640`.
    "pins_per_day",
    // `BreakerOpened`: `judge/sink.rs:426`.
    "opens_per_day",
];

/// Every (pack, rule) pair declared and unfed, sorted.
const EXPECTED_DEAD: [(&str, &str); 10] = [
    ("loop.v1", "on_path_p95"),
    ("loop.v1", "operator_stop_within_nudge"),
    ("loop.v1", "spend_ratio"),
    ("role.v1", "switches_per_exchange"),
    ("route.v10", "on_path_p95"),
    ("route.v5", "on_path_p95"),
    ("route.v6", "on_path_p95"),
    ("route.v7", "on_path_p95"),
    ("route.v8", "on_path_p95"),
    ("undo.v1", "on_path_p95"),
];

#[test]
fn the_rollback_rules_no_production_code_can_feed_are_exactly_these() {
    let packs = embedded().as_ref().expect("the embedded packs parse");
    let mut declared = 0;
    let mut dead: Vec<(String, &str)> = Vec::new();
    for p in packs {
        for r in &p.rollback {
            declared += 1;
            if !FEEDABLE.contains(&r.name()) {
                dead.push((p.name(), r.name()));
            }
        }
    }
    dead.sort();
    let expected: Vec<(String, &str)> = EXPECTED_DEAD
        .iter()
        .map(|(p, r)| ((*p).to_string(), *r))
        .collect();
    assert_eq!(
        declared, 25,
        "the embedded packs declare 25 rollback rules in all"
    );
    assert_eq!(
        dead, expected,
        "the declared rollback rules no code feeds changed"
    );
}
