//! The default `fs.read` window a turn narrows to (theseus-tdkcn): a
//! `deep_coding` turn's read with neither offset nor limit returns
//! [`NARROW_LINES`] lines, not 2,000. Kept by session, in memory, and set at
//! every turn's budget (a turn without a `deep_coding` verdict sets none), so
//! one turn's value never outlives the next turn of its session.

use std::collections::HashMap;
use std::sync::{Mutex, PoisonError};

/// The lines a narrowed default read returns.
pub const NARROW_LINES: usize = 400;

static WINDOWS: Mutex<Option<HashMap<String, usize>>> = Mutex::new(None);

/// `session`'s turn reads `lines` by default; 0 is the tool's own.
pub fn set(session: &str, lines: usize) {
    let mut m = WINDOWS.lock().unwrap_or_else(PoisonError::into_inner);
    let m = m.get_or_insert_with(HashMap::new);
    match lines {
        0 => m.remove(session),
        n => m.insert(session.to_string(), n),
    };
}

pub fn get(session: &str) -> usize {
    let m = WINDOWS.lock().unwrap_or_else(PoisonError::into_inner);
    m.as_ref()
        .and_then(|m| m.get(session))
        .copied()
        .unwrap_or(0)
}
