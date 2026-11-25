//! A question marked as needing the proof: the asker's mark, kept as a META
//! record beside the question, in the same frame that asks it. The daemon's
//! own code writes it (the other branch's change card is the first), never a
//! tool, and nothing a tool can reach deletes it: the store is on the floor.
//!
//! The mark is what the answer path reads, so a question's `needs_proof` is
//! the daemon's own record and never a field of the answer or of anything a
//! surface sends.

use serde::{Deserialize, Serialize};
use theseus_store::{kinds, NewRecord};

use crate::store::Store;

/// The META key of `correlation_id`'s mark.
fn key(correlation_id: &str) -> String {
    format!("owner.proof.marked.{correlation_id}")
}

#[derive(Serialize, Deserialize)]
struct Mark {
    /// A proof given earlier never covers it: a fresh code every time.
    fresh: bool,
}

/// The record that marks the question `correlation_id` as needing the proof,
/// to be staged in the frame that asks it. `fresh` is `needs_fresh_proof`.
pub fn mark(correlation_id: &str, fresh: bool) -> anyhow::Result<NewRecord> {
    NewRecord::json(kinds::META, Some(&key(correlation_id)), &Mark { fresh })
}

/// Whether `correlation_id` is marked, and if so whether it needs a fresh
/// code every time.
pub fn marked(store: &Store, correlation_id: &str) -> Option<bool> {
    match store.get_meta::<Mark>(&key(correlation_id)) {
        Ok(found) => found.map(|m| m.fresh),
        Err(_) => {
            // A mark that cannot be read is read as the strictest one, never as none.
            tracing::warn!(correlation_id, "an owner proof mark could not be read; treating it as fresh");
            Some(true)
        }
    }
}
