//! What the gate gave a turn no person's message opened (a wake's, a report's, a continuation's), kept beside
//! its exchange (issue 44): the exchange has no marked ask to read its presence from again, so a turn that
//! continues it (a continuation of a continuation: a retry after a failed call, the late result of a job it
//! started) reads this. It holds the narrowest of the chain, since each turn's presence is the narrowest of
//! what it continued; it never widens. Read by `view_gate::exchange_presence`.

use serde::{Deserialize, Serialize};
use theseus_memory::gate::Present;
use theseus_store::{kinds, NewRecord};

use crate::store::Store;

/// `given.<turn>`: its own prefix, apart from the exchanges'.
pub const GIVEN_PREFIX: &str = "given.";

/// The presence a message-less turn was given, and the turn it continues, when it continues one.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Given {
    pub turn_id: String,
    /// [`Present::as_str`].
    pub present: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub continues: Option<String>,
}

impl Given {
    pub fn new(turn_id: &str, present: Present, continues: Option<String>) -> Self {
        Self {
            turn_id: turn_id.into(),
            present: present.as_str().into(),
            continues,
        }
    }

    /// Its presence; a word this build does not know is `Unknown` (admits nothing).
    pub fn present(&self) -> Present {
        Present::named(&self.present).unwrap_or(Present::Unknown)
    }
}

/// Its record.
pub fn given_record(g: &Given) -> anyhow::Result<NewRecord> {
    NewRecord::json(
        kinds::META,
        Some(&format!("{GIVEN_PREFIX}{}", g.turn_id)),
        g,
    )
}

/// What the turn `turn` was given, as last written; none for a turn a person's message opened, or one from
/// before this record.
pub fn given(store: &Store, turn: &str) -> anyhow::Result<Option<Given>> {
    store.get_meta(&format!("{GIVEN_PREFIX}{turn}"))
}
