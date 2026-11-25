//! The outgoing check's test itself (`super`): its words, its line at ten, the repository's own words, and a
//! refusal that names no word of what matched.

use super::{words, Commit, Heard, Refusal, Source, RUN};

/// Words of a conversation: thirteen, none of them code.
const SAID: &str = "Kelp oars drift past the third bollard, where the heron waits each grey morning.";

fn heard(text: &str) -> Heard {
    let mut h = Heard::default();
    h.add(text, Source::History);
    h
}

fn commit(text: &str) -> Vec<Commit> {
    vec![Commit {
        id: "c0ffee00c0ffee00".into(),
        text: text.into(),
    }]
}

fn none() -> anyhow::Result<String> {
    Ok(String::new())
}

#[test]
fn words_fold_case_and_whitespace_and_drop_punctuation() {
    assert_eq!(
        words("  Kelp,  OARS\n\tdrift... // the-heron's  "),
        ["kelp", "oars", "drift", "the", "heron", "s"]
    );
    assert_eq!(RUN, 10);
}

/// Nine words in a row pass; ten are refused, whatever the case, the line breaks and the comment's marks.
#[test]
fn nine_words_pass_and_ten_are_refused() {
    let h = heard(SAID);
    let w: Vec<&str> = SAID.split_whitespace().collect();
    let nine = format!("// {}", w[..9].join(" "));
    assert_eq!(h.check(&commit(&nine), none), Ok(()));
    let ten = format!("/// {}\n/// {}", w[..5].join(" ").to_uppercase(), w[5..10].join("  "));
    assert_eq!(
        h.check(&commit(&ten), none),
        Err(Refusal::Words {
            commit: "c0ffee00c0ffee00".into(),
            source: Source::History
        })
    );
}

/// A run the repository already holds at the base is its own words, not a person's: it passes. The base is
/// read only when something matched.
#[test]
fn a_run_the_repository_holds_at_the_base_passes() {
    let h = heard(SAID);
    let read = std::cell::Cell::new(false);
    let base = || {
        read.set(true);
        Ok(format!("# Notes\n{SAID}\n"))
    };
    assert_eq!(h.check(&commit(SAID), base), Ok(()));
    assert!(read.get());
    let unread = || -> anyhow::Result<String> { panic!("read with no match") };
    assert_eq!(h.check(&commit("nothing of it"), unread), Ok(()));
}

/// A base that cannot be read, when something matched, is a refusal: nothing is cleared by default.
#[test]
fn a_base_that_cannot_be_read_refuses() {
    let h = heard(SAID);
    let r = h.check(&commit(SAID), || anyhow::bail!("gone"));
    assert!(matches!(r, Err(Refusal::Unresolved(_))), "{r:?}");
}

/// The refusal names the commit and the kind of read, and no word of what matched.
#[test]
fn a_refusal_names_no_word_of_what_matched() {
    let r = heard(SAID).check(&commit(SAID), none).unwrap_err();
    let said = r.to_string();
    assert!(said.contains("c0ffee00c0ff") && said.contains("a look back"), "{said}");
    for w in words(SAID).iter().filter(|w| w.len() > 3) {
        assert!(!said.to_lowercase().contains(w.as_str()), "{w}: {said}");
    }
}

/// The false-positive cases tried at ten: the ordinary words of a change are not ten words of a conversation,
/// and a stock phrase a conversation and a change share ("a test that fails without the fix passes with it", a
/// line of the backlog's own acceptance text) is the repository's: the base holds it, so it passes.
#[test]
fn ordinary_words_of_a_change_do_not_trip_it() {
    const STOCK: &str = "a test that fails without the fix passes with it";
    let h = heard(&format!("{SAID} Then: {STOCK}, and every gate passes."));
    for text in [
        "self: trim the sails before the wind (trim-the-sails)",
        "// Returns the value of the key, or none when it is not there at all.",
        "Co-Authored-By: a model <noreply@example.org>",
    ] {
        assert_eq!(h.check(&commit(text), none), Ok(()), "{text}");
    }
    let base = || Ok(format!("acceptance: vec![\"{STOCK}\".into()],"));
    assert_eq!(h.check(&commit(&format!("// {STOCK}")), base), Ok(()));
    assert!(h.check(&commit(&format!("// {STOCK}")), none).is_err(), "not in the base: a match");
}
