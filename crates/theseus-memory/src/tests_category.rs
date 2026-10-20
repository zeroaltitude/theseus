//! The category term in recall (theseus-jrf4c): a note from a session that
//! shares a category with the asker's ranks above an otherwise equal one,
//! weighted by the kind, and the term admits nothing: the place rule and
//! every other filter run first.

use std::collections::{BTreeMap, BTreeSet};

use crate::recall::{recall, Asker, Candidate, CategoryBoost, Pack, Params, Place, Reason};
use crate::science::Baseline;

fn cand(node: &str, session: &str, fused: f64, place: Place) -> Candidate {
    Candidate {
        node_id: node.into(),
        chunk: 0,
        session_id: session.into(),
        position: 1,
        kind: "user_message".into(),
        origin: "operator".into(),
        external: false,
        text: format!("the text of {node}"),
        fused,
        index_rank: 1,
        place,
        time_ms: 0,
        entities: Vec::new(),
        matched: Vec::new(),
    }
}

fn shares(gain: f64, of: &[(&str, f64)]) -> CategoryBoost {
    CategoryBoost {
        gain,
        shared: of.iter().map(|(s, w)| (s.to_string(), *w)).collect(),
    }
}

fn run(asker_place: &Place, category: Option<CategoryBoost>, cands: Vec<Candidate>) -> Pack {
    let none = BTreeSet::new();
    let asker = Asker {
        session_id: "ses_here",
        place: asker_place,
        in_context: &none,
        labeled: &none,
        links: &[],
        now_ms: 0,
        retention: &BTreeMap::new(),
        threads: None,
    };
    let p = Params {
        category,
        ..Params::default()
    };
    recall(&Baseline::default(), &asker, cands, &p)
}

fn order(p: &Pack) -> Vec<&str> {
    p.admitted
        .iter()
        .map(|a| a.candidate.node_id.as_str())
        .collect()
}

/// Two notes equally relevant: the one whose session shares a category comes
/// first, whatever order the index gave; with the term off the tie is the
/// index's.
#[test]
fn a_same_category_note_ranks_above_an_equal_one() {
    let cands = || {
        vec![
            cand("plain", "ses_plain", 0.050, Place::Private),
            cand("same", "ses_same", 0.050, Place::Private),
        ]
    };
    let on = run(
        &Place::Private,
        Some(shares(0.25, &[("ses_same", 0.6)])),
        cands(),
    );
    assert_eq!(order(&on)[0], "same");
    let mut reversed = cands();
    reversed.reverse();
    let again = run(
        &Place::Private,
        Some(shares(0.25, &[("ses_same", 0.6)])),
        reversed,
    );
    assert_eq!(order(&again)[0], "same");
}

/// The kind's weight orders the sharers: a project or thread outweighs a
/// topic, a topic the rest; a gain of 0 changes nothing.
#[test]
fn the_kinds_weight_orders_the_sharers_and_zero_is_off() {
    let cands = || {
        vec![
            cand("low", "ses_low", 0.050, Place::Private),
            cand("topic", "ses_topic", 0.050, Place::Private),
            cand("project", "ses_project", 0.050, Place::Private),
        ]
    };
    let w = [("ses_low", 0.3), ("ses_topic", 0.6), ("ses_project", 1.0)];
    let on = run(&Place::Private, Some(shares(0.25, &w)), cands());
    assert_eq!(order(&on), vec!["project", "topic", "low"]);
    let off = run(&Place::Private, Some(shares(0.0, &w)), cands());
    let none = run(&Place::Private, None, cands());
    assert_eq!(order(&off), order(&none));
}

/// The term is bounded: a note a third better, sharing nothing, still wins
/// at the default gain; and the term reads only sessions it was given.
#[test]
fn a_clearly_better_note_still_wins() {
    let cands = vec![
        cand("same", "ses_same", 0.050, Place::Private),
        cand("better", "ses_other", 0.050 * 1.4, Place::Private),
    ];
    let p = run(
        &Place::Private,
        Some(shares(0.25, &[("ses_same", 1.0)])),
        cands,
    );
    assert_eq!(order(&p)[0], "better");
}

/// The place rule runs first: a candidate of a place this turn may not draw
/// on stays dropped, however much it shares.
#[test]
fn a_shared_place_candidate_is_still_dropped() {
    let shared = Place::Shared("discord:channel:77".into());
    let p = run(
        &Place::Shared("discord:channel:12".into()),
        Some(shares(1.0, &[("ses_far", 1.0)])),
        vec![
            cand("far", "ses_far", 0.050, shared),
            cand(
                "ok",
                "ses_ok",
                0.040,
                Place::Shared("discord:channel:12".into()),
            ),
        ],
    );
    assert_eq!(order(&p), vec!["ok"]);
    assert_eq!(p.dropped_for(Reason::Place), 1);
}

/// The factor is a reciprocal rank among the sharers: the best sharer gets
/// the full weight, a lower one less, and a session sharing nothing is 1.
#[test]
fn the_term_is_a_reciprocal_rank_among_the_sharers() {
    let b = shares(0.5, &[("a", 1.0), ("b", 1.0)]);
    let f = b.factors([("n1", "a", 0.9), ("n2", "b", 0.5), ("n3", "z", 0.99)]);
    assert!((f["n1"] - 1.5).abs() < 1e-12);
    assert!(f["n2"] > 1.0 && f["n2"] < f["n1"]);
    assert!(!f.contains_key("n3"));
}
