//! `policy.notice_seen` (theseus-g00vv): the owner has seen a hold's notice.
//! Under `[policy] external_text = "notify"` a hold tells once, at its first
//! call that acts, marked by the META key `external.told.<session>` holding
//! the hold's `since_ms`; this writes the same record for the hold as it
//! stands, so the notice can be dismissed without a call. Nothing else
//! changes: the hold, its `session.external_read` row and the session stay,
//! and no ledger row is written. A later hold has a new `since_ms`, so it
//! tells again.
//!
//! It loosens nothing, but it is the owner's act as `policy.trust` is: judged
//! by `judge_act` (the owner, from a private place), so MCP and a shared place
//! are refused. A hold that waits (ask mode, a live execution) is an ask the
//! owner answers or trusts, never dismisses.

use anyhow::{anyhow, bail, Result};
use theseus_protocol::notice::{NoticeSeenResult, PolicyNoticeSeenParams};
use theseus_store::{kinds, NewRecord};

use super::confirms::Act;
use super::server::{Conn, RpcFailure};
use super::Core;
use crate::approval::Answerer;
use crate::session::SessionRecord;

impl Core {
    pub(super) fn policy_notice_seen(
        &self,
        p: PolicyNoticeSeenParams,
        conn: Conn<'_>,
    ) -> Result<NoticeSeenResult, RpcFailure> {
        let who = conn.answerer(p.author, p.discord);
        self.notice_seen(&p.session_id, &who)
            .map_err(super::sessions::refused)
    }

    /// Mark the session's current hold told, for `who`. An error, with
    /// nothing written, when the session holds none or the hold waits.
    pub fn notice_seen(&self, session_id: &str, who: &Answerer) -> Result<NoticeSeenResult> {
        let what = format!("Dismissing the notice of session {session_id}");
        self.judge_act(
            who,
            Act::Dismiss {
                session: session_id,
                what: &what,
            },
        )?;
        let rec = self
            .store
            .get_session::<SessionRecord>(session_id)?
            .ok_or_else(|| anyhow!("no session is named {session_id}"))?;
        if rec.external.is_none() {
            bail!("session {session_id} holds no external text, so there is no notice to dismiss");
        }
        let state = rec
            .execution_id
            .as_deref()
            .and_then(|id| self.kernel.execution(id).ok().flatten())
            .map(|e| e.state);
        if crate::external::waits(self.tools.external.mode, state) {
            bail!(
                "session {session_id}'s hold waits for you: its calls that act are asked, not told, \
                 so it cannot be dismissed. Answer the question, or trust the session again: \
                 theseus policy trust {session_id}"
            );
        }
        // Under the record's lock, so a hold that changed meanwhile is read as it
        // is: the mark is for the hold that was seen, and a new one tells again.
        let wrote = self
            .store
            .with_session(session_id, |rec| -> Result<Option<_>> {
                let Some(h) = rec.external else {
                    return Ok(None);
                };
                if crate::external::told(&self.store, session_id, &h) {
                    return Ok(Some((h, false)));
                }
                self.store.append(&[NewRecord::json(
                    kinds::META,
                    Some(&crate::external::told_key(session_id)),
                    &h.since_ms,
                )?])?;
                Ok(Some((h, true)))
            })?
            .flatten();
        let (held, wrote) = wrote.ok_or_else(|| {
            anyhow!("session {session_id} holds no external text now: it was trusted a moment ago")
        })?;
        Ok(NoticeSeenResult {
            session_id: session_id.into(),
            held,
            wrote,
        })
    }
}
