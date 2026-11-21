//! Whether a source session may be read by a summary's model: only when it
//! is PROVEN neither sealed nor veiled. Anything else is held out and its
//! exchanges are folded by code: a threads board that cannot be read, a
//! session record that is missing or does not read, a thread named by a
//! session's provenance that the board does not hold, a seal or a proposed
//! seal on that thread or on the thread it was merged into, and an imported
//! conversation (whose labels the ladder does not read).
//!
//! The thread a session served is read from durable provenance, not only
//! from the board (which names the session a thread runs on NOW): the
//! supersession links a place's move writes on both sessions
//! (`succession::bind_place_to`, `thread:<slug>`), and a task's parent. It is
//! the rule of the gate's `seal_of` (`view_gate.rs` on the gate lane), with
//! the unreadable case denied rather than cleared; when the gate's version
//! fails closed too, [`Core::ladder_hidden`] is the one call to swap for it.

use theseus_protocol::threads::slug_of_place;

use crate::rpc::Core;
use crate::session::SessionRecord;
use crate::threads::Board;

/// What a session's provenance says of its thread.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Served {
    /// It served no thread: proven.
    None,
    /// It served this thread.
    Thread(String),
    /// Its provenance cannot be read.
    Unknown,
}

/// The longest a task chain is followed to its thread.
const DEPTH: usize = 4;

/// The thread `session` served, from the board and its record.
pub fn served(store: &crate::store::Store, board: &Board, session: &str, depth: usize) -> Served {
    if let Some(t) = board.of_session(session) {
        return Served::Thread(t.slug.clone());
    }
    let rec: SessionRecord = match store.get_session(session) {
        Ok(Some(r)) => r,
        Ok(None) | Err(_) => return Served::Unknown,
    };
    let linked = [&rec.superseded_by, &rec.supersedes]
        .into_iter()
        .flatten()
        .filter_map(|l| l.place.as_deref())
        .find_map(slug_of_place);
    if let Some(slug) = linked {
        return Served::Thread(slug.to_string());
    }
    match rec.task {
        Some(t) if depth < DEPTH => served(store, board, &t.parent_session, depth + 1),
        Some(_) => Served::Unknown,
        None => Served::None,
    }
}

/// Whether thread `slug` (or one it was merged into) is sealed or a seal is
/// proposed; a thread the board does not hold is held.
pub fn held(board: &Board, slug: &str) -> bool {
    let mut at = slug.to_string();
    for _ in 0..DEPTH {
        let Some(t) = board.get(&at) else {
            return true;
        };
        if crate::compiler::place_digest::held(t) {
            return true;
        }
        match &t.merged_into {
            Some(next) => at = next.clone(),
            None => return false,
        }
    }
    true
}

impl Core {
    /// Whether session `sid`'s words are kept from a summary's model: true
    /// unless it is proven neither sealed nor veiled (above).
    pub(crate) fn ladder_hidden(&self, sid: &str) -> bool {
        if crate::import::is_imported(sid) {
            return true;
        }
        let board = match self.runner.threads.all(&self.store) {
            Ok(b) => b,
            Err(e) => {
                tracing::warn!(session = sid, error = %format!("{e:#}"),
                    "ladder: the threads cannot be read, so a source is held out");
                return true;
            }
        };
        match served(&self.store, &board, sid, 0) {
            Served::None => false,
            Served::Thread(slug) => held(&board, &slug),
            Served::Unknown => true,
        }
    }
}
