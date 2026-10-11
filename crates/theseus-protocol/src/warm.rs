//! A person's first keystroke (theseus-tnky): `session.typing`, the one cheap
//! notice every surface sends so the daemon warms what the message will
//! wait for, and health's `warm` block, which says when it last did.

use serde::{Deserialize, Serialize};

use crate::DiscordOrigin;

/// A session's idle spell, in seconds (theseus-tnky): a keystroke warms once,
/// and the next one for the same session warms again only after this long.
/// A client keeps the same rule, so a person typing for a minute sends one
/// notice, not one a key; the daemon keeps it too, for the clients that do
/// not. Well inside the provider connection's 300 s of idle time.
pub const SPELL_SECS: u64 = 120;

/// A client's memory of whom it has told (theseus-tnky): one notice per
/// session per idle spell, however many keys. Pure: the caller passes the
/// time, as the notices policy's clients do.
#[derive(Debug, Clone, Default)]
pub struct Typist {
    told: std::collections::HashMap<String, u64>,
}

impl Typist {
    /// A key into `session` at `now_ms`: true when the notice should go (the
    /// first in this session's spell, which it opens).
    pub fn keystroke(&mut self, session: &str, now_ms: u64) -> bool {
        let spell = SPELL_SECS * 1000;
        if self
            .told
            .get(session)
            .is_some_and(|t| now_ms.saturating_sub(*t) < spell)
        {
            return false;
        }
        // A handful at most: one person types into one session at a time.
        self.told.retain(|_, t| now_ms.saturating_sub(*t) < spell);
        self.told.insert(session.to_string(), now_ms);
        true
    }
}

/// `session.typing`: someone began to type into `session_id` (none yet:
/// a new session's first words). Fire and forget: the answer says only
/// whether the daemon took the notice, and nothing on a turn's path waits
/// for what it starts.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct SessionTypingParams {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub session_id: Option<String>,
    /// Who is typing, as `turn.submit`'s `author` names them (a label).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub author: Option<String>,
    /// Where it came from, for the Discord binding: the daemon warms only
    /// for an owner's typing, never for another person in a shared place.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub discord: Option<DiscordOrigin>,
}

/// What the daemon did with the notice.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct SessionTypingResult {
    /// True when it started a warm-up: false when the session had one within
    /// the idle spell, or the typist is not one the daemon warms for.
    pub started: bool,
    /// Why not, in words, when it did not start.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub why: Option<String>,
}

/// Health's `warm` block: what the notices opened, and when.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(default)]
pub struct WarmHealth {
    /// Notices that started a warm-up since the daemon began.
    #[cfg_attr(test, ts(type = "number"))]
    pub started: u64,
    /// Notices dropped: inside a session's idle spell, or from someone the
    /// daemon does not warm for.
    #[cfg_attr(test, ts(type = "number"))]
    pub dropped: u64,
    /// The model provider's connection, last warmed.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub provider: Option<WarmStamp>,
    /// The index tender's embedding model, last warmed.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub tender: Option<WarmStamp>,
}

/// One warm-up: when, what it found, and how long it took.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(default)]
pub struct WarmStamp {
    /// Unix ms when it finished.
    #[cfg_attr(test, ts(type = "number"))]
    pub at_ms: u64,
    /// How long it took, in ms.
    #[cfg_attr(test, ts(type = "number"))]
    pub took_ms: u64,
    /// What it found: `opened` (a connection was made) or `warm` (one was
    /// open already) for the provider; the model's state (`loaded`,
    /// `loading`, …) for the tender; or `failed: why`.
    pub outcome: String,
}

impl WarmStamp {
    /// `connection opened 12 s ago (took 84 ms)`.
    pub fn line(&self, what: &str, now_ms: u64) -> String {
        let ago = now_ms.saturating_sub(self.at_ms) / 1000;
        format!(
            "{what} {} {ago} s ago (took {} ms)",
            self.outcome, self.took_ms
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_typist_tells_once_per_session_per_spell() {
        let mut t = Typist::default();
        assert!(t.keystroke("ses_a", 1_000));
        assert!(!t.keystroke("ses_a", 1_001));
        assert!(!t.keystroke("ses_a", 1_000 + SPELL_SECS * 1000 - 1));
        assert!(t.keystroke("ses_b", 2_000), "its own spell");
        assert!(
            t.keystroke("ses_a", 1_000 + SPELL_SECS * 1000),
            "the spell ended"
        );
        assert!(!t.keystroke("ses_a", 1_000 + SPELL_SECS * 1000 + 1));
    }
}
