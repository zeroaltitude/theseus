//! The last cuts moved to `scrub::safe`, each where its pattern had no test
//! of its own: a long reply's two ends, laid in the view at byte budgets, and
//! a long file split into parts at a byte limit. Each holds a value the live
//! board knows whole or not at all. All values invented.

use crate::secrets::{Secret, SecretBoard};
use crate::tests_cut_sites::{whole_or_absent, V};
use crate::view::text::forms::{ends, Said, Voice};
use crate::view::Handle;

/// A board that knows [`V`], alive while the returned handle is.
fn knowing() -> std::sync::Arc<SecretBoard> {
    let board = SecretBoard::empty();
    board.hold("cut_test", Secret::new(V.into()));
    board
}

#[test]
fn a_long_replys_two_ends_hold_a_value_whole_or_not_at_all() {
    let _board = knowing();
    let per = crate::catalog::TokenRates::CLAUDE.text;
    let (head, tail) = (100, 100);
    let (h, t) = ((head as f64 * per) as usize, (tail as f64 * per) as usize);
    // One copy across the head's edge, one across the tail's.
    let text = format!(
        "{}{V}{}{V}{}",
        "a".repeat(h - 10),
        "b".repeat(5_000),
        "c".repeat(t - 10)
    );
    let part = Said {
        voice: Voice::Theseus,
        text,
        node: "n1".into(),
        call: None,
        outside: None,
    };
    let cut = ends(&part, head, tail, &Handle("h1".into()));
    assert!(cut.text.contains("[cut here: about"), "{}", cut.text);
    whole_or_absent("the reply's two ends", &cut.text, V);
}

#[test]
fn a_file_split_into_parts_keeps_a_value_in_one_part() {
    let _board = knowing();
    let limit = 200;
    // One long line, so the split falls at a character boundary.
    let text = format!("{}{V}{}", "x".repeat(limit - 10), "y".repeat(limit));
    let parts = crate::persona_import::split::parts(&text, limit);
    assert!(parts.len() > 1, "{parts:?}");
    assert_eq!(parts.concat(), text, "the parts joined are the file");
    for p in &parts {
        assert!(p.len() <= limit, "{p:?}");
        whole_or_absent("a part", p, V);
    }
    // A value longer than the limit at the start: the split still ends.
    let text = format!("{V}zz");
    let parts = crate::persona_import::split::parts(&text, 16);
    assert_eq!(parts.concat(), text, "the parts joined are the file");
}
