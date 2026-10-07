//! A `--stdio` daemon leaves a failed turn to its own client (theseus-zqxv).
//!
//! Its driver's retry (theseus-ljr: the first comes at once) raced the
//! client's `shutdown`: a headless `theseus --spawn theseusd ask` whose turn
//! failed transient, with a client slower to ask the stop than the daemon to
//! reach the retry's call, had a request go out, billed, whose answer nobody
//! read. Now the run of a stdio daemon's failed turn parks it on input
//! (`Failing::after`'s `driver_retries`), so the driver never takes it, and
//! the client retries if it wants: its next message is the retry. theseusd
//! says which daemon it is before anything is served
//! (`FailedTurns::leave_to_the_client`); the socket daemon's driver retries as
//! before.

use std::sync::atomic::{AtomicBool, Ordering};

/// Who retries a failed turn: the driver, unless the daemon left its failed
/// turns to its one client.
#[derive(Debug, Default)]
pub struct FailedTurns {
    to_the_client: AtomicBool,
}

impl FailedTurns {
    /// A `--stdio` daemon: its driver retries no failed turn.
    pub fn leave_to_the_client(&self) {
        self.to_the_client.store(true, Ordering::Relaxed);
    }

    /// Whether the driver retries a failed turn: the socket daemon's does.
    pub fn driver_retries(&self) -> bool {
        !self.to_the_client.load(Ordering::Relaxed)
    }
}
