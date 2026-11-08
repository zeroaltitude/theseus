//! What a failing check says is wrong, as a key that two runs of the same
//! failure share (theseus-fez4q).
//!
//! The progress stop counts a failing command as progress when its key is new
//! to the turn: the failure moved. So the key must not move with what changes
//! between identical runs (timings, temp directories, thread ids, line
//! numbers, the order parallel tests print in, colour codes), or every red
//! would read as a new one and the stall could never fire; and it must move
//! with what a fix changes (which test fails, what the error says).
//!
//! The key is a hash of the failure's lines as a set. A failure line is a
//! line of the output that names one (`fail`, `error`, `panic`, `assert` or
//! `exception`, in any case); a command that fails in no such words keys on
//! the last `TAIL_LINES` lines of its output. Each line is cut to its words
//! (`word`): a name (letters, digits, `_`, `::`) is kept as written, so
//! `case_1` and `case_2` stay two failures; a path keeps its file name and
//! loses its directories, which is where temp directories live; a position
//! (`lib.rs:41:9`) keeps the file; hex addresses and ids are dropped; every
//! other number is `#`. The set is sorted and de-duplicated, so order and
//! repeats do not count.
//!
//! What it takes to move the key is a change in a failure line's words: a
//! different test, a different error, a different assertion message. A
//! number in an assertion's value, a line shift, a timing or a repeat does
//! not. That is on purpose, since it errs toward calling a failure the same:
//! a stall then stops a turn that is changing numbers, and the stop is a
//! soft one, while a key that moved on noise would never let it stop.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

/// The words that make a line a failure's report, lowercase.
const MARKERS: [&str; 5] = ["fail", "error", "panic", "assert", "exception"];
/// The lines a failure that names itself in none of them is keyed on.
const TAIL_LINES: usize = 8;
/// The longest a file extension is, for a token to read as a file name.
const EXT_MAX: usize = 5;

/// The key of a failed command's `text`.
pub(super) fn key(text: &str) -> u64 {
    let text = without_colour(text);
    let mut lines: Vec<String> = text
        .lines()
        .filter(|l| names_a_failure(l))
        .map(words)
        .collect();
    if lines.is_empty() {
        lines = text.lines().rev().take(TAIL_LINES).map(words).collect();
    }
    lines.retain(|l| !l.is_empty());
    lines.sort_unstable();
    lines.dedup();
    let mut hasher = DefaultHasher::new();
    lines.hash(&mut hasher);
    hasher.finish()
}

/// `text` without its ANSI colour and cursor codes.
fn without_colour(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars();
    while let Some(c) = chars.next() {
        if c != '\u{1b}' {
            out.push(c);
        } else if chars.as_str().starts_with('[') {
            chars.next();
            // A code ends at its first byte from `@` to `~`.
            chars.by_ref().find(|c| ('@'..='~').contains(c));
        }
    }
    out
}

fn names_a_failure(line: &str) -> bool {
    let line = line.to_ascii_lowercase();
    MARKERS.iter().any(|m| line.contains(m))
}

/// A line cut to the tokens that stay the same between runs.
fn words(line: &str) -> String {
    line.split_whitespace()
        .filter_map(word)
        .collect::<Vec<_>>()
        .join(" ")
}

/// One token as it counts, or none when it is only what changes between runs.
fn word(token: &str) -> Option<String> {
    let token = token.trim_matches(|c: char| "'\"`,;()[]{}<>.:".contains(c));
    if token.is_empty() {
        return None;
    }
    if let Some((_, last)) = token.rsplit_once('/') {
        let last = before_position(last);
        return is_file_name(last).then(|| last.to_string());
    }
    let token = before_position(token);
    if is_name(token) {
        return Some(token.to_string());
    }
    let hex = token.strip_prefix("0x").unwrap_or(token);
    if hex.len() >= 8 && hex.chars().all(|c| c.is_ascii_hexdigit() || c == '-') {
        return None;
    }
    Some(numbers_out(token))
}

/// `token` up to the first `:` that a number follows (`lib.rs:41:9` is
/// `lib.rs`).
fn before_position(token: &str) -> &str {
    let bytes = token.as_bytes();
    (0..bytes.len().saturating_sub(1))
        .find(|&i| bytes[i] == b':' && bytes[i + 1].is_ascii_digit())
        .map_or(token, |i| &token[..i])
}

/// `name.ext`, not a hidden name or a bare one: a directory, and a temp
/// directory above all, is not a file name.
fn is_file_name(token: &str) -> bool {
    token.rsplit_once('.').is_some_and(|(stem, ext)| {
        !stem.is_empty()
            && (1..=EXT_MAX).contains(&ext.len())
            && ext.chars().all(|c| c.is_ascii_alphabetic())
    })
}

/// A name a program gives something: it starts with a letter or `_` and holds
/// letters, digits, `_` and `:` (a Rust path).
fn is_name(token: &str) -> bool {
    token
        .chars()
        .next()
        .is_some_and(|c| c.is_alphabetic() || c == '_')
        && token
            .chars()
            .all(|c| c.is_alphanumeric() || c == '_' || c == ':')
}

/// `token` with each run of digits as one `#`.
fn numbers_out(token: &str) -> String {
    let mut out = String::with_capacity(token.len());
    let mut digits = false;
    for c in token.chars() {
        if !c.is_ascii_digit() {
            out.push(c);
        } else if !digits {
            out.push('#');
        }
        digits = c.is_ascii_digit();
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A failed `cargo test`, as a run prints it.
    const RED: &str = "[exit code 101 · in /srv/dev/work]
running 3 tests
test tests::alpha ... ok
test tests::bravo ... FAILED
test tests::charlie ... FAILED

failures:

---- tests::bravo stdout ----
thread 'tests::bravo' (140188) panicked at src/lib.rs:41:9: frame 0x7ffd5c3a91e0 run 3f2a9c1e-77d0-4c1b-9a55-0123456789ab
assertion `left == right` failed: tide table for /tmp/.tmpA1b2C3/work
  left: 4
 right: 5

failures:
    tests::bravo
    tests::charlie

test result: FAILED. 1 passed; 2 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.31s
";

    /// The same failure on another run: other timings, another temp
    /// directory, thread, address and run id, the line shifted by an edit
    /// above it, colour codes, the failing tests printed in the other order,
    /// and blank lines.
    const RED_AGAIN: &str = "[exit code 101 · in /srv/dev/work]

running 3 tests
test tests::alpha ... ok
test tests::charlie ... \u{1b}[31mFAILED\u{1b}[0m
test tests::bravo ... \u{1b}[31mFAILED\u{1b}[0m
failures:
---- tests::bravo stdout ----
thread 'tests::bravo' (140231) panicked at src/lib.rs:57:9: frame 0x7fff00aa1122 run 9c0de1ab-0001-4c1b-9a55-ba9876543210
assertion `left == right` failed: tide table for /tmp/.tmpZ9y8X7/work
  left: 4
 right: 5

failures:
    tests::charlie
    tests::bravo

test result: FAILED. 1 passed; 2 failed; 0 ignored; 0 measured; 0 filtered out; finished in 1.02s
";

    #[test]
    fn the_same_failure_on_another_run_has_the_same_key() {
        assert_eq!(key(RED), key(RED_AGAIN));
        // Each kind of noise alone, so no one of them is carried by another.
        let noisy = [
            RED.replace("0.31s", "7.88s"),
            RED.replace("/tmp/.tmpA1b2C3/work", "/var/tmp/.tmpQwErTy/work"),
            RED.replace("lib.rs:41:9", "lib.rs:410:12"),
            RED.replace("FAILED", "\u{1b}[1;31mFAILED\u{1b}[0m"),
            RED.replace("(140188)", "(7)"),
            RED.replace("0x7ffd5c3a91e0", "0x7fff00aa1122"),
            RED.replace(
                "3f2a9c1e-77d0-4c1b-9a55-0123456789ab",
                "9c0de1ab-0001-4c1b-9a55-ba98",
            ),
            format!("{RED}\n\n"),
            RED.replace("1 passed; 2 failed", "9 passed; 2 failed"),
        ];
        let same = noisy.iter().filter(|t| key(t) == key(RED)).count();
        assert_eq!((same, noisy.len()), (9, 9));
        // A line that names no failure is no part of the key.
        let chatter = format!("{RED}compiling 41 crates in 12.5s\nwarning: unused variable `x`\n");
        assert_eq!(key(&chatter), key(RED));
    }

    #[test]
    fn a_failure_that_moved_has_another_key() {
        let moved = [
            // One of the two tests passes now.
            RED.replace("test tests::charlie ... FAILED", "test tests::charlie ... ok")
                .replace("    tests::charlie\n", ""),
            // Another test fails in its place.
            RED.replace("tests::charlie", "tests::delta"),
            // The assertion says something else.
            RED.replace("tide table for", "moon table for"),
            // It does not build any more.
            "[exit code 101 · in /srv/dev/work]\nerror[E0308]: mismatched types\n --> src/lib.rs:12:5\n"
                .to_string(),
            // A different compile error, even at the same place.
            "[exit code 101 · in /srv/dev/work]\nerror[E0425]: cannot find value `x` in this scope\n --> src/lib.rs:12:5\n"
                .to_string(),
            // Numbered cases are told apart: fixing the first leaves the second.
            RED.replace("tests::bravo", "case_1").replace("tests::charlie", "case_2"),
        ];
        let differ = moved.iter().filter(|t| key(t) != key(RED)).count();
        assert_eq!((differ, moved.len()), (6, 6));
        let cases = |a: &str, b: &str| key(&format!("test {a} ... FAILED\ntest {b} ... FAILED\n"));
        assert_ne!(
            cases("case_1", "case_2"),
            cases("case_2", "case_3"),
            "fixing case_1 moves the failure"
        );
        assert_eq!(
            cases("case_1", "case_2"),
            cases("case_2", "case_1"),
            "order is not a move"
        );
    }

    /// A command that fails in no words of its own is keyed on its tail, and
    /// one that prints nothing is one failure however often it runs.
    #[test]
    fn a_failure_with_no_words_of_its_own_is_keyed_on_its_tail() {
        let silent = "[exit code 1 · in /srv/dev/work]\n(no output)\n";
        let silent_again = "[exit code 1 · in /srv/other/work]\n(no output)\n";
        assert_eq!(key(silent), key(silent_again));
        let says = "[exit code 1 · in /srv/dev/work]\nthe tide table has 12 rows, not 13\n";
        assert_ne!(key(says), key(silent));
        let says_again = "[exit code 1 · in /srv/dev/work]\nthe tide table has 12 rows, not 14\n";
        assert_eq!(key(says), key(says_again), "a number is not a move");
        let other = "[exit code 1 · in /srv/dev/work]\nthe tide table has a gap\n";
        assert_ne!(key(says), key(other));
        // Nothing at all is a key too, and the same one.
        assert_eq!(key(""), key("\n\n"));
    }

    #[test]
    fn a_token_keeps_what_names_something_and_loses_what_changes() {
        for (token, want) in [
            ("tests::part1", Some("tests::part1")),
            ("E0308", Some("E0308")),
            ("src/deep/lib.rs:41:9:", Some("lib.rs")),
            ("lib.rs:41:9", Some("lib.rs")),
            ("/tmp/.tmpA1b2C3", None),
            ("/tmp/.tmpA1b2C3/work", None),
            ("0x7ffd5c3a91e0", None),
            ("3f2a9c1e-77d0-4c1b-9a55-0123456789ab", None),
            ("(0.31s)", Some("#.#s")),
            ("12ms", Some("#ms")),
            ("...", None),
            ("'tests::bravo'", Some("tests::bravo")),
        ] {
            assert_eq!(word(token).as_deref(), want, "{token}");
        }
    }
}
