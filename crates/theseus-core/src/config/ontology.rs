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
//! - `request`: `"batch"` rides the inbound batch (`classify.v3`, `role.v1`);
//!   `"own"` sends `onto.v1` in a request of its own that no turn waits on,
//!   the fallback when the batch's latency grows.
//! - `private_mark`: whether a judged-private picture or message marks its
//!   node private (theseus-und4g). Off: the judgment is a record and nothing
//!   is marked. A policy effect, so it ships off until the owner says on.

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
    #[serde(default = "default_name_profile")]
    pub name_profile: String,
    #[serde(default)]
    pub request: Request,
    #[serde(default)]
    pub private_mark: bool,
}

/// Which request carries `onto.v1`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Request {
    /// The inbound batch, beside `classify.v3` and `role.v1`.
    #[default]
    Batch,
    /// A request of its own, never waited on.
    Own,
}

fn default_judge() -> bool {
    true
}
fn default_act() -> f64 {
    0.80
}
fn default_confirm() -> f64 {
    0.55
}
fn default_name_profile() -> String {
    "haiku".into()
}

impl Default for OntologyConfig {
    fn default() -> Self {
        Self {
            judge: default_judge(),
            act: default_act(),
            confirm: default_confirm(),
            name_profile: default_name_profile(),
            request: Request::default(),
            private_mark: false,
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
        if self.name_profile.trim().is_empty() {
            bail!("[ontology] name_profile names no profile");
        }
        Ok(())
    }
}
