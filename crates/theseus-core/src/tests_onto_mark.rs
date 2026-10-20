//! The private mark's decision (theseus-und4g): what `onto.v1`'s privacy
//! answers come to, with the switch off and on, at and under the act band.

use serde_json::json;
use theseus_judge::band::band;
use theseus_judge::client::Answer;
use theseus_judge::judge::AnswerRecord;
use theseus_judge::{Judgment, Thresholds};

use crate::judge::onto::decide::Bands;
use crate::judge::onto::mark::{decide, Reason};

const T: Thresholds = Thresholds {
    act: 0.80,
    confirm: 0.55,
};
const BANDS: Bands = Bands {
    act: 0.80,
    confirm: 0.55,
};

fn record(question: &str, answer: Answer) -> AnswerRecord {
    AnswerRecord {
        question: question.into(),
        def: question.into(),
        about: None,
        band: band(&answer, T),
        answer,
    }
}

fn noul(question: &str, p: f64) -> AnswerRecord {
    record(question, Answer::Noul { noul: p })
}

/// `personal`'s four levels with these probabilities (none, mentions,
/// discloses, sensitive).
fn personal(levels: [f64; 4]) -> AnswerRecord {
    let score: f64 = levels.iter().enumerate().map(|(i, p)| i as f64 * p).sum();
    record(
        "personal",
        Answer::Score {
            score,
            probabilities: levels.to_vec(),
            confidence: levels.iter().cloned().fold(0.0, f64::max),
        },
    )
}

fn judgment(answers: Vec<AnswerRecord>) -> Judgment {
    serde_json::from_value(json!({
        "id": "jdg_mark", "pack": "onto.v1", "version": 1, "pack_sha256": "00",
        "point": "inbound", "mode": "live", "model": "jev-1.13.0",
        "answered_by": "jev-1.13.0", "model_drift": false,
        "state": {"sha256": "00", "bytes": 10, "tokens": 3, "cap_tokens": 3000,
            "builder": "inbound", "builder_version": 1, "truncated": [], "dropped": []},
        "questions": answers.len(), "answers": answers, "call": null,
        "timing": {"queued_ms": 0, "http_ms": 9, "total_ms": 9},
        "usage": null, "cost_micros": 1, "reserve_micros": 1,
        "outcome": {"outcome": "answered"}, "circuit": null, "rate_limit": {},
        "context": {},
    }))
    .unwrap()
}

/// With the switch off, no answer marks anything: the judgment is a record.
#[test]
fn a_switched_off_mark_decides_nothing() {
    let j = judgment(vec![
        personal([0.0, 0.0, 0.1, 0.9]),
        noul("shows_people", 0.99),
        noul("shows_identifiers", 0.99),
    ]);
    assert!(decide(&j, false, &BANDS).is_empty());
    assert_eq!(
        decide(&j, true, &BANDS),
        [
            Reason::Personal,
            Reason::ShowsPeople,
            Reason::ShowsIdentifiers
        ]
    );
}

/// `personal` marks at *discloses* or above, by the mass of those levels; a
/// mention in passing, or a doubtful disclosure, marks nothing.
#[test]
fn personal_marks_at_discloses_and_above_at_the_act_band() {
    let marks = |levels| !decide(&judgment(vec![personal(levels)]), true, &BANDS).is_empty();
    assert!(marks([0.0, 0.1, 0.5, 0.4]), "0.9 of the mass is discloses+");
    assert!(marks([0.0, 0.0, 0.0, 1.0]));
    assert!(
        !marks([0.0, 0.9, 0.1, 0.0]),
        "a mention is not a disclosure"
    );
    assert!(!marks([0.1, 0.2, 0.4, 0.3]), "0.7 is under the act band");
    assert!(!marks([1.0, 0.0, 0.0, 0.0]));
}

/// The two `shows_*` Nouls mark at the act band, and only those two:
/// a screen or a document is filing, not a private matter.
#[test]
fn shows_people_and_identifiers_mark_at_the_act_band() {
    let reasons = |answers| decide(&judgment(answers), true, &BANDS);
    assert_eq!(
        reasons(vec![noul("shows_people", 0.8)]),
        [Reason::ShowsPeople]
    );
    assert_eq!(
        reasons(vec![noul("shows_identifiers", 0.85)]),
        [Reason::ShowsIdentifiers]
    );
    assert!(reasons(vec![noul("shows_people", 0.79)]).is_empty());
    assert!(reasons(vec![noul("shows_screen_or_document", 0.99)]).is_empty());
    assert!(reasons(vec![]).is_empty(), "a text-only message asks none");
}
