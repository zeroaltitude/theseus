//! The owner's answers to the card (theseus-slvl.6): the reads and writes
//! behind `card.list`, `card.answer` and `card.done` (`rpc/card.rs` judges
//! the act and calls these).
//!
//! - **Keep and Edit** write the standing record through `standing_add` (its
//!   own judgment, frame and row, `source` `card:<id>`), then the proposal as
//!   kept with the record's id and a `card.answered` row naming the episodes
//!   it rests on (`settle`). A crash between the two leaves a live record and
//!   an open proposal; answering again finds the record already live (the
//!   same kind, scope and text) and settles the proposal.
//! - **Scope** (theseus-ly75u): `keep` and `edit` take the owner's scope in
//!   place of the proposal's; `scope` on a kept proposal writes its record
//!   again in the new scope (`standing.add` with `replaces`, so the old one
//!   ends in the same frame; the same kind, words and source), then the
//!   proposal with the new record's id and a `card.answered` row.
//! - **Not this** is the same key written again as declined: the sources are
//!   seen for good, and the statement's words are never proposed again.
//! - **Expiry** is applied when the card is read or the job runs
//!   (`card_expire`): an open proposal past its `expires_ms` becomes declined,
//!   by `expiry`, in one frame with its row. No timer.

use anyhow::{anyhow, bail, Result};
use theseus_protocol::{
    CardListResult, CardProposal, ProposalState, StandingRecord, CARD_LIST_ANSWERED,
};

use super::state::{self, State};
use crate::fact::card::CardAnswered;
use crate::judge::spend;
use crate::rpc::Core;
use theseus_protocol::{CardAsk, CardObservation, ObservationState};
use theseus_store::NewRecord;

/// An option's word, any case: `keep`, `edit`, `decline`; or `scope`, the
/// owner's move of a kept proposal's record (theseus-ly75u); or `end`, End it,
/// the owner's answer that a record a Stale question names no longer holds.
pub fn option_of(s: &str) -> Option<&'static str> {
    match s.trim().to_lowercase().replace(['_', '-'], " ").as_str() {
        "keep" | "k" | "yes" | "1" => Some("keep"),
        "edit" | "e" | "2" => Some("edit"),
        "decline" | "not this" | "no" | "drop" | "3" => Some("decline"),
        "scope" => Some("scope"),
        "end" | "end it" => Some("end"),
        _ => None,
    }
}

impl Core {
    /// The card as the list shows it: what waits, what was kept and
    /// declined, and the spend. Expires what is due first.
    pub(crate) async fn card_view(&self) -> Result<CardListResult> {
        let now = theseus_protocol::now_unix_ms();
        self.card_expire(now).await?;
        let (all, st) = theseus_store::blocking(|| {
            Ok::<_, anyhow::Error>((state::proposals(&self.store)?, State::read(&self.store)?))
        })?;
        let today = spend::local_day(now);
        let newest = |s: ProposalState| {
            let mut v: Vec<CardProposal> = state::in_state(&all, s).cloned().collect();
            v.sort_by(|a, b| b.answered_ms.cmp(&a.answered_ms).then(b.id.cmp(&a.id)));
            let total = v.len() as u64;
            v.truncate(CARD_LIST_ANSWERED);
            (v, total)
        };
        let (kept, kept_total) = newest(ProposalState::Kept);
        let (declined, declined_total) = newest(ProposalState::Declined);
        Ok(CardListResult {
            open: state::in_state(&all, ProposalState::Open)
                .cloned()
                .collect(),
            kept,
            declined,
            kept_total,
            declined_total,
            spent_today_usd: st.spent_on(&today),
            spent_total_usd: st.total_usd,
            day_usd: self.cfg.card.day_usd,
            max_open: u64::from(self.cfg.card.max_open),
            last_run_ms: st.last_run_ms,
            done: st.done,
            every_days: u64::from(self.cfg.card.every_days),
        })
    }

    /// The open proposal `id` names (after expiry), or why there is none.
    pub(crate) async fn card_open(&self, id: &str) -> Result<CardProposal> {
        self.card_expire(theseus_protocol::now_unix_ms()).await?;
        let all = theseus_store::blocking(|| state::proposals(&self.store))?;
        let p = state::resolve(&all, id).map_err(|e| anyhow!(e))?;
        if p.state != ProposalState::Open {
            bail!("{} is already {}", p.id, p.state.name());
        }
        Ok(p.clone())
    }

    /// The kept proposal `id` names, or why there is none: what a re-scope
    /// moves.
    pub(crate) fn card_kept(&self, id: &str) -> Result<CardProposal> {
        let all = theseus_store::blocking(|| state::proposals(&self.store))?;
        let p = state::resolve(&all, id).map_err(|e| anyhow!(e))?;
        if p.state != ProposalState::Kept || p.standing_id.is_none() {
            bail!(
                "{} is {}: only a kept proposal's record can be moved to another scope",
                p.id,
                p.state.name()
            );
        }
        Ok(p.clone())
    }

    /// Write `prop`'s answer: the proposal as `state` with the words kept (when
    /// they were edited), the standing record's id and scope, and its row.
    pub(crate) async fn card_settle(
        &self,
        prop: &CardProposal,
        option: &str,
        kept: Option<(String, &StandingRecord)>,
        by: &str,
        via: &str,
    ) -> Result<CardProposal> {
        let mut next = prop.clone();
        next.answered_ms = Some(theseus_protocol::now_unix_ms());
        next.answered_by = Some("owner".into());
        match kept {
            Some((text, record)) => {
                if text != prop.text {
                    next.kept_text = Some(text);
                }
                // The scope it holds in: the owner's, when he named one.
                next.scope = record.scope.clone();
                next.standing_id = Some(record.id.clone());
                next.state = ProposalState::Kept;
            }
            None => next.state = ProposalState::Declined,
        }
        let episodes: Vec<String> = prop.sources.iter().map(|s| s.episode.clone()).collect();
        let f = CardAnswered {
            id: &prop.id,
            option,
            by,
            via,
            episodes: &episodes,
            standing_id: next.standing_id.as_deref(),
        };
        let mut frame = vec![state::record(&next)?, crate::fact::row(&f, None, None)?];
        frame.extend(self.obs_moves(prop, option)?);
        self.people_write(&frame).await?;
        self.rec(None).announce(&f);
        Ok(next)
    }

    /// The records that move the observations `prop` rests on once it is
    /// answered with `option` (theseus-6f8t0): they were `used` when it was
    /// made, and stay so for an add that was kept or edited and a record that
    /// was edited or ended; a Not this, an expiry and a Keep of a Stale
    /// question set them aside (`dropped`), so none is asked again.
    fn obs_moves(&self, prop: &CardProposal, option: &str) -> Result<Vec<NewRecord>> {
        let keeps = match (prop.ask, option) {
            (CardAsk::Stale, "keep") => false,
            (_, "keep" | "edit" | "end" | "scope") => true,
            _ => false,
        };
        let mut out = Vec::new();
        if keeps {
            return Ok(out);
        }
        for id in &prop.observations {
            let held = theseus_store::blocking(|| {
                self.store.get_meta::<CardObservation>(&state::obs_key(id))
            })?;
            if let Some(o) = held.filter(|o| {
                o.state == ObservationState::Used && o.proposal.as_deref() == Some(&prop.id)
            }) {
                let to = super::live::moved(&o, ObservationState::Dropped, Some(&prop.id));
                out.push(state::obs_record(&to)?);
            }
        }
        Ok(out)
    }

    /// The card is done (or not): the nightly run stops (or resumes).
    pub(crate) async fn card_set_done(&self, done: bool) -> Result<()> {
        let mut st = theseus_store::blocking(|| State::read(&self.store))?;
        st.done = done;
        self.people_write(&[st.record()?]).await
    }

    /// Open proposals past their expiry become "Not this" (by `expiry`), each
    /// with its row, in one frame. How many.
    pub(crate) async fn card_expire(&self, now: u64) -> Result<usize> {
        let all = theseus_store::blocking(|| state::proposals(&self.store))?;
        let due: Vec<&CardProposal> = state::in_state(&all, ProposalState::Open)
            .filter(|p| p.expires_ms <= now)
            .collect();
        if due.is_empty() {
            return Ok(0);
        }
        let episodes: Vec<Vec<String>> = due
            .iter()
            .map(|p| p.sources.iter().map(|s| s.episode.clone()).collect())
            .collect();
        let facts: Vec<CardAnswered<'_>> = due
            .iter()
            .zip(&episodes)
            .map(|(p, e)| CardAnswered {
                id: &p.id,
                option: "expire",
                by: "expiry",
                via: "clock",
                episodes: e,
                standing_id: None,
            })
            .collect();
        let mut frame = Vec::new();
        for p in &due {
            let mut next = (*p).clone();
            next.state = ProposalState::Declined;
            next.answered_ms = Some(now);
            next.answered_by = Some("expiry".into());
            frame.push(state::record(&next)?);
        }
        for p in &due {
            frame.extend(self.obs_moves(p, "expire")?);
        }
        for f in &facts {
            frame.push(crate::fact::row(f, None, None)?);
        }
        self.people_write(&frame).await?;
        for f in &facts {
            self.rec(None).announce(f);
        }
        Ok(due.len())
    }
}
