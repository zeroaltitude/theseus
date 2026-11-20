//! How the reviewer checked a proof that was only a broken build (`planted::Outcome::BuildBroke`).
//!
//! The reviewer is asked to run an experiment in its own scratch (`Outcome::proof_note`) and to say
//! so: a `CHANGED:` line and a `FAILED:` line among its reasons when it did, or a line that begins
//! `CHECKED BY READING ONLY:` with the reason when it could not. They are read from the reasons the
//! way the verdict line is read from the reply. The verdict's row records which (`proof_check`:
//! [`RUNNING`] or [`READING`]), for a build-broke proof only; an accept that says neither does not
//! count, and is a reject.

use super::planted::Outcome;

/// The reviewer ran the experiment.
pub const RUNNING: &str = "running";
/// The reviewer could not, and said so.
pub const READING: &str = "reading";

fn starts(line: &str, marker: &str) -> bool {
    line.trim()
        .get(..marker.len())
        .is_some_and(|l| l.eq_ignore_ascii_case(marker))
}

/// How the reasons say the proof was checked, if they do. Reading wins when both are said.
pub fn how(reasons: &[String]) -> Option<&'static str> {
    let has = |m: &str| reasons.iter().any(|r| starts(r, m));
    if has("CHECKED BY READING ONLY:") {
        Some(READING)
    } else if has("CHANGED:") && has("FAILED:") {
        Some(RUNNING)
    } else {
        None
    }
}

/// The verdict, reasons and `proof_check` a reply comes to, given how the change was proved: other
/// than for a build-broke proof they are the reply's own, and none. An accept of a build-broke
/// proof that says how it was checked stands; one that does not is a reject, with the reply's own
/// reasons after the new one.
pub fn judge(
    planted: &Outcome,
    accept: bool,
    reasons: Vec<String>,
) -> (&'static str, Vec<String>, Option<&'static str>) {
    let verdict = if accept { "accept" } else { "reject" };
    if !matches!(planted, Outcome::BuildBroke { .. }) {
        return (verdict, reasons, None);
    }
    let how = how(&reasons);
    if accept && how.is_none() {
        let mut out = vec![
            "the review did not say how it checked the proof: only the build broke without the fix, \
             and its accept gave neither the experiment's `CHANGED:` and `FAILED:` lines nor a \
             `CHECKED BY READING ONLY:` line, so it does not count"
                .to_string(),
        ];
        out.extend(reasons);
        return ("reject", out, None);
    }
    (verdict, reasons, how)
}
