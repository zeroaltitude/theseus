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
use crate::rpc::Core;
use crate::standing;

/// Why the daemon ended it, as the ledger and the narrative say.
const VIA: &str = "a settings question for the same message";

impl Core {
    /// The owner's message that began turn `turn`: the words a settings question
    /// of that turn answers. Not the last message of the turn (one the owner
    /// steers in while it runs has its own capture, and may be a real order), and
    /// not the first (a task's report or a due wake is read into the turn before
    /// the input). The turn's inbound step names it (`Expected::note_input`);
    /// none for a turn a wake, a job or a restart began, which puts nothing to
    /// rest: the extra question stays, never a lost order.
    pub(crate) fn turn_input(&self, turn: &str) -> Option<String> {
        self.runner.standing.expected.input_of(turn)
    }

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
        // Only what capture kept by itself: a record the owner chose (Keep on
        // an ask) has the same source, and stays.
        let kept: Vec<StandingRecord> = standing::live(&all, None, now)
            .into_iter()
            .filter(|r| r.source == source)
            .filter(|r| next.kept.iter().any(|k| k.record == r.id && !k.by_owner))
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
        let written = self.runner.standing.write(
            &self.store,
            |all| {
                let mut known = all.to_vec();
                let first = known.len();
                for old in &kept {
                    known.push(StandingRecord {
                        id: standing::mint(&known),
                        kind: old.kind,
                        text: String::new(),
                        scope: old.scope.clone(),
                        since_ms: now,
                        until_ms: None,
                        source: VIA.into(),
                        ends: Some(old.id.clone()),
                    });
                }
                Ok(known.split_off(first))
            },
            |_, records| {
                for r in records {
                    rows.push(fact::row(
                        &fact::standing::StandingRevoked {
                            record: r,
                            who: "theseus",
                            via: VIA,
                        },
                        Some(session),
                        None,
                    )?);
                }
                Ok(rows)
            },
        )?;
        // The recovery handles and every ending are durable in the same frame.
        held.set(next);
        drop(held);
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
