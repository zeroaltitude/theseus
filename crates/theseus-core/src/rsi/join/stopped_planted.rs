//! A planted revert that cannot answer does not repeat without end.
//!
//! When a run of the branch's tests ends in none of the ways that say something (`planted::Undecided`:
//! a setup error, a signal, a run cut short), the step stops and the next pass runs it again. The
//! same exit on the same head will come again, at a cost each time and with the pipeline's slot
//! held, so it takes the review's rule (`unreviewable`): [`HOLD_AFTER`] identical stops in a row
//! hold the head for the owner with one card, and a different head, or the owner's retry, starts
//! the count again. A stop with no such class (git could not fetch, the tree could not be made) is
//! tried again as it always was: it clears by itself.

use serde_json::Value;

use super::planted::Undecided;
use super::unreviewable::{self, Cause, HOLD_AFTER};
use super::Run;

impl Run<'_> {
    /// The planted revert's stop: `why` in words, and what the step knows of it when its run was
    /// one that answered nothing.
    pub(super) fn stopped_planted(&mut self, why: &str, undecided: Option<&Undecided>) {
        let Some(u) = undecided else {
            return self.stop("planted", why, Value::Null);
        };
        let cause = Cause {
            class: u.class(),
            transient: false,
        };
        let repeats = unreviewable::run_with(&self.earlier, "planted", &cause);
        let numbers = cause.numbers(repeats);
        if repeats < HOLD_AFTER {
            return self.stop("planted", why, numbers);
        }
        let what = u.what();
        self.card(card(&self.b.branch, &self.b.item, repeats, &what, &u.last));
        self.hold(
            "planted",
            &format!(
                "the check that undoes the fix could not answer the same way {repeats} times ({what}), \
                 so it waits on the owner's card"
            ),
            numbers,
        );
    }
}

/// The owner's card for a change whose proof step stopped the same way again and again.
fn card(branch: &str, item: &str, repeats: usize, what: &str, last: &str) -> String {
    let cmd = theseus_protocol::rsi::retry_command(item);
    let body = format!(
        "Before a change joins, Theseus undoes its fix and runs the change's new tests again, to \
         see that they fail without it. That check could not answer, the same way {repeats} times \
         in a row: {what}. Asking again does not clear it, so it has stopped, and nothing was \
         joined. The last line of its output: {last}\nOnce the cause is dealt with, `{cmd}` has \
         Theseus check the change again from the first check."
    );
    crate::owner_note::note(
        crate::owner_note::Kind::Decision,
        &format!("Self branch {branch} could not be checked"),
        &format!("its proof step stopped the same way {repeats} times ({item})"),
        &body,
    )
}
