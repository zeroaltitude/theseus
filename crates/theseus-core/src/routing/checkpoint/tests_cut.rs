//! The checkpoint's text, which the pack reads, holds a known value whole or
//! not at all, wherever it is cut. The value is invented.

use super::{text, text_within, LoopLine, Trigger, BRIEF_CHARS};
use crate::secrets::{Secret, SecretBoard};
use crate::tests_cut_sites::{whole_or_absent, V};

/// The value runs across the brief's limit, a step's and a loop's: each
/// stops before it.
#[test]
fn a_checkpoints_text_stops_before_a_known_value() {
    let board = SecretBoard::empty();
    board.hold("cut_test", Secret::new(V.into()));
    let brief = format!("{}{V} and after", "x".repeat(BRIEF_CHARS - 10));
    let plan = vec![(format!("{}{V} and after", "y".repeat(90)), "todo")];
    let loops = vec![LoopLine {
        index: 1,
        say: Some(format!("{}{V} and after", "z".repeat(110))),
        failed: Some(format!("{}{V} and after", "w".repeat(150))),
    }];
    let s = text(&brief, &plan, &loops, &Trigger::Report);
    assert!(s.contains(&"x".repeat(BRIEF_CHARS - 10)), "{s}");
    whole_or_absent("the checkpoint's text", &s, V);
}

/// At its last resort the text is cut to its bound: that cut stops before
/// the value too.
#[test]
fn the_text_cut_to_its_bound_stops_before_a_known_value() {
    let board = SecretBoard::empty();
    board.hold("cut_test", Secret::new(V.into()));
    let s = text_within(V, &[], &[], &Trigger::Report, 18);
    assert!(s.starts_with("Task: "), "{s}");
    whole_or_absent("the text at its bound", &s, V);
}
