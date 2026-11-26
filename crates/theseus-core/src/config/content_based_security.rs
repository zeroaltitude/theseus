//! `[content_based_security]` (theseus-gyjyo.2; it was `[content]`, theseus-9ymz0): what the content classification
//! does once it has found something. The rule itself is `crate::content`; the ask is `judge/content.rs`. Every key
//! has a default, so an owner's config names none unless it differs. The old table is a hard error that names each
//! key's new home (`renamed.rs`).
//!
//! - `session_flags`: the whole mechanism, on by default (it was `[content] enabled`). Off, nothing is asked and no
//!   label is ever raised.
//! - `cn_models` and `wait_ms` are **retired** (2026-11-21): the rule that kept a session off a model of restricted
//!   origin, and the wait before a turn bound for one, were deleted on the owner's word. A config that still sets
//!   either loads, is told so once at startup ([`ContentBasedSecurityConfig::retired`]), and the key is ignored
//!   and never written back.
//! - `injection`: what an `injection` label does to a call that acts — `notify` (**the default since 1.0.6**) or
//!   `refuse`. Under `notify` the call runs and the operator is told: the label is raised and its ⚑ card posted,
//!   and nothing is refused. Under `refuse` every call but a local read of files and code is refused until the
//!   operator removes the label. The default is the owner's ruling of 2026-11-12 (S17-D8): L0 is unsandboxed and
//!   default-trusted, so "we return to the notify but execute situation", and Jev's classification of the content
//!   WARNS rather than latching the session. `refuse` is what the specialised security use writes, the one L1 is
//!   kept for, and the bench daemon's config writes it no longer — it gets the default. The design document calls
//!   that one `freeze`; the spelling in an owner's config is `refuse`, and stays.
//! - `outside`: what the looks, the settings read and the profile tools do when the conversation has
//!   read text from outside since the owner's last message, or carries the `injection` flag —
//!   `notify` (**the default**, the owner's word of 2026-11-25) or `ask`. Under `notify` the call
//!   runs and the owner is told once, in an FYI note; under `ask` it waits for his answer, which may
//!   also clear the hold. The vault's two tools ask first at both (`config_change::hold`).
//! - `max_pieces`: the bound on how many pieces one long item is split into (`content::split`). Set so the largest
//!   item that can exist — an `fs.read` at `FS_READ_MAX_CHARS`, 100,000 characters, which is 17 pieces of the pack's
//!   6,000-character cap — fits whole, with room to spare. Past the bound an item is read from the start and its row
//!   says `truncated`.
//! - `context`: the operator's one paragraph on what the assistant is and which of its work is public, handed to the
//!   pack verbatim. **Parsed and exposed only**: the pack is fed it by a later row (R9).
//! - `judge_replies`: whether Theseus's own replies are asked the four questions too, each kept on its own
//!   item and never on the session (issue 44). **On by default; the owner may turn it off.** A reply's ask is
//!   paid from the same judge day as an input's, and replies stop at `REPLY_SHARE_PERCENT` of the day's settled
//!   spend (`judge::content`): past it a reply is recorded `refused` on its item, and the inputs keep the rest.

use serde::{Deserialize, Serialize};

use crate::content::{InjectionMode, OutsideMode};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ContentBasedSecurityConfig {
    #[serde(default = "on")]
    pub session_flags: bool,
    /// Retired (2026-11-21): read, warned of ([`Self::retired`]), ignored and
    /// never written back.
    #[serde(default, skip_serializing)]
    cn_models: Option<toml::Value>,
    #[serde(default)]
    pub injection: InjectionMode,
    #[serde(default)]
    pub outside: OutsideMode,
    /// Retired with `cn_models`: read, warned of, ignored and never written
    /// back.
    #[serde(default, skip_serializing)]
    wait_ms: Option<toml::Value>,
    #[serde(default = "default_max_pieces")]
    pub max_pieces: usize,
    #[serde(default)]
    pub context: String,
    /// Whether Theseus's own replies are asked the content questions too (issue 44): **on by default; the owner
    /// may turn it off**. Their labels stay on their items alone either way.
    #[serde(default = "on")]
    pub judge_replies: bool,
}

fn on() -> bool {
    true
}

/// The pack's state cap is 3,000 tokens, which the builder takes as
/// `PIECE_CHARS` = 6,000 characters a piece. The largest item that can exist is
/// an `fs.read` at `FS_READ_MAX_CHARS` (100,000 characters), which is 17
/// pieces; 20 fits it whole with room to spare, and costs about half a
/// thousandth of a dollar for such a read.
///
/// **This was 6 until the review of 2026-10-26**, which did the arithmetic I
/// had not: 6 pieces is 36 KB, not the "about 72 KB" three documents claimed,
/// so an ordinary 60 KB file was classified a third of the way through.
fn default_max_pieces() -> usize {
    20
}

impl Default for ContentBasedSecurityConfig {
    fn default() -> Self {
        Self {
            session_flags: on(),
            cn_models: None,
            injection: InjectionMode::default(),
            outside: OutsideMode::default(),
            wait_ms: None,
            max_pieces: default_max_pieces(),
            context: String::new(),
            judge_replies: on(),
        }
    }
}

impl ContentBasedSecurityConfig {
    pub fn is_default(&self) -> bool {
        *self == Self::default()
    }

    /// The retired keys this section sets, by the name the config gives them:
    /// what [`crate::Config::parse`] warns of, once.
    pub fn retired(&self) -> Vec<String> {
        [
            ("cn_models", self.cn_models.is_some()),
            ("wait_ms", self.wait_ms.is_some()),
        ]
        .iter()
        .filter(|(_, set)| *set)
        .map(|(k, _)| format!("content_based_security.{k}"))
        .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The defaults are on, notify (the owner's S17-D8, 2026-11-12) and twenty
    /// pieces; each key parses on its own; and an unknown key is refused rather
    /// than ignored.
    #[test]
    fn the_defaults_are_on_and_notifying() {
        let d = ContentBasedSecurityConfig::default();
        assert!(d.session_flags);
        assert_eq!(d.injection, InjectionMode::Notify);
        assert_eq!(d.max_pieces, 20);
        assert_eq!(d.context, "");
        assert!(d.is_default());
        assert!(d.retired().is_empty());
        let c: ContentBasedSecurityConfig = toml::from_str("").unwrap();
        assert_eq!(c, d, "an empty table is the default");

        // The default written out is still the default.
        let c: ContentBasedSecurityConfig = toml::from_str("injection = \"notify\"").unwrap();
        assert_eq!(c.injection, InjectionMode::Notify);
        assert!(c.session_flags, "notify turns nothing else off");
        assert!(c.is_default());

        // The freeze is not dead: a config that writes `refuse` gets it.
        let c: ContentBasedSecurityConfig = toml::from_str("injection = \"refuse\"").unwrap();
        assert_eq!(c.injection, InjectionMode::Refuse);
        assert!(c.session_flags, "refuse turns nothing else off");
        assert!(!c.is_default());

        let c: ContentBasedSecurityConfig = toml::from_str("session_flags = false").unwrap();
        assert!(!c.session_flags);
        let c: ContentBasedSecurityConfig = toml::from_str("max_pieces = 2").unwrap();
        assert_eq!(c.max_pieces, 2);
        let c: ContentBasedSecurityConfig = toml::from_str("context = \"Public: ~/p.\"").unwrap();
        assert_eq!(c.context, "Public: ~/p.");

        let e = toml::from_str::<ContentBasedSecurityConfig>("inject = \"notify\"")
            .unwrap_err()
            .to_string();
        assert!(e.contains("inject"), "{e}");
        let e = toml::from_str::<ContentBasedSecurityConfig>("injection = \"allow\"")
            .unwrap_err()
            .to_string();
        assert!(e.contains("allow"), "{e}");
        // Outside text tells by default (2026-11-25); `ask` is written out, and no third word loads.
        assert_eq!(d.outside, OutsideMode::Notify);
        let c: ContentBasedSecurityConfig = toml::from_str("outside = \"notify\"").unwrap();
        assert!(c.is_default());
        let c: ContentBasedSecurityConfig = toml::from_str("outside = \"ask\"").unwrap();
        assert_eq!(c.outside, OutsideMode::Ask);
        assert!(!c.is_default());
        let e = toml::from_str::<ContentBasedSecurityConfig>("outside = \"refuse\"")
            .unwrap_err()
            .to_string();
        assert!(e.contains("refuse"), "{e}");

        let e = toml::from_str::<ContentBasedSecurityConfig>("enabled = true")
            .unwrap_err()
            .to_string();
        assert!(e.contains("enabled"), "{e}");
    }

    /// `cn_models` and `wait_ms` are retired (2026-11-21): an old settings file
    /// that sets either, with any value it could have had and the flags on or
    /// off, still loads; each is named by `retired()` and never written back.
    #[test]
    fn a_retired_key_loads_warns_and_is_never_written_back() {
        for text in [
            "cn_models = \"always\"",
            "cn_models = \"follow_flags\"",
            "cn_models = \"never\"",
            "cn_models = \"follow_flags\"\nsession_flags = false",
        ] {
            let c: ContentBasedSecurityConfig = toml::from_str(text).unwrap();
            assert_eq!(c.retired(), ["content_based_security.cn_models"], "{text}");
            assert!(
                !toml::to_string(&c).unwrap().contains("cn_models"),
                "{text}"
            );
        }
        for text in ["wait_ms = 0", "wait_ms = 30000"] {
            let c: ContentBasedSecurityConfig = toml::from_str(text).unwrap();
            assert_eq!(c.retired(), ["content_based_security.wait_ms"], "{text}");
            assert!(!toml::to_string(&c).unwrap().contains("wait_ms"), "{text}");
        }
        let both: ContentBasedSecurityConfig =
            toml::from_str("cn_models = \"never\"\nwait_ms = 4000").unwrap();
        assert_eq!(
            both.retired(),
            [
                "content_based_security.cn_models",
                "content_based_security.wait_ms"
            ]
        );
    }
}
