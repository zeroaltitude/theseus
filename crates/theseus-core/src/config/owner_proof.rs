//! `[owner_proof]` (issue 22): the owner's second proof, a code from their
//! authenticator app, for the settings that matter most. Only the length of
//! the window a good code opens is a setting; the secret is held by the
//! daemon in a file of its own, never in this file.

use serde::{Deserialize, Serialize};

/// The longest a good code may cover the place it came from: a day.
pub const MAX_WINDOW_HOURS: u32 = 24;

/// `[owner_proof]`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OwnerProofConfig {
    /// How many hours a good code covers the place it came from: a question
    /// that needs the proof, answered from that same place inside the window,
    /// goes through on the card's yes alone. 0 asks for a code every time;
    /// at most 24.
    #[serde(default = "window_hours")]
    pub window_hours: u32,
}

fn window_hours() -> u32 {
    MAX_WINDOW_HOURS
}

impl Default for OwnerProofConfig {
    fn default() -> Self {
        Self {
            window_hours: window_hours(),
        }
    }
}

impl OwnerProofConfig {
    /// Whether the window is within a day.
    pub fn validate(&self) -> anyhow::Result<()> {
        if self.window_hours > MAX_WINDOW_HOURS {
            anyhow::bail!(
                "owner_proof.window_hours = {} is more than {MAX_WINDOW_HOURS}: a good code covers a place for a day at most",
                self.window_hours
            );
        }
        Ok(())
    }
}
