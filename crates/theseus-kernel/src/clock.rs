//! Time as a dependency. The kernel never reads the wall clock directly, so
//! the simulator can run days of wakes and deadlines in milliseconds and
//! every run is reproducible from a seed.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

pub trait Clock: Send + Sync {
    /// Milliseconds since the Unix epoch (or since the simulator's origin).
    fn now_ms(&self) -> u64;
}

/// The real wall clock: the protocol's, the one every other reader takes, so a test that moves it
/// (`theseus_protocol::time::move_clock_to`) moves the kernel's stamps with the rest.
pub struct RealClock;

impl Clock for RealClock {
    fn now_ms(&self) -> u64 {
        theseus_protocol::now_unix_ms()
    }
}

/// A clock that only moves when told to. Shared by `Arc` between the
/// simulator's scheduler and the kernel under test.
#[derive(Default)]
pub struct VirtualClock {
    now: AtomicU64,
}

impl VirtualClock {
    pub fn new(start_ms: u64) -> Arc<Self> {
        Arc::new(Self {
            now: AtomicU64::new(start_ms),
        })
    }
    pub fn advance(&self, ms: u64) -> u64 {
        self.now.fetch_add(ms, Ordering::SeqCst) + ms
    }
    pub fn set(&self, ms: u64) {
        self.now.store(ms, Ordering::SeqCst);
    }
}

impl Clock for VirtualClock {
    fn now_ms(&self) -> u64 {
        self.now.load(Ordering::SeqCst)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_real_clock_is_the_protocols_and_follows_a_move_of_it() {
        let at = 4_102_444_800_000;
        theseus_protocol::time::move_clock_to(at);
        let read = RealClock.now_ms();
        theseus_protocol::time::reset_clock();
        assert!(
            (at..at + 5_000).contains(&read),
            "{read} is not where the clock was put, {at}"
        );
        assert!(
            RealClock.now_ms() < at,
            "and back on the wall clock after a reset"
        );
    }
}
