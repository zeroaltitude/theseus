//! What the backlog keeps beside its tasks: one META record, `self.backlog`
//! (a new key, not a new record kind: no store format change). The items
//! themselves are tasks (`crate::task_graph`); this holds what a task has no
//! field for: each item's cause, counts, score and rank, the owner's pins,
//! the causes the owner dropped, and when the last night read.

use std::collections::BTreeMap;

use anyhow::Result;
use serde::{Deserialize, Serialize};
use theseus_store::{kinds, NewRecord};

use crate::store::Store;

/// The record's key.
pub const KEY: &str = "self.backlog";

/// A cause the owner dropped: never filed again.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Dropped {
    pub cause: String,
    pub item: String,
    pub at_ms: u64,
    pub by: String,
}

/// What a night learned about one item's cause, summed over every night.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Meta {
    pub cause: String,
    /// Where the evidence comes from: a ledger kind, `health`, `learning`.
    pub source: String,
    /// Distinct evidence seen, over every night (the evidence list on the
    /// task is capped; this is not).
    pub count: u64,
    /// Money the cause cost, from its rows.
    pub usd: f64,
    /// A person waited on it (a turn failed, the owner corrected).
    pub person: bool,
    /// A task or a call failed because of it.
    pub task_failed: bool,
    /// The estimate of the work: 1 (small) to 4 (large).
    pub work: u8,
    /// When it was last seen (unix ms).
    pub last_ms: u64,
    /// The keel class that holds it for the owner, if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub keel: Option<String>,
    /// The score that placed it (milli-points) and its rank, from 1.
    #[serde(default)]
    pub score: u64,
    #[serde(default)]
    pub rank: u32,
    /// The rank the owner moved it to; it keeps it until moved again.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pin: Option<u32>,
}

/// The record.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct State {
    /// When the last night read (unix ms).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub read_ms: Option<u64>,
    /// The parent task, "Self-improvement backlog".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent: Option<String>,
    #[serde(default)]
    pub dropped: Vec<Dropped>,
    /// By task id.
    #[serde(default)]
    pub items: BTreeMap<String, Meta>,
}

impl State {
    pub fn read(store: &Store) -> Result<State> {
        Ok(store.get_meta::<State>(KEY)?.unwrap_or_default())
    }

    pub fn record(&self) -> Result<NewRecord> {
        NewRecord::json(kinds::META, Some(KEY), self)
    }

    /// Whether the owner dropped this cause.
    pub fn is_dropped(&self, cause: &str) -> bool {
        self.dropped.iter().any(|d| d.cause == cause)
    }
}
