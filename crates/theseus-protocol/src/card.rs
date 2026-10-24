//! The owner card's proposals (theseus-slvl.6): facts and preferences about
//! the owner, stated from his imported history by a budgeted job, each a
//! question he keeps, edits or drops. A kept one becomes a standing record
//! (`standing::StandingRecord`) whose `source` is `card:<proposal id>`; the
//! proposal keeps the episodes it rests on. Nothing is added by itself.

use serde::{Deserialize, Serialize};

use crate::standing::{StandingKind, StandingRecord, StandingScope};

/// The question's options, in order. The first two keep the proposal (the
/// second asks for the words to be changed first); the last is what a
/// question nobody answers expires to.
pub const CARD_OPTIONS: [&str; 3] = ["Keep", "Edit", "Not this"];

/// The record key's prefix of a proposal (a META key).
pub const CARD_PROPOSAL_PREFIX: &str = "card.proposal.";

/// The most proposals a list shows of the answered kinds, newest first.
pub const CARD_LIST_ANSWERED: usize = 50;

/// Where a proposal stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum ProposalState {
    /// Waiting for the owner.
    Open,
    /// Kept (as proposed or edited): a standing record was written.
    Kept,
    /// "Not this", by the owner or by expiry: never proposed again for its sources.
    Declined,
}

impl ProposalState {
    pub fn name(self) -> &'static str {
        match self {
            Self::Open => "open",
            Self::Kept => "kept",
            Self::Declined => "declined",
        }
    }
}

/// One episode a proposal rests on.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct CardSource {
    /// The imported episode's id.
    pub episode: String,
    /// The node that cites it (its summary, else the line read), whose
    /// handle `theseus history` and recall resolve.
    pub node: String,
    /// What kind of evidence it was: `feedback`, `sop`, `user` or `decision`.
    pub kind: String,
    /// When it was said, Unix ms.
    #[cfg_attr(test, ts(type = "number"))]
    pub at_ms: u64,
}

/// A proposal, as stored (the META record `card.proposal.<id>`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct CardProposal {
    /// `cdp_` and a short unique id.
    pub id: String,
    pub kind: StandingKind,
    pub scope: StandingScope,
    /// The fact or preference, as stated: one short phrase about the owner.
    pub text: String,
    /// The question the owner is asked.
    pub question: String,
    pub options: Vec<String>,
    /// The episodes it rests on, newest first; at most [`CARD_MAX_SOURCES`].
    pub sources: Vec<CardSource>,
    /// Distinct sources the cluster had (it may exceed `sources`).
    pub support: u32,
    #[cfg_attr(test, ts(type = "number"))]
    pub created_ms: u64,
    /// When an unanswered question becomes "Not this".
    #[cfg_attr(test, ts(type = "number"))]
    pub expires_ms: u64,
    pub state: ProposalState,
    /// The run that made it (`crd_…`).
    pub run: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(type = "number | null"))]
    pub answered_ms: Option<u64>,
    /// `owner`, or `expiry`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub answered_by: Option<String>,
    /// The words kept, when the owner edited them.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kept_text: Option<String>,
    /// The standing record a keep wrote.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub standing_id: Option<String>,
}

/// The most episodes one proposal stores.
pub const CARD_MAX_SOURCES: usize = 40;

/// `card.propose`: run the job now.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct CardProposeParams {
    /// Read and cluster, and say what a run would send and at most cost; no
    /// model call, nothing written.
    #[serde(default)]
    pub dry_run: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub author: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub discord: Option<crate::DiscordOrigin>,
}

/// What a run did.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct CardProposeResult {
    /// `crd_…`; empty for a dry run.
    pub run: String,
    /// Imported episodes that count as evidence (feedback, SOP, USER files,
    /// decisions and preferences).
    #[cfg_attr(test, ts(type = "number"))]
    pub evidence: u64,
    /// Episodes left unread because they are partner-confidential.
    #[cfg_attr(test, ts(type = "number"))]
    pub withheld: u64,
    /// Lines left out as an assistant's setup (its instructions to itself),
    /// not the owner's words (theseus-ly75u).
    #[serde(default)]
    #[cfg_attr(test, ts(type = "number"))]
    pub setup: u64,
    /// Lines and statements left out as one task's, one repository's,
    /// branch's or run's: the card holds what lasts.
    #[serde(default)]
    #[cfg_attr(test, ts(type = "number"))]
    pub one_task: u64,
    /// Clusters of near-duplicate lines, and those worth a call (mostly new).
    #[cfg_attr(test, ts(type = "number"))]
    pub clusters: u64,
    #[cfg_attr(test, ts(type = "number"))]
    pub fresh: u64,
    /// Model calls made.
    #[cfg_attr(test, ts(type = "number"))]
    pub calls: u64,
    /// Statements the model made that were dropped (uncited, a paste, a
    /// duplicate of a live record or an answered proposal).
    #[cfg_attr(test, ts(type = "number"))]
    pub dropped: u64,
    pub proposed: Vec<CardProposal>,
    /// Open proposals that expired to "Not this" as the run began.
    #[cfg_attr(test, ts(type = "number"))]
    pub expired: u64,
    pub spent_usd: f64,
    /// A dry run: the worst case of the calls it would make.
    pub at_most_usd: f64,
    pub spent_today_usd: f64,
    pub day_usd: f64,
    /// Why it stopped short (`full`, `budget`, `off`, `done`, a failure), if it did.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stopped: Option<String>,
    /// Statements of one call that gave opposite instructions: the one
    /// kept and the one not offered, a line each.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub conflicts: Vec<String>,
}

/// `card.list`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct CardListParams {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub author: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub discord: Option<crate::DiscordOrigin>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct CardListResult {
    /// Waiting, oldest first.
    pub open: Vec<CardProposal>,
    /// Answered, newest first, at most [`CARD_LIST_ANSWERED`] of each.
    pub kept: Vec<CardProposal>,
    pub declined: Vec<CardProposal>,
    #[cfg_attr(test, ts(type = "number"))]
    pub kept_total: u64,
    #[cfg_attr(test, ts(type = "number"))]
    pub declined_total: u64,
    pub spent_today_usd: f64,
    pub spent_total_usd: f64,
    pub day_usd: f64,
    #[cfg_attr(test, ts(type = "number"))]
    pub max_open: u64,
    #[cfg_attr(test, ts(type = "number | null"))]
    pub last_run_ms: Option<u64>,
    /// The owner said the card is done: the nightly run stops.
    pub done: bool,
    /// The nightly run's interval in days (0: off).
    #[cfg_attr(test, ts(type = "number"))]
    pub every_days: u64,
}

/// `card.answer`: keep, edit or decline one proposal.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct CardAnswerParams {
    /// A proposal's id, or the end of one that no other shares (four characters at least).
    pub id: String,
    /// `keep`, `edit` or `decline` (the question's options, any case); or
    /// `scope`, on a kept proposal, with `scope`.
    pub option: String,
    /// The words to keep, for `edit`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    /// Where the record holds, in `StandingScope::words()`'s forms, for
    /// `keep` and `edit` (else the proposal's), and for `scope`, which moves
    /// a kept proposal's record to it (theseus-ly75u).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scope: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub author: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub discord: Option<crate::DiscordOrigin>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct CardAnswerResult {
    pub proposal: CardProposal,
    /// The standing record a keep wrote (or found already live).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub record: Option<StandingRecord>,
}

/// `card.done`: say the card is done (the nightly run stops), or take it back.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct CardDoneParams {
    #[serde(default = "yes")]
    pub done: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub author: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub discord: Option<crate::DiscordOrigin>,
}

fn yes() -> bool {
    true
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct CardDoneResult {
    pub done: bool,
}

/// The question a proposal asks, in the words every surface shows.
pub fn card_question(text: &str, scope: &StandingScope, support: u32) -> String {
    let n = if support == 1 {
        "1 source".to_string()
    } else {
        format!("{support} sources")
    };
    let where_ = match scope {
        StandingScope::Everywhere => String::new(),
        other => format!(" ({})", other.words()),
    };
    format!("Add to your card{where_}: '{text}' (from {n})? [Keep] [Edit] [Not this]")
}

/// The options, as owned strings.
pub fn card_options() -> Vec<String> {
    CARD_OPTIONS.iter().map(|s| s.to_string()).collect()
}
