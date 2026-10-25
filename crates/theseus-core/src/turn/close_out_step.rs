//! How a turn that did work ends (theseus-eb4o4, theseus-rqu7j): two rules,
//! one file, both about the last stretch of a turn that edited a file or ran a
//! command. Neither builds a check; both are a sentence in the harness's
//! voice, which the model reads as it reads the progress check's.
//!
//! - **The deliverable, called as the task spells it** (eb4o4): from the loop
//!   after its first file edit, the request's tail carries
//!   [`DELIVERABLE_HINT`], the way a `deep_coding` turn carries its reading
//!   hint (`tier.rs`): after the cached prefix, so no byte of it moves and no
//!   other session's request changes. A turn that wrote no file (a question, a
//!   lookup, a run of commands) never carries it, so the tails the DM task
//!   flow's golden and the cache tests read stay as they were.
//! - **A doubt the closing message names** (rqu7j): a turn that would end with
//!   a message saying something was not checked, run or verified, with less
//!   than [`CLOSE_OUT_SHARE`] of its time and of its spend used, goes on once,
//!   told so in a harness node, and spends the next loop on it. The gate is
//!   the change: it asks nothing of Jev, builds no check and installs nothing;
//!   the model that wrote the doubt closes it, or says why it cannot. Its row
//!   is the progress check's (`turn.progress`, decision `doubt`), which
//!   already carries both shares.
//!
//! The doubt is never asked in a shared place, at a strict loop cap, while a
//! job of the turn runs (`may_continue`), in a turn that has not worked, and
//! never twice in one turn. It follows the stop nudge and the end note rather
//! than yielding to them: a message that names a doubt after the end note's
//! loop is the one this rule is for (the first live trial ended so, with the
//! end note's `__pycache__` loop spent and its unrun file named in the last
//! message). A doubt past the share is the model's to keep: a turn near its
//! end is not sent back to work.

use super::progress_step as progress;
use super::*;

/// The share of a turn's time, and of its spend, under which a doubt sends it
/// back to work: past it the budget is for finishing, not for a new check.
pub const CLOSE_OUT_SHARE: f64 = 0.4;

/// What the tail of a working turn's request says (theseus-eb4o4).
pub const DELIVERABLE_HINT: &str = "[The harness: before you report this work done, call what you built once, the way the task spells it. Named options in the task are options on the command line; the input the task lists first comes first; a path handed to a program that outlives the call is absolute. Where two readings are both cheap to support, support both.]";

/// What the harness says to a turn whose closing message named a doubt.
pub const DOUBT_TEXT: &str = "[The harness: your last message names something it did not check or run. Check it now if you can, then finish; if you cannot, say why.]";

/// What a closing message says when it names a gap in its own checking, as a
/// turn's own words have it (lowercase, straight quotes): an assumption, an
/// artifact never run, a result not verified, a margin the message calls
/// thin. Blunt on purpose: a message this matches that names nothing costs
/// one loop in which the model says it is done; one it misses ends as it did
/// before.
const GAPS: &[&str] = &[
    "not checked",
    "unchecked",
    "didn't check",
    "did not check",
    "haven't checked",
    "have not checked",
    "not verified",
    "unverified",
    "didn't verify",
    "did not verify",
    "couldn't verify",
    "could not verify",
    "can't verify",
    "cannot verify",
    "unable to verify",
    "not tested",
    "untested",
    "didn't test",
    "did not test",
    "couldn't test",
    "could not test",
    "haven't tested",
    "have not tested",
    "never ran",
    "never run",
    "not run",
    "didn't run",
    "did not run",
    "couldn't run",
    "could not run",
    "can't run",
    "cannot run",
    "haven't run",
    "have not run",
    "unable to run",
    "no torch",
    "not installed",
    "isn't installed",
    "assumed",
    "i assume",
    "assuming",
    "margin is thin",
    "thin margin",
    "within noise",
    "within the noise",
    "not confirmed",
    "unconfirmed",
    "could not confirm",
];

/// Whether `said` names a gap in its own checking.
pub fn names_a_gap(said: &str) -> bool {
    let said = said.to_lowercase().replace(['\u{2019}', '\u{2018}'], "'");
    GAPS.iter().any(|g| said.contains(g))
}

impl TurnRunner {
    /// The tail hint of a turn that has written a file (theseus-eb4o4), once
    /// per request.
    pub(super) fn deliverable_hint(t: &Turn<'_>, compiled: &mut crate::compiler::Compiled) {
        if progress::edited(t) {
            Self::attach_hint(compiled, DELIVERABLE_HINT);
        }
    }

    /// After the stop nudge and the end note declined this loop: true when
    /// the turn goes on once more, told its closing message named a doubt.
    /// `i` is the loop's index.
    pub(super) fn doubt_note(&self, t: &mut Turn<'_>, i: u32) -> Result<bool> {
        if t.stop_reason != "no_tool_calls" {
            return Ok(false);
        }
        if !t.last.as_ref().is_some_and(|r| names_a_gap(&r.text)) || !self.may_continue(t)? {
            return Ok(false);
        }
        let guarded = self.cfg.kernel.spend_limit_mode == crate::config::SpendLimitMode::Ask;
        let Some((signals, deadline)) = progress::doubtful(t, guarded, CLOSE_OUT_SHARE) else {
            return Ok(false);
        };
        progress::tell(t, "doubt", i, (&signals, deadline), DOUBT_TEXT)?;
        t.stop_reason.clear();
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The closing messages the report quotes, and ones that name nothing.
    #[test]
    fn a_closing_message_that_names_a_gap_is_known() {
        for said in [
            "I wrote pipeline.py, but the sandbox has no torch, so I never ran it.",
            "The leaderboard figure is from memory; I did not check the date.",
            "Public score 0.626, and the margin is thin, so the private set may differ.",
            "It builds. I couldn\u{2019}t verify it against the grader.",
            "Assumed the cell masks are 8-bit.",
            "The test suite is untested on Windows.",
        ] {
            assert!(names_a_gap(said), "{said}");
        }
        for said in [
            "Done: the parser reads the tide tables and the check passes.",
            "All 41 tests pass, and I ran the program with --csv_path and --rgb_path.",
            "",
        ] {
            assert!(!names_a_gap(said), "{said}");
        }
    }
}
