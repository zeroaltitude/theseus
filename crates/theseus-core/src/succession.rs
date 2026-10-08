//! A session's succession and retirement (theseus-emqx): the writes behind
//! the states `theseus_protocol::sessions` derives.
//!
//! - **A place moves** (`Core::bind_place_to`): the place's record, the old
//!   session's `superseded_by` and stored retirement, the new one's
//!   `supersedes`, and a `session.superseded` row, all in one frame, under
//!   both sessions' locks (the old's, then the new's), so no crash leaves
//!   the place moved and the records saying otherwise, or half of the pair.
//! - **Retire and reopen** (`Core::retire_session`, `Core::reopen_session`):
//!   the owner's acts (`judge_act(Act::Session)`: the owner, from a private
//!   place; the CLI refuses them inside a job), each its record and its row
//!   in one frame. Nothing is deleted: a retired session keeps its nodes, its
//!   memory and its index entries, and a reopen keeps both links as history.
//!   An imported session is the import's, and is neither.

use anyhow::{bail, Result};
use serde_json::json;
use theseus_protocol::sessions::{RetiredReason, SessionLink, SessionRetired};
use theseus_protocol::LedgerKind;
use theseus_store::{kinds, NewRecord};

use crate::approval::Answerer;
use crate::ledger::LedgerRow;
use crate::rpc::{Act, Core};
use crate::session::SessionRecord;

impl Core {
    /// A place now runs on `new` (a session opened for it, which no turn
    /// has used yet): the place's record and, when it ran on another
    /// session, the supersession both ways and its row, in one frame. Its
    /// posts go to the place from then on. The session it ran on, if any.
    pub fn bind_place_to(&self, place: &str, new: &str) -> Result<Option<String>> {
        let old = self
            .outbox
            .place_session(place)?
            .filter(|o| o.as_str() != new);
        let now = theseus_protocol::now_unix_ms();
        let moved = self.outbox.place_record(place, new)?;
        let written = match &old {
            Some(o) => self.supersede(o, new, place, now, moved.clone())?,
            None => false,
        };
        if !written {
            self.store.append(&[moved])?;
        }
        self.outbox.place_bound(place, new)?;
        Ok(old)
    }

    /// The frame that moves `place` from `old` to `new`, with both records:
    /// false, and nothing written, when either record is missing.
    fn supersede(
        &self,
        old: &str,
        new: &str,
        place: &str,
        now: u64,
        moved: NewRecord,
    ) -> Result<bool> {
        let link = |to: &str| SessionLink {
            session_id: to.to_string(),
            at_ms: now,
            place: Some(place.to_string()),
        };
        let row = LedgerRow::new(
            LedgerKind::SessionSuperseded,
            Some(old),
            None,
            json!({"superseded_by": new, "place": place}),
        );
        // The old session's lock, then the new one's: no other writer takes
        // two, and the new one has run no turn.
        let written = self.store.with_session(old, |mut was| {
            self.store.update_session(new, |rec| {
                rec.supersedes = Some(link(old));
                was.superseded_by = Some(link(new));
                was.retired.get_or_insert(SessionRetired {
                    reason: RetiredReason::Superseded,
                    at_ms: now,
                });
                Ok(vec![
                    moved,
                    NewRecord::json(kinds::LEDGER, None, &row)?,
                    NewRecord::json(kinds::SESSION, Some(old), &was)?,
                ])
            })
        })?;
        Ok(matches!(written, Some(Some(_))))
    }

    /// `session.retire`: the owner retires `id` by hand. One that is
    /// retired already keeps its first reason, and nothing is written.
    pub fn retire_session(&self, id: &str, who: &Answerer) -> Result<SessionRecord> {
        let what = format!("Retiring session {id}");
        self.judge_act(
            who,
            Act::Session {
                method: theseus_protocol::method::SESSION_RETIRE,
                what: &what,
            },
        )?;
        let now = theseus_protocol::now_unix_ms();
        let rec = self.own_session(id)?;
        if rec.retired.is_some() {
            return Ok(rec);
        }
        let row = LedgerRow::new(
            LedgerKind::SessionRetired,
            Some(id),
            None,
            json!({"reason": RetiredReason::ByHand, "by": who.label}),
        );
        self.store
            .update_session(id, |r| {
                r.retired = Some(SessionRetired {
                    reason: RetiredReason::ByHand,
                    at_ms: now,
                });
                Ok(vec![NewRecord::json(kinds::LEDGER, None, &row)?])
            })?
            .ok_or_else(|| anyhow::anyhow!("no session {id}"))
    }

    /// `session.reopen`: clears `id`'s retirement, whichever it was, and
    /// marks it reopened, so it is neither retired nor empty-retired. A
    /// superseded session keeps both links, and its place stays on its
    /// successor.
    pub fn reopen_session(&self, id: &str, who: &Answerer) -> Result<SessionRecord> {
        let what = format!("Reopening session {id}");
        self.judge_act(
            who,
            Act::Session {
                method: theseus_protocol::method::SESSION_REOPEN,
                what: &what,
            },
        )?;
        let now = theseus_protocol::now_unix_ms();
        self.own_session(id)?;
        self.store
            .update_session(id, |r| {
                let was = r.retired.take().map(|w| w.reason);
                r.reopened_ms = Some(now);
                let row = LedgerRow::new(
                    LedgerKind::SessionReopened,
                    Some(id),
                    None,
                    json!({"was": was, "by": who.label}),
                );
                Ok(vec![NewRecord::json(kinds::LEDGER, None, &row)?])
            })?
            .ok_or_else(|| anyhow::anyhow!("no session {id}"))
    }

    /// A session of the owner's own: not imported, and there.
    fn own_session(&self, id: &str) -> Result<SessionRecord> {
        let Some(rec) = self.store.get_session::<SessionRecord>(id)? else {
            bail!("no session {id}");
        };
        if rec.imported.is_some() {
            bail!("{id} is an imported session: the import's to keep, never retired or reopened");
        }
        Ok(rec)
    }

    /// Whether a place's session was retired (by hand or superseded), so
    /// its next message starts a successor (the Discord binding's read,
    /// one key).
    pub fn session_retired(&self, id: &str) -> bool {
        self.store
            .get_session::<SessionRecord>(id)
            .ok()
            .flatten()
            .is_some_and(|r| r.retired.is_some())
    }
}
