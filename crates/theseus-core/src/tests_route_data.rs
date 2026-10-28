//! The route pack's verdict as the core reads it: its `mode`, and nothing of
//! its `data` question, which the labels on a session replaced
//! (theseus-9ymz0): the verdict carries no data answer, whatever Jev said.

use serde_json::json;
use theseus_judge::band::band;
use theseus_judge::client::Answer;
use theseus_judge::judge::AnswerRecord;
use theseus_judge::{Judgment, Thresholds};

use crate::judge::inbound::{verdict, ROUTE_PACK};

const T: Thresholds = Thresholds {
    act: 0.90,
    confirm: 0.60,
};

pub(crate) fn choice(question: &str, picked: &str, confidence: f64) -> AnswerRecord {
    let answer = Answer::Choice {
        choice: picked.into(),
        probabilities: vec![(picked.into(), confidence)],
        confidence,
    };
    AnswerRecord {
        question: question.into(),
        def: question.into(),
        about: None,
        band: band(&answer, T),
        answer,
    }
}

pub(crate) fn judgment(answers: Vec<AnswerRecord>, drift: bool) -> Judgment {
    serde_json::from_value(json!({
        "id": "jdg_data", "pack": ROUTE_PACK, "version": 6, "pack_sha256": "00",
        "point": "inbound", "mode": "live", "model": "jev-1.13.0",
        "answered_by": "jev-1.13.0", "model_drift": drift,
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

#[test]
fn the_acting_route_pack_is_v8() {
    assert_eq!(ROUTE_PACK, "route.v8");
}

#[test]
fn the_verdict_reads_the_mode_and_ignores_the_data_answer() {
    let mode = choice("mode", "chat", 0.95);
    for (picked, confidence) in [
        ("open", 0.95),
        ("open", 0.70),
        ("private", 0.97),
        ("unclear", 0.40),
    ] {
        let j = judgment(
            vec![mode.clone(), choice("data", picked, confidence)],
            false,
        );
        let v = verdict(&j, "trn_1").unwrap();
        assert_eq!(v.mode, "chat", "the mode is read beside it");
    }
}

#[test]
fn no_verdict_is_read_from_a_drifted_judgment() {
    let j = judgment(
        vec![choice("mode", "chat", 0.95), choice("data", "open", 0.99)],
        true,
    );
    assert!(
        verdict(&j, "trn_1").is_none(),
        "only an actionable judgment is read"
    );
}

/// A message that names a configured name is found by the matcher the pack's
/// state and the turn's floor share.
#[test]
fn a_sensitive_name_is_found_in_a_message() {
    use crate::judge::inbound::names_hit;
    use theseus_judge::builders::DataNames;
    let names = DataNames {
        open: vec![],
        private: vec!["Examplecorp".into()],
    };
    assert!(!names_hit(&names, "Summarise the release notes."));
    assert!(names_hit(&names, "Draft the examplecorp's budget memo."));
}
