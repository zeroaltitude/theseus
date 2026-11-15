//! An in-process tool's own refusal is classed `refused` on its ledger row.

use serde_json::json;

use crate::failure::classify;

/// A refusal's flag is read only where a tool's failure put it.
#[test]
fn a_flag_that_is_not_set_is_no_class() {
    assert_eq!(classify(Some(&json!({"meta": {"refusal": false}}))), None);
    assert_eq!(classify(Some(&json!({"meta": null}))), None);
}

#[test]
fn an_in_process_tools_refusal_is_refused() {
    let r = json!({"duration_ms": 0, "meta": {"refusal": true}});
    assert_eq!(classify(Some(&r)), Some("refused"));
}
