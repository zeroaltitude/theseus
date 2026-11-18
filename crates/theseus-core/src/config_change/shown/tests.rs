use super::*;

fn long(n: usize) -> String {
    (1..=n).map(|i| format!("line {i}\n")).collect()
}

#[test]
fn an_unseen_character_is_written_out_and_counted_and_a_backslash_is_doubled() {
    let (s, n) = escape_line("a\u{202E}b\u{200B}c\td\u{E0041}e \\u{202e} \\");
    assert_eq!(n, 4, "{s}");
    assert_eq!(
        s,
        "a\\u{202e}b\\u{200b}c\\u{0009}d\\u{e0041}e \\\\u{202e} \\\\"
    );
    assert!(!s.chars().any(unseen), "{s}");
    assert_eq!(
        escape_line("plain: é, 日本, 🙂"),
        ("plain: é, 日本, 🙂".into(), 0)
    );
}

#[test]
fn a_small_change_to_a_long_file_shows_its_changed_lines_whole_with_context() {
    let before = long(120);
    let after = before.replace("line 60\n", "line sixty\nline 60 and a half\n");
    let d = diff(&before, &after);
    assert_eq!((d.added, d.removed), (2, 1), "{d:?}");
    assert_eq!(d.hunks.len(), 1);
    let block = d.block(2_000).expect("it fits");
    assert_eq!(
        block,
        "```diff\n@@ -58,5 +58,6 @@\n line 58\n line 59\n-line 60\n+line sixty\n+line 60 and a half\n line 61\n line 62\n```\n"
    );
    assert_eq!(d.places(), "lines 58-63");
}

#[test]
fn changes_that_do_not_fit_are_not_shown_cut() {
    let before = long(400);
    let mut after = before.clone();
    for i in (10..400).step_by(40) {
        after = after.replace(&format!("line {i}\n"), &format!("changed {i}\n"));
    }
    let d = diff(&before, &after);
    assert!(d.hunks.len() > 5);
    assert_eq!(d.block(300), None, "over the room: a summary, never a cut");
    assert!(d.block(100_000).is_some());
    // A line holding a triple backtick cannot sit in the card's block.
    let fenced = diff("a\n", "a\n```\n");
    assert_eq!(fenced.block(100_000), None);
}

#[test]
fn a_change_hidden_in_an_unseen_character_is_shown_and_counted() {
    let d = diff("be kind\n", "be kind\u{200B}\n");
    assert_eq!((d.added, d.removed, d.unseen), (1, 1, 1));
    let b = d.block(1_000).unwrap();
    assert!(b.contains("-be kind\n+be kind\\u{200b}\n"), "{b}");
}

#[test]
fn a_missing_final_line_break_is_said_and_never_looks_like_no_change() {
    let d = diff("one\ntwo", "one\ntwo\n");
    assert_eq!((d.added, d.removed), (1, 1));
    let b = d.block(1_000).unwrap();
    assert!(
        b.contains("-two\n\\ no line break at the end of the file\n+two\n"),
        "{b}"
    );
}

#[test]
fn the_facts_name_the_hash_and_its_length() {
    let f = facts("a\nb\n");
    assert_eq!(f.fingerprint.len(), DIGITS);
    assert_eq!(f.fingerprint, &hex::encode(Sha256::digest(b"a\nb\n"))[..16]);
    assert_eq!(
        f.words(),
        format!("4 bytes, 2 lines, fingerprint {}", f.fingerprint)
    );
}

#[test]
fn the_whole_text_is_every_line_with_its_unseen_characters_counted() {
    let text = format!("{}tail\u{202E}", long(300));
    let w = whole("before", &text);
    assert!(w.starts_with("===== before: "), "{w}");
    assert!(w.contains("1 unseen character (shown as \\u{..})"), "{w}");
    assert_eq!(
        w.matches("\nline ").count(),
        300 - 1 + 1,
        "every line is there"
    );
    assert!(w.contains("line 300\ntail\\u{202e}\n\\ no line break at the end of the file\n"));
}
