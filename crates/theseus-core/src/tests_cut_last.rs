//! The last cuts moved to `scrub::safe`, each where its pattern had no test
//! of its own: a long reply's two ends, laid in the view at byte budgets, a
//! long file split into parts at a byte limit (or refused), and a digest cut
//! at a line end. Each holds a value the live board knows whole or not at
//! all. All values invented.

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
    let parts = crate::persona_import::split::parts(&text, limit).expect("it splits");
    assert!(parts.len() > 1, "{parts:?}");
    assert_eq!(parts.concat(), text, "the parts joined are the file");
    for p in &parts {
        assert!(p.len() <= limit, "{p:?}");
        whole_or_absent("a part", p, V);
    }
    // A value longer than the limit at the start: no part can hold it, and
    // the split is refused.
    let text = format!("{V}zz");
    assert_eq!(crate::persona_import::split::parts(&text, 16), None);
}

#[test]
fn a_line_start_inside_a_known_value_is_no_place_to_split() {
    let lines = "kelp-one-0417\nferry-two-0417";
    let board = SecretBoard::empty();
    board.hold("lines", Secret::new(lines.into()));
    // The latest line start within the limit is inside the value.
    let text = format!(
        "{}\n{}{lines}\n{}\n",
        "a".repeat(40),
        "b".repeat(20),
        "c".repeat(60)
    );
    let parts = crate::persona_import::split::parts(&text, 85).expect("it splits");
    assert_eq!(parts.concat(), text, "the parts joined are the file");
    assert_eq!(parts[0], format!("{}\n", "a".repeat(40)));
    for p in &parts {
        assert!(p.len() <= 85, "{p:?}");
        whole_or_absent("a part", p, lines);
    }
}

#[test]
fn a_digests_line_end_inside_a_known_value_is_no_place_to_cut() {
    let lines = "kelp-one-0417\nferry-two-0417";
    let board = SecretBoard::empty();
    board.hold("lines", Secret::new(lines.into()));
    // The post's head ends in the `e`s; its last line end is inside the value.
    let text = format!("{}{lines}{}", "d".repeat(1_700), "e".repeat(200));
    let post = crate::rsi::digest::capped(&text);
    assert_eq!(
        post,
        format!("{}\n… the rest: `theseus self digest`.", "d".repeat(1_700))
    );
    whole_or_absent("the capped digest", &post, lines);
}
