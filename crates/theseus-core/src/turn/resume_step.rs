//! A turn no person's message, wake or report opened (issue 44: a job's late result, an answered call, a
//! resume after an interruption or a failed call) continues earlier turns of its session: the turn that
//! made each call its own results answer, else the session's newest turn before it.
//!
//! **Where it answers** (anything goes, 2026-11-22): where its session speaks now; a session that posts
//! nowhere answers where the work it continues was asked. It never fails for the difference.
//!
//! A task's turns are the task's own (their place is set elsewhere): nothing here moves them.

use super::{Turn, TurnRunner};
use crate::node::Body;
use crate::stub::Kind;

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

    /// Where a turn no message opened answers (`read`: where its session's reply goes now): there, when
    /// its session speaks somewhere now; when it posts nowhere, where the work it continues was asked (the
    /// first of those turns' places that reads), else `read`. Never a failure.
    pub(super) fn resumed_place(&self, t: &Turn<'_>, read: Option<String>) -> Option<String> {
        let opened = t.exchange.mark().is_some() || t.exchange.occasion().is_none();
        if opened || t.tc.task.is_some() || !self.cfg.context.ledger_on() {
            return read;
        }
        if matches!(self.outbox.speaks_at(t.tc.session_id), Ok(Some(_))) {
            return read;
        }
        let asked = (self.continued_turns(t).iter().flatten())
            .find_map(|id| crate::exchange::exchange(&self.store, id).ok().flatten())
            .map(|e| e.place);
        asked.or(read)
    }
}
