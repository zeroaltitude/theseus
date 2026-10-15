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
//! - **Not this** is the same key written again as declined: the sources are
//!   seen for good, and the statement's words are never proposed again.
//! - **Expiry** is applied when the card is read or the job runs
//!   (`card_expire`): an open proposal past its `expires_ms` becomes declined,
//!   by `expiry`, in one frame with its row. No timer.

use anyhow::{anyhow, bail, Result};
use theseus_protocol::{CardListResult, CardProposal, ProposalState, CARD_LIST_ANSWERED};

use super::state::{self, State};
use crate::fact::card::CardAnswered;
use crate::judge::spend;
use crate::rpc::Core;

/// An option's word, any case: `keep`, `edit`, `decline`.
pub fn option_of(s: &str) -> Option<&'static str> {
    match s.trim().to_lowercase().replace(['_', '-'], " ").as_str() {
        "keep" | "k" | "yes" | "1" => Some("keep"),
        "edit" | "e" | "2" => Some("edit"),
        "decline" | "not this" | "no" | "drop" | "3" => Some("decline"),
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

    /// Write `prop`'s answer: the proposal as `state` with the words kept (when
    /// they were edited) and the standing record's id, and its row.
    pub(crate) async fn card_settle(
        &self,
        prop: &CardProposal,
        option: &str,
        kept: Option<(String, String)>,
        by: &str,
        via: &str,
    ) -> Result<CardProposal> {
        let mut next = prop.clone();
        next.answered_ms = Some(theseus_protocol::now_unix_ms());
        next.answered_by = Some("owner".into());
        match kept {
            Some((text, standing_id)) => {
                if text != prop.text {
                    next.kept_text = Some(text);
                }
                next.standing_id = Some(standing_id);
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
        let frame = [state::record(&next)?, crate::fact::row(&f, None, None)?];
        self.people_write(&frame).await?;
        self.rec(None).announce(&f);
        Ok(next)
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
