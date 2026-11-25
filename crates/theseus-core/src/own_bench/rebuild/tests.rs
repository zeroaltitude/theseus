//! A replay is refused when a look back's result it laid, kept by reference, quotes words that do not read now as
//! they were written: erased since, or not at their position. Its parts are read on their own: the result here
//! has no copy edge. All names and words invented.

use super::erased_since;
use crate::exchange::{exchange_record, Exchange, Opener};
use crate::node::{Body, Node};
use crate::quotes::{Field, Form, Part, Quote};
use crate::store::Store;

const WORDS: &str = "the heron nests by the north weir";

fn store() -> (tempfile::TempDir, Store) {
    let dir = tempfile::tempdir().unwrap();
    let s = Store::open(&dir.path().join("store")).unwrap();
    (dir, s)
}

/// The exchange `t1` of the session `s`, and in it a history result kept by reference that quotes the first
/// word of `of`'s words, read at `at`, with no copy edge: the result.
fn laid(s: &Store, of: &str, at: u64) -> Node {
    let ex = Exchange {
        turn_id: "t1".into(),
        session_id: "s".into(),
        place: "cli".into(),
        private: false,
        opened_by: Opener::Message,
        person: None,
        start_ms: 1,
        end_ms: Some(2),
        ask: None,
        reply: None,
        messages: Vec::new(),
        stop: None,
        rebuilt: false,
    };
    let mut r = crate::compiler::tests_view::result("t1", "h1", "");
    if let Body::ToolResult { tool, parts, .. } = &mut r.body {
        *tool = crate::view::history::HISTORY_TOOL.into();
        *parts = vec![
            Part::Code("[Exchange \"t0\", 2026-03-03: what was said]\n\u{2502} ".into()),
            Part::Quote(Quote {
                node: of.into(),
                position: at,
                field: Field::Text,
                from: 0,
                to: 3,
                form: Form::Margin1,
            }),
        ];
    }
    s.append(&[exchange_record(&ex).unwrap(), r.record().unwrap()])
        .unwrap();
    r
}

/// A replay that laid the result, in its exchange or recalled, is refused once the message it quoted is erased.
#[test]
fn a_replay_refuses_a_parts_result_whose_quote_was_erased() {
    let (_d, s) = store();
    let source = Node::user("src", Some("t0"), "cli", WORDS);
    let at = s.append(&[source.record().unwrap()]).unwrap()[0];
    let r = laid(&s, &source.id, at);
    let t1 = ["t1".to_string()];
    assert!(!erased_since(&s, &t1, &[]), "laid in its exchange");
    assert!(!erased_since(&s, &[], std::slice::from_ref(&r.id)), "recalled");
    s.append(&[source.erased(2, "a test").record().unwrap()])
        .unwrap();
    assert!(erased_since(&s, &t1, &[]), "laid in its exchange");
    assert!(erased_since(&s, &[], &[r.id]), "recalled");
}

/// A quote that does not read as it was written (its position holds another record) is treated as erased.
#[test]
fn a_replay_refuses_a_parts_result_whose_quote_cannot_be_read() {
    let (_d, s) = store();
    let source = Node::user("src", Some("t0"), "cli", WORDS);
    let other = Node::user("src", Some("t0"), "cli", "another message");
    let at = s
        .append(&[source.record().unwrap(), other.record().unwrap()])
        .unwrap();
    let r = laid(&s, &source.id, at[1]);
    assert!(erased_since(&s, &["t1".to_string()], &[]));
    assert!(erased_since(&s, &[], &[r.id]));
}
