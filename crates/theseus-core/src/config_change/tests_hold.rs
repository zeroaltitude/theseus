//! A hold's words ([`super::hold_words`]): each cause says itself, the flag names the way to
//! clear it where the conversation is, and every sentence fits whole in what a card prints.

use super::{flag_way_in, FLAG_HOLD, OUTSIDE_HOLD};

const SESSION: &str = "ses_01a1593f784974cf92a4a4666c968b67";

/// The ways to clear the flag a hold can name: in the owner's Discord DM, and anywhere else.
pub(crate) fn ways() -> [String; 2] {
    [
        flag_way_in(Some("discord:dm:1"), SESSION),
        flag_way_in(None, SESSION),
    ]
}

/// What a card prints of a hold's words: the clause of the reason the gate gives a call held
/// at layer 1 (`policy.rs`'s wording), as every surface reads it.
fn printed(name: &str, said: &str) -> String {
    crate::needs_you::rule_clause(&format!("look: {name} — approve (layer 1: {said})"))
        .unwrap_or_default()
}

/// `name`'s hold, on each pair of causes and each way: a clean turn is not held; outside text
/// alone says it and when it ends; the flag, alone or with outside text, says it and the way
/// to clear it, and never the outside sentence; and the card prints each sentence whole, with
/// no "layer 1:", inside the Discord card's cut at 200 characters.
pub(crate) fn each_cause_is_said(name: &str, held: impl Fn(bool, bool, &str) -> Option<String>) {
    for way in ways() {
        assert_eq!(held(false, false, &way), None, "{name}");
        let outside = held(true, false, &way).expect(name);
        assert!(outside.ends_with(&format!(": {OUTSIDE_HOLD}")), "{outside}");
        assert!(!outside.contains(FLAG_HOLD), "{outside}");
        for (o, s) in [(false, true), (true, true)] {
            let flag = held(o, s, &way).expect(name);
            assert!(
                flag.ends_with(&format!(": {FLAG_HOLD}: {way}")),
                "{name} {o} {s}: {flag}"
            );
            assert!(!flag.contains("read text from outside"), "{flag}");
        }
        for said in [outside, held(false, true, &way).unwrap()] {
            let card = printed(name, &said);
            assert_eq!(card, said, "{name}: the card prints the sentence whole");
            assert!(
                card.chars().count() <= 200,
                "{name}: {} characters: {card}",
                card.chars().count()
            );
        }
    }
}

/// In the owner's DM the flag is cleared there, by `/trust` or the notice's Remove; anywhere
/// else by the command, which names the session by its last four characters.
#[test]
fn the_way_to_clear_the_flag_is_the_one_that_works_where_the_conversation_is() {
    let [dm, elsewhere] = ways();
    assert!(dm.contains("/trust") && dm.contains("Remove"), "{dm}");
    assert_eq!(elsewhere, "`theseus content remove 8b67 injection`");
    assert_eq!(
        flag_way_in(Some("cli:1"), SESSION),
        elsewhere,
        "a place with no /trust gets the command"
    );
}
