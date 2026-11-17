//! How a failed job failed, in one word, for its ledger row.
//!
//! A job's wrapper reports `Outcome::Failed` for a command that exited
//! non-zero (the command's own answer, like a `grep` that finds nothing) and
//! equally for a job that timed out, was stopped, was killed by a signal or
//! never started. The `action.failed` row carried only "failed", so the
//! nightly read could not tell them apart and filed every one under one
//! cause, `action.failed:wrapper`. The wrapper's completion detail holds the
//! facts; this reads them into a class the row keeps.

use serde_json::{json, Value};

use crate::types::{Completion, Outcome};

/// The detail key, and the `failure` word, of a call that failed because its own session's working
/// copy was gone (issue 20): the nightly read does not file it under the tool's cause.
pub const TREE_GONE: &str = "tree_gone";

/// The class of a failed completion's detail; `None` when the detail names
/// none of them (an in-process call, a provider, an older wrapper).
pub fn classify(detail: Option<&Value>) -> Option<&'static str> {
    let d = detail?;
    let set = |k: &str| d.get(k).is_some_and(|v| !v.is_null());
    let flag = |k: &str| d.get(k).and_then(Value::as_bool).unwrap_or(false);
    if flag(TREE_GONE) {
        // A self session's call whose own working copy had been closed: not a fault of the tool.
        Some(TREE_GONE)
    } else if set("spawn_error") {
        Some("spawn")
    } else if flag("stopped") {
        Some("stopped")
    } else if flag("timed_out") {
        Some("timeout")
    } else if set("signal") {
        Some("signal")
    } else if set("exit_code") {
        Some("exit")
    } else if d.pointer("/sandbox/error").is_some() {
        // An L1 sandbox that did not start: the command never ran.
        Some("spawn")
    } else if d.pointer("/meta/refusal").and_then(Value::as_bool) == Some(true) {
        // An in-process tool's own "no" (`ToolFailure::refusal`): the answer
        // the model reads and changes its call by, as an exit is a job's.
        Some("refused")
    } else {
        None
    }
}

/// Stamp `failure` on the ledger row of a failed completion whose detail
/// names how it failed; any other row is left as it was.
pub fn stamp(data: &mut Value, c: &Completion) {
    if c.outcome != Outcome::Failed {
        return;
    }
    if let Some(f) = classify(c.detail.as_ref()) {
        data["failure"] = json!(f);
    }
    if let Some(why) = error_of(c.detail.as_ref()) {
        data["detail"] = json!({ "error": why });
    }
}

/// The most of a failure's words a row keeps.
const ERROR_MAX: usize = 500;

/// The words a failed completion's detail gives for why it failed
/// (`detail.error`), cut to [`ERROR_MAX`] characters; `None` when it gave
/// none. Without them the row said only "failed after N ms", and the item
/// filed for it could not tell a patch that did not apply from a fault.
fn error_of(detail: Option<&Value>) -> Option<String> {
    let s = detail?.get("error")?.as_str()?.trim();
    (!s.is_empty()).then(|| s.chars().take(ERROR_MAX).collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn a_non_zero_exit_is_an_exit_and_a_signal_is_a_signal() {
        let exit = json!({"exit_code": 1, "signal": null});
        assert_eq!(classify(Some(&exit)), Some("exit"));
        let sig = json!({"exit_code": null, "signal": 9});
        assert_eq!(classify(Some(&sig)), Some("signal"));
    }

    #[test]
    fn a_timeout_a_stop_and_a_spawn_failure_outrank_the_exit_they_leave() {
        let t = json!({"timed_out": true, "exit_code": null, "signal": 9});
        assert_eq!(classify(Some(&t)), Some("timeout"));
        let s = json!({"stopped": true, "signal": 15});
        assert_eq!(classify(Some(&s)), Some("stopped"));
        let p = json!({"spawn_error": "No such file"});
        assert_eq!(classify(Some(&p)), Some("spawn"));
        let l1 = json!({"sandbox": {"error": {"stage": "clone", "error": "EPERM"}}});
        assert_eq!(classify(Some(&l1)), Some("spawn"));
    }

    #[test]
    fn a_detail_that_says_nothing_is_no_class() {
        assert_eq!(classify(None), None);
        assert_eq!(classify(Some(&json!({"bytes": 3}))), None);
    }
}
