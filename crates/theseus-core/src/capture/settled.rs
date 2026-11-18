//! Words whose turn asked a settings question are no record (issue 22).
//!
//! The owner's "set X to Y", "add a line to the persona" is an act to do now,
//! and the turn that follows asks it as a card. Capture reads words, not acts,
//! so the same message was also offered as a standing order (an ask) or kept
//! as a standing preference or fact (a record that rides every prompt of its
//! scope). When a turn puts any settings question to the owner for its own
//! input message (`config_change::ask`), what capture made of that message is
//! undone: its open asks are withdrawn and the records it kept are ended, with
//! no label on the pack's judgment (the pack is not blamed for a rule this
//! code makes). A capture that lands after the question is dropped at its
//! door (`Expected::acted_on`), so the order of the two does not matter.

use anyhow::Result;
use theseus_protocol::StandingRecord;

use crate::fact;
use crate::node::Origin;
use crate::rpc::Core;
use crate::standing;
use crate::stub::{Kind, Stub};
use crate::toolrun::TurnCtx;

/// Why the daemon ended it, as the ledger and the narrative say.
const VIA: &str = "a settings question for the same message";

/// The message that STARTED the turn, when the owner wrote it: the words a
/// settings question answers. Not the last message of the turn: one the owner
/// steers in while it runs has its own capture, and may be a real order. A turn
/// a wake or a job started has none.
pub(crate) fn turn_input(tc: &TurnCtx<'_>) -> Option<String> {
    first_input(&tc.store.transcript(tc.session_id).ok()?, tc.turn_id)
}

/// `turn_input`'s choice, from the session's nodes in order.
fn first_input(nodes: &[(u64, Stub)], turn_id: &str) -> Option<String> {
    nodes
        .iter()
        .find(|(_, n)| n.turn_id.as_deref() == Some(turn_id) && n.kind == Kind::UserMessage)
        .filter(|(_, n)| n.origin == Origin::Operator)
        .map(|(_, n)| n.id.clone())
}

impl Core {
    /// The turn of message `node` put a settings question to the owner: what
    /// capture made of those words is undone, and capture makes nothing more
    /// of them. A failure is logged and changes nothing else.
    pub(crate) fn settled_by_act(&self, session: &str, node: &str) {
        self.runner.standing.expected.note_act(session, node);
        if let Err(e) = theseus_store::blocking(|| self.undo_capture_of(session, node)) {
            tracing::warn!(error = %format!("{e:#}"), "capture: a message's words were not set aside");
        }
    }

    fn undo_capture_of(&self, session: &str, node: &str) -> Result<()> {
        let now = theseus_protocol::now_unix_ms();
        let mut held = self.runner.standing.capture.lock(&self.store)?;
        let mut next = (*held).clone();
        let (gone, left): (Vec<_>, Vec<_>) = next
            .asks
            .into_iter()
            .partition(|o| o.session == session && o.ask.node == node);
        next.asks = left;
        let source = format!("capture:{node}");
        let all = self.runner.standing.all(&self.store)?;
        let kept: Vec<StandingRecord> = standing::live(&all, None, now)
            .into_iter()
            .filter(|r| r.source == source)
            .cloned()
            .collect();
        if gone.is_empty() && kept.is_empty() {
            return Ok(());
        }
        next.kept.retain(|k| !kept.iter().any(|r| r.id == k.record));
        let mut rows = Vec::new();
        for o in &gone {
            let f = fact::capture::StandingAnswered {
                ask: &o.ask.id,
                answer: "withdrawn",
                record: None,
                who: "theseus",
                via: VIA,
            };
            rows.push(fact::row(&f, Some(session), None)?);
        }
        rows.push(next.row()?);
        self.store.append(&rows)?;
        held.set(next);
        drop(held);
        for old in &kept {
            self.end_kept(session, old)?;
        }
        Ok(())
    }

    /// One record capture kept, ended by a record that ends it, as a revoke
    /// does.
    fn end_kept(&self, session: &str, old: &StandingRecord) -> Result<()> {
        let now = theseus_protocol::now_unix_ms();
        let written = self.runner.standing.write(
            &self.store,
            |all| {
                Ok(vec![StandingRecord {
                    id: standing::mint(all),
                    kind: old.kind,
                    text: String::new(),
                    scope: old.scope.clone(),
                    since_ms: now,
                    until_ms: None,
                    source: VIA.into(),
                    ends: Some(old.id.clone()),
                }])
            },
            |_, records| {
                records
                    .iter()
                    .map(|r| {
                        fact::row(
                            &fact::standing::StandingRevoked {
                                record: r,
                                who: "theseus",
                                via: VIA,
                            },
                            Some(session),
                            None,
                        )
                    })
                    .collect()
            },
        )?;
        for r in &written {
            self.rec(Some(session))
                .announce(&fact::standing::StandingRevoked {
                    record: r,
                    who: "theseus",
                    via: VIA,
                });
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests;
