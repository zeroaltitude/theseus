//! `thread.import` (theseus-71ib, T11): candidate threads from the owner's
//! earlier harness. One call serves both halves: with no `apply` it is the
//! dry run (candidates, no write), and with `apply` it makes the named
//! candidates' threads, each with a card written from its episodes. Imported
//! history stays testimony: a card cites the episodes it used, and the
//! episodes reach a thread only through recall.

use serde::{Deserialize, Serialize};

use crate::threads::WorkingSet;
use crate::DiscordOrigin;

/// The window's length by default: owner messages in the last 30 days.
pub const SINCE_DAYS: u32 = 30;
/// The fewest owner messages a DM topic cluster needs by default.
pub const MIN_CLUSTER: u32 = 8;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct ThreadImportParams {
    /// The source of the streams: `openclaw` (every imported source that
    /// starts with it).
    pub from: String,
    /// Owner messages since this many days ago count (30).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub since_days: Option<u32>,
    /// A DM topic cluster needs this many owner messages (8).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub min_cluster: Option<u32>,
    /// The slugs to make. None is the dry run: nothing is written.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub apply: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub author: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub discord: Option<DiscordOrigin>,
}

/// One candidate thread, as the dry run's table shows it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct ThreadCandidate {
    /// Its place in the table, from 1, in the table's order.
    pub n: u32,
    pub slug: String,
    pub title: String,
    /// `channel` (one imported channel conversation) or `dm` (a topic
    /// cluster of the DMs).
    pub kind: String,
    /// The persona its agent names, only when that persona is configured.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub persona: Option<String>,
    pub topics: Vec<String>,
    pub working_set: WorkingSet,
    /// The owner's messages in the window.
    pub owner_messages: u32,
    pub episodes: u32,
    pub last_active_ms: u64,
    /// Owner messages in the window by their episodes' sensitivity
    /// (`personal`, `company-confidential`, `partner-confidential`,
    /// `public`); a partner-confidential one counts and gives no text.
    pub sensitivity: std::collections::BTreeMap<String, u32>,
    /// A thread of this slug exists: applying it is refused.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub exists: bool,
    /// The Discord channel this stream came from, when it is one: the place
    /// the thread can be bound to (`thread-discord`, T6), noted for now.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub channel: Option<String>,
    /// What the dry run says of it beyond the table (the binding's note).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub notes: Vec<String>,
}

/// A thread an apply made.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct ThreadApplied {
    pub slug: String,
    /// The card's lines, each ending with the handle of the episode it
    /// cites (`[m:a1b2c3]`).
    pub card: Vec<String>,
    /// The handles the card cites, once each.
    pub cited: Vec<String>,
    pub cost_usd: f64,
    /// Why the thread was made without a card (the budget, the model).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub card_failed: Option<String>,
}

/// A candidate an apply did not make, and why.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct ThreadSkipped {
    pub slug: String,
    pub why: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct ThreadImportResult {
    pub from: String,
    pub since_days: u32,
    pub min_cluster: u32,
    /// Every candidate, in the table's order (a dry run and an apply alike).
    pub candidates: Vec<ThreadCandidate>,
    pub applied: Vec<ThreadApplied>,
    pub skipped: Vec<ThreadSkipped>,
    /// What this call's cards cost, today's total, and the day's cap
    /// (`[threads] import_day_usd`).
    pub spent_usd: f64,
    pub spent_today_usd: f64,
    pub cap_usd: f64,
    /// What the scan left out, counted: episodes with no topic, clusters
    /// under the minimum, channels with no owner message in the window.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub notes: Vec<String>,
}
