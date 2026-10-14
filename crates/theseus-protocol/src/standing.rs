//! Standing (theseus-a7vi): the owner's standing orders, preferences and
//! facts, one record each, kept in every prompt of their scope.
//!
//! - **`standing.list`**: the live records (or every one, ended and expired
//!   too), and the manifests' tally: how many compilations of their scope
//!   since they began, and how many of those lacked one. A read.
//! - **`standing.add`** and **`standing.revoke`**: the owner's acts, from a
//!   private place (refused inside a job's process). A record is never
//!   edited: a revoke is a new record naming the one it ends, and so is a
//!   replacement.
//! - **Capture** (theseus-slvl.5): `directive.v1` reads the owner's own words
//!   at the inbound point. What it is sure of is kept as a record with a
//!   notice, and the owner's undo is `standing.revoke`; what it is unsure of
//!   waits as an ask, which **`standing.answer`** keeps or drops. `standing.list`
//!   shows the open asks and, per record, the week's "said it again" count.

use serde::{Deserialize, Serialize};

/// What a record is: an order the model follows, a preference it honours, or
/// a fact about the owner it knows.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum StandingKind {
    #[default]
    Order,
    Preference,
    Fact,
}

impl StandingKind {
    pub const fn name(self) -> &'static str {
        match self {
            Self::Order => "order",
            Self::Preference => "preference",
            Self::Fact => "fact",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        [Self::Order, Self::Preference, Self::Fact]
            .into_iter()
            .find(|k| k.name() == s)
    }
}

/// Where a record holds: one session, one place (a transport target such as
/// `discord:channel:<id>`), or everywhere.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "snake_case", tag = "kind", content = "of")]
pub enum StandingScope {
    Session(String),
    Place(String),
    #[default]
    Everywhere,
}

impl StandingScope {
    /// `everywhere`, `place:<target>`, or `session:<id>`.
    pub fn words(&self) -> String {
        match self {
            Self::Session(s) => format!("session:{s}"),
            Self::Place(p) => format!("place:{p}"),
            Self::Everywhere => "everywhere".into(),
        }
    }
}

/// One record, as stored (a META record keyed `standing:<id>`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct StandingRecord {
    /// `std_` and a UUIDv7.
    pub id: String,
    pub kind: StandingKind,
    /// What it says. Empty in a revoke.
    pub text: String,
    pub scope: StandingScope,
    #[cfg_attr(test, ts(type = "number"))]
    pub since_ms: u64,
    /// When it lapses by itself; none holds until revoked.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(type = "number | null"))]
    pub until_ms: Option<u64>,
    /// The node it came from, or the surface that made it (`cli`, `discord`).
    pub source: String,
    /// The record this one ends: a revoke, or the older half of an edit.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ends: Option<String>,
}

/// A record as `standing.list` shows it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct StandingInfo {
    #[serde(flatten)]
    pub record: StandingRecord,
    /// `live`, `revoked`, `replaced`, or `expired`.
    pub state: String,
    /// Compilations of its scope since it began, from the manifests, and how
    /// many of them did not list it.
    #[cfg_attr(test, ts(type = "number"))]
    pub compiles: u64,
    #[cfg_attr(test, ts(type = "number"))]
    pub missing: u64,
    /// Times in the last seven days the owner said it again (a message
    /// `directive.v1` matched to this record): a non-zero week means the
    /// record's wording or its delivery failed.
    #[serde(default)]
    #[cfg_attr(test, ts(type = "number"))]
    pub repeats: u64,
    /// How it came to be: `capture` when `directive.v1` kept it from the
    /// owner's words (then `source` names the node), else empty.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub captured_by: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct StandingListParams {
    /// Every record, the ended and the expired too.
    #[serde(default)]
    pub all: bool,
    /// Whose scope: records of this session, its place, and everywhere. Every
    /// scope when none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    /// Who asks. The text of a preference or a fact goes only to the owner,
    /// from a private place; to anyone else it reads `(private)`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub author: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub discord: Option<crate::DiscordOrigin>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct StandingListResult {
    pub records: Vec<StandingInfo>,
    /// The compilations of the live records' scopes since each began (each
    /// counted once), and the (record, compilation) pairs that lacked it.
    #[cfg_attr(test, ts(type = "number"))]
    pub compiles: u64,
    #[cfg_attr(test, ts(type = "number"))]
    pub missing: u64,
    /// The asks still open: things `directive.v1` heard and was not sure
    /// enough of to keep (or an order, which it never keeps unasked).
    #[serde(default)]
    pub asks: Vec<StandingAsk>,
}

/// A captured directive waiting on the owner: keep it, keep it everywhere, or
/// not this. It lapses by itself after a week, kept as nothing.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct StandingAsk {
    /// `ask_` and a UUIDv7.
    pub id: String,
    pub kind: StandingKind,
    /// The owner's words, as one line (`(private)` to anyone else).
    pub text: String,
    /// Where it would hold if kept.
    pub scope: StandingScope,
    /// Why it was asked: `unsure` (between 0.5 and 0.8), `order` (an order is
    /// never kept unasked), or `twice` (a correction made twice in a week).
    pub why: String,
    /// The node the words came from.
    pub node: String,
    #[cfg_attr(test, ts(type = "number"))]
    pub at_ms: u64,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct StandingAnswerParams {
    /// An ask's id, or the start of one that no other shares.
    pub id: String,
    /// `keep` (where it was judged to hold), `everywhere` (keep it, for every
    /// session), or `not` (drop it).
    pub answer: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub author: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub discord: Option<crate::DiscordOrigin>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct StandingAnswerResult {
    pub id: String,
    pub answer: String,
    /// The record a keep wrote.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub record: Option<StandingRecord>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct StandingAddParams {
    #[serde(default)]
    pub kind: StandingKind,
    pub text: String,
    #[serde(default)]
    pub scope: StandingScope,
    /// Lapses at this time (Unix ms); none holds until revoked.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(type = "number | null"))]
    pub until_ms: Option<u64>,
    /// The node it came from; the surface's name when none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    /// The record this one replaces, ended in the same frame.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub replaces: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub author: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub discord: Option<crate::DiscordOrigin>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct StandingRevokeParams {
    /// A record's id, or the start of one that no other shares.
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub author: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub discord: Option<crate::DiscordOrigin>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct StandingChanged {
    /// The record written: the new one, or the revoke.
    pub record: StandingRecord,
    /// The id the write ended, when it ended one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ended: Option<String>,
}
