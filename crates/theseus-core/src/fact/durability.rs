//! The durability tender's facts (AWS step 15): a sealed WAL segment
//! shipped whole to S3 is a `durability.shipped` row, one per segment (a
//! segment is 64 MiB, so a busy day makes a handful). Tails of the open
//! segment, blobs, and index rows are counted in health and telemetry
//! instead: a row each would be a frame each, which ships again. No
//! notification, no sentence, and no span: the tender runs outside every
//! turn.

use serde_json::{json, Value};
use theseus_protocol::LedgerKind;

use super::Fact;

/// A sealed segment, shipped whole: its object, its bytes and SHA-256 (hex),
/// the last position it holds, its parts (1: a single put), and how long
/// it took.
pub struct SegmentShipped<'a> {
    pub segment: u32,
    pub key: &'a str,
    pub bytes: u64,
    pub sha256: &'a str,
    pub last_position: u64,
    pub parts: u32,
    pub took_ms: u64,
    /// The ledger chain's head as the segment shipped, `SEQ:DIGEST`
    /// (theseus-bmkxk): the anchor for `theseus ledger verify --anchor`.
    pub ledger_head: Option<&'a str>,
}

impl Fact for SegmentShipped<'_> {
    const KIND: Option<LedgerKind> = Some(LedgerKind::DurabilityShipped);

    fn row(&self) -> Value {
        let mut row = json!({"segment": self.segment, "key": self.key, "bytes": self.bytes,
            "sha256": self.sha256, "last_position": self.last_position,
            "parts": self.parts, "took_ms": self.took_ms});
        if let Some(h) = self.ledger_head {
            row["ledger_head"] = json!(h);
        }
        row
    }
}
