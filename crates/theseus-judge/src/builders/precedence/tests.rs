//! `precedence.v1`'s own tests (theseus-3nk): the two questions are asked
//! once a pair and keyed by the caller's key; the two facts the rubric
//! turns on are stated as words and never asked; and the reason a reader is
//! given is worded from those same fields.

use super::*;
use crate::pack::by_name;
use crate::state::NoScrub;

/// A source, with the day and the time its day stands for.
fn src(said_by: Said, day: &str, at_ms: u64, text: &str) -> PrecedenceSource {
    PrecedenceSource {
        said_by,
        day: day.into(),
        at_ms,
        text: text.into(),
    }
}

const DAY: u64 = 86_400_000;

/// The owner said the threshold is 0.55 in March; a note written in
/// September says 0.70. His own statement is the older of the two, so the
/// rubric's rules pull against each other.
fn owner_then_document() -> Pair {
    Pair {
        key: "nod_a|nod_b".into(),
        subject: "the flagging threshold".into(),
        first: src(
            Said::Owner,
            "2026-03-02",
            20_880 * DAY,
            "Use 0.55 for flagging, not 0.7.",
        ),
        second: src(
            Said::Document,
            "2026-09-14",
            21_076 * DAY,
            "The flagging threshold is 0.70.",
        ),
    }
}

fn input(n: usize) -> PrecedenceInput {
    PrecedenceInput {
        pairs: (0..n)
            .map(|i| Pair {
                key: format!("nod_{i}a|nod_{i}b"),
                subject: format!("setting {i}"),
                first: src(Said::Owner, "2026-03-02", 20_880 * DAY, "  Keep it at 3.  "),
                second: src(Said::Model, "2026-09-14", 21_076 * DAY, "It is 4 now."),
            })
            .collect(),
    }
}

/// One Noul a pair for each of the two questions, keyed by the caller's own
/// key, with the pair's subject and both sources in the state; at most ten
/// pairs, and the state holds its cap.
#[test]
fn each_pair_is_asked_twice_and_keyed_by_its_caller() {
    let p = by_name("precedence.v1").expect("precedence.v1 is embedded");
    let prepared = prepare(&p, &Input::Precedence(input(12)), &NoScrub).unwrap();
    let v = prepared.state.value();
    assert_eq!(v["pairs"][0]["about"], "setting 0");
    assert_eq!(v["pairs"][0]["first"]["said_by"], "the owner himself");
    assert_eq!(v["pairs"][0]["first"]["says"], "Keep it at 3.");
    assert_eq!(v["pairs"][0]["second"]["said_by"], "the model");
    let asked = p.ask(&prepared.dynamic);
    // Two questions, each a Noul a pair, over the ten pairs one request asks.
    assert_eq!(asked.len(), PRECEDENCE_PAIRS * 2);
    let ids: Vec<&str> = asked.iter().map(|a| a.id.as_str()).collect();
    assert!(ids.contains(&"disagree.1"), "{ids:?}");
    assert!(ids.contains(&"first_prevails.1"), "{ids:?}");
    for a in &asked {
        assert!(a.about.is_some(), "every Noul is keyed: {}", a.id);
    }
    let first = asked
        .iter()
        .find(|a| a.id == "disagree.1")
        .expect("the first pair is asked");
    assert_eq!(first.about.as_deref(), Some("nod_0a|nod_0b"));
    assert!(prepared.state.tokens <= p.state_cap_tokens);
}

/// The two facts the rubric turns on are stated as words, never asked: who
/// wrote each source, which of the two is the newer, and whether the two
/// starting rules point the same way.
#[test]
fn the_rubrics_two_facts_are_stated_not_asked() {
    let pair = owner_then_document();
    assert_eq!(pair.newer(), Newer::Second);
    assert_eq!(pair.by_author(), Some(Newer::First));
    assert!(
        !pair.rules_agree(),
        "his own words are the older of the two"
    );
    let p = by_name("precedence.v1").unwrap();
    let in_ = Input::Precedence(PrecedenceInput {
        pairs: vec![pair.clone()],
    });
    let v = prepare(&p, &in_, &NoScrub).unwrap();
    let one = &v.state.value()["pairs"][0];
    assert_eq!(one["newer"], "the second");
    assert_eq!(
        one["rules_agree"],
        "the two rules pull against each other: who said it, against which is newer"
    );
    assert_eq!(one["first"]["when"], "2026-03-02");
    // Two sources of one day: freshness decides nothing, so the rules can
    // never be said to disagree.
    let same = Pair {
        second: src(Said::Document, "2026-03-02", 20_880 * DAY, "It is 0.70."),
        ..pair
    };
    assert_eq!(same.newer(), Newer::Same);
    assert!(same.rules_agree());
}

/// Two sources disagreeing produce an answer that names which took
/// precedence AND why, worded from the stated fields and not by any model.
/// Each of the four ways the two rules can fall has its own clause.
#[test]
fn the_answer_names_which_took_precedence_and_why() {
    // His own words, older, against a document: the author rule carries it.
    let pair = owner_then_document();
    assert_eq!(
        pair.why(Newer::First),
        "2026-03-02 (the owner himself) took precedence: the owner himself said it, over a \
         document, though it is the earlier of the two"
    );
    // The same pair the other way: freshness carried it against the higher
    // standing, and the clause says which rule it was and which it was not.
    assert_eq!(
        pair.why(Newer::Second),
        "2026-09-14 (a document) took precedence: it is the later of the two, though the owner \
         himself said the other"
    );
    // An answer that departed from BOTH rules reads as a departure, not as a
    // reason it did not have: a caller passes Jev's answer here.
    let against = Pair {
        first: src(Said::Document, "2026-03-02", 20_880 * DAY, "It is 0.70."),
        second: src(Said::Owner, "2026-09-14", 21_076 * DAY, "Use 0.55."),
        ..owner_then_document()
    };
    assert_eq!(
        against.why(Newer::First),
        "2026-03-02 (a document) took precedence: neither who said it nor its day favours it: a \
         document said it, the owner himself said the other"
    );
    // Two documents: standing equal, so freshness alone decides.
    let docs = Pair {
        first: src(Said::Document, "2026-09-14", 21_076 * DAY, "It is 0.70."),
        second: src(Said::Document, "2026-03-02", 20_880 * DAY, "It is 0.55."),
        ..owner_then_document()
    };
    assert_eq!(docs.by_author(), None);
    assert!(docs.rules_agree(), "freshness stands alone, unopposed");
    assert_eq!(
        docs.why(Newer::First),
        "2026-09-14 (a document) took precedence: it is the later of the two, both being a document"
    );
    // His own words, and also the later: both rules point the same way.
    let both = Pair {
        first: src(Said::Owner, "2026-09-14", 21_076 * DAY, "Use 0.55."),
        second: src(Said::Document, "2026-03-02", 20_880 * DAY, "It is 0.70."),
        ..owner_then_document()
    };
    assert!(both.rules_agree());
    assert_eq!(
        both.why(Newer::First),
        "2026-09-14 (the owner himself) took precedence: the owner himself said it, over a \
         document, and it is the later of the two"
    );
}
