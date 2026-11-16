//! A trim found the resident set over the floor (issue 21): one
//! `resident.peak` row naming what ran since the trim before
//! (`resident::activity`), so the next peak names its own cause.
//! `Core::record_peak` records it; `docs/design/memory-peak-record.md` says
//! how to read it.

use serde_json::{json, Value};
use theseus_protocol::LedgerKind;

use super::Fact;
use crate::resident::activity::Window;

/// The resident set a trim found, and the window of work before it.
pub struct PeakNamed<'a> {
    pub window: &'a Window,
    pub rss_before_bytes: u64,
    /// What the trim left; `None` where no trim ran (the free heap was under
    /// its floor, or the build has no trim): the memory is live, not freed.
    pub rss_after_bytes: Option<u64>,
    pub floor_bytes: u64,
}

impl Fact for PeakNamed<'_> {
    const KIND: Option<LedgerKind> = Some(LedgerKind::ResidentPeak);

    fn row(&self) -> Value {
        let w = self.window;
        json!({"rss_before_bytes": self.rss_before_bytes,
            "rss_after_bytes": self.rss_after_bytes,
            "floor_bytes": self.floor_bytes,
            "since_ms": w.since_ms, "until_ms": w.until_ms,
            "ran": w.entries.iter().map(|e| e.to_json()).collect::<Vec<_>>(),
            "left_out": w.left_out})
    }
}
