//! A job cut at a limit its own call chose is no cause for the nightly read.

use serde_json::json;

use super::read::of_row;
use crate::ledger::LedgerRow;

fn cut(producer: &str, failure: &str, duration_ms: u64) -> LedgerRow {
    LedgerRow::named(
        "action.failed",
        None,
        None,
        json!({"correlation_id": "act_1", "producer": producer, "duration_ms": duration_ms,
               "outcome": "failed", "failure": failure}),
    )
}

/// A probe that asked for `timeout_secs: 2` and was cut at 2 s got the answer
/// it asked for: not a fault of ours.
#[test]
fn a_job_cut_at_a_short_limit_its_call_chose_is_not_a_cause() {
    assert!(of_row(1, &cut("wrapper:10", "timeout", 2001)).is_none());
    assert!(of_row(1, &cut("wrapper:11", "timeout", 3_599_999)).is_none());
}

/// A job cut at the default limit or past it is a wedge, and ours.
#[test]
fn a_job_cut_at_the_default_limit_is_still_a_cause() {
    let o = of_row(1, &cut("wrapper:10", "timeout", 3_600_000)).unwrap();
    assert_eq!(o.key, "action.failed:wrapper:timeout");
}

/// Only a short cut timeout is let go: a stop, a signal and a spawn failure
/// after a short run stay causes, as does an in-process tool's timeout.
#[test]
fn other_short_failures_are_still_causes() {
    for f in ["spawn", "signal", "stopped"] {
        assert!(of_row(1, &cut("wrapper:10", f, 2001)).is_some(), "{f}");
    }
    assert!(of_row(1, &cut("inproc:fs.edit", "timeout", 2001)).is_some());
}
