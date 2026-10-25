//! The Flux path counts time in the engine's tick, `FRAME`, not in a literal.

use std::time::Duration;

use super::flux_path::ticks_in;
use crate::audio::FRAME;

/// A silence's tick count is its length divided by `FRAME`, rounded down: it
/// guards the derivation, so a changed `FRAME` moves the count with it.
#[test]
fn a_silence_is_counted_in_frames() {
    for (silence, ticks) in [(0, 0), (19, 0), (20, 1), (1_000, 50)] {
        let silence = Duration::from_millis(silence);
        assert_eq!(
            ticks_in(silence),
            silence.as_millis() as u64 / FRAME.as_millis() as u64
        );
        assert_eq!(ticks_in(silence), ticks);
    }
}
