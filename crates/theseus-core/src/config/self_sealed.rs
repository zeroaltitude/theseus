//! `[self.sealed]` (theseus-pw1q.5, theseus-pw1q.21.5): the sealed holdout sets
//! (`crate::rsi::sealed`) and how a candidate is scored against them. Part of
//! the hard keel, with `[self.budget]`.
//!
//! - `dir`: where the sets live, out of Theseus's reach. Unset (the default)
//!   means no sealed set. It must be an absolute path outside every workspace
//!   root, the state dir, the repository and every `[sandbox] ro_paths` entry
//!   (neither inside one nor holding one), refused at load by the key's name
//!   (`SelfImproveConfig::validate`). It was `[self] sealed_dir`.
//! - `every_branch` (false): the self-join scores every behaviour-path branch
//!   against the sealed sets (theseus-pw1q.21.7 reads it).
//! - `item_usd` (0.25): what one item's run may spend, through the model relay
//!   (`rsi::sealed::relay`): a call that would pass it is refused.
//! - `drift_every_days` (7): how often main is scored again to see the
//!   candidate-free drift (theseus-pw1q.21.7 reads it).

use serde::{Deserialize, Serialize};

/// `[self.sealed]`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SelfSealedConfig {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dir: Option<String>,
    #[serde(default)]
    pub every_branch: bool,
    #[serde(default = "default_item_usd")]
    pub item_usd: f64,
    #[serde(default = "default_drift_every_days")]
    pub drift_every_days: u32,
}

fn default_item_usd() -> f64 {
    0.25
}

fn default_drift_every_days() -> u32 {
    7
}

impl Default for SelfSealedConfig {
    fn default() -> Self {
        Self {
            dir: None,
            every_branch: false,
            item_usd: default_item_usd(),
            drift_every_days: default_drift_every_days(),
        }
    }
}
