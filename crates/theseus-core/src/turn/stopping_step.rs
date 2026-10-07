//! A daemon whose clean stop has begun sends no model call (theseus-jtrc).
//! The stop's first step (`stop_record`) stops the outbox's sending; a call
//! the loop plans from then on is settled failed before its request goes
//! out, so the turn's run of failures says backoff, its execution is woken,
//! and the next start's driver takes the retry. Before, a call sent during
//! the stop met a refused connection whenever its connect came after the
//! runtime's end began; when the connect came first, the request reached
//! the provider: a headless run (`theseus --spawn theseusd ask`) whose turn
//! failed had the driver's retry begin as its client asked the stop, a
//! request paid for and never read. A stopping daemon makes no in-turn
//! retry (`[model.retries] transient`) either.
//!
//! The failure has a class of its own, `stopping`, which passes with time
//! (theseus-36re). It is no provider's failure: its run posts no notice, and
//! health and telemetry count no provider error for it. Settled as a refused
//! connection (`network`), it had a place's "Turn failed" notice written
//! during the stop and dispatched at the next start, before the retry's
//! answer.

use super::*;

impl TurnRunner {
    /// The failure of a call about to be sent once the daemon's stop has
    /// begun, or `None` while it serves.
    pub(super) fn not_sent_for_the_stop(&self) -> Option<anyhow::Error> {
        self.stopping().then(|| {
            ProviderError::Stopping {
                message: "not sent: the daemon's stop began before the call went out".into(),
            }
            .into()
        })
    }

    /// Whether the daemon's stop has begun.
    pub(super) fn stopping(&self) -> bool {
        self.outbox.stopping()
    }
}
