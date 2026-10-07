//! How long the writer took over each frame, kept for a turn to read at its
//! end (theseus-w7dk).
//!
//! A frame's time is what the WAL can't hold: a frame is one `fdatasync`, and
//! a lone slow turn is a frame whose sync stalled or time spent outside the
//! frames. The writer reads its clock twice a batch (before the write, after
//! the index) and notes each frame it answered here, in a small ring: its
//! first position, the batch's time, and when it was answered. A turn asks
//! for the slowest frame answered since it arrived and puts it on its trace.
//! A frame in a batch shares the batch's time: one sync commits them all.

use std::sync::Mutex;
use std::time::Instant;

/// Frames kept; a turn that writes more than this since it arrived loses its
/// oldest.
const KEPT: usize = 64;

/// One frame's time in the writer, from its batch's first write to the
/// batch's last index write (the write, the sync, and the index's commit).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FrameTime {
    /// The frame's first record's position.
    pub first: u64,
    /// The batch's time in the writer, in microseconds.
    pub us: u64,
}

#[derive(Default)]
pub(crate) struct FrameTimes(Mutex<Ring>);

#[derive(Default)]
struct Ring {
    seen: Vec<(Instant, FrameTime)>,
    next: usize,
}

impl FrameTimes {
    /// Note a frame answered now.
    pub(crate) fn note(&self, at: Instant, frame: FrameTime) {
        let mut r = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if r.seen.len() < KEPT {
            r.seen.push((at, frame));
        } else {
            let i = r.next;
            r.seen[i] = (at, frame);
            r.next = (i + 1) % KEPT;
        }
    }

    /// The slowest frame answered at or after `since`; the earliest of equals.
    pub(crate) fn slowest_since(&self, since: Instant) -> Option<FrameTime> {
        let r = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        r.seen
            .iter()
            .filter(|(at, _)| *at >= since)
            .map(|(_, f)| *f)
            .reduce(|a, b| if b.us > a.us { b } else { a })
    }
}
