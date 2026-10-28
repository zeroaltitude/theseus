//! `[content_based_security]` (theseus-gyjyo.2; it was `[content]`, theseus-9ymz0): what the content classification
//! does once it has found something, and the switch for models of restricted origin. The rule itself is
//! `crate::content`; the ask is `judge/content.rs`. Every key has a default, so an owner's config names none unless
//! it differs. The old table is a hard error that names each key's new home (`renamed.rs`).
//!
//! - `session_flags`: the whole mechanism, on by default (it was `[content] enabled`). Off, nothing is asked and no
//!   label is ever raised. A model of restricted origin then has no labels to follow, so `cn_models` must say what
//!   it does without them (`never` or `always`); `follow_flags` with the flags off refuses to start.
//! - `cn_models`: `always`, `follow_flags` (the default) or `never`: whether a request to a model of restricted
//!   origin is sent whatever the session carries, only when its labels allow it, or never. Read by the provider
//!   chokepoint (`data_gate.rs`, which reads the session's record and nothing else) and by the route
//!   (`turn/data_step.rs`, which skips a profile `never` refuses).
//! - `injection`: what an `injection` label does to a call that acts — `refuse` (the default) or `notify`. **The
//!   bench daemon sets `notify`**, so a trial never freezes waiting for the operator to tap a button. The design
//!   document calls the first one `freeze`; the spelling in an owner's config is `refuse`, and stays.
//! - `wait_ms`: how long a turn bound for a model of restricted origin waits for the content asks still out for its
//!   session to land (`content_wait`, a counter of them). It then goes either way on the labels it reads: it is never
//!   kept off a model for an ask that did not land. A Claude turn never waits, whatever this says. Zero means it
//!   never waits.
//! - `max_pieces`: the bound on how many pieces one long item is split into (`content::split`). Set so the largest
//!   item that can exist — an `fs.read` at `FS_READ_MAX_CHARS`, 100,000 characters, which is 17 pieces of the pack's
//!   6,000-character cap — fits whole, with room to spare. Past the bound an item is read from the start and its row
//!   says `truncated`.
//! - `context`: the operator's one paragraph on what the assistant is and which of its work is public, handed to the
//!   pack verbatim. **Parsed and exposed only**: the pack is fed it by a later row (R9).

use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};

use crate::content::InjectionMode;

/// What a request to a model of restricted origin (`cn` or `unknown`) is sent under.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CnModels {
    /// Sent whatever the session carries.
    Always,
    /// Refused when the session carries a disqualifying label, else sent.
    #[default]
    FollowFlags,
    /// Never sent.
    Never,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ContentBasedSecurityConfig {
    #[serde(default = "on")]
    pub session_flags: bool,
    #[serde(default)]
    pub cn_models: CnModels,
    #[serde(default)]
    pub injection: InjectionMode,
    #[serde(default = "default_wait_ms")]
    pub wait_ms: u64,
    #[serde(default = "default_max_pieces")]
    pub max_pieces: usize,
    #[serde(default)]
    pub context: String,
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

impl Default for ContentBasedSecurityConfig {
    fn default() -> Self {
        Self {
            session_flags: on(),
            cn_models: CnModels::default(),
            injection: InjectionMode::default(),
            wait_ms: default_wait_ms(),
            max_pieces: default_max_pieces(),
            context: String::new(),
        }
    }
}

impl ContentBasedSecurityConfig {
    pub fn is_default(&self) -> bool {
        *self == Self::default()
    }

    /// With the flags off nothing is judged, so `follow_flags` would have no labels to read: the old behaviour was to
    /// refuse every request of restricted origin, the opposite of what an off switch says. Say what to write instead.
    pub fn validate(&self) -> Result<()> {
        if !self.session_flags && self.cn_models == CnModels::FollowFlags {
            bail!(
                "content_based_security.session_flags = false with cn_models = \"follow_flags\": with nothing \
                 judged there are no labels to follow. Write cn_models = \"never\" to keep models of restricted \
                 origin off, or cn_models = \"always\" to send to them without labels"
            );
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The defaults are on, follow the flags, refuse, a four-second wait and twenty pieces; each key parses on its
    /// own; and an unknown key is refused rather than ignored.
    #[test]
    fn the_defaults_are_on_and_refusing() {
        let d = ContentBasedSecurityConfig::default();
        assert!(d.session_flags);
        assert_eq!(d.cn_models, CnModels::FollowFlags);
        assert_eq!(d.injection, InjectionMode::Refuse);
        assert_eq!(d.wait_ms, 4_000);
        assert_eq!(d.max_pieces, 20);
        assert_eq!(d.context, "");
        assert!(d.is_default());
        let c: ContentBasedSecurityConfig = toml::from_str("").unwrap();
        assert_eq!(c, d, "an empty table is the default");

        // The bench daemon's setting.
        let c: ContentBasedSecurityConfig = toml::from_str("injection = \"notify\"").unwrap();
        assert_eq!(c.injection, InjectionMode::Notify);
        assert!(c.session_flags, "notify turns nothing else off");
        assert!(!c.is_default());

        let c: ContentBasedSecurityConfig = toml::from_str("wait_ms = 0").unwrap();
        assert_eq!(c.wait_ms, 0, "zero means a cn request never waits");
        let c: ContentBasedSecurityConfig = toml::from_str("session_flags = false").unwrap();
        assert!(!c.session_flags);
        let c: ContentBasedSecurityConfig = toml::from_str("max_pieces = 2").unwrap();
        assert_eq!(c.max_pieces, 2);
        let c: ContentBasedSecurityConfig = toml::from_str("context = \"Public: ~/p.\"").unwrap();
        assert_eq!(c.context, "Public: ~/p.");
        for (word, want) in [
            ("always", CnModels::Always),
            ("follow_flags", CnModels::FollowFlags),
            ("never", CnModels::Never),
        ] {
            let c: ContentBasedSecurityConfig =
                toml::from_str(&format!("cn_models = \"{word}\"")).unwrap();
            assert_eq!(c.cn_models, want);
        }

        let e = toml::from_str::<ContentBasedSecurityConfig>("inject = \"notify\"")
            .unwrap_err()
            .to_string();
        assert!(e.contains("inject"), "{e}");
        let e = toml::from_str::<ContentBasedSecurityConfig>("injection = \"allow\"")
            .unwrap_err()
            .to_string();
        assert!(e.contains("allow"), "{e}");
        let e = toml::from_str::<ContentBasedSecurityConfig>("enabled = true")
            .unwrap_err()
            .to_string();
        assert!(e.contains("enabled"), "{e}");
    }

    /// Only the flags off with `follow_flags` refuses, and the message names the two ways out.
    #[test]
    fn only_flags_off_with_follow_flags_is_refused() {
        for flags in [true, false] {
            for cn in [CnModels::Always, CnModels::FollowFlags, CnModels::Never] {
                let c = ContentBasedSecurityConfig {
                    session_flags: flags,
                    cn_models: cn,
                    ..Default::default()
                };
                let r = c.validate();
                assert_eq!(
                    r.is_err(),
                    !flags && cn == CnModels::FollowFlags,
                    "{flags} {cn:?}: {r:?}"
                );
                if let Err(e) = r {
                    let e = e.to_string();
                    assert!(e.contains("never") && e.contains("always"), "{e}");
                }
            }
        }
    }
}
