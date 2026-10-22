//! Theseus's own replies in an open run (theseus-a1wvq.15, the owner's Q5 of
//! 2026-10-17): the run includes a reply when Jev's data question judges it
//! `open` at the act band, the same test a person's message gets. A reply no
//! one judges ends the run, since `admits` extends the stored run only when the
//! node before the next input is its last.
//!
//! The pieces here are pure. A turn that wrote a reply after an open run
//! makes a [`ReplyAsk`] (`turn/reply_data_step.rs`); the judge asks the
//! question off the turn's path (`judge/reply_data.rs`); [`lands`] says what
//! the answer changes, and it can only keep the run going: a missing, late,
//! private, unclear or below-act answer, a refused reservation, a tainted
//! session, or a run that moved meanwhile changes nothing, so the reply stays
//! the end of the run (fail closed). `admits` itself is unchanged.

use super::{DataState, OpenRun};
use crate::routing::data::DataAnswer;

/// One reply to ask the data question about.
#[derive(Debug, Clone)]
pub struct ReplyAsk {
    pub session: String,
    /// The reply's node, and its text (scrubbed when the state is built).
    pub node: String,
    pub text: String,
    /// The open run the reply would continue: it ends at the node before the
    /// reply. The stored one, else the one the turn's input was admitted with.
    pub base: OpenRun,
    /// The stored run when the reply was queued: the write is a compare and
    /// set against it.
    pub expect: Option<OpenRun>,
    /// The operator's own names for open and private matter, as route.v7 has them.
    pub names: theseus_judge::builders::DataNames,
}

/// The run the answer makes of the session's, if it makes one: `open` at the
/// act band, no taint, and the stored run still the one the reply was queued
/// against. `None` leaves the run as it is.
pub fn lands(
    stored: Option<&DataState>,
    ask: &ReplyAsk,
    answer: Option<&DataAnswer>,
) -> Option<OpenRun> {
    let a = answer?;
    if a.choice != "open" || a.band != theseus_judge::band::Band::Act {
        return None;
    }
    if stored.is_some_and(|s| s.taint.is_some())
        || stored.and_then(|s| s.run.as_ref()) != ask.expect.as_ref()
    {
        return None;
    }
    Some(OpenRun {
        from: ask.base.from.clone(),
        through: ask.node.clone(),
    })
}
