//! The index gap's state: where the log's frames outrun the index, since when,
//! for how many batches, and why. The log keeps recording through it and
//! nothing is refused; what the gap costs is that no checkpoint claims past it,
//! so the next open replays from the last good checkpoint. It is never
//! invisible: health carries it and the owner is told once (theseus-lu5n; the
//! owner, 2026-11-09, on keeping the daemon recording and telling him instead
//! of stopping it: "Yes, we never want invisibility.").

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;

/// The longest a reason is kept, in characters: health and the owner's post
/// say it whole, and an error chain can be long.
const REASON_CHARS: usize = 240;

/// What the owner is told of an open gap.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IndexGap {
    /// The log's last position when it opened.
    pub position: u64,
    /// When it opened, unix ms.
    pub since_ms: u64,
    /// The batches the log has taken since it opened, the one whose index
    /// write failed included.
    pub batches: u64,
    /// Why the first index write failed.
    pub reason: String,
}

/// The gap, as the store's writer keeps it. The writer alone opens and counts;
/// health and the notice read.
#[derive(Default)]
pub(super) struct Gap {
    /// `IndexGap::position`, 0 while there is none (positions start at 1):
    /// every checkpoint reads it, so it is the one atomic.
    at: AtomicU64,
    open: Mutex<Option<IndexGap>>,
}

/// The clock, unix ms.
fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as u64)
}

impl Gap {
    pub(super) fn is_open(&self) -> bool {
        self.at.load(Ordering::Acquire) != 0
    }

    pub(super) fn position(&self) -> Option<u64> {
        Some(self.at.load(Ordering::Acquire)).filter(|p| *p != 0)
    }

    pub(super) fn get(&self) -> Option<IndexGap> {
        if !self.is_open() {
            return None;
        }
        self.open.lock().unwrap().clone()
    }

    /// A batch whose frames are synced and whose index write failed past its
    /// bound. The first opens the gap, which stands at that failure; a later
    /// one is only counted. True when this one opened it.
    pub(super) fn failed(&self, at: u64, why: &anyhow::Error) -> bool {
        let mut open = self.open.lock().unwrap();
        if let Some(g) = open.as_mut() {
            g.batches += 1;
            return false;
        }
        *open = Some(IndexGap {
            position: at,
            since_ms: now_ms(),
            batches: 1,
            reason: format!("{why:#}").chars().take(REASON_CHARS).collect(),
        });
        self.at.store(at, Ordering::Release);
        true
    }

    /// A batch indexed while the gap stands: the log took it, the index has
    /// it, and no checkpoint may claim it.
    pub(super) fn took(&self) {
        if self.is_open() {
            if let Some(g) = self.open.lock().unwrap().as_mut() {
                g.batches += 1;
            }
        }
    }
}
