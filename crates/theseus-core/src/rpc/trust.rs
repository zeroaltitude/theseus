//! Trusting a session again (theseus-9bp, spec §3.9): the operator clears a
//! session's hold on external text, so its calls that act go back to their
//! postures. It loosens, so it is judged as an answer is (`judge_act`): never
//! from a Theseus job's process (theseus-6qy), and under `[approval]` only
//! from a trusted user through a trusted channel. Ledgered as
//! `session.trusted`, with who, how, and the hold it cleared.

use anyhow::{anyhow, bail, Result};
use serde_json::json;
use theseus_protocol::LedgerKind;
use theseus_protocol::{Event, Message, PolicyTrustParams, TrustResult};

use super::confirms::Act;
use super::server::{Conn, RpcFailure};
use super::Core;
use crate::approval::{Answerer, Refusal};
use crate::ledger::LedgerRow;
use crate::narrative::narrate;
use crate::peer::Traced;
use crate::session::SessionRecord;
use theseus_protocol::error_code;
use theseus_store::{kinds, NewRecord};

impl Core {
    /// `policy.trust`: the session no longer holds external text. A session
    /// that holds none is an error, and nothing is written.
    pub fn trust_session(&self, session_id: &str, by: impl Into<Answerer>) -> Result<TrustResult> {
        let who = by.into();
        let rec = self
            .store
            .get_session::<SessionRecord>(session_id)?
            .ok_or_else(|| anyhow!("no session is named {session_id}"))?;
        if rec.external.is_none() {
            bail!(
                "session {session_id} holds no external text, so there is nothing to trust again"
            );
        }
        let asker = self.judge_act(
            &who,
            Act::Trust {
                session: session_id,
            },
        )?;
        self.clear_hold(
            session_id,
            &who,
            theseus_protocol::method::POLICY_TRUST,
            None,
            &asker,
        )?
        .ok_or_else(|| {
            anyhow!("session {session_id} holds no external text now: it was trusted a moment ago")
        })
    }

    /// Clear a session's hold, the act judged already: its record and the
    /// `session.trusted` row in one frame, under the record's lock. None, and
    /// nothing written, when it holds none. `how` is the method that did it,
    /// and `correlation_id` the approval that did, when one did.
    pub(super) fn clear_hold(
        &self,
        session_id: &str,
        who: &Answerer,
        how: &str,
        correlation_id: Option<&str>,
        asker: &Traced,
    ) -> Result<Option<TrustResult>> {
        let out = self
            .store
            .with_session(session_id, |rec| {
                let Some((r, frame)) = trusted(rec, session_id, who, how, correlation_id, asker)?
                else {
                    return Ok(None);
                };
                self.store.append(&frame)?;
                Ok(Some(r))
            })?
            .flatten();
        if let Some(r) = &out {
            self.announce_trust(session_id, r);
        }
        Ok(out)
    }

    /// A trust written: narrated, and published to the session's watchers.
    pub(super) fn announce_trust(&self, session_id: &str, r: &TrustResult) {
        narrate!(
            self.narrator,
            Approval,
            Some(session_id),
            None,
            "Trusted again by {}: this session no longer holds external text ({}), so its \
             calls that act are back at their postures.",
            r.by,
            crate::external::source(&r.held)
        );
        self.bus.publish(
            session_id,
            &Message::from(Event::SessionTrusted(r.clone())),
            None,
        );
    }
}

/// The frame that clears `rec`'s hold (`clear_hold`, and an approval's trust
/// in the answer's own frame): the `session.trusted` row, then the record.
/// None when it holds none.
pub(super) fn trusted(
    mut rec: SessionRecord,
    session_id: &str,
    who: &Answerer,
    how: &str,
    correlation_id: Option<&str>,
    asker: &Traced,
) -> Result<Option<(TrustResult, Vec<NewRecord>)>> {
    let Some(held) = rec.external.take() else {
        return Ok(None);
    };
    let at_ms = theseus_protocol::now_unix_ms();
    let r = TrustResult {
        session_id: session_id.into(),
        by: who.label.clone(),
        who: who.who(),
        via: who.via(),
        how: how.into(),
        correlation_id: correlation_id.map(str::to_string),
        at_ms,
        since_local: crate::external::since_local(&held, at_ms),
        held,
    };
    let mut data = serde_json::to_value(&r)?;
    if *asker != Traced::NoProcess {
        data["asker"] = asker.json();
    }
    let row = LedgerRow::new(LedgerKind::SessionTrusted, Some(session_id), None, data);
    let frame = vec![
        NewRecord::json(kinds::LEDGER, None, &row)?,
        NewRecord::json(kinds::SESSION, Some(session_id), &rec)?,
    ];
    Ok(Some((r, frame)))
}

impl Core {
    pub(super) fn policy_trust(
        &self,
        p: PolicyTrustParams,
        conn: Conn<'_>,
    ) -> Result<TrustResult, RpcFailure> {
        // A trust names the surface or the person, as a cancel does (DD8),
        // and as an approval's trust does (theseus-qiy): `Conn::answerer`.
        let who = conn.answerer(p.author, p.discord);
        self.trust_session(&p.session_id, who)
            .map_err(|e| match e.downcast::<Refusal>() {
                Ok(r) => RpcFailure {
                    code: error_code::REFUSED,
                    message: format!(
                        "trusting the session again from {} does not count: {}. It still holds \
                         external text.",
                        r.who, r.why
                    ),
                    data: json!({"who": r.who, "via": r.via, "why": r.why}),
                },
                Err(e) => RpcFailure::invalid(e),
            })
    }
}
