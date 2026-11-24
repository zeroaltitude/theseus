//! The exchange ledger (issue 44, step 1): `exchange.report` sums the
//! measuring rows (`context.turn`, one per turn in a private place) into
//! counts and shares; `exchange.rebuild` writes the exchanges and touches of
//! history from before them, from the nodes, with a dry run that only counts.
//! Both carry ids, kinds and counts, never a message's text.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// `exchange.report`: the newest `limit` rows (default all).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(default)]
pub struct ExchangeReportParams {
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub limit: Option<u64>,
}

/// The rows, summed. `counts` is a group's values and how many rows had
/// each (`prev`: `transcript`, `place_tail`, `missed`, `none`; `peer`;
/// `kind`; …); `tokens` is each part of the sent context, added up over the
/// rows, est. at 3.6 bytes a token.
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

/// `exchange.topics` (issue 46): the exchanges from before topics were filed
/// per exchange, each one's opening message asked of the ontology's judge
/// once, and touches written for the topics it names. The owner's, in person.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(default)]
pub struct ExchangeTopicsParams {
    /// Count and price what it would ask, and ask nothing.
    pub dry_run: bool,
    /// The most this run may spend, in dollars (none: the default, $10).
    /// [`topics_cap`] holds what it may be, at the CLI and at the daemon.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub cap_usd: Option<f64>,
}

/// A run's cap of `exchange.topics` (issue 46), as the CLI reads it before
/// it builds a request and the daemon again: a finite number of dollars
/// that is at least one micro-dollar once rounded, as budgets count in
/// them. NaN, an infinity, zero, a negative number and a sum too small to
/// round to any money are refused; a large one is the owner's to name.
pub fn topics_cap(usd: f64) -> Result<f64, String> {
    match usd.is_finite() && (usd * 1_000_000.0).round() >= 1.0 {
        true => Ok(usd),
        false => Err(format!(
            "--cap-usd is a number of dollars of at least $0.000001, not {usd}"
        )),
    }
}

/// What a filing of the history found, would do or did: counts and dollars,
/// never a message's text.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(default)]
pub struct ExchangeTopicsResult {
    pub dry_run: bool,
    pub cap_usd: f64,
    /// Every exchange in the ledger.
    pub exchanges: u64,
    /// Those that already touch a topic, and those this job asked before.
    pub touched_before: u64,
    pub asked_before: u64,
    /// Those opened by no person's message that can be read.
    pub no_message: u64,
    /// Those the ontology's judge would not act on now (off, or in shadow).
    pub not_acting: u64,
    /// Those left to ask, and their worst case together.
    pub to_ask: u64,
    pub worst_usd: f64,
    /// Asked this run, of them answered, and the exchanges and touches the
    /// answers came to.
    pub asked: u64,
    pub answered: u64,
    pub filed: u64,
    pub touches: u64,
    pub spent_usd: f64,
    /// Exchanges still to ask when the run stopped.
    pub left: u64,
    /// Why it stopped before it was done.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub stopped: Option<String>,
}
