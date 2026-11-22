//! Share this exact message to this place, the owner's own act (issue 44; `docs/design/release-card.md`).
//!
//! - **The record** (`release.id.<id>`, indexed by `release.to.<place>\u{1f}<node>`): ONE message of the owner's,
//!   by its node and a digest of its words as he was shown them, released to ONE place, by him, from a surface, at
//!   a time. An immutable reference: nothing else of its exchange (the reply, another person's steer, a tool's
//!   stub, anything added later) is released with it. Written only through `exchange.release`, behind
//!   `places::owner_in_person_of` (his terminal or the web UI, in person; an agent's process, the MCP server and an
//!   unnamed connection are refused before anything is read).
//! - **One check** ([`piece`]), at the release and at every read, failing closed: the node is a message whose
//!   owner's mark says it is his, of a private place's exchange; its session is neither sealed nor seal-pending as
//!   seen by no one (`Core::ladder_hidden`, with no thread's own exception); and the message was never moved into
//!   any thread (`moved.of.<node>`, whatever the thread's seal). A read also needs the words to match the digest.
//! - **Once.** Grants and withdrawals are serialized, so one message is released to one place by one record,
//!   however many requests race. A withdrawal writes a second record (`release.end.<id>`); nothing is rewritten.
//!   What was already shown or sent cannot be unsent. An unknown or withdrawn id writes nothing.
//! - **The reader** is the history tool in the named place alone ([`listed`], [`opened`]): the message's words
//!   only, labelled [`LABEL`], inside the page's own bounds.

use std::sync::Mutex;

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use theseus_store::{kinds, NewRecord, Store as _};

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
/// What a withdrawal says.
pub const UNSENT: &str =
    "it is no longer readable there; what Theseus was already shown or sent there cannot be unsent";
/// The released messages one history page lists at most.
pub const PER_PAGE: usize = 10;

/// Grants and withdrawals, one at a time: a check and its append are never interleaved with another's.
static WRITES: Mutex<()> = Mutex::new(());

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

/// Whether `node` was ever moved into a thread, whatever its seal; unreadable is moved.
fn moved(store: &Store, node: &str) -> bool {
    !matches!(
        store.get_meta::<Option<crate::moved::MovedTo>>(&crate::moved::moved_of_key(node)),
        Ok(None | Some(None))
    )
}

/// The one check, at the release and at every read: the message `node`, if it may be released today.
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
    if core.ladder_hidden(&n.session_id) || moved(&core.store, node) {
        bail!(
            "it is in a sealed thread, was moved into a thread, or its seal cannot be proven clear"
        );
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

/// What one history page lists in `here`: the readable releases made in `(after, upto]` (release time, the page's
/// own stretch), newest first, [`PER_PAGE`] at most, each `(id, at_ms, words)`; and how many more that stretch holds.
pub fn listed(
    core: &Core,
    here: &str,
    after: u64,
    upto: u64,
) -> (Vec<(String, u64, String)>, usize) {
    let all: Vec<(String, u64, String)> = released_to(&core.store, here)
        .into_iter()
        .filter(|r| r.at_ms > after && r.at_ms <= upto)
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

/// Every release, with its end when withdrawn, newest first.
pub fn all(store: &Store) -> Result<Vec<(Release, Option<Ended>)>> {
    let recs = store
        .inner()
        .first_with_prefix(kinds::META, ID_PREFIX, 1_000)?;
    let mut out = Vec::new();
    for rec in &recs {
        let r: Release = rec.decode()?;
        let end = store.get_meta::<Ended>(&format!("{END_PREFIX}{}", r.id))?;
        out.push((r, end));
    }
    out.sort_by_key(|(r, _)| std::cmp::Reverse(r.at_ms));
    Ok(out)
}

/// Writes the owner's release of the message `node`, whose words he was shown as `shown` (their digest), to `to`.
/// The CALLER has proven the owner in person.
pub fn grant(core: &Core, node: &str, shown: &str, to: &str, surface: &str) -> Result<Release> {
    let _one = WRITES
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let p = piece(core, node).map_err(|e| anyhow::anyhow!("not released: {e:#}"))?;
    if p.digest != shown {
        bail!("not released: the message's words are not the words you were shown");
    }
    if to.trim().is_empty() || to == p.from {
        bail!("not released: a message is released to a place other than its own");
    }
    if core.store.get_meta::<String>(&to_key(to, node))?.is_some() {
        bail!(
            "not released: this message was released to {to} once already; a release is made once"
        );
    }
    let r = Release {
        id: format!("rel_{}", uuid::Uuid::now_v7().simple()),
        node: node.into(),
        digest: p.digest,
        from: p.from,
        to: to.into(),
        surface: surface.into(),
        at_ms: theseus_protocol::now_unix_ms(),
    };
    core.store.append(&[
        NewRecord::json(kinds::META, Some(&format!("{ID_PREFIX}{}", r.id)), &r)?,
        NewRecord::json(kinds::META, Some(&to_key(to, node)), &r.id)?,
    ])?;
    Ok(r)
}

/// Writes the owner's withdrawal of release `id`. The CALLER has proven the owner in person.
pub fn withdraw(store: &Store, id: &str, surface: &str) -> Result<Ended> {
    let _one = WRITES
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let r: Release = store
        .get_meta(&format!("{ID_PREFIX}{id}"))?
        .with_context(|| format!("no release {id}"))?;
    if store
        .get_meta::<Ended>(&format!("{END_PREFIX}{id}"))?
        .is_some()
    {
        bail!("release {id} was withdrawn already");
    }
    let e = Ended {
        id: r.id,
        surface: surface.into(),
        at_ms: theseus_protocol::now_unix_ms(),
    };
    store.append(&[NewRecord::json(
        kinds::META,
        Some(&format!("{END_PREFIX}{id}")),
        &e,
    )?])?;
    Ok(e)
}

#[cfg(test)]
mod tests;
