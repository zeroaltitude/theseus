//! A long panic message through the real hook: a child process installs it
//! and panics, and the crash file it writes is read back through `take` and
//! `last`. A message over the cap is not kept, not even in part.

use std::path::Path;

use super::{file, install, last, take, MESSAGE_MAX};

/// A word of 1,800 bytes with no whitespace in it, from byte 2,400 of the
/// message: the cap falls inside it.
const ACROSS: &str = "across";
/// One word longer than the cap.
const ONE: &str = "one";
/// A made-up value of two words, its space before the cap and its end after
/// it.
const TWO: &str = "two";
const TWO_WORDS: &str = "lopa-0417 vesk-9932";

fn message(kind: &str) -> String {
    let word = "kqzv0417".repeat(225);
    match kind {
        ACROSS => format!("{}{word} and the end", "word ".repeat(480)),
        TWO => format!("{}{TWO_WORDS} and the end", "word ".repeat(817)),
        _ => "kqzv0417".repeat(600),
    }
}

/// The child of the tests below: with `THESEUS_TEST_CRASH_LONG` naming a
/// state dir and `THESEUS_TEST_CRASH_KIND` a message, it installs the hook
/// over one that ends the process, as the release build's abort does after
/// it, and panics with that message. Run alone, it does nothing.
#[test]
fn long_panicking_child() {
    let (Some(state), Some(kind)) = (
        std::env::var_os("THESEUS_TEST_CRASH_LONG"),
        std::env::var("THESEUS_TEST_CRASH_KIND").ok(),
    ) else {
        return;
    };
    std::panic::set_hook(Box::new(|_| std::process::exit(101)));
    install(Path::new(&state), "socket");
    panic!("{}", message(&kind));
}

/// The message the crash file of a child panicking with `kind` holds, read
/// by `take` (which keeps the file under `crashes/`), then by `last` and
/// from the kept file itself; all three must agree.
fn written(kind: &str) -> String {
    let dir = tempfile::tempdir().unwrap();
    let out = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "crash::tests_cut::long_panicking_child",
            "--nocapture",
        ])
        .env("THESEUS_TEST_CRASH_LONG", dir.path())
        .env("THESEUS_TEST_CRASH_KIND", kind)
        .stdin(std::process::Stdio::null())
        .output()
        .unwrap();
    assert!(
        !out.status.success(),
        "the child ends: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        file(dir.path(), "socket").exists(),
        "the hook wrote the file"
    );
    let (taken, kept) = take(dir.path(), "socket").unwrap().unwrap();
    let (newest, path) = last(dir.path(), "socket").unwrap();
    assert_eq!(path, kept);
    let read: super::Crash = serde_json::from_slice(&std::fs::read(&kept).unwrap()).unwrap();
    assert_eq!(newest.message, taken.message);
    assert_eq!(read.message, taken.message);
    taken.message
}

/// The cap falls inside a long word: the word is not in the file, not even
/// in part; only a line gives the length.
#[test]
fn a_word_the_cap_falls_in_is_not_kept_in_part() {
    let m = message(ACROSS);
    assert!(m.len() > MESSAGE_MAX && m.find("kqzv").unwrap() < MESSAGE_MAX);
    let kept = written(ACROSS);
    for piece in ["kqzv", "0417", "zv04", "17kq"] {
        assert!(!kept.contains(piece), "{piece} is in the file: {kept:?}");
    }
    assert_eq!(
        kept,
        format!("[a panic message of {} bytes, not kept]", m.len())
    );
}

/// A value of two words lies across the cap, its space before it: neither
/// word is in the file, not even in part.
#[test]
fn a_two_word_value_across_the_cap_leaves_no_piece_in_the_file() {
    let m = message(TWO);
    let at = m.find(TWO_WORDS).unwrap();
    assert!(at + 9 < MESSAGE_MAX && at + TWO_WORDS.len() > MESSAGE_MAX);
    let kept = written(TWO);
    crate::tests_cut_sites::whole_or_absent("the crash file", &kept, TWO_WORDS);
    assert_eq!(
        kept,
        format!("[a panic message of {} bytes, not kept]", m.len())
    );
}

/// A message that is one word longer than the cap: nothing of it is in the
/// file, only a line that gives its length in bytes.
#[test]
fn a_message_of_one_long_word_is_kept_as_its_length() {
    let kept = written(ONE);
    assert!(!kept.contains("kqzv") && !kept.contains("0417"), "{kept:?}");
    assert_eq!(kept, "[a panic message of 4800 bytes, not kept]");
}
