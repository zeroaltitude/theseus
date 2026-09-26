//! Time as a dependency. The kernel never reads the wall clock directly, so
//! the simulator can run days of wakes and deadlines in milliseconds and
//! every run is reproducible from a seed.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

pub trait Clock: Send + Sync {
    /// Milliseconds since the Unix epoch (or since the simulator's origin).
    fn now_ms(&self) -> u64;
}

/// The real wall clock.
pub struct RealClock;

impl Clock for RealClock {
    fn now_ms(&self) -> u64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0)
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
