//! The owner card's methods (theseus-slvl.6): `card.propose`, `card.list`,
//! `card.answer` and `card.done`, routed with the standing records'. The job,
//! the reads and the writes are `card/`; each method is the owner's act from a
//! private place (`judge_act`, as `standing.add` is), and the CLI refuses the
//! writes inside a job.

use anyhow::anyhow;
use serde_json::{json, Value};
use theseus_protocol::{
    error_code, method, CardAnswerParams, CardAnswerResult, CardDoneParams, CardDoneResult,
    CardListParams, CardProposeParams, StandingAddParams, StandingChanged,
};

use super::confirms::Act;
use super::server::{parse, reply, Conn, RpcFailure};
use super::Core;
use crate::approval::{Answerer, Refusal};
use crate::card::answer::option_of;

/// A write's error on the wire: a judged refusal is `REFUSED`.
fn failure(e: anyhow::Error) -> RpcFailure {
    match e.downcast::<Refusal>() {
        Ok(r) => RpcFailure {
            code: error_code::REFUSED,
            message: format!(
                "a card act from {} does not count: {}. Nothing was written.",
                r.who, r.why
            ),
            data: json!({"who": r.who, "via": r.via, "why": r.why}),
        },
        Err(e) => RpcFailure::invalid(e),
    }
}

impl Core {
    pub(super) async fn rpc_card(
        &self,
        name: &str,
        params: Value,
        conn: Conn<'_>,
    ) -> Result<Value, RpcFailure> {
        match name {
            method::CARD_PROPOSE => {
                let p: CardProposeParams = parse(params)?;
                let who = conn.answerer(p.author.clone(), p.discord.clone());
                reply(self.card_propose(&p, who).await?)
            }
            method::CARD_LIST => {
                let p: CardListParams = parse(params)?;
                let who = conn.answerer(p.author.clone(), p.discord.clone());
                reply(self.card_list(&p, who).await?)
            }
            method::CARD_ANSWER => {
                let p: CardAnswerParams = parse(params)?;
                let who = conn.answerer(p.author.clone(), p.discord.clone());
                reply(self.card_answer(&p, who).await?)
            }
            method::CARD_DONE => {
                let p: CardDoneParams = parse(params)?;
                let who = conn.answerer(p.author.clone(), p.discord.clone());
                reply(self.card_done(&p, who).await?)
            }
            other => Err(RpcFailure::new(
                error_code::METHOD_NOT_FOUND,
                format!("unknown method {other:?}"),
            )),
        }
    }

    fn judge_card(
        &self,
        who: &Answerer,
        method: &'static str,
        what: &str,
    ) -> Result<(), RpcFailure> {
        self.judge_act(who, Act::Standing { method, what })
            .map_err(failure)
    }

    /// `card.propose`: run the job now (or only say what it would send).
    pub async fn card_propose(
        &self,
        p: &CardProposeParams,
        who: impl Into<Answerer>,
    ) -> Result<theseus_protocol::CardProposeResult, RpcFailure> {
        self.judge_card(&who.into(), method::CARD_PROPOSE, "the card's proposals")?;
        self.card_run(p.dry_run).await.map_err(RpcFailure::invalid)
    }

    /// `card.list`: what waits, what was kept and declined, the spend.
    pub async fn card_list(
        &self,
        _p: &CardListParams,
        who: impl Into<Answerer>,
    ) -> Result<theseus_protocol::CardListResult, RpcFailure> {
        self.judge_card(&who.into(), method::CARD_LIST, "the card's proposals")?;
        self.card_view().await.map_err(RpcFailure::invalid)
    }

    /// `card.done`: the card is done (or not): the nightly run stops (or resumes).
    pub async fn card_done(
        &self,
        p: &CardDoneParams,
        who: impl Into<Answerer>,
    ) -> Result<CardDoneResult, RpcFailure> {
        self.judge_card(&who.into(), method::CARD_DONE, "the card's nightly run")?;
        self.card_set_done(p.done)
            .await
            .map_err(RpcFailure::invalid)?;
        Ok(CardDoneResult { done: p.done })
    }

    /// `card.answer`: keep, edit or decline one proposal.
    pub async fn card_answer(
        &self,
        p: &CardAnswerParams,
        who: impl Into<Answerer>,
    ) -> Result<CardAnswerResult, RpcFailure> {
        let who = who.into();
        self.judge_card(&who, method::CARD_ANSWER, "an answer to a card proposal")?;
        let option = option_of(&p.option).ok_or_else(|| {
            RpcFailure::invalid(anyhow!(
                "{:?} is not one of the question's options: keep, edit, or decline (\"Not this\")",
                p.option
            ))
        })?;
        let prop = self.card_open(&p.id).await.map_err(RpcFailure::invalid)?;
        let (by, via) = (who.who(), who.via());
        if option == "decline" {
            let proposal = self
                .card_settle(&prop, option, None, &by, &via)
                .await
                .map_err(RpcFailure::invalid)?;
            return Ok(CardAnswerResult {
                proposal,
                record: None,
            });
        }
        let text = match (option, p.text.as_deref().map(str::trim)) {
            ("edit", Some(t)) if !t.is_empty() => {
                t.split_whitespace().collect::<Vec<_>>().join(" ")
            }
            ("edit", _) => {
                return Err(RpcFailure::invalid(anyhow!(
                    "an edit gives the words to keep: --text"
                )))
            }
            _ => prop.text.clone(),
        };
        let add = StandingAddParams {
            kind: prop.kind,
            text: text.clone(),
            scope: prop.scope.clone(),
            source: Some(format!("card:{}", prop.id)),
            author: p.author.clone(),
            discord: p.discord.clone(),
            ..StandingAddParams::default()
        };
        let StandingChanged { record, .. } = self.standing_add(&add, who.clone())?;
        let proposal = self
            .card_settle(&prop, option, Some((text, record.id.clone())), &by, &via)
            .await
            .map_err(RpcFailure::invalid)?;
        Ok(CardAnswerResult {
            proposal,
            record: Some(record),
        })
    }
}
