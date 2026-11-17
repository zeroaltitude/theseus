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

/// A call that failed because its own session's working copy was gone (issue 20) is classed
/// `tree_gone`, and outranks the other words a detail may carry.
#[test]
fn a_call_in_a_closed_working_copy_is_tree_gone() {
    let d = json!({"error": "working directory x does not exist", "tree_gone": true});
    assert_eq!(classify(Some(&d)), Some(crate::TREE_GONE));
    assert_eq!(crate::TREE_GONE, "tree_gone");
    let plain = json!({"error": "working directory x does not exist"});
    assert_eq!(
        classify(Some(&plain)),
        None,
        "an unmarked failure has no class"
    );
    let off = json!({"tree_gone": false});
    assert_eq!(classify(Some(&off)), None);
}
