//! A routine's carried report and its first line, cut through `scrub::safe`:
//! with a live board that knows a value, each value is whole or absent in the
//! head and in the tail. All values invented.

use crate::secrets::{Secret, SecretBoard};
use crate::tests_cut_sites::whole_or_absent;

const V: &str = "rbnt-5520-qelk-0718-zuwc";
const W: &str = "moss-hinge-3304-taper";

#[test]
fn a_carried_report_keeps_each_value_whole_or_not_at_all() {
    let board = SecretBoard::empty();
    board.hold("cut_test", Secret::new(V.into()));
    board.hold("cut_test_2", Secret::new(W.into()));
    // The head ends at 1,333 characters, inside V; the tail starts 666
    // characters from the end, inside W.
    let text = format!(
        "{}{V}{}{W}{}",
        ".".repeat(1_320),
        "-".repeat(3_000),
        ".".repeat(650)
    );
    let carried = super::clip(&text, super::MAX_CARRY);
    assert!(carried.contains(" characters left out …]"), "{carried:?}");
    whole_or_absent("the carried report", &carried, V);
    whole_or_absent("the carried report", &carried, W);
    let line = super::first_line(&format!("{}{V} and on\nnext", "x".repeat(150)));
    assert!(line.ends_with('…'), "{line:?}");
    whole_or_absent("the report's first line", &line, V);
}
