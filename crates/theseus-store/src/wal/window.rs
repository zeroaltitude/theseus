//! How `verify_history` reads a segment: a window at a time
//! (theseus-mem-daemon-rest).

use super::{u32_at, Layout, FRAME_HEADER};

/// How much of a segment `verify_history` reads at once. A segment is up to
/// `WalConfig::segment_bytes` (64 MiB), and the check kept all of it, so a
/// daemon held about that much, and what the allocator kept after, for the
/// check's first seconds (theseus-mem-daemon-rest). A frame bigger than the
/// window is read whole, so the peak is the larger of the two.
pub(super) const WINDOW_BYTES: usize = 4 << 20;

/// The bytes the frame at `off` needs from its start when the window it was
/// read in (which begins at offset `at` of a segment read to `to`) cut it
/// short, and the rest of it is in the segment. `None` when the frame is
/// what is wrong, not the window: a whole frame that does not check, bad
/// magic, or a stated length that runs past the segment's end.
pub(super) fn cut_by_window(bytes: &[u8], off: usize, at: u64, to: u64) -> Option<u64> {
    if off + FRAME_HEADER > bytes.len() {
        return Some(FRAME_HEADER as u64);
    }
    Layout::at(bytes, off)?;
    let need = FRAME_HEADER as u64 + u64::from(u32_at(bytes, off + 4));
    let (start, held) = (at + off as u64, (bytes.len() - off) as u64);
    (need > held && start + need <= to).then_some(need)
}
