//! What a person's first keystroke warms (theseus-tnky), and what the
//! daemon remembers of it. Two waits a person felt that no bench saw: a
//! message after the provider connection's idle time paid a new connect and
//! TLS (80 to 140 ms) before its request left, and a message after the
//! index tender had unloaded its embedding model (10 idle minutes) had no
//! vectors in its recall, since recall waits 0 ms for a load.
//!
//! So each surface sends one cheap `session.typing` on a first keystroke,
//! and the daemon, never on a turn's path and with nothing written (no
//! frame, no row), opens the session's provider connection if it is cold
//! (a keyless `HEAD`, [`crate::provider::Provider::warm`]) and sends the
//! tender `index.warm`. A session is warmed at most once per idle spell
//! ([`SPELL`]); the notice of anyone but the owner, in a shared place, warms
//! nothing. Health's `warm` block says when each was last warmed; the
//! metric `theseus.warm` counts them by target and outcome.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::Duration;

use theseus_protocol::warm::{WarmHealth, WarmStamp, SPELL_SECS};
use tokio::time::Instant;

/// A session's idle spell: one warm-up inside it, however many keystrokes.
pub const SPELL: Duration = Duration::from_secs(SPELL_SECS);

/// The sessions that have a spell open, and the last warm-ups, in memory
/// only: a restart starts cold anyway, so there is nothing to keep.
#[derive(Default)]
pub struct Warmth {
    /// Session (or `""`, a new one's first words) to the time its last
    /// accepted notice came.
    spells: Mutex<HashMap<String, Instant>>,
    counts: Mutex<Counts>,
}

#[derive(Default)]
struct Counts {
    started: u64,
    dropped: u64,
    provider: Option<WarmStamp>,
    tender: Option<WarmStamp>,
}

/// How many spells are kept before the lapsed ones are swept.
const SWEEP_AT: usize = 256;

impl Warmth {
    /// Whether a notice for `key` at `now` starts a warm-up: the first in
    /// its spell does, and opens a new one. Counted either way.
    pub fn admit(&self, key: &str, now: Instant) -> bool {
        let mut spells = self.spells.lock().unwrap_or_else(|e| e.into_inner());
        let fresh = spells
            .get(key)
            .is_none_or(|t| now.saturating_duration_since(*t) >= SPELL);
        if fresh {
            if spells.len() >= SWEEP_AT {
                spells.retain(|_, t| now.saturating_duration_since(*t) < SPELL);
            }
            spells.insert(key.to_string(), now);
        }
        drop(spells);
        let mut c = self.counts.lock().unwrap_or_else(|e| e.into_inner());
        if fresh {
            c.started += 1;
        } else {
            c.dropped += 1;
        }
        fresh
    }

    /// A notice that was not admitted, for a reason that is not the spell.
    pub fn refuse(&self) {
        self.counts
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .dropped += 1;
    }

    pub fn provider_warmed(&self, took: Duration, outcome: String) {
        let stamp = stamp(took, outcome);
        self.counts
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .provider = Some(stamp);
    }

    pub fn tender_warmed(&self, took: Duration, outcome: String) {
        let stamp = stamp(took, outcome);
        self.counts.lock().unwrap_or_else(|e| e.into_inner()).tender = Some(stamp);
    }

    /// Health's `warm` block.
    pub fn health(&self) -> WarmHealth {
        let c = self.counts.lock().unwrap_or_else(|e| e.into_inner());
        WarmHealth {
            started: c.started,
            dropped: c.dropped,
            provider: c.provider.clone(),
            tender: c.tender.clone(),
        }
    }
}

fn stamp(took: Duration, outcome: String) -> WarmStamp {
    WarmStamp {
        at_ms: theseus_protocol::now_unix_ms(),
        took_ms: took.as_millis() as u64,
        outcome,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test(start_paused = true)]
    async fn a_session_is_admitted_once_per_spell() {
        let w = Warmth::default();
        let t0 = Instant::now();
        assert!(w.admit("ses_a", t0));
        assert!(!w.admit("ses_a", t0 + Duration::from_secs(1)));
        assert!(!w.admit("ses_a", t0 + SPELL - Duration::from_secs(1)));
        assert!(
            w.admit("ses_b", t0 + Duration::from_secs(2)),
            "its own spell"
        );
        assert!(w.admit("ses_a", t0 + SPELL), "the spell ended");
        let h = w.health();
        assert_eq!((h.started, h.dropped), (3, 2));
    }

    #[tokio::test(start_paused = true)]
    async fn lapsed_spells_are_swept_at_the_bound() {
        let w = Warmth::default();
        let t0 = Instant::now();
        for i in 0..SWEEP_AT {
            assert!(w.admit(&format!("ses_{i}"), t0));
        }
        let later = t0 + SPELL;
        assert!(w.admit("ses_new", later));
        assert_eq!(w.spells.lock().unwrap().len(), 1, "the lapsed were swept");
    }
}
