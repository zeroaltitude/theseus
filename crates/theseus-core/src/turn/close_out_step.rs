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
/// turn's own words have it (lowercase, straight quotes): a file never run,
/// a result not verified, a score the message calls optimistic or thin. The
/// list was cut on 764 closing messages of the routed arm's trials in the
/// v1, 10-21 and 10-23 Terminal-Bench runs: the plain words of doubt that
/// ended most solved trials too ("assumed", "assuming", "isn't installed",
/// "caveat", "didn't run") are left out. It matches 21% of the solved
/// endings and 49% of the failed ones; on the report's three tasks, 12 of 12
/// failed `train-fasttext` endings, 5 of 5 `mteb-leaderboard` and 16 of 17
/// `torch-pipeline-parallelism`. Blunt on purpose: a message this matches
/// that names nothing costs one loop in which the model says it is done; one
/// it misses ends as it did before.
const GAPS: &[&str] = &[
    "haven't run",
    "have not run",
    "haven't tested",
    "have not tested",
    "haven't checked",
    "have not checked",
    "untested",
    "unverified",
    "not verified",
    "didn't test",
    "couldn't test",
    "couldn't verify",
    "could not verify",
    "cannot verify",
    "can't verify",
    "can't run",
    "cannot run",
    "unable to run",
    "unable to verify",
    "never run",
    "never ran",
    "haven't tried",
    "have not tried",
    "before relying",
    "before you rely",
    "could land",
    "may land",
    "private set",
    "private test set",
    "optimistic",
    "within noise",
    "within the noise",
    "margin is thin",
    "thin margin",
    "not confirmed",
    "unconfirmed",
    "no torch",
    "not installed",
    "can't guarantee",
    "cannot guarantee",
    "less sure",
    "just below",
    "did not check",
    "didn't check",
    "not checked",
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
            "The test suite is untested on Windows.",
            "It scores 0.6263, only about 0.6 points over the target. The private set may land a little either way.",
            "I would trust it, but I haven't tried a bigger variant.",
            "Please confirm it on the leaderboard before you rely on it.",
            "The fit is good, though the private set could land a little higher or lower.",
            "I wrote the timer and never ran it.",
            "I\u{2019}ve never run it against the real queue.",
        ] {
            assert!(names_a_gap(said), "{said}");
        }
        for said in [
            "Done: the parser reads the tide tables and the check passes.",
            "All 41 tests pass, and I ran the program with --csv_path and --rgb_path.",
            "Assumed the cell masks are 8-bit, and the grader's own check agrees.",
            "I installed it; the caveat is only that the build took a while.",
            "",
        ] {
            assert!(!names_a_gap(said), "{said}");
        }
    }

    /// Plain prose that only shares words with the list (theseus-6lpxx): a
    /// release date, a fact about a timer, a sandbox without a library.
    #[test]
    fn ordinary_prose_that_shares_a_phrase_names_no_gap() {
        for said in [
            "The fix may land in 1.0.4, after the release is tagged.",
            "The reminder could land on Friday; I checked the schedule.",
            "The timer has never run on this host, which is why the log is empty.",
            "There is no torch in this sandbox, so I used numpy, and the check passes.",
            "The score is on the private test set; I ran the public one and it passes.",
        ] {
            assert!(!names_a_gap(said), "{said}");
        }
    }

    /// Output the message quotes is not the model's own voice: a fenced block,
    /// a block quote, an inline code span and a quoted string are cut before
    /// the list is read, and an unmatched marker cuts nothing.
    #[test]
    fn quoted_output_is_not_the_models_own_doubt() {
        for said in [
            "The build passed. The log ends:\n```\nwarning: optimistic locking is not installed\n```\nAll green.",
            "The build passed.\n> error: torch is not installed\nThat is the old log; it now passes.",
            "The build passed; the old error was `pip: not installed`, now gone.",
            "The status page shows \"unverified\" for the three old files, and the new one passes.",
            "The status page shows \u{201c}not verified\u{201d} for the old files; the new one passes.",
        ] {
            assert!(!names_a_gap(said), "{said}");
        }
        // The model's own doubt beside a quote still counts, and so does one
        // after a stray marker (it cuts nothing).
        for said in [
            "The log says \"ok\", but I haven't tested the second path.",
            "```\nok\n```\nI haven't run the migration.",
            "It builds (the `check` target is fine). I haven't tested it on Windows` and ever.",
            "```\nnever closed\nI have not tested this.",
            "It says \"ok. I did not check the date.",
        ] {
            assert!(names_a_gap(said), "{said}");
        }
    }
}
