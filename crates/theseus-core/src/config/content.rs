//! `[content]` (theseus-9ymz0): what the content classification does once it
//! has found something. The rule itself is `crate::content`; the ask is
//! `judge/content.rs`. Every key has a default, so an owner's config names none
//! unless it differs.
//!
//! - `injection`: what an `injection` label does to a call that acts —
//!   `refuse` (the default) or `notify`. **The bench daemon sets `notify`**, so
//!   a trial never freezes waiting for the operator to tap a button.
//! - `wait_ms`: how long a turn bound for a model of restricted origin waits
//!   for an item's verdict before it goes to Claude instead. A Claude turn
//!   never waits, whatever this says. Zero means it never waits, so an
//!   unjudged item sends that one request to Claude at once.
//! - `max_pieces`: the bound on how many pieces one long item is split into
//!   (`content::split`). Set so the largest item that can exist — an `fs.read`
//!   at `FS_READ_MAX_CHARS`, 100,000 characters, which is 17 pieces of the
//!   pack's 6,000-character cap — fits whole, with room to spare. Past the
//!   bound an item is read from the start and its row says `truncated`.
//! - `enabled`: the whole mechanism, on by default. Off, nothing is asked, no
//!   label is ever raised, and no session is frozen — but **a model of
//!   restricted origin is then refused every request**, because nothing has
//!   judged what the session carries. Off is "no China-origin models", not "no
//!   checking"; the sensitive-names tripwire is unaffected either way.

use serde::{Deserialize, Serialize};

use crate::content::InjectionMode;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ContentConfig {
    #[serde(default = "on")]
    pub enabled: bool,
    #[serde(default)]
    pub injection: InjectionMode,
    #[serde(default = "default_wait_ms")]
    pub wait_ms: u64,
    #[serde(default = "default_max_pieces")]
    pub max_pieces: usize,
}

fn on() -> bool {
    true
}

/// Long enough for a Jev call made the moment the item landed to be read by the
/// turn that follows it, short enough that a session waiting on it does not
/// feel stalled. Provisional, like the bands: it moves when the live numbers
/// say so.
fn default_wait_ms() -> u64 {
    4_000
}

/// The pack's state cap is 3,000 tokens, which the builder takes as
/// `PIECE_CHARS` = 6,000 characters a piece. The largest item that can exist is
/// an `fs.read` at `FS_READ_MAX_CHARS` (100,000 characters), which is 17
/// pieces; 20 fits it whole with room to spare, and costs about half a
/// thousandth of a dollar for such a read.
///
/// **This was 6 until the review of 2026-10-26**, which did the arithmetic I
/// had not: 6 pieces is 36 KB, not the "about 72 KB" three documents claimed,
/// so an ordinary 60 KB file was classified a third of the way through and the
/// session stayed eligible for a China-origin model.
fn default_max_pieces() -> usize {
    20
}

impl Default for ContentConfig {
    fn default() -> Self {
        Self {
            enabled: on(),
            injection: InjectionMode::default(),
            wait_ms: default_wait_ms(),
            max_pieces: default_max_pieces(),
        }
    }
}

impl ContentConfig {
    pub fn is_default(&self) -> bool {
        *self == Self::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The defaults are on, refuse, and a four-second wait; each key parses on
    /// its own; and an unknown key is refused rather than ignored.
    #[test]
    fn the_defaults_are_on_and_refusing() {
        let d = ContentConfig::default();
        assert!(d.enabled);
        assert_eq!(d.injection, InjectionMode::Refuse);
        assert_eq!(d.wait_ms, 4_000);
        assert_eq!(d.max_pieces, 20);
        assert!(d.is_default());
        let c: ContentConfig = toml::from_str("").unwrap();
        assert_eq!(c, d, "an empty table is the default");

        // The bench daemon's setting.
        let c: ContentConfig = toml::from_str("injection = \"notify\"").unwrap();
        assert_eq!(c.injection, InjectionMode::Notify);
        assert!(c.enabled, "notify turns nothing else off");
        assert!(!c.is_default());

        let c: ContentConfig = toml::from_str("wait_ms = 0").unwrap();
        assert_eq!(c.wait_ms, 0, "zero means a cn request never waits");
        let c: ContentConfig = toml::from_str("enabled = false").unwrap();
        assert!(!c.enabled);
        let c: ContentConfig = toml::from_str("max_pieces = 2").unwrap();
        assert_eq!(c.max_pieces, 2);

        let e = toml::from_str::<ContentConfig>("inject = \"notify\"")
            .unwrap_err()
            .to_string();
        assert!(e.contains("inject"), "{e}");
        let e = toml::from_str::<ContentConfig>("injection = \"allow\"")
            .unwrap_err()
            .to_string();
        assert!(e.contains("allow"), "{e}");
    }
}
