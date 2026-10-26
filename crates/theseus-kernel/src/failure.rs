//! How a failed job failed, in one word, for its ledger row.
//!
//! A job's wrapper reports `Outcome::Failed` for a command that exited
//! non-zero (the command's own answer, like a `grep` that finds nothing) and
//! equally for a job that timed out, was stopped, was killed by a signal or
//! never started. The `action.failed` row carried only "failed", so the
//! nightly read could not tell them apart and filed every one under one
//! cause, `action.failed:wrapper`. The wrapper's completion detail holds the
//! facts; this reads them into a class the row keeps.

use serde_json::Value;

/// The class of a failed completion's detail; `None` when the detail names
/// none of them (an in-process call, a provider, an older wrapper).
pub fn classify(detail: Option<&Value>) -> Option<&'static str> {
    let d = detail?;
    let set = |k: &str| d.get(k).is_some_and(|v| !v.is_null());
    let flag = |k: &str| d.get(k).and_then(Value::as_bool).unwrap_or(false);
    if set("spawn_error") {
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
    } else {
        None
    }
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
