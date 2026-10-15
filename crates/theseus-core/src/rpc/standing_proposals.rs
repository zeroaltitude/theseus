//! `standing.proposals`, `standing.keep` and `standing.dismiss`
//! (theseus-a8n7): the standing records an imported agent's feedback
//! proposes. A proposal reaches no prompt. A keep is `standing.add` of the
//! proposed words, scoped to the persona (judged as the add is: the owner,
//! from a private place), then the proposal closes; a keep that finds the
//! same record live already closes it too, so a crash between the two
//! writes costs nothing. A dismissal closes it for good: the same words are
//! never proposed again.

use anyhow::anyhow;
use theseus_protocol::{
    method, StandingAddParams, StandingProposal, StandingProposalAnswer, StandingProposalAnswered,
    StandingProposalsParams, StandingProposalsResult, StandingScope,
};
use theseus_store::{kinds, NewRecord};

use super::confirms::Act;
use super::server::RpcFailure;
use super::Core;
use crate::approval::Answerer;
use crate::fact;
use crate::standing_proposals as props;

impl Core {
    /// `standing.proposals`: the open ones, or all; the words only for the
    /// owner in a private place.
    pub fn standing_proposals(
        &self,
        p: &StandingProposalsParams,
        who: impl Into<Answerer>,
    ) -> Result<StandingProposalsResult, RpcFailure> {
        let who = who.into();
        let owner =
            crate::places::owner_in_private(&who, &self.runner.place_rule, &self.cfg).is_ok();
        let mut proposals: Vec<StandingProposal> = props::all(&self.store)
            .map_err(RpcFailure::invalid)?
            .into_iter()
            .filter(|x| p.all || x.state == "open")
            .filter(|x| p.persona.as_deref().is_none_or(|n| n == x.persona))
            .collect();
        if !owner {
            for x in &mut proposals {
                x.text = "(private)".into();
            }
        }
        Ok(StandingProposalsResult { proposals })
    }

    /// `standing.keep`: write the record, then close the proposal.
    pub fn standing_keep(
        &self,
        p: &StandingProposalAnswer,
        who: impl Into<Answerer>,
    ) -> Result<StandingProposalAnswered, RpcFailure> {
        let who = who.into();
        let all = props::all(&self.store).map_err(RpcFailure::invalid)?;
        let proposal = props::resolve(&all, &p.id).map_err(RpcFailure::invalid)?;
        let changed = self.standing_add(
            &StandingAddParams {
                kind: proposal.kind,
                text: proposal.text.clone(),
                scope: StandingScope::Persona(proposal.persona.clone()),
                source: Some(format!("persona-import:{}", proposal.id)),
                author: p.author.clone(),
                discord: p.discord.clone(),
                ..Default::default()
            },
            who.clone(),
        )?;
        let closed = StandingProposal {
            state: "kept".into(),
            record: Some(changed.record.id.clone()),
            ..proposal
        };
        self.close_proposal(&closed, &who)?;
        Ok(StandingProposalAnswered {
            proposal: closed,
            record: Some(changed.record),
        })
    }

    /// `standing.dismiss`: close the proposal; nothing is written to any
    /// prompt, and the words are not proposed again.
    pub fn standing_dismiss(
        &self,
        p: &StandingProposalAnswer,
        who: impl Into<Answerer>,
    ) -> Result<StandingProposalAnswered, RpcFailure> {
        let who = who.into();
        let all = props::all(&self.store).map_err(RpcFailure::invalid)?;
        let proposal = props::resolve(&all, &p.id).map_err(RpcFailure::invalid)?;
        let what = format!("the dismissal of {}", proposal.id);
        self.judge_act(
            &who,
            Act::Standing {
                method: method::STANDING_DISMISS,
                what: &what,
            },
        )
        .map_err(|e| refused(e, "a standing answer"))?;
        let closed = StandingProposal {
            state: "dismissed".into(),
            ..proposal
        };
        self.close_proposal(&closed, &who)?;
        Ok(StandingProposalAnswered {
            proposal: closed,
            record: None,
        })
    }

    /// The proposal's new version and its row, one frame.
    fn close_proposal(&self, p: &StandingProposal, who: &Answerer) -> Result<(), RpcFailure> {
        let (who_s, via) = (who.who(), who.via());
        let frame: Vec<NewRecord> = vec![
            NewRecord::json(kinds::META, Some(&props::key(&p.id)), p)
                .map_err(|e| RpcFailure::invalid(anyhow!("{e:#}")))?,
            fact::row(
                &fact::persona_import::StandingProposalAnswered {
                    proposal: p,
                    who: &who_s,
                    via: &via,
                },
                None,
                None,
            )
            .map_err(RpcFailure::invalid)?,
        ];
        theseus_store::blocking(|| self.store.append(&frame)).map_err(RpcFailure::invalid)?;
        self.rec(None)
            .announce(&fact::persona_import::StandingProposalAnswered {
                proposal: p,
                who: &who_s,
                via: &via,
            });
        Ok(())
    }
}

fn refused(e: anyhow::Error, what: &str) -> RpcFailure {
    match e.downcast::<crate::approval::Refusal>() {
        Ok(r) => RpcFailure {
            code: theseus_protocol::error_code::REFUSED,
            message: format!(
                "{what} from {} does not count: {}. Nothing was written.",
                r.who, r.why
            ),
            data: serde_json::json!({"who": r.who, "via": r.via, "why": r.why}),
        },
        Err(e) => RpcFailure::invalid(e),
    }
}
