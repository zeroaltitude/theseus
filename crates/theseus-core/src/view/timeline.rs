//! The one read the view needs from the ledger (`crate::exchange`, which owns the owner's mark, the exchange
//! and the touch index): the newest exchanges of a root, before a cursor. The ledger's code implements
//! [`Timeline`] over its touch index; the view never names the ledger's records, so either can change inside.

use serde::{Deserialize, Serialize};

/// A root of the touch index, owned (the ledger's `exchange::Root<'_>` maps onto it one for one).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TimelineRoot {
    /// A place, as `exchange::place_of` writes it.
    Place(String),
    /// A person.
    Person(String),
    /// A thing, as the index's extractor names it: `type:value`.
    Thing(String),
}

/// Where a page of a timeline stops: everything strictly older than this exchange. Ordered as the touch
/// index is (time, then turn id).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Cursor {
    pub at_ms: u64,
    pub turn_id: String,
}

impl Ord for Cursor {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.at_ms
            .cmp(&other.at_ms)
            .then_with(|| other.turn_id.cmp(&self.turn_id))
    }
}

impl PartialOrd for Cursor {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

/// One exchange on a timeline, by reference (the ledger's `Touch`, with the turn id as `turn_id`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExchangeRef {
    pub turn_id: String,
    pub session_id: String,
    pub at_ms: u64,
}

impl ExchangeRef {
    /// The cursor that pages past this exchange.
    pub fn cursor(&self) -> Cursor {
        Cursor {
            at_ms: self.at_ms,
            turn_id: self.turn_id.clone(),
        }
    }
}

/// The newest exchanges of a root. Implemented by the ledger.
pub trait Timeline {
    /// At most `n` exchanges of `root`, newest first, each strictly older than `before` when it is given.
    fn newest(
        &self,
        root: &TimelineRoot,
        before: Option<&Cursor>,
        n: usize,
    ) -> anyhow::Result<Vec<ExchangeRef>>;
}
