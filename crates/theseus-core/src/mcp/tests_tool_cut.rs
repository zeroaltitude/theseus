//! An MCP server's tool description is cut at its cap before any value a live
//! board knows. The value is invented.

use crate::secrets::{Secret, SecretBoard};
use crate::tests_cut_sites::whole_or_absent;

const V: &str = "tuvo-4471-brag-9036-felk";

#[test]
fn a_tool_description_cut_at_its_cap_holds_a_value_whole_or_not_at_all() {
    let board = SecretBoard::empty();
    board.hold("cut_test", Secret::new(V.into()));
    let mut d = format!("{}{V} after", "d".repeat(super::DESCRIPTION_MAX - 10));
    super::cap_description(&mut d);
    assert!(d.ends_with(" …[cut at 2,000 characters]"), "{d:?}");
    whole_or_absent("the description", &d, V);
}
