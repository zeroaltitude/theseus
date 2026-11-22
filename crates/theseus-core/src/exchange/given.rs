//! What the gate gave a turn no person's message opened (a wake's, a report's, a continuation's), kept beside
//! its exchange (issue 44) as `given.<turn>`. Anything goes (2026-11-22): nothing writes it now and nothing
//! reads a presence from it; an old record still decodes.

use serde::{Deserialize, Serialize};
use theseus_memory::gate::Present;

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
    /// Its presence; a word this build does not know is `Unknown` (admits nothing).
    pub fn present(&self) -> Present {
        Present::named(&self.present).unwrap_or(Present::Unknown)
    }
}

/// What the turn `turn` was given, as last written; none for a turn a person's message opened, or one from
/// before this record.
pub fn given(store: &Store, turn: &str) -> anyhow::Result<Option<Given>> {
    store.get_meta(&format!("{GIVEN_PREFIX}{turn}"))
}
