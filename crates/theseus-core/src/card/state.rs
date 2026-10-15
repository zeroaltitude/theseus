//! What the card keeps in the store (theseus-slvl.6): the proposals, the
//! episodes the job has already sent a model, and the job's own state. All
//! are META records, so no store format changes; a proposal's answer is the
//! same key written again (the latest wins), in the frame of its row.

use std::collections::HashSet;

use anyhow::Result;
use serde::{Deserialize, Serialize};
use theseus_protocol::{CardProposal, ProposalState, CARD_PROPOSAL_PREFIX};
use theseus_store::{kinds, NewRecord, Store as _};

use crate::store::Store;

/// The job's state, one META record.
pub const STATE_KEY: &str = "card.state";
/// The episodes one call was sent, keyed by the call.
pub const SENT_PREFIX: &str = "card.sent.";

/// The job's own state: when it last ran, whether the owner said done, and
/// what it has spent.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct State {
    #[serde(default)]
    pub last_run_ms: Option<u64>,
    #[serde(default)]
    pub done: bool,
    /// The local day `day_usd` counts.
    #[serde(default)]
    pub day: String,
    #[serde(default)]
    pub day_usd: f64,
    #[serde(default)]
    pub total_usd: f64,
}

impl State {
    pub fn read(store: &Store) -> Result<State> {
        Ok(store.get_meta::<State>(STATE_KEY)?.unwrap_or_default())
    }

    pub fn record(&self) -> Result<NewRecord> {
        NewRecord::json(kinds::META, Some(STATE_KEY), self)
    }

    /// What `day` has spent so far.
    pub fn spent_on(&self, day: &str) -> f64 {
        if self.day == day {
            self.day_usd
        } else {
            0.0
        }
    }

    /// Add `usd` to `day`'s spend and the total.
    pub fn spend(&mut self, day: &str, usd: f64) {
        if self.day != day {
            self.day = day.to_string();
            self.day_usd = 0.0;
        }
        self.day_usd += usd;
        self.total_usd += usd;
    }
}

pub fn key(id: &str) -> String {
    format!("{CARD_PROPOSAL_PREFIX}{id}")
}

pub fn record(p: &CardProposal) -> Result<NewRecord> {
    NewRecord::json(kinds::META, Some(&key(&p.id)), p)
}

/// Every proposal, oldest first. A record that does not read is left out and
/// warned of.
pub fn proposals(store: &Store) -> Result<Vec<CardProposal>> {
    let mut out = Vec::new();
    for r in store
        .inner()
        .latest_with_prefix(kinds::META, CARD_PROPOSAL_PREFIX)?
    {
        match r.decode::<CardProposal>() {
            Ok(p) => out.push(p),
            Err(e) => {
                tracing::warn!(key = ?r.key, error = %e, "a card proposal does not read: left out")
            }
        }
    }
    out.sort_by(|a, b| (a.created_ms, &a.id).cmp(&(b.created_ms, &b.id)));
    Ok(out)
}

/// The record of the episodes a call was sent.
pub fn sent_record(call: &str, episodes: &[String]) -> Result<NewRecord> {
    NewRecord::json(
        kinds::META,
        Some(&format!("{SENT_PREFIX}{call}")),
        &episodes,
    )
}

/// Every episode a proposal rests on, and every one a call was sent: what the
/// job has seen. A declined proposal's sources are in it for good.
pub fn seen(store: &Store, all: &[CardProposal]) -> Result<HashSet<String>> {
    let mut out: HashSet<String> = all
        .iter()
        .flat_map(|p| p.sources.iter().map(|s| s.episode.clone()))
        .collect();
    for r in store.inner().latest_with_prefix(kinds::META, SENT_PREFIX)? {
        if let Ok(v) = r.decode::<Vec<String>>() {
            out.extend(v);
        }
    }
    Ok(out)
}

/// The proposals in `state`.
pub fn in_state(all: &[CardProposal], state: ProposalState) -> impl Iterator<Item = &CardProposal> {
    all.iter().filter(move |p| p.state == state)
}

/// A proposal by its id or the end of one (four characters at least) that no
/// other shares.
pub fn resolve<'a>(all: &'a [CardProposal], id: &str) -> Result<&'a CardProposal, String> {
    let id = id.trim();
    if let Some(p) = all.iter().find(|p| p.id == id) {
        return Ok(p);
    }
    if id.len() < 4 {
        return Err(format!(
            "{id:?} names no proposal (give the whole id, or the last four characters at least)"
        ));
    }
    let mut hits = all.iter().filter(|p| p.id.ends_with(id));
    match (hits.next(), hits.next()) {
        (Some(p), None) => Ok(p),
        (Some(_), Some(_)) => Err(format!(
            "{id:?} names more than one proposal; give more of its id"
        )),
        _ => Err(format!("no proposal {id:?}")),
    }
}
