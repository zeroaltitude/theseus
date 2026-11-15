//! A failed action's ledger row keeps the words its completion gave for the
//! failure (`detail.error`), so the nightly read can say why a call failed and
//! not only that it did (`action.failed:inproc:fs.patch`: "failed after 21 ms").

use serde_json::json;

use crate::tests::*;
use crate::types::*;

fn failed_row(detail: Option<serde_json::Value>) -> serde_json::Value {
    let w = world();
    let (sid, _e, g) = running(&w);
    let a = dispatched(&w, &g, "fs.patch", 0);
    let mut c = completion(&a.correlation_id, Outcome::Failed, None);
    c.producer = "inproc:fs.patch".into();
    c.detail = detail;
    w.kernel.accept_completion(&c).unwrap();
    let mut failed = rows(&w, &sid, "action.failed");
    assert_eq!(failed.len(), 1);
    failed.remove(0)
}

#[test]
fn a_failed_rows_detail_carries_the_error_it_was_given() {
    let row = failed_row(Some(json!({
        "duration_ms": 21,
        "error": "the patch does not apply to src/a.rs: error applying hunk #1"
    })));
    assert_eq!(
        row["detail"]["error"],
        "the patch does not apply to src/a.rs: error applying hunk #1"
    );
    assert_eq!(row["producer"], "inproc:fs.patch");
}

#[test]
fn a_long_error_is_cut_and_a_missing_or_empty_one_leaves_no_detail() {
    let row = failed_row(Some(json!({"error": "x".repeat(2000)})));
    assert_eq!(
        row["detail"]["error"].as_str().unwrap().chars().count(),
        500
    );
    assert!(failed_row(Some(json!({"error": "  "})))
        .get("detail")
        .is_none());
    assert!(failed_row(Some(json!({"duration_ms": 3})))
        .get("detail")
        .is_none());
    assert!(failed_row(None).get("detail").is_none());
}

#[test]
fn a_succeeded_row_never_carries_an_error() {
    let w = world();
    let (sid, _e, g) = running(&w);
    let a = dispatched(&w, &g, "fs.read", 0);
    let mut c = completion(&a.correlation_id, Outcome::Succeeded, None);
    c.detail = Some(json!({"error": "not a failure"}));
    w.kernel.accept_completion(&c).unwrap();
    let ok = rows(&w, &sid, "action.succeeded");
    assert_eq!(ok.len(), 1);
    assert!(ok[0].get("detail").is_none());
}
