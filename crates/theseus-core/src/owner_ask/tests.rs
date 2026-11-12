//! `owner_ask`'s own tests (theseus-mqret): the retrieval is pure, so every
//! case here is a table over records built in the test. No clock is read and
//! no daemon runs: `answer` takes slices and gives a selection.

use theseus_protocol::card::{CardObservation, ObservationKind, ObservationState};
use theseus_protocol::standing::{StandingKind, StandingRecord, StandingScope};

use super::{answer, Held, MAX_ITEMS, MIN_COVER};

/// A live standing record. The names are invented (AGENTS.md, Item 16).
fn rec(id: &str, kind: StandingKind, text: &str, since_ms: u64) -> StandingRecord {
    StandingRecord {
        id: id.into(),
        kind,
        text: text.into(),
        scope: StandingScope::Everywhere,
        since_ms,
        until_ms: None,
        source: "capture:nod_1".into(),
        ends: None,
    }
}

fn obs(id: &str, text: &str, at_ms: u64, state: ObservationState) -> CardObservation {
    CardObservation {
        id: id.into(),
        session: "ses_4821".into(),
        nodes: vec!["nod_2".into()],
        kind: ObservationKind::Preference,
        text: text.into(),
        at_ms,
        contradicts: None,
        state,
        proposal: None,
    }
}

/// The answer is a selection of lines Theseus already holds, each with the
/// record to look up: the decision that governs this card is retrieval WITH
/// CITATION, so a line with no citation is not an answer this module can give.
#[test]
fn every_line_of_an_answer_names_the_record_it_came_from() {
    let standing = [rec(
        "std_1",
        StandingKind::Fact,
        "the owner's favourite animal is the pangolin",
        1_000,
    )];
    let observed = [obs(
        "cob_1",
        "the owner prefers short replies",
        2_000,
        ObservationState::Open,
    )];
    let a = answer(
        "what is the owner's favourite animal?",
        Held {
            standing: &standing,
            observed: &observed,
        },
    );
    assert_eq!(a.searched, 2, "both lines were read: {a:?}");
    assert_eq!(a.items.len(), 1, "one matched: {a:?}");
    let it = &a.items[0];
    assert_eq!(it.citation, "standing:std_1");
    assert_eq!(it.held_as, "fact");
    assert_eq!(it.source, "capture:nod_1");
    assert_eq!(it.text, "the owner's favourite animal is the pangolin");
    assert!(!it.unconfirmed, "a standing record is the owner's own");
    assert!(it.covers >= MIN_COVER, "{}", it.covers);
}

/// An observation is unconfirmed and on no card: the owner has not answered its
/// question, so an item made from one says so and a surface can never state it
/// as a settled fact about him.
#[test]
fn an_observations_item_says_it_is_unconfirmed() {
    let observed = [obs(
        "cob_7",
        "the owner walks at dawn",
        5_000,
        ObservationState::Open,
    )];
    let a = answer(
        "does the owner walk at dawn?",
        Held {
            standing: &[],
            observed: &observed,
        },
    );
    assert_eq!(a.items.len(), 1, "{a:?}");
    assert_eq!(a.items[0].citation, "card.obs:cob_7");
    assert_eq!(a.items[0].held_as, "observation");
    assert!(a.items[0].unconfirmed, "{:?}", a.items[0]);
}

/// A used or dropped observation has had its question answered already, so it
/// is no longer what Theseus holds unconfirmed and is not searched; an order is
/// what to do and not a fact about him. Both are asserted beside the positive
/// count of what IS searched, so an empty input cannot pass this.
#[test]
fn an_order_and_a_settled_observation_are_not_searched() {
    let standing = [
        rec(
            "std_2",
            StandingKind::Order,
            "always walk the dog at dawn",
            1_000,
        ),
        rec(
            "std_3",
            StandingKind::Preference,
            "the owner walks at dawn",
            1_000,
        ),
        rec("std_4", StandingKind::Fact, "   ", 1_000),
    ];
    let observed = [
        obs(
            "cob_2",
            "the owner walks at dawn",
            2_000,
            ObservationState::Used,
        ),
        obs(
            "cob_3",
            "the owner walks at dawn",
            2_000,
            ObservationState::Dropped,
        ),
        obs(
            "cob_4",
            "the owner walks at dawn",
            2_000,
            ObservationState::Open,
        ),
    ];
    let a = answer(
        "does the owner walk at dawn?",
        Held {
            standing: &standing,
            observed: &observed,
        },
    );
    assert_eq!(
        a.searched, 2,
        "the preference and the open observation: {a:?}"
    );
    let mut cites: Vec<&str> = a.items.iter().map(|i| i.citation.as_str()).collect();
    cites.sort_unstable();
    assert_eq!(cites, ["card.obs:cob_4", "standing:std_3"], "{a:?}");
}

/// A question whose words no line shares answers with nothing — and says how
/// many lines it read, so "Theseus holds nothing about that" is told from
/// "Theseus holds nothing at all". The bar "no items" is met by an empty input,
/// which is why the count rides beside it.
#[test]
fn an_answer_with_no_items_still_says_what_it_searched() {
    let standing = [rec(
        "std_5",
        StandingKind::Fact,
        "the owner's favourite animal is the pangolin",
        1_000,
    )];
    let held = Held {
        standing: &standing,
        observed: &[],
    };
    let none = answer("which port does the alder relay use?", held);
    assert!(none.items.is_empty(), "{none:?}");
    assert_eq!(none.searched, 1, "it read the one line it holds");
    let empty = answer("which port does the alder relay use?", Held::default());
    assert!(empty.items.is_empty());
    assert_eq!(empty.searched, 0, "nothing was held at all");
}

/// A question with no content words of its own matches nothing, whatever is
/// held: the cover is a share of the question's words, and a share of none is
/// no answer rather than every answer.
#[test]
fn a_question_with_no_content_words_matches_nothing() {
    let standing = [rec(
        "std_6",
        StandingKind::Fact,
        "the owner is an early riser",
        1_000,
    )];
    let a = answer(
        "and the? you",
        Held {
            standing: &standing,
            observed: &[],
        },
    );
    assert!(a.words.is_empty(), "{a:?}");
    assert!(a.items.is_empty(), "{a:?}");
    assert_eq!(a.searched, 1, "it still read the line");
}

/// The order is total and it is the same order every run: best cover first,
/// then the newest, then the citation. Two lines of equal cover and equal time
/// are ordered by their citation alone, so no answer depends on the order the
/// caller read its records in.
#[test]
fn the_same_question_over_the_same_records_gives_one_order() {
    let standing = [
        rec(
            "std_b",
            StandingKind::Fact,
            "the owner walks at dawn",
            3_000,
        ),
        rec(
            "std_a",
            StandingKind::Fact,
            "the owner walks at dawn",
            3_000,
        ),
        rec(
            "std_c",
            StandingKind::Fact,
            "the owner walks at dawn by the river",
            9_000,
        ),
    ];
    let forward = answer(
        "does the owner walk at dawn",
        Held {
            standing: &standing,
            observed: &[],
        },
    );
    let mut backward_recs = standing;
    backward_recs.reverse();
    let backward = answer(
        "does the owner walk at dawn",
        Held {
            standing: &backward_recs,
            observed: &[],
        },
    );
    assert_eq!(
        forward, backward,
        "the order is the records', not the caller's"
    );
    let cites: Vec<&str> = forward.items.iter().map(|i| i.citation.as_str()).collect();
    // Every line covers the same two words of the question, so the order is
    // the tie-breaks alone: std_c is the newest, and the two of equal time go
    // by citation.
    assert_eq!(
        cites,
        ["standing:std_c", "standing:std_a", "standing:std_b"]
    );
    assert!(
        forward.items[0].covers >= forward.items[1].covers,
        "{forward:?}"
    );
}

/// An answer is short enough to check line by line: at most [`MAX_ITEMS`],
/// and the count it searched still says how many there were.
#[test]
fn an_answer_holds_at_most_its_cap() {
    let standing: Vec<_> = (0..MAX_ITEMS + 5)
        .map(|i| {
            rec(
                &format!("std_{i:02}"),
                StandingKind::Fact,
                "the owner walks at dawn",
                1_000 + i as u64,
            )
        })
        .collect();
    let a = answer(
        "does the owner walk at dawn",
        Held {
            standing: &standing,
            observed: &[],
        },
    );
    assert_eq!(a.items.len(), MAX_ITEMS, "{}", a.items.len());
    assert_eq!(a.searched, MAX_ITEMS + 5);
}

/// Nothing is rewritten: the line in the item is the line in the record, byte
/// for byte, so a surface shows the owner his own words.
#[test]
fn the_item_carries_the_held_line_verbatim() {
    let text = "the owner's  favourite   animal is the pangolin (he says so often)";
    let standing = [rec("std_9", StandingKind::Preference, text, 1_000)];
    let a = answer(
        "favourite animal",
        Held {
            standing: &standing,
            observed: &[],
        },
    );
    assert_eq!(a.items.len(), 1, "{a:?}");
    assert_eq!(a.items[0].text, text);
}
