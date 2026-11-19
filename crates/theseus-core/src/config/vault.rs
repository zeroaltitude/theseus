//! `[vault]` (issue 29): where the owner may give a secret's value to be kept in the
//! vault. The terminal and the cockpit are always on; the only place that can be
//! turned off is the form in the owner's Discord direct messages, because the text
//! typed there passes through the chat service's servers on its way to Theseus.

use serde::{Deserialize, Serialize};

/// `[vault]`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VaultConfig {
    /// A keep card in the owner's Discord direct messages has a button that opens a
    /// form for the value (the owner's yes of 2026-11-19). Off: the card is the
    /// terminal's and the cockpit's alone, with a Decline and nothing to type into,
    /// and a value sent from Discord is refused.
    #[serde(default = "yes")]
    pub keep_by_discord: bool,
}

fn yes() -> bool {
    true
}

impl Default for VaultConfig {
    fn default() -> Self {
        Self {
            keep_by_discord: yes(),
        }
    }
}

impl VaultConfig {
    pub fn is_default(&self) -> bool {
        *self == Self::default()
    }
}
