//! `[people]` (theseus-wy7y): proposing people from a session's text. A
//! generative profile extracts the candidates (`extract_profile`), Jev judges
//! each (`people.v1`), and code decides with these bands. Every key has a
//! default, so an owner's config names none of them unless it differs.
//!
//! - `extract_profile`: the profile whose model reads a session's text and
//!   returns its candidate people through a tool schema (`haiku`).
//! - `not_people`: names that are never proposed, beside the owner, the
//!   personas and the agents the sessions name (folded: case and spaces).
//! - `act`, `confirm`: a candidate whose least probability (real, involved,
//!   and its match) reaches `act` is proposed in the act band, one at
//!   `confirm` in the confirm band, and one under `confirm` is dropped.
//! - `live`: `people.v1` at a private conversation's exchange end (on).

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

impl Default for PeopleConfig {
    fn default() -> Self {
        Self {
            extract_profile: default_extract_profile(),
            not_people: Vec::new(),
            act: default_act(),
            confirm: default_confirm(),
            live: default_live(),
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
        if self.extract_profile.trim().is_empty() {
            bail!("[people] extract_profile names no profile");
        }
        Ok(())
    }
}
