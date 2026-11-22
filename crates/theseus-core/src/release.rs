//! The owner's release of one message to one place (issue 44; `docs/design/release-card.md`), as stored.
//!
//! The verb that wrote releases (`exchange.offer`, `exchange.release`, `exchange.releases`, `exchange.withdraw`)
//! was retired on 2026-11-22: every place now reads everything, so nothing needs to be let through one message at
//! a time. What stays:
//! - **The record** (`release.id.<id>`, indexed by `release.to.<place>\u{1f}<node>`, ended by `release.end.<id>`):
//!   [`Release`] and [`Ended`] still decode, and nothing is rewritten.
//! - **The reader** the history tool still calls ([`listed`], [`opened`]): a live release's words, labelled
//!   [`LABEL`], when the message is still the owner's own of a private place and its words match the digest. No
//!   seal or move is read any more.

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use theseus_store::{kinds, Store as _};

use crate::exchange::{self, OWNER};
use crate::node::Body;
use crate::store::Store;
use crate::Core;

const ID_PREFIX: &str = "release.id.";
const TO_PREFIX: &str = "release.to.";
const END_PREFIX: &str = "release.end.";
const SEP: char = '\u{1f}';

/// What is shown beside a released message, wherever it is rendered.
pub const LABEL: &str = "released here by the owner from another conversation";
/// The released messages one history page lists at most.
pub const PER_PAGE: usize = 10;

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

/// A message that may be released, as the one check found it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Piece {
    pub node: String,
    pub text: String,
    pub digest: String,
    /// Its place, as the owner's mark names it.
    pub from: String,
    pub at_ms: u64,
}

/// The digest of a message's words.
pub fn digest(text: &str) -> String {
    hex::encode(Sha256::digest(text.as_bytes()))
}

fn to_key(to: &str, node: &str) -> String {
    format!("{TO_PREFIX}{to}{SEP}{node}")
}

/// The one check at every read: the message `node`, if a release of it still reads.
pub fn piece(core: &Core, node: &str) -> Result<Piece> {
    let (_, n) = core
        .store
        .get_node(node)?
        .with_context(|| format!("no message {node}"))?;
    let Body::UserMessage { text, .. } = &n.body else {
        bail!("{node} is not a message");
    };
    let mark = exchange::mark(&core.store, node)?.context("the message has no owner's mark")?;
    if mark.person().as_deref() != Some(OWNER) {
        bail!("only the owner's own words, by his mark, are released");
    }
    let turn = n
        .turn_id
        .as_deref()
        .context("the message belongs to no exchange")?;
    let ex =
        exchange::exchange(&core.store, turn)?.context("the message's exchange cannot be read")?;
    if !ex.private || ex.place == exchange::rebuild::UNKNOWN {
        bail!("only a message of a private place is released");
    }
    Ok(Piece {
        node: node.into(),
        digest: digest(text),
        text: text.clone(),
        from: mark.place,
        at_ms: n.created_at_ms,
    })
}

/// The release of `node` to `to`, when there is one and it is not withdrawn. A record that does not read is none.
pub fn live(store: &Store, to: &str, node: &str) -> Option<Release> {
    let id: String = store.get_meta(&to_key(to, node)).ok()??;
    let r: Release = store.get_meta(&format!("{ID_PREFIX}{id}")).ok()??;
    let ended = store.get_meta::<Ended>(&format!("{END_PREFIX}{id}"));
    match ended {
        Ok(None) if r.to == to && r.node == node => Some(r),
        _ => None,
    }
}

/// The words a live release `r` shows in its place today: the one check again, and the words unchanged.
pub fn readable(core: &Core, r: &Release) -> Option<String> {
    live(&core.store, &r.to, &r.node)?;
    let p = piece(core, &r.node).ok()?;
    (p.digest == r.digest).then_some(p.text)
}

/// The live releases to `to`, newest release first.
pub fn released_to(store: &Store, to: &str) -> Vec<Release> {
    let prefix = format!("{TO_PREFIX}{to}{SEP}");
    let Ok(recs) = store.inner().first_with_prefix(kinds::META, &prefix, 4_096) else {
        return Vec::new();
    };
    let mut out: Vec<Release> = recs
        .iter()
        .filter_map(|r| r.decode::<String>().ok())
        .filter_map(|id| {
            store
                .get_meta::<Release>(&format!("{ID_PREFIX}{id}"))
                .ok()
                .flatten()
        })
        .filter_map(|r| live(store, to, &r.node))
        .collect();
    out.sort_by(|a, b| b.at_ms.cmp(&a.at_ms).then(a.id.cmp(&b.id)));
    out
}

/// What a bare first page of `here` lists: its readable releases, newest first, [`PER_PAGE`] at most, each
/// `(id, at_ms, words)`; and how many more there are. No position of history bounds it.
pub fn listed(core: &Core, here: &str) -> (Vec<(String, u64, String)>, usize) {
    let all: Vec<(String, u64, String)> = released_to(&core.store, here)
        .into_iter()
        .filter_map(|r| readable(core, &r).map(|t| (r.id, r.at_ms, t)))
        .collect();
    let more = all.len().saturating_sub(PER_PAGE);
    (all.into_iter().take(PER_PAGE).collect(), more)
}

/// The words of release `id` as opened in `here`: only a live, readable release to `here`.
pub fn opened(core: &Core, here: &str, id: &str) -> Option<String> {
    let r: Release = core.store.get_meta(&format!("{ID_PREFIX}{id}")).ok()??;
    if r.to != here {
        return None;
    }
    readable(core, &r).map(|t| format!("[{LABEL}]\n{t}"))
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
