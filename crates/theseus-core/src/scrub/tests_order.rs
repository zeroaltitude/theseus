//! Values met side by side: an encoded reading that is read only once a
//! verbatim value before it is taken (a value that ends in a backslash
//! changes which escape the next backslash starts), through `Substitution`
//! and through an exit; and the time a text with many separate copies takes.
//! All values invented.

use std::sync::Arc;
use std::time::{Duration, Instant};

use super::*;
use crate::exits::Exits;
use crate::secrets::Secret;

/// One case: the values as `(value, name)`, the text, and stretches of the
/// text that must not be left once both values are masked.
struct Case {
    values: Vec<(&'static str, &'static str)>,
    text: String,
    rest: Vec<String>,
}

fn cases() -> Vec<Case> {
    let b64 = |s: &str| base64::engine::general_purpose::STANDARD.encode(s);
    vec![
        // `a` takes two of the four backslashes before `u0061`; the two
        // left read as one, and then `abcdefgh` as `abcdefgh`.
        Case {
            values: vec![(r"prefixxx\\", "a"), ("abcdefgh", "b")],
            text: r"prefixxx\\\\u0061bcdefgh".into(),
            rest: vec!["bcdefgh".into(), "u0061".into()],
        },
        // `a` ends in one backslash: the three left read as `\` and `a`.
        Case {
            values: vec![(r"prefixxx\", "a"), ("abcdefgh", "b")],
            text: r"prefixxx\\\\u0061bcdefgh".into(),
            rest: vec!["bcdefgh".into(), "u0061".into()],
        },
        // The longer value given first.
        Case {
            values: vec![("abcdefghijklmn", "b"), (r"prefixxx\\", "a")],
            text: r"prefixxx\\\\u0061bcdefghijklmn".into(),
            rest: vec!["bcdefghijklmn".into(), "u0061".into()],
        },
        // A value that ends in a backslash, then another's base64, whose
        // first letter makes an escape with that backslash.
        Case {
            values: vec![(r"prefixx\", "a"), ("lmnopqrstu", "b")],
            text: format!(r"prefixx\{} done", b64("lmnopqrstu")),
            rest: vec![b64("lmnopqrstu")[2..12].to_string()],
        },
        // The same, with the other's percent-encoding after it.
        Case {
            values: vec![(r"prefixx\", "a"), ("lmnopqrstu", "b")],
            text: format!(
                r"prefixx\{} done",
                "lmnopqrstu"
                    .bytes()
                    .map(|c| format!("%{c:02X}"))
                    .collect::<String>()
            ),
            rest: vec!["%6E%6F%70%71".into()],
        },
    ]
}

fn masked_whole(out: &str, case: &Case) {
    for (v, _) in &case.values {
        assert!(!out.contains(v), "text={:?}, out={out:?}", case.text);
    }
    for r in &case.rest {
        assert!(!out.contains(r.as_str()), "text={:?}, out={out:?}", case.text);
    }
}

#[test]
fn a_reading_after_a_taken_value_is_masked() {
    for case in cases() {
        let (out, counts) = Substitution::new(case.values.clone()).apply(&case.text);
        masked_whole(&out, &case);
        for (_, name) in &case.values {
            assert!(counts.contains_key(*name), "text={:?}, {counts:?}", case.text);
        }
    }
}

#[test]
fn an_exit_masks_a_reading_after_a_taken_value() {
    for case in cases() {
        let names: Vec<String> = case.values.iter().map(|(_, n)| n.to_string()).collect();
        let board = SecretBoard::new(names.clone(), Instant::now());
        board.publish(
            case.values
                .iter()
                .map(|(v, n)| (n.to_string(), Ok(Secret::new(v.to_string()))))
                .collect(),
            "fake",
        );
        let exits: Arc<Exits> = Exits::new(board, true);
        let (out, found) = exits.text(&case.text);
        masked_whole(&out, &case);
        let mut found = found.names();
        found.sort();
        let mut want = names;
        want.sort();
        assert_eq!(found, want, "text={:?}", case.text);
    }
}

/// The time to mask `copies` separate copies of a value, the least of three
/// runs.
fn masking(copies: usize) -> Duration {
    let text = "abcdefgh ".repeat(copies);
    let sub = Substitution::new(vec![("abcdefgh", "x")]);
    (0..3)
        .map(|_| {
            let t = Instant::now();
            let (out, counts) = sub.apply(&text);
            let took = t.elapsed();
            assert_eq!(counts.get("x"), Some(&(copies as u32)));
            assert_eq!(out.len(), copies * "[redacted:x] ".len());
            took
        })
        .min()
        .unwrap()
}

/// Four times the copies takes about four times as long: well under ten.
#[test]
fn many_separate_copies_are_masked_in_time_that_grows_with_the_text() {
    let (t1, t2, t4) = (masking(10_000), masking(20_000), masking(40_000));
    eprintln!("scrub: copies 10k {t1:?}, 20k {t2:?}, 40k {t4:?}");
    let ratio = t4.as_secs_f64() / t1.as_secs_f64().max(1e-6);
    assert!(ratio < 10.0, "40k copies took {ratio:.1} times 10k's");
}
