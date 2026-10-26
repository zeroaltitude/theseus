//! Removing a content label (theseus-9ymz0, design §6.3): the operator clears
//! one flag from a session, as `policy.trust` clears a hold. A label is raised
//! by what entered the session and removed only by the operator, so a session
//! that carries `injection` stays frozen, and one that carries `secret`,
//! `confidential` or `personal` stays off the China-origin models, until this.
//!
//! It loosens, so it is judged as an answer is (`judge_act(Act::Unlabel)`):
//! only from the owner, from a private place, never from a job's process. It
//! is `Act::Unlabel` and not `Act::Trust` because a trust's refusal row, its
//! method and its words are about a hold on external text; this refusal is
//! the session's row naming `content.unlabel` and the flag. Every other part
//! is `trust.rs`'s: the record and the `content.removed` row in one frame
//! under the session's lock, a sentence in the narrative, and the event to the
//! session's watchers. Removing a flag the session does not carry is an error
//! and writes nothing.

use anyhow::{anyhow, bail, Result};
use serde_json::json;
use theseus_protocol::{error_code, ContentUnlabelParams, LabelView, UnlabelResult};
use theseus_store::{kinds, NewRecord};

use super::confirms::Act;
use super::server::{Conn, RpcFailure};
use super::Core;
use crate::approval::{Answerer, Refusal};
use crate::content::{Flag, Label};
use crate::fact::{self, content::ContentRemoved};
use crate::session::SessionRecord;

/// A label as the protocol shows it.
pub(crate) fn label_view(l: &Label) -> LabelView {
    LabelView {
        flag: l.flag.as_str().into(),
        node: l.node.clone(),
        kind: l.kind.as_str().into(),
        source: l.source.clone(),
        confidence: l.confidence,
        band: match l.band {
            theseus_judge::band::Band::Act => "act",
            theseus_judge::band::Band::Confirm => "confirm",
            theseus_judge::band::Band::Escalate => "escalate",
        }
        .into(),
        at_ms: l.at_ms,
    }
}

impl Core {
    /// `content.unlabel`: the session no longer carries `flag`. An unknown
    /// session, a word that is not a flag, and a flag the session does not
    /// carry are each an error, and nothing is written.
    pub fn unlabel_session(
        &self,
        session_id: &str,
        flag: &str,
        by: impl Into<Answerer>,
    ) -> Result<UnlabelResult> {
        let who = by.into();
        let flag = Flag::parse(flag).ok_or_else(|| {
            anyhow!("`{flag}` is not a content flag (injection, secret, confidential or personal)")
        })?;
        self.store
            .get_session::<SessionRecord>(session_id)?
            .ok_or_else(|| anyhow!("no session is named {session_id}"))?;
        let what = format!("Removing {} from session {session_id}", flag.as_str());
        self.judge_act(
            &who,
            Act::Unlabel {
                session: session_id,
                what: &what,
            },
        )?;
        let out = self
            .store
            .with_session(session_id, |rec| {
                let Some((r, frame)) = removed(
                    rec,
                    session_id,
                    flag,
                    &who,
                    theseus_protocol::method::CONTENT_UNLABEL,
                    None,
                )?
                else {
                    return Ok(None);
                };
                self.store.append(&frame)?;
                Ok(Some(r))
            })?
            .flatten();
        let Some(r) = out else {
            bail!(
                "session {session_id} carries no {} label, so there is nothing to remove",
                flag.as_str()
            );
        };
        self.announce_removal(session_id, &r);
        Ok(r)
    }

    /// A removal written: narrated, and published to the session's watchers.
    fn announce_removal(&self, session_id: &str, r: &UnlabelResult) {
        fact::Rec {
            narrator: &self.narrator,
            session: Some(session_id),
            turn: None,
            to: fact::To::Session(&self.bus, session_id),
            store: &self.store,
        }
        .announce(&ContentRemoved { result: r });
    }

    pub(super) fn content_unlabel(
        &self,
        p: ContentUnlabelParams,
        conn: Conn<'_>,
    ) -> Result<UnlabelResult, RpcFailure> {
        // A removal names the surface or the person, as a trust does:
        // `Conn::answerer`.
        let who = conn.answerer(p.author, p.discord);
        self.unlabel_session(&p.session_id, &p.flag, who)
            .map_err(|e| match e.downcast::<Refusal>() {
                Ok(r) => RpcFailure {
                    code: error_code::REFUSED,
                    message: format!(
                        "removing a content label from {} does not count: {}. The session keeps \
                         its labels.",
                        r.who, r.why
                    ),
                    data: json!({"who": r.who, "via": r.via, "why": r.why}),
                },
                Err(e) => RpcFailure::invalid(e),
            })
    }
}

/// The frame that removes `flag` from `rec`: the `content.removed` row, then
/// the record. None, and nothing to write, when the session does not carry it.
fn removed(
    mut rec: SessionRecord,
    session_id: &str,
    flag: Flag,
    who: &Answerer,
    how: &str,
    correlation_id: Option<&str>,
) -> Result<Option<(UnlabelResult, Vec<NewRecord>)>> {
    let Some(mut flags) = rec.data.take() else {
        return Ok(None);
    };
    let at_ms = theseus_protocol::now_unix_ms();
    let Some(label) = flags.remove(flag, &who.label, &who.via(), at_ms) else {
        return Ok(None);
    };
    rec.data = Some(flags);
    let r = UnlabelResult {
        session_id: session_id.into(),
        flag: flag.as_str().into(),
        by: who.label.clone(),
        who: who.who(),
        via: who.via(),
        how: how.into(),
        correlation_id: correlation_id.map(str::to_string),
        at_ms,
        label: label_view(&label),
    };
    let frame = vec![
        fact::row(&ContentRemoved { result: &r }, Some(session_id), None)?,
        NewRecord::json(kinds::SESSION, Some(session_id), &rec)?,
    ];
    Ok(Some((r, frame)))
}
