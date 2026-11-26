//! The outgoing check's test itself (`super`): its words, its line at ten, the repository's own words, one piece at
//! a time, and a refusal that names no word of what matched.

use std::collections::HashSet;

use super::leave::{Leaving, Out};
use super::{runs, words, Heard, Part, Refusal, Source, Why, RUN};

/// Words of a conversation: thirteen, none of them code.
const SAID: &str =
    "Kelp oars drift past the third bollard, where the heron waits each grey morning.";

fn heard(text: &str) -> Heard {
    let mut h = Heard::default();
    h.add(text, Source::History);
    h
}

/// One commit whose message is `text`.
fn commit(text: &str) -> Leaving {
    Leaving {
        branch: "self/20261125-trim".into(),
        commits: vec![Out {
            id: "c0ffee00c0ffee00".into(),
            message: text.into(),
            ..Out::default()
        }],
    }
}

fn none(_: &HashSet<u64>) -> Result<HashSet<u64>, Why> {
    Ok(HashSet::new())
}

/// A base holding `text` in one file.
fn base_of(text: String) -> impl FnOnce(&HashSet<u64>) -> Result<HashSet<u64>, Why> {
    move |wanted| {
        Ok(runs(&words(&text))
            .into_iter()
            .filter(|h| wanted.contains(h))
            .collect())
    }
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
    let ten = format!(
        "/// {}\n/// {}",
        w[..5].join(" ").to_uppercase(),
        w[5..10].join("  ")
    );
    assert_eq!(
        h.check(&commit(&ten), none),
        Err(Refusal::Words {
            commit: Some("c0ffee00c0ffee00".into()),
            part: Part::Message,
            source: Source::History
        })
    );
}

/// A run the repository already holds at the base is its own words, not a person's: it passes. The base is read
/// only when something matched.
#[test]
fn a_run_the_repository_holds_at_the_base_passes() {
    let h = heard(SAID);
    let read = std::cell::Cell::new(false);
    let base = |wanted: &HashSet<u64>| {
        read.set(true);
        base_of(format!("# Notes\n{SAID}\n"))(wanted)
    };
    assert_eq!(h.check(&commit(SAID), base), Ok(()));
    assert!(read.get());
    let unread = |_: &HashSet<u64>| -> Result<HashSet<u64>, Why> { panic!("read with no match") };
    assert_eq!(h.check(&commit("nothing of it"), unread), Ok(()));
}

/// A base that cannot be read, when something matched, is a refusal: nothing is cleared by default.
#[test]
fn a_base_that_cannot_be_read_refuses() {
    let h = heard(SAID);
    let r = h.check(&commit(SAID), |_| Err(Why::Base));
    assert_eq!(r, Err(Refusal::Unresolved(Why::Base)));
}

/// The refusal names the commit, the piece's kind and the kind of read, and no word of what matched.
#[test]
fn a_refusal_names_no_word_of_what_matched() {
    let r = heard(SAID).check(&commit(SAID), none).unwrap_err();
    let said = r.to_string();
    assert!(
        said.contains("c0ffee00c0ff") && said.contains("a look back"),
        "{said}"
    );
    for w in words(SAID).iter().filter(|w| w.len() > 3) {
        assert!(!said.to_lowercase().contains(w.as_str()), "{w}: {said}");
    }
    for why in [Why::Quote, Why::Fold, Why::ViewMissing, Why::Commits] {
        assert!(!Refusal::Unresolved(why).to_string().contains("bollard"));
    }
}

/// The false-positive cases tried at ten: the ordinary words of a change are not ten words of a conversation, and a
/// stock phrase a conversation and a change share ("a test that fails without the fix passes with it", a line of
/// the backlog's own acceptance text) is the repository's: the base holds it, so it passes.
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
    let base = base_of(format!("acceptance: vec![\"{STOCK}\".into()],"));
    assert_eq!(h.check(&commit(&format!("// {STOCK}")), base), Ok(()));
    assert!(
        h.check(&commit(&format!("// {STOCK}")), none).is_err(),
        "not in the base: a match"
    );
}

/// Each piece is read on its own: a run is never made across two files, two paths, or a message and a file; and a
/// run in a path, the names or the branch's name is refused as that piece.
#[test]
fn each_piece_is_its_own_and_every_piece_is_read() {
    let h = heard(SAID);
    let w: Vec<&str> = SAID.split_whitespace().collect();
    let mut split = commit(&w[..5].join(" "));
    split.commits[0].files = vec![w[5..10].join(" ")];
    assert_eq!(h.check(&split, none), Ok(()), "five and five in two pieces");
    let hyphened = w[..10].join("-").replace([',', '.'], "");
    type Edit = Box<dyn Fn(&mut Out, &str)>;
    let edits: Vec<(Part, Edit)> = vec![
        (
            Part::Path,
            Box::new(|o, t| o.paths = vec![format!("docs/{t}.md")]),
        ),
        (Part::Names, Box::new(|o, t| o.names = t.to_string())),
        (Part::File, Box::new(|o, t| o.files = vec![t.to_string()])),
    ];
    for (part, edit) in edits {
        let mut l = commit("trim");
        edit(&mut l.commits[0], &hyphened);
        assert_eq!(
            h.check(&l, none),
            Err(Refusal::Words {
                commit: Some("c0ffee00c0ffee00".into()),
                part,
                source: Source::History
            })
        );
    }
    let mut named = commit("trim");
    named.branch = format!("self/{hyphened}");
    assert_eq!(
        h.check(&named, none),
        Err(Refusal::Words {
            commit: None,
            part: Part::Branch,
            source: Source::History
        })
    );
    let said = h.check(&named, none).unwrap_err().to_string();
    assert!(
        !said.contains("bollard") && !said.contains("self/"),
        "{said}"
    );
}
