//! The exchange ledger (issue 44, step 1): `exchange.report` sums the
//! measuring rows (`context.turn`, one per turn in a private place) into
//! counts and shares; `exchange.rebuild` writes the exchanges and touches of
//! history from before them, from the nodes, with a dry run that only counts.
//! Both carry ids, kinds and counts, never a message's text.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// `exchange.report`: the newest `limit` rows (default all), none older than `since_ms`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(default)]
pub struct ExchangeReportParams {
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub limit: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub since_ms: Option<u64>,
}

/// The rows' answers to the measuring questions (issue 44, step 1), counts only. The previous exchange and
/// the things named are counted over the turns his own message opened; the context over the turns whose
/// first call was measured.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(default)]
pub struct ExchangeAnswers {
    /// Rows whose exchange a message of the owner's opened, by the exchange's own person.
    pub his_turns: u64,
    /// Rows whose exchange does not read (erased, or never written).
    pub unreadable: u64,
    /// The place's previous exchange in the context sent: `shown`, `missed` (held by a seal or a gate, or
    /// a real miss), `after_new` (before the place's `/new`, which the view leaves out), `first_here`,
    /// `not_measured` (the first call was never measured).
    pub previous: BTreeMap<String, u64>,
    /// Of the `missed`, where the previous exchange ran: `same` session, `other`.
    pub missed_by_session: BTreeMap<String, u64>,
    /// Turns whose message named a thing the index recognises: `some`, `none`, `unknown` (the index was
    /// late).
    pub named: BTreeMap<String, u64>,
    /// For each kind of thing, the turns that named it at least once.
    pub named_kinds: BTreeMap<String, u64>,
    /// Rows whose first call was measured, and each part of what it sent, summed (tokens, est.).
    pub context_turns: u64,
    pub context: BTreeMap<String, u64>,
    /// The provider's cache, read and written, summed over the same rows (tokens).
    pub cache_read: u64,
    pub cache_write: u64,
}

/// The rows, summed. `counts` is a group's values and how many rows had
/// each (`prev`: `transcript`, `place_tail`, `missed`, `none`; `peer`;
/// `kind`; …); `tokens` is each part of the sent context, added up over the
/// rows, est. at 3.6 bytes a token. `answers` is none from a daemon that
/// does not count them.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(default)]
pub struct ExchangeReportResult {
    pub rows: u64,
    /// The first and last row's time.
    pub from_ms: Option<u64>,
    pub to_ms: Option<u64>,
    pub counts: BTreeMap<String, BTreeMap<String, u64>>,
    pub tokens: BTreeMap<String, u64>,
    /// The places bound now, by kind (`dm`, `channel_private`, …): counts.
    pub places_bound: BTreeMap<String, u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub answers: Option<ExchangeAnswers>,
}

/// `exchange.rebuild`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(default)]
pub struct ExchangeRebuildParams {
    /// Count what it would write, and write nothing.
    pub dry_run: bool,
}

/// What a rebuild found and wrote.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(default)]
pub struct ExchangeRebuildResult {
    pub dry_run: bool,
    pub sessions: u64,
    /// Turns found in the nodes, and of them those that already had an
    /// exchange (written as they happened; kept as they are).
    pub turns: u64,
    pub kept: u64,
    pub exchanges: u64,
    pub touches: u64,
    /// Of the new exchanges, those whose place, and whose person, the owner's
    /// mark of their message proved; the rest say `unknown` and name no person.
    pub place_proven: u64,
    pub person_proven: u64,
    /// Touches keyed off their exchange's start (an older build's reply
    /// touches), moved to it so that one exchange holds one touch of a root.
    pub repaired: u64,
    /// Why the things named in them were not looked for, if they were not.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub things_skipped: Option<String>,
}

/// `exchange.ladder` (issue 44, step 4): build a place's ladder from its
/// history since its latest `/new`: the missing `did` lines, then the folds.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(default)]
pub struct ExchangeLadderParams {
    /// The place, as the ledger names it (`discord:channel:<id>`, `local:<dir>`).
    pub place: String,
    /// Count and price what it would do, and write nothing.
    pub dry_run: bool,
    /// The most this run may spend, in dollars: the smaller of it and what is
    /// left of the day's line for summaries (none: that line alone).
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub cap_usd: Option<f64>,
    /// Leave to pass the day's line, up to `cap_usd` (which it needs).
    pub over_day_line: bool,
}

/// What a ladder build found, would do or did.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(default)]
pub struct ExchangeLadderResult {
    pub dry_run: bool,
    pub place: String,
    /// The place's latest `/new`; 0 for a place that has had none.
    pub since_ms: u64,
    /// The place's name holds a character no ladder key may: nothing done.
    pub skipped: bool,
    /// What is left of the day's line, and whether this run would need
    /// `over_day_line` to finish.
    pub day_left_usd: f64,
    pub needs_over_day_line: bool,
    pub exchanges: u64,
    /// Lines there already, and lines it would write (wrote).
    pub lines_had: u64,
    pub lines: u64,
    /// First-level nodes it would make (made), and the nodes above made by code.
    pub folds: u64,
    pub above: u64,
    pub estimated_usd: f64,
    pub spent_usd: f64,
    /// Why it stopped before it was done.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub stopped: Option<String>,
}
