//! The disk's crossings (theseus-f337): when free space under the state dir
//! crosses `[server] disk_warn_mb` or `disk_floor_mb`, or comes back, one
//! ledger row (`disk.low`, `disk.below_floor`, `disk.ok`) and one notice on
//! the operator's lane. The heartbeat's timer calls it, a minute apart, and
//! not a wrapper's notice, which comes in bursts; the check is one `statvfs`.

use serde_json::{json, Value};

use super::Core;
use crate::disk::{Crossing, Level};
use crate::fact;

/// The operator's notice for a crossing: kind `disk`, which theseus-discord's
/// courier words.
pub(crate) fn notice(c: &Crossing) -> Value {
    json!({"kind": "disk", "state": c.state.as_str(), "left": c.left.map(|l| l.as_str()),
        "free_mb": c.free_mb, "total_mb": c.total_mb, "warn_mb": c.warn_mb,
        "floor_mb": c.floor_mb})
}

impl Core {
    /// Write the crossing's row and notice, if free space has crossed a line
    /// since the last call. Returns the crossing.
    pub fn watch_disk(&self) -> Option<Crossing> {
        let c = self.tools.disk.crossing()?;
        let rec = self.rec(None);
        match c.state {
            Level::Ok => rec.record(&fact::disk::DiskOk(&c)),
            Level::Low => rec.record(&fact::disk::DiskLow(&c)),
            Level::BelowFloor => rec.record(&fact::disk::DiskBelowFloor(&c)),
        }
        if let Err(e) = self.outbox.to_operator(None, notice(&c)) {
            tracing::warn!(error = %format!("{e:#}"), "the disk's notice was not written");
        }
        tracing::warn!(
            state = c.state.as_str(),
            free_mb = c.free_mb,
            warn_mb = c.warn_mb,
            floor_mb = c.floor_mb,
            "free space under the state dir crossed a line"
        );
        Some(c)
    }
}
