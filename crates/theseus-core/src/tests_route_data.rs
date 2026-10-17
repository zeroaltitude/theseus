//! route.v6's `data` question as the core reads it (theseus-a1wvq.2): the
//! verdict carries Jev's answer with its band, and nothing acts on it yet.

use serde_json::json;
use theseus_judge::band::{band, Band};
use theseus_judge::client::Answer;
use theseus_judge::judge::AnswerRecord;
use theseus_judge::{Judgment, Thresholds};

use crate::judge::inbound::{verdict, ROUTE_PACK};

const T: Thresholds = Thresholds {
    act: 0.90,
    confirm: 0.60,
};

fn choice(question: &str, picked: &str, confidence: f64) -> AnswerRecord {
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

fn judgment(answers: Vec<AnswerRecord>, drift: bool) -> Judgment {
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
fn the_acting_route_pack_is_v6() {
    assert_eq!(ROUTE_PACK, "route.v6");
}

#[test]
fn the_verdict_reads_the_data_answer() {
    let mode = choice("mode", "chat", 0.95);
    for (picked, confidence, want) in [
        ("open", 0.95, Band::Act),
        ("open", 0.70, Band::Confirm),
        ("private", 0.97, Band::Act),
        ("unclear", 0.40, Band::Escalate),
    ] {
        let j = judgment(
            vec![mode.clone(), choice("data", picked, confidence)],
            false,
        );
        let v = verdict(&j, "trn_1").unwrap();
        let d = v.data.expect("the data answer rides the verdict");
        assert_eq!(
            (d.choice.as_str(), d.confidence, d.band),
            (picked, confidence, want)
        );
        assert_eq!(v.mode, "chat", "the mode is read beside it");
    }
}

#[test]
fn a_verdict_without_the_data_answer_carries_none() {
    let v = verdict(
        &judgment(vec![choice("mode", "chat", 0.95)], false),
        "trn_1",
    )
    .unwrap();
    assert!(v.data.is_none());
}

#[test]
fn no_data_answer_is_read_from_a_drifted_judgment() {
    let j = judgment(
        vec![choice("mode", "chat", 0.95), choice("data", "open", 0.99)],
        true,
    );
    assert!(
        verdict(&j, "trn_1").is_none(),
        "only an actionable judgment is read"
    );
}
