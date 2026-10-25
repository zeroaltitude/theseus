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
    /// The digest of the soul the writer loaded (`persona.soul_read`'s). When
    /// given and the file's digest now differs, the write is refused and
    /// nothing changes. Absent: the write replaces the file as it stands.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub expect_digest: Option<String>,
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

/// `persona.soul_read`: a persona's soul file as it stands, or one of its
/// backups. Judged as the writes are: the owner, from a private place.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct PersonaSoulReadParams {
    pub key: String,
    /// Read this backup (a `ms` the live answer lists) instead of the file.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional, type = "number"))]
    pub bak_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub author: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub discord: Option<crate::DiscordOrigin>,
}

/// One `<file>.<ms>.bak` beside a soul.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct PersonaSoulBackup {
    /// When the replaced file was kept (the name's milliseconds).
    #[cfg_attr(test, ts(type = "number"))]
    pub ms: u64,
    #[cfg_attr(test, ts(type = "number"))]
    pub bytes: u64,
    #[cfg_attr(test, ts(type = "number"))]
    pub tokens: u64,
}

/// A soul as it stands. A persona awaiting its soul answers with no text and
/// no backups.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct PersonaSoulReadResult {
    pub key: String,
    /// The text read: the live file, or the backup `bak_ms` named. Absent
    /// while the persona awaits its soul.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub text: Option<String>,
    /// The soul file's path as the category holds it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub path: Option<String>,
    #[cfg_attr(test, ts(type = "number"))]
    pub bytes: u64,
    #[cfg_attr(test, ts(type = "number"))]
    pub tokens: u64,
    /// The digest of `text`, the one the category keeps.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub digest: Option<String>,
    /// The digest of the live file, which a write's `expect_digest` is
    /// checked against (it is `digest` unless a backup was read).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub live_digest: Option<String>,
    /// The backups beside the file, newest first, at most
    /// [`SOUL_BACKUPS_SHOWN`].
    #[serde(default)]
    pub backups: Vec<PersonaSoulBackup>,
    /// The cap in tokens (`[context] persona_max_tokens`).
    #[cfg_attr(test, ts(type = "number"))]
    pub cap: u64,
}

/// How many backups `persona.soul_read` lists.
pub const SOUL_BACKUPS_SHOWN: usize = 20;
