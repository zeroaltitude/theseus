//! The owner's release of one message to one place (issue 44; `docs/design/release-card.md`), as stored.
//!
//! The verb that wrote releases (`exchange.offer`, `exchange.release`, `exchange.releases`, `exchange.withdraw`)
//! was retired on 2026-11-22, and with it the history tool's lens over them: every place now reads everything,
//! so nothing needs to be let through one message at a time. What stays is **the record**
//! (`release.id.<id>`, indexed by `release.to.<place>\u{1f}<node>`, ended by `release.end.<id>`): [`Release`]
//! and [`Ended`] still decode, and nothing is rewritten.

use serde::{Deserialize, Serialize};

/// One release.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Release {
    pub id: String,
    /// The message, by its node, and the digest of its words as the owner was shown them.
    pub node: String,
    pub digest: String,
    /// The private place it is from, and the place it is released to, as the ledger names places.
    pub from: String,
    pub to: String,
    /// The surface he released it from (`cli`, `web`).
    pub surface: String,
    pub at_ms: u64,
}

/// A release's end.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Ended {
    pub id: String,
    pub surface: String,
    pub at_ms: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A release and its end written before the verb was retired still decode.
    #[test]
    fn a_stored_release_and_its_end_still_decode() {
        let r: Release = serde_json::from_value(serde_json::json!({
            "id": "rel_1", "node": "n1", "digest": "d", "from": "discord:dm:1",
            "to": "discord:channel:2", "surface": "cli", "at_ms": 5
        }))
        .unwrap();
        assert_eq!(
            (r.id.as_str(), r.to.as_str(), r.at_ms),
            ("rel_1", "discord:channel:2", 5)
        );
        let e: Ended = serde_json::from_value(serde_json::json!({
            "id": "rel_1", "surface": "web", "at_ms": 6
        }))
        .unwrap();
        assert_eq!(e.id, "rel_1");
    }
}
