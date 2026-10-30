//! `[ontology]` (theseus-xzxqv): Jev files each message's session into the
//! ontology. `onto.v1` rides the inbound request, and its answers act in
//! three bands: from `act` a membership is written (and a new category
//! minted), from `confirm` the owner is asked, below it the answer is only a
//! record. Every key has a default, so an owner's config names none unless it
//! differs.
//!
//! - `judge`: the switch, and the rollback. Off, `onto.v1` is not asked: the
//!   inbound request is byte for byte what it was, and nothing is written.
//! - `act`, `confirm`: the bands (0.80, 0.55).
//! - `name_profile`: the profile whose model names a new category when Jev
//!   says `new` and no mined name fits (`haiku`), off the turn's path, at
//!   most once a session, kind and day.
//! - `request`: `"batch"` rides the inbound batch (`classify.v4`, `role.v1`);
//!   `"own"` sends `onto.v1` in a request of its own that no turn waits on,
//!   the fallback when the batch's latency grows.
//! - `recall_boost`: how much a note from a session that shares a category
//!   with the asking one gains in recall's rank (0.25; 0 is off), at most
//!   this times the kind's weight, by reciprocal rank among the sharers.
//! - `private_mark`: whether a judged-private picture or message marks its
//!   node private (theseus-und4g). Off: the judgment is a record and nothing
//!   is marked. A policy effect, so it ships off until the owner says on.
//! - `sweep`: the nightly sweep (theseus-63tfk): a machine's membership
//!   unconfirmed for 30 days decays, and a duplicate category a machine wrote
//!   merges into its twin, each an undoable write. No model, no spend; off
//!   is the rollback.
//! - `sweep_usd_per_day`: what the sweep's model calls may spend a local day
//!   (theseus-nzzz4, 1.0): `same_thing.v1` asks of each pair of categories
//!   that look alike whether they are one, `two_things.v1` of each large
//!   category whether it holds two, `name_it.v1` of a stale category or a
//!   minted kind which mined name names it, and `same_kind.v1` of two mined
//!   names whether they are one kind; a call whose worst case would pass what
//!   is left of the day's cap is not begun. 0 turns the model's part off and
//!   the deterministic sweep still runs: that is its rollback.
//! - `dream_guidance`: after the nightly consolidation, a category a machine
//!   wrote that five private sessions hold and that has no guidance gets its
//!   first, written by the dream (theseus-olt3h; `consolidate/guidance.rs`),
//!   under `[memory] synth_limit_usd_per_day`. On by default; off is its
//!   rollback (theseus-23wd8): nothing is read, called or written.
//! - `not_topics`: topic names Jev never proposes (theseus-648f8), as
//!   `[people] not_people` names people never proposed. Matched on the
//!   proposed topic's name, case and spacing ignored. The owner's `Never`
//!   buttons add to the same set without touching this file (a stored
//!   record, `rpc/never.rs`); this is the part kept in the config.

use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OntologyConfig {
    #[serde(default = "default_judge")]
    pub judge: bool,
    #[serde(default = "default_act")]
    pub act: f64,
    #[serde(default = "default_confirm")]
    pub confirm: f64,
    #[serde(default)]
    pub name_profile: Option<String>,
    #[serde(default)]
    pub request: Request,
    #[serde(default = "default_recall_boost")]
    pub recall_boost: f64,
    #[serde(default)]
    pub private_mark: bool,
    #[serde(default = "default_sweep")]
    pub sweep: bool,
    #[serde(default = "default_sweep_usd_per_day")]
    pub sweep_usd_per_day: f64,
    #[serde(default = "default_dream_guidance")]
    pub dream_guidance: bool,
    #[serde(default)]
    pub not_topics: Vec<String>,
}

/// Which request carries `onto.v1`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Request {
    /// The inbound batch, beside `classify.v4` and `role.v1`.
    #[default]
    Batch,
    /// A request of its own, never waited on.
    Own,
}

fn default_judge() -> bool {
    true
}
fn default_sweep() -> bool {
    true
}
fn default_dream_guidance() -> bool {
    true
}
fn default_sweep_usd_per_day() -> f64 {
    1.0
}
fn default_act() -> f64 {
    0.80
}
fn default_confirm() -> f64 {
    0.55
}
fn default_recall_boost() -> f64 {
    0.25
}

impl Default for OntologyConfig {
    fn default() -> Self {
        Self {
            judge: default_judge(),
            act: default_act(),
            confirm: default_confirm(),
            name_profile: None,
            request: Request::default(),
            recall_boost: default_recall_boost(),
            private_mark: false,
            sweep: default_sweep(),
            sweep_usd_per_day: default_sweep_usd_per_day(),
            dream_guidance: default_dream_guidance(),
            not_topics: Vec::new(),
        }
    }
}

impl OntologyConfig {
    pub fn is_default(&self) -> bool {
        *self == Self::default()
    }

    pub(crate) fn validate(&self) -> Result<()> {
        if !(self.confirm > 0.0 && self.confirm <= self.act && self.act <= 1.0) {
            bail!(
                "[ontology] needs 0 < confirm <= act <= 1, not confirm {} and act {}",
                self.confirm,
                self.act
            );
        }
        if !(0.0..=1.0).contains(&self.recall_boost) {
            bail!(
                "[ontology] recall_boost is a gain from 0 (off) to 1, not {}",
                self.recall_boost
            );
        }
        if !(self.sweep_usd_per_day.is_finite() && self.sweep_usd_per_day >= 0.0) {
            bail!(
                "[ontology] sweep_usd_per_day is dollars a day, 0 or more (0 turns the model's \
                 part of the sweep off), not {}",
                self.sweep_usd_per_day
            );
        }
        if self
            .name_profile
            .as_deref()
            .is_some_and(|p| p.trim().is_empty())
        {
            bail!("[ontology] name_profile names no profile");
        }
        Ok(())
    }
}
