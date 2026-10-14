//! How far ahead and how many wakes (theseus-8ux6x): `[kernel]
//! max_wake_ahead_days` (365, the old 30-day constant's replacement, which a
//! deferred task's start reads too), `max_wakes` (5 check-ins a conversation),
//! `max_reminders` (100 over the daemon), and `max_watches`
//! (20, declared for its row); the routines' cap moved to `[routines]`. The kernel reads them as
//! `KernelConfig::wakes`.

use super::KernelSection;
use theseus_kernel::WakeLimits;

pub(super) fn default_max_ahead_days() -> u64 {
    365
}
pub(super) fn default_max_wakes() -> usize {
    5
}
pub(super) fn default_max_reminders() -> usize {
    100
}
pub(super) fn default_max_watches() -> usize {
    20
}

/// The kernel's limits from the section: at least a day, at least one of each.
pub(super) fn limits(k: &KernelSection) -> WakeLimits {
    WakeLimits {
        max_ahead_ms: k.max_wake_ahead_days.max(1).saturating_mul(86_400_000),
        max_wakes: k.max_wakes.max(1),
        max_reminders: k.max_reminders.max(1),
        // The routines' cap is `[routines] max_routines` (set where the kernel is built).
        max_routines: WakeLimits::default().max_routines,
        max_watches: k.max_watches.max(1),
    }
}
