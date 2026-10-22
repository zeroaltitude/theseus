//! `thread.placed` (theseus-7kjb7): the router put a message in a thread's
//! session. It goes to the connection that sent the message, when the turn
//! is placed, so a chat can say where the message went while the turn runs.
//! `thread.routed` at the turn's end stays the record.

use serde::{Deserialize, Serialize};

use crate::thread_route::ThreadTag;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct ThreadPlaced {
    /// The conversation the message was sent to: the submitting chat's own.
    pub session_id: String,
    /// The session the turn runs in: the thread's.
    pub placed: String,
    /// The id the turn runs as.
    pub turn_id: String,
    pub tag: ThreadTag,
}
