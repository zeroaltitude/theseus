//! Earned autonomy (theseus-uttu, killer-features plan §4.8, row R18): the
//! loosening card, proposed from the record and never silent. What the owner
//! approved every time, without one decline, is offered back as a grant: the
//! command (or the file tool in a workspace root) runs with a notice instead
//! of asking, and the record that earned it is one press away.
//!
//! A grant is the owner's to give (the place rule: from a private place),
//! never the model's and never a job's. It never reaches the floor, a shared
//! place, an MCP session's floor or the external-text hold, and the notice
//! it posts carries "should have asked", which revokes it. Each name carries
//! `Earned`, since the web apps' types share one namespace.

use serde::{Deserialize, Serialize};

use crate::DiscordOrigin;

/// What a group of approvals shows: the record that earned a grant.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(default)]
pub struct EarnedRecord {
    /// How many approvals the group holds.
    pub approvals: u64,
    /// The first and the last, unix ms.
    pub first_ms: u64,
    pub last_ms: u64,
    /// How many sessions asked.
    pub sessions: u64,
    /// Up to three calls, newest first, as the card shows them.
    pub samples: Vec<String>,
    /// A digest over the group's correlation ids, which a grant's row keeps.
    pub digest: String,
}

/// One group of approvals: a candidate for a grant, or what a grant is for.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(default)]
pub struct EarnedGroup {
    /// `ern_` and eight hex characters of the group's key.
    pub id: String,
    /// `proc.run`, or a file tool (`fs.write`).
    pub tool: String,
    /// For `proc.run`: the argv prefix, its program by its base name.
    pub prefix: Vec<String>,
    /// For a file tool: the workspace root its approvals were under.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub root: Option<String>,
    /// The words the card and `theseus policy` use: `cargo nextest run -p x`.
    pub label: String,
    /// Every call under the prefix, not the prefix alone, may follow it.
    pub open_ended: bool,
    pub record: EarnedRecord,
}

/// A grant: a group the owner accepted. It runs at `notify` in private
/// places, every surface, until it is revoked.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(default)]
pub struct EarnedGrant {
    pub group: EarnedGroup,
    /// Who accepted it, by label, and as the place rule saw them.
    pub by: String,
    pub who: String,
    pub via: String,
    pub at_ms: u64,
}

/// A card: one batch of candidates posted to the owner's DM, at most one a
/// week, with each candidate's answer.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(default)]
pub struct EarnedCard {
    pub proposed_ms: u64,
    pub candidates: Vec<EarnedGroup>,
    /// Candidate ids the owner has answered, yes or no.
    pub answered: Vec<String>,
}

/// One week of the number: interrupts per active hour.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(default)]
pub struct EarnedWeek {
    pub since_ms: u64,
    pub until_ms: u64,
    /// Questions a person was asked: `tool.confirm_requested` rows.
    pub interrupts: u64,
    /// Hours in which a turn began or a question was asked.
    pub active_hours: u64,
    pub per_hour: f64,
}

/// `policy.earned`: the proof's measure, the grants, and the open card. With
/// `propose_now` it posts a card now, whatever the week's limit says (the
/// owner's act); with `show` it returns one group's record.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(default)]
pub struct PolicyEarnedParams {
    pub propose_now: bool,
    /// The grants and the card alone: no read of the four weeks' ledger,
    /// which the number needs (`theseus policy` lists the grants this way).
    pub brief: bool,
    /// A candidate's or a grant's id: its record is the answer's `shown`.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub show: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub author: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub discord: Option<DiscordOrigin>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(default)]
pub struct PolicyEarnedResult {
    pub enabled: bool,
    /// The four weeks before now, oldest first.
    pub weeks: Vec<EarnedWeek>,
    pub interrupts: u64,
    pub active_hours: u64,
    pub per_hour: f64,
    /// Of those interrupts, the approvals the current grants would have saved.
    pub saved: u64,
    /// The rate had the grants held through the four weeks.
    pub per_hour_with_grants: f64,
    pub grants: Vec<EarnedGrant>,
    /// The card waiting for answers, if one is.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub card: Option<EarnedCard>,
    /// A card `propose_now` posted.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub proposed: Option<EarnedCard>,
    /// When the next card may be posted, unix ms (0: now).
    pub next_card_ms: u64,
    /// `show`'s group.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub shown: Option<EarnedGroup>,
    /// Said when the ledger's index was still being built, or a read was cut.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub note: Option<String>,
}

/// `policy.earned.answer`: yes, with a notice (a grant), or not this one
/// (the group is not proposed again for 90 days). A yes loosens, so it is
/// the owner's, from a private place.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(default)]
pub struct PolicyEarnedAnswerParams {
    pub id: String,
    pub accept: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub author: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub discord: Option<DiscordOrigin>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(default)]
pub struct PolicyEarnedAnswerResult {
    pub id: String,
    pub accepted: bool,
    /// The grant a yes made.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub grant: Option<EarnedGrant>,
    /// Nothing changed: it was granted or passed on already.
    pub already: bool,
}

/// `policy.revoke`: a grant ends, and its group is not proposed again for 90
/// days. It tightens, so anyone may, as "should have asked" may be pressed.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(default)]
pub struct PolicyRevokeParams {
    pub id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub author: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub discord: Option<DiscordOrigin>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(default)]
pub struct PolicyRevokeResult {
    pub id: String,
    /// The earned grant it ended; empty when it ended a conversation's.
    pub grant: EarnedGrant,
    /// The conversation's grant it ended (theseus-drjs).
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub session_grant: Option<crate::grants::SessionGrant>,
}
