//! A `vault.keep` or `vault.use` error is the tool's own answer to the model,
//! so its completion detail carries the refusal mark the kernel reads into
//! `failure = "refused"`: the nightly read files no cause for it
//! (`action.failed:harness:vault.keep`).

use serde_json::json;

use crate::toolrun::extend::failure_meta;
use crate::toolrun::inproc::failed_detail;
use crate::toolrun::ResultStatus;

fn detail_of(tool: &str, words: &str) -> serde_json::Value {
    let meta = failure_meta(tool);
    failed_detail(ResultStatus::Error, words, 2411, &meta)
}

#[test]
fn a_vault_keep_or_use_error_is_marked_a_refusal() {
    let words = "no item \"x\" in the vault \"y\"";
    for tool in ["vault.keep", "vault.use"] {
        let d = detail_of(tool, words);
        assert_eq!(d.pointer("/meta/refusal"), Some(&json!(true)), "{tool}");
        assert_eq!(d["error"], words, "{tool}");
    }
}

#[test]
fn another_harness_error_is_still_a_fault() {
    assert_eq!(failure_meta("task.create"), json!(null));
    let d = detail_of("task.create", "the store is gone");
    assert_eq!(d.pointer("/meta/refusal"), None);
}
