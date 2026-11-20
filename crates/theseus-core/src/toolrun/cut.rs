//! What a job cut at its time limit says in its result (issue 37): the limit it was held to, how long it ran, and
//! where, in the line a finished job's exit code and directory take. The model read `[timed out and killed]` and
//! nothing else, and could only infer how long the job had run from the limit it had asked for.

use serde_json::Value;
use theseus_kernel::patience::CEILING_FACTOR;

/// A duration as the result says it: whole seconds when they are whole, else to the tenth.
fn secs(ms: u64) -> String {
    match ms % 1000 {
        0 => format!("{} s", ms / 1000),
        _ => format!("{:.1} s", ms as f64 / 1000.0),
    }
}

/// The line after `[timed out and killed]`: how the limit cut it, how long it ran, and `ran_in`, the directory
/// part a finished job's line carries (` · in /w`, or nothing). A job cut by an older wrapper, which did not
/// record its limit, is told without it.
pub(super) fn line(detail: &Value, ran_in: &str) -> String {
    let limit = detail.get("limit_ms").and_then(Value::as_u64);
    let quiet = detail.get("silent_ms").and_then(Value::as_u64);
    let cut = match (limit, quiet, detail.get("ceiling_hit").and_then(Value::as_bool)) {
        (Some(l), _, Some(true)) => format!(
            "cut at its ceiling, {} ({CEILING_FACTOR} times its {} limit, which is on silence): it was \
             still printing",
            secs(l.saturating_mul(u64::from(CEILING_FACTOR))),
            secs(l)
        ),
        (Some(l), Some(q), _) => format!(
            "cut at its {} limit, a limit on silence: it printed nothing for {}",
            secs(l),
            secs(q)
        ),
        (Some(l), None, _) => format!("cut at its {} limit", secs(l)),
        (None, ..) => "cut at its limit".to_string(),
    };
    let ran = detail
        .get("duration_ms")
        .and_then(Value::as_u64)
        .map(|ms| format!(" · ran {:.1} s", ms as f64 / 1000.0))
        .unwrap_or_default();
    format!("[{cut}{ran}{ran_in}]\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn a_cut_by_silence_names_the_limit_the_quiet_the_run_and_the_directory() {
        let d =
            json!({"timed_out": true, "limit_ms": 2000, "silent_ms": 2013, "duration_ms": 2022});
        assert_eq!(
            line(&d, " · in /w"),
            "[cut at its 2 s limit, a limit on silence: it printed nothing for 2.0 s · ran 2.0 s · in /w]\n"
        );
    }

    #[test]
    fn a_cut_at_the_ceiling_says_it_was_still_printing() {
        let d =
            json!({"timed_out": true, "limit_ms": 300, "ceiling_hit": true, "duration_ms": 7212});
        let said = line(&d, "");
        assert!(
            said.contains("cut at its ceiling, 7.2 s (24 times its 0.3 s limit"),
            "{said}"
        );
        assert!(said.ends_with("· ran 7.2 s]\n"), "{said}");
    }

    #[test]
    fn a_cut_with_no_record_of_its_limit_is_told_without_it() {
        let d = json!({"timed_out": true, "duration_ms": 5400});
        assert_eq!(line(&d, ""), "[cut at its limit · ran 5.4 s]\n");
    }
}
