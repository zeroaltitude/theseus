//! An in-process call's completion detail names why a call failed
//! (`action.failed:inproc:fs.patch` read "failed after 21 ms" and nothing
//! else), and stays what it was for a call that succeeded.

use serde_json::json;

use crate::toolrun::inproc::failed_detail;
use crate::toolrun::ResultStatus;

#[test]
fn a_failed_calls_detail_carries_its_words() {
    let meta = json!({"k": 1});
    let d = failed_detail(
        ResultStatus::Error,
        "the patch does not apply to a.rs: error applying hunk #1",
        21,
        &meta,
    );
    assert_eq!(
        d["error"],
        "the patch does not apply to a.rs: error applying hunk #1"
    );
    assert_eq!(d["duration_ms"], 21);
    assert_eq!(d["meta"], meta);
}

#[test]
fn a_succeeded_calls_detail_has_no_error_and_an_empty_failure_says_nothing() {
    let meta = json!(null);
    let ok = failed_detail(ResultStatus::Ok, "fine", 5, &meta);
    assert_eq!(ok, json!({"duration_ms": 5, "meta": null}));
    let empty = failed_detail(ResultStatus::Error, "  ", 5, &meta);
    assert!(empty.get("error").is_none());
}
