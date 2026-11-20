//! `judge.wrong:<pack>` is filed from a person's label, not from the nightly run's own guess.

use serde_json::{json, Value};

use super::read::of_row;
use crate::ledger::LedgerRow;

fn label_row(source: &str, label: Value) -> LedgerRow {
    LedgerRow {
        at_unix_ms: 1,
        kind: "judge.label".into(),
        session_id: None,
        turn_id: None,
        data: json!({
            "judgment": "jdg_a",
            "pack": "loop.v1",
            "question": "work_state",
            "label": label,
            "source": source,
            "weight": if source == "system" { 0.5 } else { 1.0 },
        }),
    }
}

/// The system's `false_completion` label is `{"not": "complete"}`: it once filed
/// `judge.wrong:loop.v1` against Jev, though no person had said Jev was wrong.
#[test]
fn a_system_label_files_no_wrong_judgment() {
    for label in [json!({"not": "complete"}), json!("wrong")] {
        assert!(
            of_row(7, &label_row("system", label.clone())).is_none(),
            "{label}"
        );
    }
}

#[test]
fn a_persons_wrong_label_still_files_the_judgment() {
    for label in [json!({"not": "complete"}), json!("wrong"), json!("noise")] {
        let o = of_row(7, &label_row("operator", label.clone())).expect("filed");
        assert_eq!(o.key, "judge.wrong:loop.v1", "{label}");
        assert!(o.person, "{label}");
    }
}

#[test]
fn a_label_that_agrees_files_nothing() {
    assert!(of_row(7, &label_row("operator", json!("progressing"))).is_none());
}
