//! The tasks the owner holds (`task.pause`): one set in memory, by session,
//! with a count, so a loop boundary pays one atomic load while none is held.
//! The truth is the META key `task.paused.<session>` (`rpc/task_pause.rs`
//! reads it back once, at the first ask).

use std::collections::HashSet;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Mutex;

#[derive(Default)]
pub struct Pauses {
    held: Mutex<HashSet<String>>,
    count: AtomicUsize,
    warmed: AtomicBool,
}

impl Pauses {
    /// Whether the stored holds were read.
    pub fn warmed(&self) -> bool {
        self.warmed.load(Ordering::Acquire)
    }

    /// The stored holds, read once: `held` are the sessions the store says are held.
    pub fn warm(&self, held: impl IntoIterator<Item = String>) {
        let mut g = self.held.lock().unwrap_or_else(|e| e.into_inner());
        if self.warmed() {
            return;
        }
        g.extend(held);
        self.count.store(g.len(), Ordering::Release);
        self.warmed.store(true, Ordering::Release);
    }

    /// Whether any task is held: the loop boundary's one load.
    pub fn any(&self) -> bool {
        self.count.load(Ordering::Acquire) > 0
    }

    pub fn is(&self, session: &str) -> bool {
        self.any()
            && self
                .held
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .contains(session)
    }

    /// Hold or release `session`; whether that changed anything.
    pub fn set(&self, session: &str, on: bool) -> bool {
        let mut g = self.held.lock().unwrap_or_else(|e| e.into_inner());
        let changed = if on {
            g.insert(session.to_string())
        } else {
            g.remove(session)
        };
        self.count.store(g.len(), Ordering::Release);
        changed
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_hold_is_counted_and_released() {
        let p = Pauses::default();
        assert!(!p.any() && !p.is("ses_a"));
        assert!(p.set("ses_a", true));
        assert!(!p.set("ses_a", true), "already held");
        assert!(p.any() && p.is("ses_a") && !p.is("ses_b"));
        assert!(p.set("ses_a", false));
        assert!(!p.any());
    }
}
