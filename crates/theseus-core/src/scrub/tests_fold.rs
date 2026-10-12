//! A board value that YAML's double-quoted style folds (theseus-d80h): PyYAML
//! breaks a long double-quoted value at a space (or, past an escape, inside a
//! word) by ending the line in `\`, and the break and the next line's
//! indentation then read as nothing; a following `\ ` keeps a leading space.
//! A passphrase printed by `yaml.dump` in that style (forced by any character
//! that needs escaping) is split by the fold. All values invented.

use super::Scrubber;

/// A long value with spaces and one character PyYAML escapes, so it prints
/// double-quoted and folded.
fn phrase() -> String {
    format!(
        "{}tail",
        "Inv\u{e9}nted pass phrase with many words ".repeat(4)
    )
}

/// `yaml.dump({"token": phrase()})`, as PyYAML 6 prints it.
const DUMPED: &str = "token: \"Inv\\xE9nted pass phrase with many words Inv\\xE9nted pass phrase with many\\\n  \\ words Inv\\xE9nted pass phrase with many words Inv\\xE9nted pass phrase with many\\\n  \\ words tail\"\n";

#[test]
fn a_value_yaml_folds_across_lines_is_withheld() {
    let s = Scrubber::with_values(vec![(phrase(), "passphrase".into())]);
    let (out, n) = s.scrub(DUMPED);
    assert_eq!(
        (out.as_str(), n),
        ("token: \"[redacted:passphrase]\"\n", 1),
        "{DUMPED}"
    );
    // The same with Windows line ends, and inside other output.
    let crlf = DUMPED.replace('\n', "\r\n");
    let (out, n) = s.scrub(&format!("kind: Secret\r\n{crlf}done"));
    assert_eq!(
        (out.as_str(), n),
        (
            "kind: Secret\r\ntoken: \"[redacted:passphrase]\"\r\ndone",
            1
        )
    );
}

/// PyYAML folds inside a word when the line's room ends just past an escape:
/// the next line goes on with the word, after the indentation alone.
#[test]
fn a_fold_inside_a_word_is_read_through() {
    let v = "Inv\u{e9}nted-word yyyyy";
    let s = Scrubber::with_values(vec![(v.into(), "word".into())]);
    let text = format!(
        "token: \"{} Inv\\xE9\\\n  nted-word yyyyy\"\n",
        "x".repeat(70)
    );
    let (out, n) = s.scrub(&text);
    assert_eq!(
        (out, n),
        (
            format!("token: \"{} [redacted:word]\"\n", "x".repeat(70)),
            1
        )
    );
}

/// A backslash at a line's end that folds nothing a value needs leaves the
/// text as it was, and a shell's line continuation is read the same way.
#[test]
fn a_fold_alone_changes_nothing() {
    let s = Scrubber::with_values(vec![(phrase(), "passphrase".into())]);
    for kept in [
        "run \\\n  --flag value\n",
        "a\\\r\n\tb\\",
        "ends in a backslash\\",
        "\\\n",
    ] {
        assert_eq!(s.scrub(kept), (kept.to_string(), 0), "{kept:?}");
    }
    let s = Scrubber::with_values(vec![("--flag inv3nted-value".into(), "flag".into())]);
    assert_eq!(
        s.scrub("run \\\n  --flag inv3nted-value\n").0,
        "run \\\n  [redacted:flag]\n"
    );
}
