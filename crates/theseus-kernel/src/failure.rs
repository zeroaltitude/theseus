//! How a job failed, for its `action.failed` ledger row.
//!
//! A wrapper settles a job as failed for five different reasons: the command
//! ran and exited non-zero (its own answer, like a `grep` that finds nothing),
//! a signal ended it, its deadline killed it, a stop asked for, or it never
//! started. The row used to say only "failed", so the nightly read could not
//! tell the command's answer from a fault of the machinery. The completion's
//! detail already holds the facts; this names the one that decided.

use serde_json::Value;

/// The command ran and exited non-zero: its own answer.
pub const EXIT: &str = "exit";
/// A signal ended the command, and nothing here asked for it.
pub const SIGNAL: &str = "signal";
/// The job's deadline ended it.
pub const TIMEOUT: &str = "timeout";
/// The command never started.
pub const SPAWN: &str = "spawn";
/// A stop asked for ended it.
pub const STOPPED: &str = "stopped";

/// Why a failed job failed, from its completion's detail; `None` when the
/// detail is not a wrapper's (an in-process tool, a courier) and says nothing
/// of how a command ended. The order is the order of cause: a job that never
/// started has no exit, a deadline's kill is also a signal.
pub fn of_detail(detail: Option<&Value>) -> Option<&'static str> {
    let d = detail?;
    if d.get("spawn_error").is_some_and(|v| !v.is_null()) {
        return Some(SPAWN);
    }
    if d["timed_out"].as_bool() == Some(true) {
        return Some(TIMEOUT);
    }
    if d["stopped"].as_bool() == Some(true) {
        return Some(STOPPED);
    }
    if !d["signal"].is_null() {
        return Some(SIGNAL);
    }
    if !d["exit_code"].is_null() {
        return Some(EXIT);
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn a_plain_non_zero_exit_is_the_commands_answer() {
        let d = json!({"exit_code": 1, "signal": null});
        assert_eq!(of_detail(Some(&d)), Some(EXIT));
    }

    #[test]
    fn a_deadline_beats_the_signal_that_carried_it_out() {
        let d = json!({"exit_code": null, "signal": 9, "timed_out": true});
        assert_eq!(of_detail(Some(&d)), Some(TIMEOUT));
        let d = json!({"exit_code": null, "signal": 15, "stopped": true});
        assert_eq!(of_detail(Some(&d)), Some(STOPPED));
        let d = json!({"exit_code": null, "signal": 11});
        assert_eq!(of_detail(Some(&d)), Some(SIGNAL));
    }

    #[test]
    fn a_job_that_never_started_and_a_detail_that_says_nothing() {
        let d = json!({"spawn_error": "No such file or directory"});
        assert_eq!(of_detail(Some(&d)), Some(SPAWN));
        assert_eq!(of_detail(Some(&json!({"error": "refused"}))), None);
        assert_eq!(of_detail(None), None);
    }
}
