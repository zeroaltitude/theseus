//! `[people]` (theseus-wy7y): proposing people from a session's text. A
//! generative profile extracts the candidates (`extract_profile`), Jev judges
//! each (`people.v1`), and code decides with these bands. Every key has a
//! default, so an owner's config names none of them unless it differs.
//!
//! - `extract_profile`: the profile whose model reads a session's text and
//!   returns its candidate people through a tool schema (`haiku`).
//! - `not_people`: names that are never proposed, beside the owner, the
//!   personas, the agents the store knows and the house's names (folded:
//!   case, spaces and a leading "@"): the names the store cannot know, the
//!   owner's full name, say (theseus-0p1r).
//! - `act`, `confirm`: a candidate whose least probability (real, involved,
//!   and its match) reaches `act` is proposed in the act band, one at
//!   `confirm` in the confirm band, and one under `confirm` is dropped.
//! - `live`: `people.v1` at a private conversation's exchange end (on).
//! - `gate`: live, the owner's "combine, gated by Jev" (theseus-u5n8): at
//!   each due exchange end Jev alone first (`people_seen.v1`: which held
//!   people it involves, and whether a person not held is); only when that
//!   last Noul reaches `gate` does the extractor read the exchange (0.6).

use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PeopleConfig {
    #[serde(default = "default_extract_profile")]
    pub extract_profile: String,
    #[serde(default)]
    pub not_people: Vec<String>,
    #[serde(default = "default_act")]
    pub act: f64,
    #[serde(default = "default_confirm")]
    pub confirm: f64,
    #[serde(default = "default_live")]
    pub live: bool,
    #[serde(default = "default_gate")]
    pub gate: f64,
}

fn default_extract_profile() -> String {
    "haiku".into()
}
fn default_act() -> f64 {
    0.9
}
fn default_confirm() -> f64 {
    0.6
}
fn default_live() -> bool {
    true
}
fn default_gate() -> f64 {
    0.6
}

impl Default for PeopleConfig {
    fn default() -> Self {
        Self {
            extract_profile: default_extract_profile(),
            not_people: Vec::new(),
            act: default_act(),
            confirm: default_confirm(),
            live: default_live(),
            gate: default_gate(),
        }
    }
}

impl PeopleConfig {
    pub fn is_default(&self) -> bool {
        *self == Self::default()
    }

    pub(crate) fn validate(&self) -> Result<()> {
        if !(0.5 < self.confirm && self.confirm <= self.act && self.act <= 1.0) {
            bail!(
                "[people] needs 0.5 < confirm <= act <= 1 (confirm = {}, act = {})",
                self.confirm,
                self.act
            );
        }
        if !(0.0 < self.gate && self.gate <= 1.0) {
            bail!("[people] needs 0 < gate <= 1 (gate = {})", self.gate);
        }
        if self.extract_profile.trim().is_empty() {
            bail!("[people] extract_profile names no profile");
        }
        Ok(())
    }
}
