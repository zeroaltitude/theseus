//! The persona as data (theseus-slvl.2): `persona.list`, the personas the
//! config defines and the built-in default, each with its fields, its files
//! (digest, size, when edited), whether every compile since carried it, and
//! the places that speak as it. A read. The changes are `persona.changed`
//! rows (`ledger.tail { kind }`); a persona's short fields are edited in the
//! config file, and its words in its own file.

use serde::{Deserialize, Serialize};

/// `persona.list`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct PersonaListParams {
    /// The session whose persona to mark as in play; absent: the default's.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub session_id: Option<String>,
}

/// One of a persona's files.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct PersonaFile {
    /// `~` expanded.
    pub path: String,
    #[serde(default)]
    pub bytes: u64,
    /// The first 16 hex digits of the SHA-256 of the text a compile carries;
    /// absent when it cannot be read.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub digest: Option<String>,
    /// Why it cannot be read: `not found`, `permission denied`, …
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub missing: Option<String>,
    /// When the file was last written.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub edited_ms: Option<u64>,
}

/// The compiles of the sessions that speak as a persona, since the persona
/// last changed: how many carried its current file, and how many did not.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct PersonaTally {
    pub since_ms: u64,
    pub compiles: u64,
    pub missing: u64,
}

/// A bound place that names a persona.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct PersonaPlace {
    /// `#openclaw`, `DM @name`.
    pub place: String,
    pub target: String,
    /// The persona the bindings file names; it may not be one the config has.
    pub persona: String,
    /// The config has no such persona: the place speaks as the default.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub unknown: bool,
}

/// A persona as `persona.list` gives it.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct PersonaInfo {
    /// The `[personas]` key; absent for the built-in default.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub key: Option<String>,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub emoji: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub card: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub tts_voice: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub avatar: Option<String>,
    /// `quiet`, `normal` or `chatty`.
    pub initiative: String,
    /// It is the one in play for the asked session (else `default_persona`).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub in_play: bool,
    #[serde(default)]
    pub files: Vec<PersonaFile>,
    /// A digest of its files' and fields': what its `persona.changed` rows
    /// move.
    pub digest: String,
    /// When a compile last saw it change.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub changed_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub tally: Option<PersonaTally>,
    /// The built-in default's text stands in for it: no file is readable.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub on_default: bool,
    #[serde(default)]
    pub places: Vec<PersonaPlace>,
    /// Plain words about what is wrong, if anything.
    #[serde(default)]
    pub warnings: Vec<String>,
}

/// `persona.list`'s answer.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct PersonaListResult {
    /// The configured personas, then the built-in default last.
    pub personas: Vec<PersonaInfo>,
    /// The key in play for the asked session; absent: the built-in.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub in_play: Option<String>,
    /// The built-in default's size and digest.
    pub built_in_bytes: u64,
    pub built_in_digest: String,
    /// Bound places that name a persona, whatever the config has.
    #[serde(default)]
    pub places: Vec<PersonaPlace>,
}
