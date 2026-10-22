//! The arrangement's quote floor is a token floor (theseus-e31an): a quote
//! must match exactly one node, so a short instruction quoted whole and
//! exactly is admitted, and the same short quote in two nodes is refused as
//! ambiguous, with candidates.

use serde_json::{json, Value};

use crate::arrangement::{resolve, Input};
use crate::node::Node;

fn at(mut n: Node, ms: u64) -> (u64, crate::stub::Stub) {
    n.created_at_ms = ms;
    (ms, n.into())
}

fn quoting(quote: &str) -> Input {
    let v: Value = json!({"pieces": [{"quote": quote, "role": "objective"}]});
    serde_json::from_value(v).unwrap()
}

fn user(text: &str, ms: u64) -> (u64, crate::stub::Stub) {
    at(Node::user("ses_walk", None, "discord", text), ms)
}

/// The walkthrough's own quote: 17 characters, exact, in one node.
#[test]
fn a_short_unique_quote_is_admitted() {
    let t = vec![
        user(
            "Start two tasks:\nA) census of the repo\nB) slow.sh load 3",
            1_790_000_000_000,
        ),
        user("Thanks, that is all.", 1_790_000_060_000),
    ];
    let got = resolve(&quoting("B) slow.sh load 3"), &t, None).unwrap();
    assert_eq!(got.len(), 1);
    assert_eq!(got[0].node, t[0].1.id);
}

/// A short node's whole text is a quote too.
#[test]
fn a_short_nodes_whole_text_is_admitted() {
    let t = vec![user("Do it.", 1_790_000_000_000)];
    let got = resolve(&quoting("Do  it."), &t, None).unwrap();
    assert_eq!(got[0].node, t[0].1.id);
}

/// The same short quote in two nodes is ambiguous, and names both.
#[test]
fn a_short_quote_in_two_nodes_is_ambiguous_with_candidates() {
    let t = vec![
        user("B) slow.sh load 3, please", 1_790_000_000_000),
        user("again: B) slow.sh load 3", 1_790_000_060_000),
    ];
    let err = resolve(&quoting("B) slow.sh load 3"), &t, None).unwrap_err();
    assert_eq!(err.class, "ambiguous");
    assert!(err.message.contains(&t[0].1.id), "{}", err.message);
    assert!(err.message.contains(&t[1].1.id), "{}", err.message);
}

/// The token floor still stops an empty or punctuation-only quote, with a
/// usable hint.
#[test]
fn a_punctuation_quote_is_refused_with_a_usable_hint() {
    let t = vec![user("ok?! then go.", 1_790_000_000_000)];
    let err = resolve(&quoting(" ?! "), &t, None).unwrap_err();
    assert_eq!(err.class, "short");
    assert!(
        err.message.contains("non-space characters"),
        "{}",
        err.message
    );
}
