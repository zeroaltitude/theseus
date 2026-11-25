//! A join that cannot be reviewed does not repeat without end (issue 20).
//!
//! A review that fails stops its pass before the review, and the next pass runs it again. When the
//! reason is one that time clears (the network, a rate limit, an overload) that is right, and each
//! try waits longer than the last (`waiting`), so a provider that is down is not asked every pass.
//! When it is not (the provider's `invalid_request`, a refusal, anything its turn calls
//! `transient: false`) the same review fails the same way, at a cost each time and with the
//! pipeline's one slot held. After [`HOLD_AFTER`] identical stops in a row the change is held for the
//! owner with one card (`Run::stop_review`). That is not a failed try of the item, which counts
//! gates and verdicts and never a join's stop, and no part of the streak that halts self builds,
//! which reads gates alone.
//!
//! The stops are read back from the ledger's `self.join.stopped` rows: each carries the class of
//! the failure and whether it is transient. Any row of the head that is not an identical stop
//! (a verdict, another phase, another class, the owner's retry) ends the run.

use std::time::Duration;

use anyhow::Error;
use serde_json::{json, Value};

use crate::turn::TurnError;

/// How many identical stops of a reason that will not clear hold the change for the owner.
pub const HOLD_AFTER: usize = 3;

/// Between passes, which the first transient stop waits for and each after doubles.
const EVERY: Duration = super::tender::EVERY;
/// The longest wait between two tries of a review that keeps failing for a reason that clears.
const LONGEST: Duration = Duration::from_secs(4 * 3600);
/// A pass starts a little after its interval, not at it: the wait is this much shorter.
const SLACK: Duration = Duration::from_secs(60);

/// One stop of a head, as its row says it.
#[derive(Debug, Clone, PartialEq)]
pub(super) struct Stop {
    pub phase: String,
    /// The failure's class (`invalid_request`); empty for a stop that names none.
    pub class: String,
    pub transient: bool,
    pub at_ms: u64,
}

/// Why a review did not answer, when its turn said.
pub(super) struct Cause {
    pub class: String,
    pub transient: bool,
}

impl Cause {
    /// The turn's own classification of the failure; none for an error that is not a failed turn,
    /// which is tried again as it always was.
    pub fn of(e: &Error) -> Option<Cause> {
        e.chain()
            .find_map(|c| c.downcast_ref::<TurnError>())
            .map(|t| Cause {
                class: t.class.clone(),
                transient: t.transient || clears(&t.class),
            })
    }

    /// What the stop's row carries of it, for the next pass to read.
    pub fn numbers(&self, repeats: usize) -> Value {
        json!({"class": self.class, "transient": self.transient, "repeats": repeats})
    }
}

/// Failures a turn calls final that pass without anyone's hand, and so are no reason to hold a
/// change: the self budget's day line, spent until the day turns (a refused reservation costs
/// nothing), and a turn the queue could not admit while the load held (`admission_stuck`).
fn clears(class: &str) -> bool {
    matches!(class, crate::turn::SELF_BUDGET_CLASS | "admission_stuck")
}

/// A stop's row, as a [`Stop`].
pub(super) fn stop_of(phase: &str, numbers: &Value, at_ms: u64) -> Stop {
    Stop {
        phase: phase.into(),
        class: numbers["class"].as_str().unwrap_or_default().into(),
        transient: numbers["transient"] == true,
        at_ms,
    }
}

/// The identical stops that end `stops`: the ones of the same phase and class as the newest.
fn tail(stops: &[Stop]) -> &[Stop] {
    let Some(last) = stops.last().filter(|s| !s.class.is_empty()) else {
        return &[];
    };
    let n = stops
        .iter()
        .rev()
        .take_while(|s| s.phase == last.phase && s.class == last.class)
        .count();
    &stops[stops.len() - n..]
}

/// How many identical stops this one makes, itself included.
pub(super) fn run_with(earlier: &[Stop], phase: &str, cause: &Cause) -> usize {
    tail(earlier)
        .iter()
        .filter(|s| s.phase == phase && s.class == cause.class)
        .count()
        + 1
}

/// Whether the review is to wait for its next try: the head's last stops were the same transient
/// failure, `n` of them, and `EVERY` doubled `n - 1` times has not passed since the newest.
pub(super) fn waiting(earlier: &[Stop], now_ms: u64) -> bool {
    let run = tail(earlier);
    let Some(last) = run.last().filter(|s| s.transient && s.phase == "review") else {
        return false;
    };
    let doubled = 1u32 << (run.len() - 1).min(16);
    let wait = (EVERY * doubled).min(LONGEST).saturating_sub(SLACK);
    now_ms < last.at_ms + wait.as_millis() as u64
}

/// The owner's card for a change whose review failed the same way, again and again, for a reason
/// that repeating does not clear.
pub(super) fn card(branch: &str, item: &str, class: &str, repeats: usize, why: &str) -> String {
    let cmd = theseus_protocol::rsi::retry_command(item);
    let (said, _) = crate::scrub::safe::head(why, 400);
    let body = format!(
        "Theseus asked another model to review this change and the request failed the same way \
         {repeats} times in a row ({class}), a failure that asking again does not clear. It has \
         stopped asking, and nothing was joined. What it was told: {said}\nOnce the cause is dealt \
         with, `{cmd}` has Theseus check the change again from the first check."
    );
    crate::owner_note::note(
        crate::owner_note::Kind::Decision,
        &format!("Self branch {branch} could not be reviewed"),
        &format!("its review failed the same way {repeats} times ({item})"),
        &body,
    )
}
