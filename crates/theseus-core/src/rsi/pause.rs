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
//! **Who.** Setting a pause is anyone's, as a halt is (it only makes Theseus do less); clearing
//! it is the owner's, from any place they are, never from a job's shell (`Core::self_pause`,
//! judged as the backlog's words are). It is reached by `theseus self pause` and `unpause`, by
//! the RPC method `self.pause`, and from chat by the tool `self.pause` (`pause_tool.rs`), which
//! follows its own posture and tells the owner, its note written before the change (issue 57).

use theseus_protocol::rsi::{SelfPauseParams, SelfPauseResult};

use crate::approval::Answerer;
use crate::rpc::{Act, Core};

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

    /// `self.pause`: set a pause (anyone's) or, with `clear`, clear it (the owner's, from any place,
    /// never a job's shell). Idempotent: the answer says whether anything changed.
    pub fn self_pause(
        &self,
        p: &SelfPauseParams,
        who: &Answerer,
    ) -> anyhow::Result<SelfPauseResult> {
        if p.clear {
            let from_job = p
                .from_job
                .as_deref()
                .map(str::trim)
                .filter(|j| !j.is_empty());
            self.judge_act(
                who,
                Act::SelfBacklog {
                    method: theseus_protocol::method::SELF_PAUSE,
                    what: "the pause",
                    from_job,
                },
            )?;
        }
        Ok(self.self_pause_apply(p.clear, p.why.as_deref(), &who.who()))
    }

    /// The chat tool's move (issue 57): when it would change anything, `tell` writes the owner's
    /// note first, under the pause's lock, and only then is the change applied, so a note that
    /// cannot be written leaves the pause as it was (the act is not done) and a call that changes
    /// nothing tells nothing. A pause lives in memory, so a restart after the note clears it, as
    /// the note says a restart does.
    pub(crate) fn self_pause_told(
        &self,
        clear: bool,
        why: Option<&str>,
        by: &str,
        tell: impl FnOnce() -> anyhow::Result<()>,
    ) -> anyhow::Result<SelfPauseResult> {
        let mut slot = self
            .rsi
            .pipeline
            .paused
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let changes = slot.is_some() == clear;
        if changes {
            tell()?;
        }
        if clear {
            *slot = None;
        } else {
            *slot = Some(Pause {
                why: why
                    .map(str::trim)
                    .filter(|w| !w.is_empty())
                    .map(str::to_string),
                by: by.to_string(),
                at_ms: theseus_protocol::now_unix_ms(),
            });
        }
        let now = slot.clone();
        drop(slot);
        if clear && changes {
            self.rsi.pipeline.kick();
        }
        Ok(SelfPauseResult {
            paused: now.is_some(),
            changed: changes,
            words: now.map(|n| n.words()),
        })
    }

    /// The move itself, judged by the caller: the RPC's `self_pause` above, and the chat tool,
    /// whose wall (the owner's own place, never a task's) and the gate's ask for the owner's yes
    /// stand for the judgement.
    pub(crate) fn self_pause_apply(
        &self,
        clear: bool,
        why: Option<&str>,
        by: &str,
    ) -> SelfPauseResult {
        let changed = if clear {
            self.self_pause_clear()
        } else {
            self.self_pause_set(why, by)
        };
        let now = self.self_paused();
        SelfPauseResult {
            paused: now.is_some(),
            changed,
            words: now.map(|n| n.words()),
        }
    }
}
