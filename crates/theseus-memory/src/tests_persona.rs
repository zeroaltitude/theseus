//! The persona's recall boost (theseus-a8n7): an engaged persona's own
//! agent's imported episodes rank above others at equal relevance, by a
//! bounded multiplier, and never otherwise: a clearly better note of another
//! source still wins, and the boost admits nothing the filters dropped.

use std::collections::{BTreeMap, BTreeSet};

use crate::recall::{recall, Asker, Candidate, Pack, Params, Place, Reason};
use crate::science::Baseline;
use crate::PersonaBoost;

fn cand(node: &str, session: &str, fused: f64) -> Candidate {
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
        place: Place::Private,
        time_ms: 0,
        entities: Vec::new(),
        matched: Vec::new(),
    }
}

fn boost(b: f64) -> PersonaBoost {
    PersonaBoost {
        agent: "wren".into(),
        boost: b,
        of_session: [
            ("ses_wren".to_string(), "wren".to_string()),
            ("ses_finch".to_string(), "finch".to_string()),
        ]
        .into_iter()
        .collect(),
    }
}

fn run(persona: Option<PersonaBoost>, cands: Vec<Candidate>) -> Pack {
    let none = BTreeSet::new();
    let asker = Asker {
        session_id: "ses_here",
        place: &Place::Private,
        in_context: &none,
        labeled: &none,
        links: &[],
        now_ms: 0,
        retention: &BTreeMap::new(),
        threads: None,
    };
    let p = Params {
        persona,
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

/// At equal relevance the engaged persona's own episode comes first; with no
/// persona engaged the order is the index's.
#[test]
fn at_equal_relevance_the_engaged_personas_own_comes_first() {
    let cands = || {
        vec![
            cand("other", "ses_finch", 0.050),
            cand("own", "ses_wren", 0.050),
            cand("plain", "ses_native", 0.050),
        ]
    };
    let on = run(Some(boost(1.2)), cands());
    assert_eq!(order(&on)[0], "own");
    let pack = run(None, cands());
    let off = order(&pack);
    assert_eq!(off.len(), 3);
    assert_ne!(
        off[0], "own",
        "with no persona the tie is the index's: {off:?}"
    );
}

/// The boost is bounded: a note a fifth better than the persona's own still
/// outranks it at 1.2, and a boost of 1.0 changes nothing.
#[test]
fn a_clearly_better_note_still_wins_and_one_is_off() {
    let cands = || {
        vec![
            cand("own", "ses_wren", 0.050),
            cand("better", "ses_finch", 0.050 * 1.25),
        ]
    };
    let on = run(Some(boost(1.2)), cands());
    assert_eq!(order(&on)[0], "better");
    let (one, none) = (run(Some(boost(1.0)), cands()), run(None, cands()));
    assert_eq!(order(&one), order(&none));
    // A boost past the bound is held to 2.0.
    assert_eq!(boost(9.0).factor("ses_wren"), 2.0);
    assert_eq!(boost(1.2).factor("ses_finch"), 1.0);
    assert_eq!(boost(1.2).factor("ses_unknown"), 1.0);
}

/// The boost reorders the pack and admits nothing: a note a filter drops
/// stays dropped.
#[test]
fn the_boost_admits_nothing_a_filter_dropped() {
    let mut own = cand("own", "ses_wren", 0.0001);
    own.external = true;
    let p = run(Some(boost(2.0)), vec![own, cand("ok", "ses_finch", 0.05)]);
    assert_eq!(order(&p), vec!["ok"]);
    assert!(p.dropped.iter().any(|d| d.candidate.node_id == "own"
        && matches!(d.reason, Reason::Untrusted | Reason::Threshold)));
}
