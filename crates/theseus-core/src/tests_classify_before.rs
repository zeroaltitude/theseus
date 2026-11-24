//! A message judged by `classify.v4` just before the build that made `classify.v5` the wired version (issue 46)
//! still gives its answers to every reader that looks a message's classify judgment up: the exchange row's
//! judged answers, the task its words addressed, and the offer of a task. Those readers once compared the
//! judgment's pack with the wired name only, so a repoint would have dropped that message's answers.

use serde_json::json;
use theseus_judge::band::band;
use theseus_judge::client::Answer;
use theseus_judge::judge::AnswerRecord;
use theseus_judge::Judgment;

const S: &str = "ses_classify_before";
const NODE: &str = "nod_classify_before";
const TASK: &str = "tsk_00000000000000000000000000000046";

fn choice(question: &str, choice: &str, options: &[&str]) -> AnswerRecord {
    let answer = Answer::Choice {
        choice: choice.into(),
        probabilities: options
            .iter()
            .map(|o| (o.to_string(), if *o == choice { 0.97 } else { 0.01 }))
            .collect(),
        confidence: 0.97,
    };
    AnswerRecord {
        question: question.into(),
        def: question.into(),
        about: None,
        band: band(
            &answer,
            theseus_judge::Thresholds {
                act: 0.9,
                confirm: 0.6,
            },
        ),
        no_match: None,
        answer,
    }
}

fn noul(question: &str, p: f64) -> AnswerRecord {
    let answer = Answer::Noul { noul: p };
    AnswerRecord {
        question: question.into(),
        def: question.into(),
        about: None,
        band: band(
            &answer,
            theseus_judge::Thresholds {
                act: 0.9,
                confirm: 0.6,
            },
        ),
        no_match: None,
        answer,
    }
}

/// A live `classify.v4` judgment of `NODE`, as the sink wrote it before the upgrade.
fn v4_judgment() -> Judgment {
    let answers = vec![
        choice("kind", "new_ask", &["new_ask", "follow_up", "other"]),
        choice("addressed_task", TASK, &[TASK, "none"]),
        noul("offer_task", 0.97),
    ];
    serde_json::from_value(json!({
        "id": "jdg_classify_before", "pack": "classify.v4", "version": 4, "pack_sha256": "00",
        "point": "inbound", "mode": "live", "model": "jev-1.13.0", "answered_by": "jev-1.13.0",
        "model_drift": false,
        "state": {"sha256": "00", "bytes": 2, "tokens": 1, "cap_tokens": 3000, "builder": "inbound",
            "builder_version": 1, "truncated": [], "dropped": []},
        "questions": answers.len(), "answers": answers, "call": null,
        "timing": {"queued_ms": 0, "http_ms": 300, "total_ms": 300},
        "usage": null, "cost_micros": 40, "reserve_micros": 50,
        "outcome": {"outcome": "answered"}, "circuit": null, "rate_limit": {},
        "context": {"session": S, "turn": "trn_classify_before", "node": NODE},
    }))
    .unwrap()
}

fn stored(j: &Judgment) -> (tempfile::TempDir, crate::store::Store) {
    let d = tempfile::tempdir().unwrap();
    let store = crate::store::Store::open(d.path()).unwrap();
    let call = crate::fact::judge::JudgeCall {
        judgment: j,
        budget: "live",
    };
    let mut r = crate::fact::row(&call, Some(S), Some("trn_classify_before")).unwrap();
    r.key = Some(j.id.clone());
    store
        .append(&[r.scoped(&crate::rpc::judge::scope_of(&j.pack))])
        .unwrap();
    (d, store)
}

#[test]
fn a_message_judged_by_the_previous_classify_version_still_gives_its_addressed_task() {
    let j = v4_judgment();
    let (_d, store) = stored(&j);
    assert_eq!(
        crate::task_graph::joined::judged_task(&store, S, NODE).as_deref(),
        Some(TASK),
        "the task its words addressed"
    );
    let judged = crate::exchange::ledger::judged(&store, &[S], NODE);
    assert_eq!(
        judged.get("addressed_task").map(String::as_str),
        Some(TASK),
        "{judged:?}"
    );
    assert_eq!(
        judged.get("kind").map(String::as_str),
        Some("new_ask"),
        "{judged:?}"
    );
    assert!(
        crate::judge::inbound::offer_answer(&j),
        "the offer of a task"
    );
}
