//! `content.v4` (theseus-3i6d1): the privacy bar, restated in the owner's words.
//!
//! The owner, 2026-11-08, after a line he planted in a disk question -- a trip to
//! the shop for a praying mantis, his favourite animal -- raised `personal` and
//! cost the session its restricted-origin models: "this shouldn't be 'is it
//! personal' it should be 'is this specifically sensitive information that would
//! not be appropriate to be publicly shared'."
//!
//! v4 changes that one question and nothing else, which is what this module holds.
//! It is here rather than in `pack.rs`'s own tests because `pack.rs` is at the
//! shape budget's 2,500 lines.

use crate::pack::{by_name, QuestionDef};

/// One question of a pack, by name.
fn q<'a>(p: &'a crate::pack::Pack, n: &str) -> &'a QuestionDef {
    p.question(n).unwrap_or_else(|| panic!("{n}"))
}

/// `content.v4` (theseus-3i6d1) changes exactly one question. The owner's
/// words are the specification: "this shouldn't be 'is it personal' it should
/// be 'is this specifically sensitive information that would not be
/// appropriate to be publicly shared'." So `personal` is restated and
/// `injection`, `secret` and `confidential` are v3's byte for byte -- a
/// version that quietly moved the other three would move three bars on one
/// measurement, and nothing would say which one did the work.
#[test]
fn content_v4_restates_personal_and_keeps_the_rest_of_v3() {
    let (v3, v4) = (
        by_name("content.v3").unwrap(),
        by_name("content.v4").unwrap(),
    );
    assert_eq!(v4.version, 4);
    assert_eq!(v4.id, v3.id);
    assert_eq!(
        (
            v4.point,
            v4.builder,
            v4.baseline,
            v4.action,
            v4.state_cap_tokens,
            v4.sample
        ),
        (
            v3.point,
            v3.builder,
            v3.baseline,
            v3.action,
            v3.state_cap_tokens,
            v3.sample
        )
    );
    assert_eq!(v4.rollback, v3.rollback);
    assert_eq!(v4.jev_model, v3.jev_model);
    for n in ["injection", "secret", "confidential"] {
        assert_eq!(q(&v4, n), q(&v3, n), "{n} is v3's, byte for byte");
    }
    let means = |d: &QuestionDef, id: &str| {
        d.options
            .iter()
            .find(|o| o.id == id)
            .unwrap()
            .means
            .clone()
            .unwrap_or_default()
    };
    let (a, b) = (q(&v4, "personal"), q(&v3, "personal"));
    // Everything but the words: the bar, what it decides, and when it is asked.
    assert_eq!(
        (&a.decides, &a.thresholds, &a.no_match, &a.only_when),
        (&b.decides, &b.thresholds, &b.no_match, &b.only_when),
    );
    // The reveal rule of v2 stays at the head of the instructions; the question
    // after it is the owner's, not v3's.
    assert!(a.instructions.contains("not what it is about"));
    assert!(
        a.instructions
            .contains("specifically sensitive information about a person that would not be appropriate to be publicly shared"),
        "personal does not ask the owner's question: {}",
        a.instructions
    );
    assert!(
        !a.instructions
            .contains("does it reveal a person's private information"),
        "personal still asks v3's question: {}",
        a.instructions
    );
    // `yes` is the publication test, and it still names what v3 named, so the
    // narrowing is on the innocuous side alone.
    let yes = means(a, "yes");
    assert!(yes.contains("publishing would not be appropriate"), "{yes}");
    for kept in [
        "Health",
        "finances",
        "identification numbers",
        "home address",
        "personal contact details",
        "private communications",
    ] {
        assert!(yes.contains(kept), "yes drops {kept:?}: {yes}");
    }
    // `no` names the region v3 had no words for, and keeps v3's own cues.
    let no = means(a, "no");
    for named in [
        "favourite animal",
        "a hobby",
        "a pet",
        "a birthday",
        "a mood",
    ] {
        assert!(no.contains(named), "no does not name {named:?}: {no}");
    }
    assert!(
        no.contains("not whether the detail is personal"),
        "no does not say which question is being asked: {no}"
    );
    assert!(no.contains("`public`"), "{no}");
    assert!(no.contains("`context`"), "{no}");
    assert!(no.contains("Placeholder identities are not people"), "{no}");
    assert!(
        no.contains("Public is not clean"),
        "public must still not mean clean: {no}"
    );
    assert_ne!(v4.description, v3.description);
}
