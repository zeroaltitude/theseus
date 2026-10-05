//! Free space under the state dir crossed a line, or came back
//! (theseus-f337): `disk.low`, `disk.below_floor`, `disk.ok`, one row each
//! time `Disk::crossing` says it did (`rpc/disk_watch.rs`).

use serde_json::{json, Value};
use theseus_protocol::LedgerKind;

use super::Fact;
use crate::disk::Crossing;

/// The row's data, the same under each kind: the free and total space and
/// both lines in MB, and the state it left (none at the first read).
fn row(c: &Crossing) -> Value {
    json!({"free_mb": c.free_mb, "total_mb": c.total_mb, "warn_mb": c.warn_mb,
        "floor_mb": c.floor_mb, "left": c.left.map(|l| l.as_str())})
}

/// Free space fell under `[server] disk_warn_mb`: `disk.low`.
pub struct DiskLow<'a>(pub &'a Crossing);

impl Fact for DiskLow<'_> {
    const KIND: Option<LedgerKind> = Some(LedgerKind::DiskLow);

    fn row(&self) -> Value {
        row(self.0)
    }
}

/// Free space fell under `[server] disk_floor_mb`, where jobs are
/// refused: `disk.below_floor`.
pub struct DiskBelowFloor<'a>(pub &'a Crossing);

impl Fact for DiskBelowFloor<'_> {
    const KIND: Option<LedgerKind> = Some(LedgerKind::DiskBelowFloor);

    fn row(&self) -> Value {
        row(self.0)
    }
}

/// Free space is back over the warning, by its margin: `disk.ok`.
pub struct DiskOk<'a>(pub &'a Crossing);

impl Fact for DiskOk<'_> {
    const KIND: Option<LedgerKind> = Some(LedgerKind::DiskOk);

    fn row(&self) -> Value {
        row(self.0)
    }
}
