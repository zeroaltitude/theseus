//! A tool's own refusal is no cause for the nightly read.

use serde_json::json;

use super::read::of_row;
use crate::ledger::LedgerRow;

fn failed(producer: &str, failure: Option<&str>) -> LedgerRow {
    let mut d = json!({"correlation_id": "act_1", "producer": producer,
                       "duration_ms": 0, "outcome": "failed"});
    if let Some(f) = failure {
        d["failure"] = json!(f);
    }
    LedgerRow::named("action.failed", None, None, d)
}

/// A tool's own refusal (an edit whose text is not in the file) is its
/// answer to the model, not a fault of ours: no cause, as for an exit.
#[test]
fn an_in_process_tools_refusal_is_not_a_cause() {
    assert!(of_row(1, &failed("inproc:fs.edit", Some("refused"))).is_none());
}

/// A timeout of the same tool is still ours to fix.
#[test]
fn an_in_process_tools_timeout_is_still_a_cause() {
    let o = of_row(1, &failed("inproc:fs.edit", Some("timeout"))).unwrap();
    assert!(
        o.key.starts_with("action.failed:inproc:fs.edit"),
        "{}",
        o.key
    );
}
