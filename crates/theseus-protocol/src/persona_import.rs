//! `persona.import` and `persona.card_confirm` (theseus-a8n7): the owner's
//! agents come over as personas, unchanged. The daemon reads an agent's
//! workspace on its own machine, copies the persona files byte for byte into
//! `<state>/personas/<name>/`, drafts a specialization card from them without
//! a model, and finds the feedback in the agent's memory notes, which become
//! *proposed* standing records and nothing live. A dry run writes nothing.
//! The CLI then writes `[personas.<name>]` (the files and the card) into the
//! config file, as `theseus persona edit` does.

use serde::{Deserialize, Serialize};

use crate::StandingKind;

/// `persona.import`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct PersonaImportParams {
    /// The agent's workspace folder, on the daemon's machine.
    pub from: String,
    /// The persona's key in `[personas]`.
    pub name: String,
    /// The name its imported episodes carry (`ImportedFrom.agent`); absent:
    /// the persona's key.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub agent: Option<String>,
    /// Report what would be carried and written; write nothing.
    #[serde(default)]
    pub dry_run: bool,
}

/// One file the import carries (or would).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct ImportedFile {
    /// Its name under the persona's folder: `SOUL.md`, `memory/lessons.md`,
    /// `AGENTS.part2.md`.
    pub rel: String,
    /// Where it is (or would be) on the daemon's machine.
    pub path: String,
    #[cfg_attr(test, ts(type = "number"))]
    pub bytes: u64,
    /// The first 16 hex digits of its SHA-256.
    pub digest: String,
    /// Part of an over-limit file, cut at headings: the source's name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub part_of: Option<String>,
    /// A curated note carried for reference, not compiled into the persona.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub note: bool,
    /// Its bytes differ from the copy already there (or none was).
    #[serde(default)]
    pub changed: bool,
}

/// A file over the context-file limit, split verbatim at its headings.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct OverLimit {
    pub rel: String,
    #[cfg_attr(test, ts(type = "number"))]
    pub bytes: u64,
    pub parts: u32,
}

/// A standing record the agent's feedback proposes. It is live only after the
/// owner keeps it.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct ProposedStanding {
    /// `stp_` and the first 12 hex digits of a digest of the persona, the
    /// kind and the text: the same item proposed twice is one proposal.
    pub id: String,
    pub kind: StandingKind,
    pub text: String,
    /// The note it came from, under the persona's folder.
    pub from: String,
}

/// `persona.import`'s answer.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct PersonaImportResult {
    pub name: String,
    pub agent: String,
    pub dry_run: bool,
    /// The persona's folder.
    pub dir: String,
    /// 1 for the first import of this persona, then one more each time the
    /// files differ.
    pub version: u32,
    pub files: Vec<ImportedFile>,
    /// The paths for `[personas.<name>] files`, in compile order.
    pub context_paths: Vec<String>,
    pub over_limit: Vec<OverLimit>,
    /// What the workspace held that is not carried, and why.
    pub skipped: Vec<String>,
    /// The drafted card (a few lines; no model wrote it).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub card: Option<String>,
    /// Every file's bytes equal the copy already there.
    #[serde(default)]
    pub unchanged: bool,
    pub proposals: Vec<ProposedStanding>,
    /// Proposals already open or closed before this import.
    #[serde(default)]
    pub proposals_known: u32,
}

/// `persona.card_confirm`: the owner has read the drafted card (or edited it)
/// and it goes live for Jev.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct PersonaCardConfirmParams {
    pub name: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct PersonaCardConfirmResult {
    pub name: String,
    pub card: String,
}
