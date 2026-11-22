//! A turn no person's message, wake or report opened (issue 44: a job's late result, an answered call, a
//! resume after an interruption or a failed call) continues earlier turns of its session. What it reads of
//! them, so it carries the work it resumes or fails plainly:
//!
//! - **Which turns**: the turn that made each call its own results answer, else the session's newest turn
//!   before it. A continuation of a continuation finds the continuation, whose presence was kept
//!   (`exchange::given`), so the chain holds its narrowest presence and never borrows another mark.
//! - **Where it answers**: where that work was asked (its exchange's place). When the session's reply goes
//!   somewhere else now (a reply target that changed), the turn cannot be resumed here: the view fails it
//!   plainly rather than lay that place's conversation for another audience. A session that posts nowhere
//!   answers where the work was asked.
//! - **Its presence**: each continued turn's, read as a set-up's. One that cannot be read, or reads as
//!   `Unknown`, is the unknown presence (the conductor's decision, issue 44): the turn is not failed, and
//!   under it the view shows only what its own session said at its own place (`view_gate::own_turns`): the
//!   question, the calls, the results, and nothing of another session or place. Only a place that cannot be
//!   read, or work asked where the conversation does not answer now, fails it plainly.
//!
//! A task's turns are the task's own (their place and presence are set elsewhere): they keep the presences,
//! and nothing here fails them.

use theseus_memory::gate::Present;
use theseus_memory::recall::Place;

use super::exchange_step::Unresumable;
use super::{Turn, TurnRunner};
use crate::node::Body;
use crate::stub::Kind;
use crate::view_gate;

impl TurnRunner {
    /// The turns a turn no message opened continues: the turn that made each call its own results answer,
    /// else the session's newest turn before it. A call whose turn is not found, or a transcript that
    /// cannot be read, is a `None`.
    fn continued_turns(&self, t: &Turn<'_>) -> Vec<Option<String>> {
        let (sid, turn) = (t.tc.session_id, t.tc.turn_id);
        let Ok(nodes) = self.store.transcript(sid) else {
            return vec![None];
        };
        let calls: Vec<String> = (nodes.iter())
            .filter(|(_, n)| n.turn_id.as_deref() == Some(turn) && n.kind == Kind::ToolResult)
            .filter_map(|(_, n)| match &n.node().body {
                Body::ToolResult { tool_use_id, .. } => Some(tool_use_id.clone()),
                _ => None,
            })
            .collect();
        if !calls.is_empty() {
            return (calls.iter())
                .map(|call| {
                    nodes.iter().find_map(|(_, n)| match &n.node().body {
                        Body::ToolCall { tool_use_id, .. } if tool_use_id == call => {
                            n.turn_id.clone()
                        }
                        _ => None,
                    })
                })
                .collect();
        }
        let earlier = (nodes.iter().rev())
            .filter(|(_, n)| !n.kind.is_note())
            .find_map(|(_, n)| n.turn_id.as_deref().filter(|id| *id != turn));
        vec![earlier.map(str::to_string)]
    }

    /// Where a turn no message opened answers (`read`: where its session's reply goes now): where the work
    /// it continues was asked, when that is where its reply goes, or its session posts nowhere. Otherwise,
    /// or when that place cannot be read, the turn is marked unresumable, and `read` is kept.
    pub(super) fn resumed_place(&self, t: &mut Turn<'_>, read: Option<String>) -> Option<String> {
        let opened = t.exchange.mark().is_some() || t.exchange.occasion().is_none();
        if opened || t.tc.task.is_some() || !self.cfg.context.ledger_on() {
            return read;
        }
        let turns = self.continued_turns(t);
        let places: Vec<Option<String>> = (turns.iter())
            .map(|id| {
                let id = id.as_deref()?;
                crate::exchange::exchange(&self.store, id).ok().flatten()
            })
            .map(|e| e.map(|e| e.place))
            .collect();
        t.exchange.set_continues(turns);
        let first = places.first().cloned().flatten();
        let Some(first) = first.filter(|_| places.iter().all(Option::is_some)) else {
            t.exchange.cannot_resume(Unresumable::Unread);
            return read;
        };
        if places.iter().any(|p| p.as_deref() != Some(first.as_str())) {
            t.exchange.cannot_resume(Unresumable::Elsewhere);
            return read;
        }
        if read.as_deref() == Some(first.as_str()) {
            return read;
        }
        match self.outbox.speaks_at(t.tc.session_id) {
            Ok(None) => Some(first),
            _ => {
                t.exchange.cannot_resume(Unresumable::Elsewhere);
                read
            }
        }
    }

    /// The presence of each turn a turn no message opened continues, read as a set-up's at `class`. One
    /// that cannot be read is `None`, which the gate reads as the unknown presence: the turn goes on.
    pub(super) fn continued_presence(
        &self,
        t: &mut Turn<'_>,
        class: &Place,
    ) -> Vec<Option<Present>> {
        let turns = match t.exchange.continues() {
            Some(c) => c.to_vec(),
            None => self.continued_turns(t),
        };
        (turns.iter())
            .map(|id| {
                let id = id.as_deref()?;
                view_gate::continued(&self.store, class, id)
            })
            .collect()
    }
}
