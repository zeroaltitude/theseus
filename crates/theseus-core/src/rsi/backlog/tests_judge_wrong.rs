//! `judge.wrong:<pack>` is filed from a person's label, not from the nightly run's own guess.

use serde_json::{json, Value};

use crate::ledger::LedgerRow;
use crate::rsi::backlog::read::of_row;

fn label_row(source: &str, label: Value) -> LedgerRow {
    LedgerRow {
        at_unix_ms: 1,
        kind: "judge.label".into(),
        session_id: None,
        turn_id: None,
        data: json!({
            "judgment": "jdg_a",
            "pack": "loop.v2",
            "question": "work_state",
            "label": label,
            "source": source,
            "weight": if source == "system" { 0.5 } else { 1.0 },
        }),
    }
}

/// The system's `false_completion` label is `{"not": "complete"}`: it once filed
/// `judge.wrong:loop.v2` against Jev, though no person had said Jev was wrong. A person's
/// label of the same words still files it, and a label that agrees files nothing.
#[test]
fn only_a_persons_wrong_label_files_a_wrong_judgment() {
    for label in [json!({"not": "complete"}), json!("wrong"), json!("noise")] {
        assert!(
            of_row(7, &label_row("system", label.clone())).is_none(),
            "system {label}"
        );
        let o = of_row(7, &label_row("operator", label.clone())).expect("filed");
        assert_eq!(o.key, "judge.wrong:loop.v2", "{label}");
        assert!(o.person, "{label}");
    }
    assert!(of_row(7, &label_row("operator", json!("progressing"))).is_none());
}
