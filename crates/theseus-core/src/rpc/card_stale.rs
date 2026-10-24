//! The owner's answers to a Stale question (theseus-6f8t0): a record of the
//! card that something he said since seems to contradict. Only his answer
//! changes the record:
//!
//! - **Keep** keeps it as it is (the proposal is kept, naming the record) and
//!   sets the observation aside.
//! - **Edit** ends it and adds his words in one `standing.add` with
//!   `replaces` (the new record's `ends` is the old one, so there is no frame
//!   in which only one of the two happened), source `card:<id>`. A crash after
//!   that frame and before the proposal's is settled by answering again: the
//!   record `card:<id>` made is found live.
//! - **End it** revokes it through `standing.revoke`.
//!
//! Not this (`decline`) is the generic answer: the record stays, the
//! observation is set aside.

use anyhow::anyhow;
use theseus_protocol::{
    CardAnswerParams, CardAnswerResult, CardProposal, StandingAddParams, StandingChanged,
    StandingRecord, StandingRevokeParams, StandingScope,
};

use super::server::RpcFailure;
use super::Core;
use crate::approval::Answerer;
use crate::standing;

impl Core {
    /// The live record `id` names, if it is live now.
    fn live_record(&self, id: &str) -> anyhow::Result<Option<StandingRecord>> {
        let board = self.runner.standing.all(&self.store)?;
        let now = theseus_protocol::now_unix_ms();
        Ok(standing::live(&board, None, now)
            .into_iter()
            .find(|r| r.id == id)
            .cloned())
    }

    /// Answer the open Stale question `prop` with `keep`, `edit` (`text` is
    /// the owner's words, `scope` his scope if he named one) or `end`.
    pub(crate) async fn card_stale(
        &self,
        p: &CardAnswerParams,
        prop: &CardProposal,
        option: &str,
        text: Option<String>,
        scope: Option<StandingScope>,
        who: Answerer,
    ) -> Result<CardAnswerResult, RpcFailure> {
        let invalid = |m: String| RpcFailure::invalid(anyhow!(m));
        let rid = prop
            .record
            .clone()
            .ok_or_else(|| invalid(format!("{} names no record", prop.id)))?;
        let (by, via) = (who.who(), who.via());
        let live = self.live_record(&rid).map_err(RpcFailure::invalid)?;
        let settled = |kept| async move { self.card_settle(prop, option, kept, &by, &via).await };
        match option {
            "keep" => {
                if scope.is_some() {
                    return Err(invalid(
                        "a keep leaves the record as it is: it takes no scope".into(),
                    ));
                }
                let rec = live.ok_or_else(|| {
                    invalid(format!(
                        "{rid} no longer holds: answer end it, or Not this, to close the question"
                    ))
                })?;
                let proposal = settled(Some((rec.text.clone(), &rec)))
                    .await
                    .map_err(RpcFailure::invalid)?;
                Ok(CardAnswerResult {
                    proposal,
                    record: Some(rec),
                })
            }
            "edit" => {
                let text =
                    text.ok_or_else(|| invalid("an edit gives the words to keep: --text".into()))?;
                let source = format!("card:{}", prop.id);
                let record = match live {
                    Some(old) => {
                        let add = StandingAddParams {
                            kind: old.kind,
                            text: text.clone(),
                            scope: scope.unwrap_or_else(|| old.scope.clone()),
                            source: Some(source),
                            replaces: Some(old.id),
                            author: p.author.clone(),
                            discord: p.discord.clone(),
                            ..StandingAddParams::default()
                        };
                        let StandingChanged { record, .. } =
                            self.standing_add(&add, who.clone())?;
                        record
                    }
                    // An earlier answer wrote the record and not the proposal.
                    None => self
                        .live_by_source(&source)
                        .map_err(RpcFailure::invalid)?
                        .ok_or_else(|| {
                            invalid(format!("{rid} no longer holds: there is nothing to edit"))
                        })?,
                };
                let proposal = settled(Some((text, &record)))
                    .await
                    .map_err(RpcFailure::invalid)?;
                Ok(CardAnswerResult {
                    proposal,
                    record: Some(record),
                })
            }
            _ => {
                if live.is_some() {
                    let revoke = StandingRevokeParams {
                        id: rid,
                        author: p.author.clone(),
                        discord: p.discord.clone(),
                    };
                    self.standing_revoke(&revoke, who.clone())?;
                }
                let proposal = settled(None).await.map_err(RpcFailure::invalid)?;
                Ok(CardAnswerResult {
                    proposal,
                    record: None,
                })
            }
        }
    }

    /// The live record a card answer wrote (its `source`).
    fn live_by_source(&self, source: &str) -> anyhow::Result<Option<StandingRecord>> {
        let board = self.runner.standing.all(&self.store)?;
        let now = theseus_protocol::now_unix_ms();
        Ok(standing::live(&board, None, now)
            .into_iter()
            .find(|r| r.source == source)
            .cloned())
    }
}
