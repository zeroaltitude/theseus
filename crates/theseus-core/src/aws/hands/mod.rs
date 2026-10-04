//! Hands (AWS design §3.3; step 40 part 1, theseus-mgw.6): jobs that run in
//! AWS and keep the job wrapper's contract, detached, durable, and
//! cancellable.
//!
//! - [`envelope`]: the per-dispatch key, the spec a hand is given, and the
//!   signed completion it sends home.
//! - [`hand`]: the `hand` role of the daemon's binary (`theseusd hand`), the
//!   wrapper inside the hand image.

pub mod envelope;
pub mod hand;

#[cfg(test)]
mod tests_hand;
