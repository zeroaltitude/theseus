//! Your own benchmark (own-benchmark, theseus-pw1q.12; the pitch of
//! killer-features §4.5: "which model you actually need, measured on your own
//! past work, and routes by it"). Parts (a) and (b): a past turn's first
//! request rebuilt byte for byte and sent to another profile
//! (`turn.replay`), `compare.v1` judging the two replies, a nightly sampler
//! that does it one rung down the routing table under a day's cap, and
//! `route.report`.
//!
//! - `rebuild.rs`: the first request, from the compilation, the system
//!   blocks the blobs hold by digest, the tools by the manifest's digest, and
//!   the nodes before the answer. It is rebuilt only when it is the same
//!   bytes: its digest equals the one the answer recorded
//!   (`request_digest`); otherwise the replay is refused and says which part
//!   it could not rebuild. It never guesses.
//! - `replay.rs`: `turn.replay`. It writes a `turn.replayed` row and nothing
//!   else: no node, no session, no execution.
//! - `rung.rs`: the rung table, pure.
//! - `whole/`: part (c), `turn.replay {scope: "whole_turn"}` (theseus-pw1q.20):
//!   the whole turn run again with its tools on a scratch copy of its directory
//!   (`scratch.rs`, `undo.rs`), the programs in L1 and the owner-facing tools
//!   stubbed (`hands.rs`), under caps (`runloop.rs`), compared by words and by
//!   facts (`compare.rs`), and a match becomes a machine label for routing's
//!   learning (`label.rs`).
//! - `sampler.rs`: the nightly pass. `report.rs`: `route.report`.
//!
//! A yardstick, not a self-change: nothing here moves routing. A pack that
//! would climbs the ladder (shadow, canary, live) on its frozen holdout.

pub mod drafts;
pub mod rebuild;
pub mod replay;
pub mod report;
pub mod rung;
pub mod sampler;
pub mod whole;

#[cfg(test)]
mod tests;
#[cfg(test)]
mod tests_sampler;

use theseus_protocol::LedgerKind;

use crate::ledger::LedgerRow;
use crate::rpc::Core;

/// The most a replay's reply may take: a comparison reads a reply, not a
/// session's whole output, and the cap bounds what a replay can cost.
pub const REPLAY_MAX_TOKENS: u32 = 8_192;

/// The `turn.replayed` row's `by` for the owner's own replay and the nightly
/// sampler's.
pub const BY_OWNER: &str = "owner";
pub const BY_SAMPLER: &str = "sampler";

impl Core {
    /// The ledger's rows of `kind` since `since_ms`, oldest first.
    pub(crate) fn rows_since(
        &self,
        kind: LedgerKind,
        since_ms: u64,
    ) -> anyhow::Result<Vec<LedgerRow>> {
        self.ledger_rows(kind, None, Some(since_ms), None)
    }
}

/// A word for an effort, as a profile's config writes it.
pub fn effort_word(e: crate::config::Effort) -> String {
    serde_json::to_value(e)
        .ok()
        .and_then(|v| v.as_str().map(str::to_string))
        .unwrap_or_default()
}

/// An effort from its word.
pub fn parse_effort(s: &str) -> Option<crate::config::Effort> {
    serde_json::from_value(serde_json::Value::String(s.to_string())).ok()
}
