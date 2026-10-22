//! One step of a draft at a time (theseus-b0ozi, LOW-8). A draft's end, its
//! review's end, a first pass's end and the start sweep can all reach the same
//! draft at once (a cancel and the turn's own end, say); two of them would
//! each commit in the tree, collide on its `index.lock`, and write two rows.
//! Each step takes the draft's lock first and reads the draft's record under
//! it, so the second finds what the first wrote and does nothing.
//!
//! A per-draft async mutex rather than a compare-and-set on the META record,
//! because the store has no compare-and-set on a key and the commit and the
//! apply are not records.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};

use tokio::sync::OwnedMutexGuard;

static HELD: OnceLock<Mutex<HashMap<String, Arc<tokio::sync::Mutex<()>>>>> = OnceLock::new();

/// The lock of the draft whose session is `draft_session`, held until the
/// guard drops. A lock no one holds or waits on is forgotten at the next call.
pub(super) async fn hold(draft_session: &str) -> OwnedMutexGuard<()> {
    let one = {
        let mut held = HELD.get_or_init(Mutex::default).lock().unwrap();
        held.retain(|_, m| Arc::strong_count(m) > 1);
        held.entry(draft_session.to_string()).or_default().clone()
    };
    one.lock_owned().await
}
