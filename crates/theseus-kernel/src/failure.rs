//! How a failed action failed, in one word, for its `action.failed` row.
//!
//! A job's wrapper settles `Failed` for four different reasons: the command
//! ran and exited non-zero (the command's own answer, as a `grep` that finds
//! nothing), a signal ended it, the deadline killed it, or the wrapper could
//! not start it at all. The row held only the outcome, so the nightly read
//! could not tell the owner's work failing from the machinery failing, and
//! folded every job into one cause, `action.failed:wrapper`.

use serde_json::Value;

/// The command ran to its end and exited with a status.
pub const EXIT: &str = "exit";

/// The word for a failed completion's `detail`; `None` when the detail says
/// nothing of how (a provider's or an in-process call's).
pub fn failure_class(detail: Option<&Value>) -> Option<&'static str> {
    let d = detail?;
    let has = |k: &str| d.get(k).is_some_and(|v| !v.is_null() && *v != Value::Bool(false));
    if has("spawn_error") {
        Some("spawn")
    } else if has("timed_out") {
        Some("timeout")
    } else if has("stopped") {
        Some("stopped")
    } else if has("signal") {
        Some("signal")
    } else if has("exit_code") {
        Some(EXIT)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn a_command_that_exits_non_zero_is_an_exit() {
        let d = json!({"exit_code": 1, "signal": null});
        assert_eq!(failure_class(Some(&d)), Some(EXIT));
    }

    #[test]
    fn what_stopped_the_command_names_the_class() {
        let class = |d: Value| failure_class(Some(&d));
        assert_eq!(class(json!({"exit_code": null, "signal": 9})), Some("signal"));
        assert_eq!(class(json!({"timed_out": true, "exit_code": null})), Some("timeout"));
        assert_eq!(class(json!({"spawn_error": "no such file"})), Some("spawn"));
        assert_eq!(class(json!({"stopped": true})), Some("stopped"));
    }

    #[test]
    fn a_detail_that_says_nothing_has_no_class() {
        assert_eq!(failure_class(None), None);
        assert_eq!(failure_class(Some(&json!({"bytes": 3}))), None);
    }
}
