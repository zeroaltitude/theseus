//! The watch's cross-pass memory of the reds it has looked at (theseus-8p1oi).
//! A red is either **settled**, acted on and never again (reverted, left
//! alone with the reason, or handed to the owner with a card), or **attempted**:
//! the revert was tried and stopped (a halt, a hold with no card, an error)
//! without being acted on. An attempted red is tried again by the next pass,
//! up to [`TRIES`] times, and then said once as given up on; it is never
//! reported as settled, because a reader takes "settled" to mean handled.
//!
//! A retry cannot make a second revert: `doing::make` refuses a merge whose
//! revert already stands on main, and the watch leaves a join with a
//! `self.reverted` row alone.

use std::collections::HashMap;

/// How many times a red's revert is tried before the watch gives up on it.
pub const TRIES: u32 = 3;

/// What became of one red.
#[derive(Debug, Clone, PartialEq)]
pub enum Fate {
    /// Acted on; not looked at again.
    Settled,
    /// Tried and not acted on: how many times, and why the last try stopped.
    Attempted { tries: u32, why: String },
}

/// The reds the watch has looked at, by key.
#[derive(Debug, Default)]
pub struct Known(HashMap<String, Fate>);

impl Known {
    pub fn new() -> Self {
        Self::default()
    }

    /// Whether a pass leaves this red alone: it was acted on, or its tries ran out.
    pub fn skips(&self, key: &str) -> bool {
        match self.0.get(key) {
            Some(Fate::Settled) => true,
            Some(Fate::Attempted { tries, .. }) => *tries >= TRIES,
            None => false,
        }
    }

    /// The red was acted on.
    pub fn settle(&mut self, key: String) {
        self.0.insert(key, Fate::Settled);
    }

    /// The red was tried and not acted on. Whether that was the last try.
    pub fn attempt(&mut self, key: String, why: String) -> bool {
        let tries = match self.0.get(&key) {
            Some(Fate::Attempted { tries, .. }) => tries + 1,
            _ => 1,
        };
        self.0.insert(key, Fate::Attempted { tries, why });
        tries >= TRIES
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// The reds tried and not acted on.
    pub fn attempted(&self) -> impl Iterator<Item = (&str, &Fate)> {
        self.0
            .iter()
            .filter(|(_, f)| matches!(f, Fate::Attempted { .. }))
            .map(|(k, f)| (k.as_str(), f))
    }
}
