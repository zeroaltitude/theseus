//! How the reviewer checked a proof that was only a broken build (`planted::Outcome::BuildBroke`).
//!
//! While a reviewer cannot run an experiment inside the sandbox (`planted::REVIEWER_CAN_RUN_THE_EXPERIMENT`
//! is false) it is told to check by reading and to say so: a line that begins `CHECKED BY READING:`
//! (the earlier spelling, `CHECKED BY READING ONLY:`, is read too) naming the test and its assertion.
//! When it can run the experiment it says so with a `CHANGED:` line and a `FAILED:` line. They are read
//! from the reasons the way the verdict line is read from the reply. The verdict's row records which
//! (`proof_check`: [`RUNNING`] or [`READING`]), for a build-broke proof only, and `running` only
//! when the experiment is on; an accept that says how it checked in neither way does not count, and
//! is a reject.

use super::planted::{Outcome, REVIEWER_CAN_RUN_THE_EXPERIMENT};

/// The reviewer ran the experiment.
pub const RUNNING: &str = "running";
/// The reviewer checked by reading, and said so.
pub const READING: &str = "reading";

/// Whether `line` begins with `marker`, in any case, as a reviewer writes a line among its reasons:
/// after a list number or bullet or a quote mark, in backticks or in emphasis, the colon inside or
/// outside it (`1. CHECKED BY READING: x`, `> **CHECKED BY READING**: x`, `` `CHECKED BY READING:` x ``).
fn starts(line: &str, marker: &str) -> bool {
    let bare: String = line.chars().filter(|c| !matches!(c, '*' | '`')).collect();
    let mut rest =
        bare.trim_start_matches(|c: char| c.is_whitespace() || matches!(c, '>' | '-' | '#'));
    let digits = rest.trim_start_matches(|c: char| c.is_ascii_digit());
    if digits.len() < rest.len() && digits.starts_with(['.', ')']) {
        rest = digits[1..].trim_start_matches(|c: char| c.is_whitespace() || c == '>');
    }
    // plain cut: compared with a fixed marker, never kept
    rest.get(..marker.len())
        .is_some_and(|l| l.eq_ignore_ascii_case(marker))
}

/// How the reasons say the proof was checked, if they do. Reading wins when both are said; the
/// experiment's lines count only when `experiment` is on.
pub fn how(reasons: &[String], experiment: bool) -> Option<&'static str> {
    let has = |m: &str| reasons.iter().any(|r| starts(r, m));
    if has("CHECKED BY READING:") || has("CHECKED BY READING ONLY:") {
        Some(READING)
    } else if experiment && has("CHANGED:") && has("FAILED:") {
        Some(RUNNING)
    } else {
        None
    }
}

/// [`judge_with`], for what a reviewer can do today.
pub fn judge(
    planted: &Outcome,
    accept: bool,
    reasons: Vec<String>,
) -> (&'static str, Vec<String>, Option<&'static str>) {
    judge_with(planted, accept, reasons, REVIEWER_CAN_RUN_THE_EXPERIMENT)
}

/// The verdict, reasons and `proof_check` a reply comes to, given how the change was proved: other
/// than for a build-broke proof they are the reply's own, and none. An accept of a build-broke
/// proof that says how it was checked stands; one that does not is a reject, with the reply's own
/// reasons after the new one.
pub fn judge_with(
    planted: &Outcome,
    accept: bool,
    reasons: Vec<String>,
    experiment: bool,
) -> (&'static str, Vec<String>, Option<&'static str>) {
    let verdict = if accept { "accept" } else { "reject" };
    if !matches!(planted, Outcome::BuildBroke { .. }) {
        return (verdict, reasons, None);
    }
    let how = how(&reasons, experiment);
    if accept && how.is_none() {
        let said = if experiment {
            "neither the experiment's `CHANGED:` and `FAILED:` lines nor a `CHECKED BY READING:` line"
        } else {
            "no `CHECKED BY READING:` line"
        };
        let mut out = vec![format!(
            "the review did not say how it checked the proof: only the build broke without the fix, \
             and its accept gave {said}, so it does not count"
        )];
        out.extend(reasons);
        return ("reject", out, None);
    }
    (verdict, reasons, how)
}
