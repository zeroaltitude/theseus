//! A line cut for the people extraction and then scrubbed into its request
//! holds a known value whole or not at all. The value is invented.

use std::sync::Arc;

use super::extract::request;
use super::{lines, LINE_CHARS};
use crate::node::Node;
use crate::provider::one_tool::Forcing;
use crate::scrub::Scrubber;
use crate::secrets::{Secret, SecretBoard};
use crate::tests_cut_sites::{whole_or_absent, V};

/// The value runs across the line's limit: the line stops before it, and
/// the scrub of the request masks every whole copy.
#[test]
fn a_line_cut_then_scrubbed_carries_no_piece_of_a_value() {
    let board = SecretBoard::empty();
    board.hold("cut_test", Secret::new(V.into()));
    let scrubber = Scrubber::from_board(Arc::clone(&board));
    let before = "x".repeat(LINE_CHARS - 10);
    let said = format!("{before}{V} and after");
    let nodes = vec![(1, Node::user("ses_a", None, "owner", &said))];
    let lines = lines(&nodes);
    assert_eq!(lines.len(), 1);
    let how = Forcing {
        thinking: None,
        force: true,
    };
    let req = request("m", how, "Ferry", &lines, &|t| scrubber.scrub(t).0);
    let sent = req.messages[0]["content"][0]["text"].as_str().unwrap();
    assert!(sent.contains(&before), "the line is in the request");
    whole_or_absent("the people request", sent, V);
}
