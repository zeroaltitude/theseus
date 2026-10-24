//! The owner's persona writes (theseus-utfnl): a persona is a category of
//! the ontology, so adding, setting, retiring and marking one default are
//! writes to it, and its soul is a file the owner writes (`persona.soul`).
//! Each is the owner's, from a private place, and refused inside a job. A
//! persona added without a soul awaits one: the owner is asked once.

use serde::{Deserialize, Serialize};

/// `persona.add`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct PersonaAddParams {
    /// The persona's key: the category `persona:<key>`.
    pub key: String,
    /// Shown on replies; absent: the key, capitalised.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub emoji: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub card: Option<String>,
    /// The soul's text. Absent: the persona awaits its soul, and the owner
    /// is asked for it once.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub soul: Option<String>,
    /// What the owner's note says made it: `the CLI` or `the cockpit`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub from: Option<String>,
    /// Who pressed, when the caller is a surface acting for a person (a
    /// Discord press): the core judges the act as theirs, from where they
    /// pressed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub author: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub discord: Option<crate::DiscordOrigin>,
}

/// `persona.soul`: write a persona's soul file.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct PersonaSoulParams {
    pub key: String,
    pub text: String,
    /// Who pressed, when the caller is a surface acting for a person (a
    /// Discord press): the core judges the act as theirs, from where they
    /// pressed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub author: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub discord: Option<crate::DiscordOrigin>,
}

/// `persona.set`: a persona's short fields. A field present is set (the
/// empty string clears it); one absent is left as it is.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct PersonaSetParams {
    pub key: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub name: Option<String>,
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub initiative: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub shareable: Option<bool>,
    /// Who pressed, when the caller is a surface acting for a person (a
    /// Discord press): the core judges the act as theirs, from where they
    /// pressed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub author: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub discord: Option<crate::DiscordOrigin>,
}

/// `persona.retire`, and `persona.default` (no key clears the mark).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct PersonaKeyParams {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub key: Option<String>,
    /// Who pressed, when the caller is a surface acting for a person (a
    /// Discord press): the core judges the act as theirs, from where they
    /// pressed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub author: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub discord: Option<crate::DiscordOrigin>,
}

/// What a persona write did.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct PersonaWriteResult {
    pub key: String,
    /// `live`, `awaiting`, or `retired`.
    pub state: String,
    /// The soul file's path, when it has one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub soul: Option<String>,
    /// The soul's tokens against the cap, when it has a soul.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional, type = "number"))]
    pub tokens: Option<u64>,
    #[cfg_attr(test, ts(type = "number"))]
    pub cap: u64,
}
