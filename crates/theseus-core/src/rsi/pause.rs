//! Pausing the loop's builds (issue 6, theseus-7glm6).
//!
//! The self-improvement loop could not be paused short of its kill switch, so its own builds
//! tripped every local benchmark's quiet check. A pause is a **state of the running daemon**:
//! while it is set, no new self build starts (the pipeline's start is held, saying why, and the
//! night's own start does nothing); clearing it lets them start; a restart of the daemon
//! clears it, because it lives in this process's memory and nowhere in the store. Builds
//! already running finish, and reviews, joins and the night's filing go on: only a new build
//! is what trips a benchmark's quiet check.
//!
//! It is not the kill switch, which stays in the store, stops builds in flight and waits for the
//! owner's resume. A pause is the lighter word for "not now".
//!
//! Setting it and clearing it will be the owner's, from a command (`theseus self pause`, `unpause`)
//! and from chat (a tool the model calls). **Neither is here yet**: they add an RPC method and a tool,
//! which wait for the hot files' split to land on main, so this module is the state and its two
//! moves and nothing in production calls them yet.

use crate::rpc::Core;

/// A pause now in force.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pause {
    /// Why, in the setter's few words, when they gave any.
    pub why: Option<String>,
    /// Who set it, as the ledger names them.
    pub by: String,
    /// When it was set (unix ms).
    pub at_ms: u64,
}

impl Pause {
    /// The sentence a hold, a status line and a refusal say.
    pub fn words(&self) -> String {
        let at = crate::wake::local(self.at_ms);
        let why = self
            .why
            .as_deref()
            .map(|w| format!(": {w}"))
            .unwrap_or_default();
        format!(
            "paused by {} at {:02}:{:02}{why}. No new self build starts until it is unpaused or \
             the daemon restarts",
            self.by, at.hour, at.minute
        )
    }
}

impl Core {
    /// The pause in force, if any.
    pub fn self_paused(&self) -> Option<Pause> {
        self.rsi
            .pipeline
            .paused
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }

    /// Pause new self builds. The pause already in force is replaced (its words change); whether
    /// there was none before.
    pub fn self_pause_set(&self, why: Option<&str>, by: &str) -> bool {
        let why = why
            .map(str::trim)
            .filter(|w| !w.is_empty())
            .map(str::to_string);
        let mut slot = self
            .rsi
            .pipeline
            .paused
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let fresh = slot.is_none();
        *slot = Some(Pause {
            why,
            by: by.to_string(),
            at_ms: theseus_protocol::now_unix_ms(),
        });
        fresh
    }

    /// Clear the pause and wake the tender, so a waiting build starts at once. Whether one was
    /// in force.
    pub fn self_pause_clear(&self) -> bool {
        let was = self
            .rsi
            .pipeline
            .paused
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take()
            .is_some();
        if was {
            self.rsi.pipeline.kick();
        }
        was
    }
}
