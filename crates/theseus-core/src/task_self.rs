//! A task that started itself, and why (theseus-o1h3f.1): the read of a
//! turn with no owner input, and the `because` its `task.create` must give.
//!
//! Self-started is a fact about the turn that made the call, read from its
//! own transcript: the turn has no message the owner (or an MCP prompt the
//! owner chose) wrote. A wake, a task report, a continuation and a routine's
//! run bring none, so a task they open is self-started; a task session's turn
//! is not told from the session's kind, which would mark a turn with typed
//! input too.

use theseus_protocol::tasks::{TaskOrigin, BECAUSE_MAX};

use crate::node::Origin;
use crate::stub::Kind;
use crate::toolrun::TurnCtx;

/// What a `task.create` call says of who started the task.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Own {
    pub self_started: bool,
    pub because: Option<String>,
}

impl Own {
    /// The task is the owner's: no new key on its record.
    pub fn owners() -> Self {
        Self::default()
    }

    /// Read the turn that made the call `correlation_id`, and the call's `because`. A self-started turn without
    /// one is refused, in words the model reads; a reason over
    /// `BECAUSE_MAX` characters is refused on any turn.
    pub fn read(
        tc: &TurnCtx<'_>,
        correlation_id: &str,
        because: Option<&str>,
    ) -> Result<Self, String> {
        // A task session's own plan is its task's work, and the task carries
        // the mark: its turns are not new initiative.
        if tc.task.is_some() {
            return Self::of(false, because);
        }
        let nodes = tc
            .store
            .transcript(tc.session_id)
            .map_err(|e| format!("Not started: {e:#}"))?;
        // The turn that made the call: a call the owner approved runs in a
        // later turn, but its call node belongs to the one it was made in.
        let made_in = crate::task::holder_of(&nodes, correlation_id)
            .and_then(|reply| nodes.iter().find(|(_, n)| n.id == reply))
            .and_then(|(_, n)| n.turn_id.clone())
            .unwrap_or_else(|| tc.turn_id.to_string());
        let owner_input = nodes.iter().any(|(_, n)| {
            n.turn_id.as_deref() == Some(made_in.as_str())
                && n.kind == Kind::UserMessage
                && matches!(n.origin, Origin::Operator | Origin::Mcp)
        });
        Self::of(!owner_input, because)
    }

    /// The pure rule: whether `self_started`, with `because` cut to nothing
    /// on the owner's turn and required on its own.
    pub fn of(self_started: bool, because: Option<&str>) -> Result<Self, String> {
        let because = because.map(str::trim).filter(|b| !b.is_empty());
        if let Some(b) = because {
            let n = b.chars().count();
            if n > BECAUSE_MAX {
                return Err(format!(
                    "Not started: `because` is {n} characters and the limit is {BECAUSE_MAX}. \
                     Say why in one short line."
                ));
            }
        }
        if !self_started {
            return Ok(Self::owners());
        }
        match because {
            Some(b) => Ok(Self {
                self_started: true,
                because: Some(b.to_string()),
            }),
            None => Err(REFUSAL.into()),
        }
    }

    /// Mark a new task's origin. A self-started task's objective and
    /// acceptance are the model's, so a change to its layer 1 applies at
    /// once (`TaskOrigin::by_model`).
    pub fn apply(&self, o: &mut TaskOrigin) {
        if self.self_started {
            o.self_started = true;
            o.by_model = true;
            o.because = self.because.clone();
        }
    }
}

/// Why a self-started `task.create` without a `because` is refused.
pub const REFUSAL: &str = "Not started: no one asked for this turn, so the task starts itself \
     and the owner is told why. Add `because`: one line, in your own words, of why you are \
     starting it (at most 160 characters).";
