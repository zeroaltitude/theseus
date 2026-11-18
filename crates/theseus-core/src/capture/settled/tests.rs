//! Which message of a turn a settings question puts to rest (issue 22): the one
//! that began it, never one the owner steered in while it ran.

use super::first_input;
use crate::node::{Node, Origin};
use crate::stub::Stub;

fn nodes(list: Vec<Node>) -> Vec<(u64, Stub)> {
    list.into_iter()
        .zip(1u64..)
        .map(|(n, p)| (p, Stub::hydrated(p, std::sync::Arc::new(n))))
        .collect()
}

fn said(turn: &str, text: &str) -> Node {
    Node::user_with("ses_x", Some(turn), "owner", text, vec![])
}

/// The turn's first message is the one; a second the owner steered in is not,
/// so its own capture question (it may be a real order) is left alone.
#[test]
fn the_message_that_began_the_turn_is_the_one_and_a_steered_one_is_not() {
    let first = said("turn_a", "please set it to auto");
    let steer = said(
        "turn_a",
        "and from now on always ask before you restart yourself",
    );
    let want = first.id.clone();
    let list = nodes(vec![first, steer]);
    assert_eq!(first_input(&list, "turn_a"), Some(want));
}

/// Messages of other turns are not this turn's.
#[test]
fn a_message_of_another_turn_is_not_this_turns() {
    let earlier = said("turn_a", "an earlier request");
    let here = said("turn_b", "this turn's request");
    let want = here.id.clone();
    let list = nodes(vec![earlier, here]);
    assert_eq!(first_input(&list, "turn_b"), Some(want));
    assert_eq!(first_input(&list, "turn_c"), None);
}

/// A turn a wake or a job started has no message of the owner's to put to
/// rest, even when the owner steers one in: that one has its own capture.
#[test]
fn a_turn_a_wake_started_has_none_even_with_a_steer() {
    let mut woke = said("turn_a", "a reminder fired");
    woke.origin = Origin::Harness;
    let steer = said("turn_a", "set it to auto");
    let list = nodes(vec![woke, steer]);
    assert_eq!(first_input(&list, "turn_a"), None);
}
