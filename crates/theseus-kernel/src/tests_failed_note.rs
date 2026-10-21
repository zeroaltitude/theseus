//! A failed call's ledger row says why, in a line (`action.failed:wrapper`):
//! a job's `exit status: 1` is its program's own answer, a spawn error is
//! the machinery's, and the nightly read tells them apart by this note.

use serde_json::json;

use crate::tests::*;
use crate::types::*;

/// The `action.failed` rows after one call settles with `outcome` and `note`.
fn settled(outcome: Outcome, note: Option<&str>) -> Vec<serde_json::Value> {
    let w = world();
    let (s, _e, g) = running(&w);
    let a = dispatched(&w, &g, "proc.run", 0);
    let mut c = completion(&a.correlation_id, outcome, None);
    c.producer = "wrapper:4242".into();
    c.detail = note.map(|n| json!({"note": n, "duration_ms": 6}));
    w.kernel.accept_completion(&c).unwrap();
    rows(&w, &s, "action.failed")
}

#[test]
fn a_failed_calls_row_carries_the_note() {
    let r = settled(Outcome::Failed, Some("exit status: 1"));
    assert_eq!(r.len(), 1, "{r:?}");
    assert_eq!(r[0]["note"], "exit status: 1");
    assert_eq!(r[0]["producer"], "wrapper:4242");
}

#[test]
fn a_note_is_cut_and_a_missing_one_leaves_the_row_as_it_was() {
    let long = "x".repeat(500);
    let r = settled(Outcome::Failed, Some(&long));
    assert_eq!(r[0]["note"].as_str().unwrap().len(), 200);
    let r = settled(Outcome::Failed, None);
    assert!(r[0].get("note").is_none(), "{r:?}");
}

#[test]
fn a_call_that_succeeded_has_no_failed_row() {
    assert!(settled(Outcome::Succeeded, Some("exit status: 0")).is_empty());
}
