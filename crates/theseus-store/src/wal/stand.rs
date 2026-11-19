//! Where the log stands, kept where a reader need not take the writer's lock (the writer holds it across a frame's
//! write, and a disk that stalls holds it as long): the last position, the bytes of every segment, and the segment
//! written last. The writer sets them, under its own lock, at each place it moves them (a frame written, a roll, a cut
//! after a failed sync), so what a reader sees is what the lock would have shown a moment before or after. For a
//! number that is shown (health, the status page, a startup row). Anything that decides what is safe to read from a
//! position keeps `Wal::last_position`, which takes the lock.

use std::sync::atomic::{AtomicU64, Ordering::Relaxed};

use super::Writer;

#[derive(Debug, Default)]
pub(super) struct Stand {
    position: AtomicU64,
    bytes: AtomicU64,
    segments: AtomicU64,
}

impl Stand {
    /// Set to the writer's own numbers; called with its lock held.
    pub(super) fn publish(&self, w: &Writer) {
        self.position.store(w.next_position - 1, Relaxed);
        self.bytes.store(w.total_len, Relaxed);
        self.segments.store(u64::from(w.segment), Relaxed);
    }

    /// The last position, the bytes of all segments, and the segment written last.
    pub(super) fn read(&self) -> (u64, u64, u32) {
        (
            self.position.load(Relaxed),
            self.bytes.load(Relaxed),
            self.segments.load(Relaxed) as u32,
        )
    }
}

/// A test's hold on the writer inside a frame's write: `pass` says it is there and waits to be let go.
#[cfg(test)]
#[derive(Default)]
pub(crate) struct Gate {
    state: std::sync::Mutex<(bool, bool)>,
    changed: std::sync::Condvar,
}

#[cfg(test)]
impl Gate {
    pub(super) fn pass(&self) {
        let mut s = self.state.lock().unwrap();
        s.0 = true;
        self.changed.notify_all();
        while !s.1 {
            s = self.changed.wait(s).unwrap();
        }
    }

    /// Wait until the writer is inside its write.
    pub(crate) fn entered(&self) {
        let mut s = self.state.lock().unwrap();
        while !s.0 {
            s = self.changed.wait(s).unwrap();
        }
    }

    pub(crate) fn release(&self) {
        self.state.lock().unwrap().1 = true;
        self.changed.notify_all();
    }
}

#[cfg(test)]
impl super::Wal {
    /// Hold the writer inside its next frame's write, with its lock, until the gate's `release`.
    pub(crate) fn hold_writes(&self) -> std::sync::Arc<Gate> {
        let gate = std::sync::Arc::<Gate>::default();
        self.w.lock().unwrap().planted.hold = Some(gate.clone());
        gate
    }
}
