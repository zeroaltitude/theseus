//! Readings of a value that overlap: each is found, and the stand-in covers
//! their union, through `Substitution` and through an exit.

use std::sync::Arc;
use std::time::Instant;

use super::tests::{percent_oracle, small_words};
use super::*;
use crate::exits::Exits;
use crate::secrets::Secret;

/// Every byte of every encoded reading the oracle finds lies inside a span
/// the matcher returns.
#[test]
fn percent_spans_cover_every_oracle_span() {
    let values = small_words(b"%aA", 3);
    for text in small_words(b"%25aA61", 5) {
        for value in &values {
            let actual = percent_spans(&text, value);
            for (start, end) in percent_oracle(&text, value) {
                for byte in start..end {
                    assert!(
                        actual.iter().any(|(a, b)| *a <= byte && byte < *b),
                        "text={:?}, value={:?}, byte {byte} of ({start}, {end}), actual={actual:?}",
                        String::from_utf8_lossy(&text),
                        String::from_utf8_lossy(value)
                    );
                }
            }
        }
    }
}

/// Each input: the text, the value, and the text once the value is masked.
fn overlapping() -> Vec<(String, String, String)> {
    // A value whose base64 is periodic: wrapped at 40, its second reading
    // starts on the first line and ends on the third.
    let periodic = "abc".repeat(14);
    let blob = base64::engine::general_purpose::STANDARD.encode("abc".repeat(25));
    let wrapped: Vec<&str> = blob
        .as_bytes()
        .chunks(40)
        .map(|c| std::str::from_utf8(c).unwrap())
        .collect();
    vec![
        // A value that repeats itself: two verbatim readings, one inside
        // the other's tail.
        (
            "abcabcabcab".into(),
            "abcabcab".into(),
            "[redacted:x]".into(),
        ),
        // A verbatim reading and an encoded one that runs past it.
        (
            "%%%%%%%%%25".into(),
            "%%%%%%%%".into(),
            "[redacted:x]".into(),
        ),
        (wrapped.join("\n"), periodic, "[redacted:x]".into()),
        // The same through one JSON escape.
        (
            r#"a\"ba\"ba\"ba\"b"#.into(),
            r#"a"ba"ba"b"#.into(),
            "[redacted:x]".into(),
        ),
        // Two readings that only touch stay two.
        (
            "abcabcababcabcab".into(),
            "abcabcab".into(),
            "[redacted:x][redacted:x]".into(),
        ),
    ]
}

#[test]
fn an_overlapping_second_reading_is_masked_with_the_first() {
    for (text, value, masked) in overlapping() {
        let (out, counts) = Substitution::new(vec![(value.as_str(), "x")]).apply(&text);
        assert_eq!(out, masked, "text={text:?}, value={value:?}");
        assert!(counts.get("x").is_some_and(|n| *n >= 1), "{counts:?}");
    }
}

#[test]
fn an_exit_masks_the_union_of_overlapping_readings() {
    for (text, value, masked) in overlapping() {
        let board = SecretBoard::new(["x".to_string()], Instant::now());
        board.publish(
            [("x".to_string(), Ok(Secret::new(value.clone())))].into(),
            "fake",
        );
        let exits: Arc<Exits> = Exits::new(board, true);
        let (out, found) = exits.text(&text);
        assert_eq!(out, masked, "text={text:?}, value={value:?}");
        assert_eq!(found.names(), vec!["x".to_string()]);
    }
}
