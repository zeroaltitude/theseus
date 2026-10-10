//! `[tools] outputs_keep_days` and `outputs_max_bytes`: how long a capped
//! result's kept output lives (theseus-v73m; `crate::outputs`).

/// A week.
pub(super) fn keep_days() -> u64 {
    7
}

/// 2 GiB of kept outputs in all.
pub(super) fn max_bytes() -> u64 {
    2 * 1024 * 1024 * 1024
}
