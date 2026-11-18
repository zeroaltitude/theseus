//! Which message of a turn a settings question puts to rest (issue 22): the one
//! that began it, as the turn's inbound step named it; never a report or a wake
//! the turn read first, and never one the owner steered in while it ran.

use crate::capture::expect::Expected;

/// The node noted for a turn is that turn's, exactly; another turn has none.
#[test]
fn a_turns_input_is_the_one_noted_for_that_turn_alone() {
    let e = Expected::default();
    e.note_input("turn_a", "msg_input");
    assert_eq!(e.input_of("turn_a").as_deref(), Some("msg_input"));
    assert_eq!(e.input_of("turn_b"), None);
    // A turn noted again (a retry under the same id) holds its latest input.
    e.note_input("turn_a", "msg_again");
    assert_eq!(e.input_of("turn_a").as_deref(), Some("msg_again"));
}

/// The notes are bounded, oldest first.
#[test]
fn the_notes_are_bounded() {
    let e = Expected::default();
    for i in 0..300 {
        e.note_input(&format!("turn_{i}"), &format!("msg_{i}"));
    }
    assert_eq!(e.input_of("turn_0"), None);
    assert_eq!(e.input_of("turn_299").as_deref(), Some("msg_299"));
}
