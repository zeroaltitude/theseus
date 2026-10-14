//! Deferred tools on the wire (theseus-7m7w.2): `tool.defer`'s answer, which
//! lists each tool family a place kind loads or defers with its use and its
//! misses, and health's one line.

use serde::{Deserialize, Serialize};

/// One family in one kind of place.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(default)]
pub struct DeferFamily {
    pub family: String,
    /// What the family holds, in a line.
    pub summary: String,
    /// The family's tools, by wire name.
    pub tools: Vec<String>,
    /// What its tool definitions cost a request that loads them, estimated at
    /// the live profile's model's rate.
    pub tokens: u32,
    /// Whether this kind of place loads it now.
    pub loaded: bool,
    /// Why it does or does not: `always loaded`, `default for this place`,
    /// `used in 12% of turns`, `unused for 16 days`, `deferred`.
    pub why: String,
    /// Turns in the window that called a tool of the family, or searched for one.
    pub used_turns: u32,
    /// Searches that surfaced a tool of the family.
    pub searched: u32,
    /// Calls to its tools that were not loaded and not searched for.
    pub missed: u32,
    /// Turns in which the family was loaded and its conversation had not used it.
    pub unused_turns: u32,
    /// When it was last used (unix ms), if the ledger shows it.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional, type = "number"))]
    pub last_used_ms: Option<u64>,
}

/// One kind of place (`cli`, `dm`, `channel`, `task`, `headless`).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(default)]
pub struct DeferPlace {
    pub place: String,
    /// Turns of this kind in the selector's window.
    pub turns: u32,
    /// The tools part of a request here, estimated: every tool loaded, and as
    /// this kind of place sends it (the loaded families, plus the deferred
    /// tools' constant and the search tool, or `tools_find`).
    pub tokens_all: u32,
    pub tokens_sent: u32,
    pub families: Vec<DeferFamily>,
}

/// `tool.defer`'s answer.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(default)]
pub struct DeferListResult {
    /// `[tools.defer] enabled`.
    pub enabled: bool,
    /// How the live profile's model reaches a hidden tool: `search` (the
    /// provider's tool search) or `find` (the harness's `tools_find`).
    pub mechanism: String,
    pub window_days: u32,
    pub hide_after_days: u32,
    #[cfg_attr(test, ts(type = "number"))]
    pub load_share: f64,
    /// Whether the ledger's use has been read yet (after serving).
    pub warmed: bool,
    pub places: Vec<DeferPlace>,
}

/// Health's one line.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(default)]
pub struct DeferHealth {
    pub enabled: bool,
    pub mechanism: String,
    pub warmed: bool,
    /// Turns the selector looked at, over all kinds of place.
    pub turns: u32,
    /// Searches that surfaced a tool, and calls that were no search's, in the window.
    pub searched: u32,
    pub missed: u32,
    /// The family with the most misses in the window, if any.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub top_miss: Option<String>,
}
