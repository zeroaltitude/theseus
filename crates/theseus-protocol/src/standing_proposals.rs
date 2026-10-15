//! Proposed standing records (theseus-a8n7): what `persona.import` finds in an
//! agent's feedback notes. A proposal is no record: nothing of it reaches a
//! prompt until the owner keeps it (`standing.keep`, which writes the record,
//! scoped to the persona), and a dismissed one is never proposed again.
//! `standing.proposals` lists them; the owner's two answers are acts, from a
//! private place, refused inside a job's process.

use serde::{Deserialize, Serialize};

use crate::{StandingKind, StandingRecord};

/// One proposal, as stored (a META record keyed `standing.proposal:<id>`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct StandingProposal {
    /// `stp_` and 12 hex digits: the same words for the same persona are one
    /// proposal.
    pub id: String,
    /// The `[personas]` key whose scope the record would hold in.
    pub persona: String,
    pub kind: StandingKind,
    pub text: String,
    /// The note it came from, under the persona's folder.
    pub from: String,
    #[cfg_attr(test, ts(type = "number"))]
    pub proposed_ms: u64,
    /// `open`, `kept` or `dismissed`.
    pub state: String,
    /// The record a keep wrote.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub record: Option<String>,
}

/// `standing.proposals`: the open ones, or every one.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct StandingProposalsParams {
    #[serde(default)]
    pub all: bool,
    /// Only this persona's.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub persona: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct StandingProposalsResult {
    pub proposals: Vec<StandingProposal>,
}

/// `standing.keep` and `standing.dismiss`: a proposal's id, or the start of
/// one that no other shares.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct StandingProposalAnswer {
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub author: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub discord: Option<crate::DiscordOrigin>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct StandingProposalAnswered {
    pub proposal: StandingProposal,
    /// The record a keep wrote; none for a dismissal.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub record: Option<StandingRecord>,
}
