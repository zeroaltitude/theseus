//! What waiting executions wait on (theseus-g00vv): health splits its
//! `waiting` count by wake, so a conversation at rest on the owner's input
//! (one per conversation, by design) reads apart from one that waits on a
//! time or on work. A label only: no execution changes state.
//!
//! Each is a term of the waiting execution (`wk:time`, `wk:work`, see
//! `terms.rs`), counted like the states, so health reads no execution; the
//! rest of the waiting count rests on input.

use anyhow::Result;
use serde::{Deserialize, Serialize};

use theseus_store::kinds;

use crate::kernel::Kernel;
use crate::terms;

/// The waiting executions, by what each waits on.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct WaitingByWake {
    /// On the owner's input: a conversation between exchanges.
    pub input: u64,
    /// On a due time: a scheduled wake, a retry's backoff, a deferred task.
    pub time: u64,
    /// On work: an action, a question, a budget reset, or a child task.
    pub work: u64,
}

impl Kernel {
    /// The waiting executions by wake, given how many wait in all (`s:waiting`).
    pub fn waiting_by_wake(&self, waiting: u64) -> Result<WaitingByWake> {
        let n = self.counts_by(
            kinds::EXECUTION,
            &[terms::WAIT_TIME.to_string(), terms::WAIT_WORK.to_string()],
        )?;
        Ok(WaitingByWake {
            input: waiting.saturating_sub(n[0] + n[1]),
            time: n[0],
            work: n[1],
        })
    }
}
