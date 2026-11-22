//! A piece released by the owner's own hand (issue 44, the design's step 3b; `docs/design/release-card.md`):
//! one exchange of a private place of his, released to one other place, by him, and read by the gate there.
//!
//! - **The record** (`release.id.<id>`): the exchange, the place it is from, the place it is released to, the
//!   surface he released it from, and when. Its index (`release.to.<place>\u{1f}<exchange>`) names the record's
//!   id, so the gate reads one keyed record for one exchange in one place. Written ONLY by the owner in person
//!   (the caller's proof: `places::owner_in_person_of`, the same as his other acts; an agent's process, a job,
//!   a channel and the MCP server are refused before anything is read). One record for one exchange in one
//!   place, ever: a second release of it there is refused, withdrawn or not.
//! - **What may be released**: an exchange of a private place whose person is the owner by his mark, to a place
//!   other than its own, not held by a seal (its session not proven clear, `Core::ladder_hidden`) or by a
//!   message of it moved into a thread not proven clear (`view_gate::Moved`). The same checks are asked again at
//!   every read ([`admitted`] beside `history_tool::page::Lens::admits`), failing closed.
//! - **Withdrawal** writes a second record (`release.end.<id>`); nothing is rewritten. From then on the piece is
//!   not admissible in that place. What a model was already sent cannot be unsent. An unknown id, or one already
//!   withdrawn, writes nothing.
//! - **The gate's reading** ([`live`], [`admitted`]): in the place a record names, that one exchange of his and
//!   nothing else of its place is admissible; everywhere else nothing changes.

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use theseus_store::{kinds, NewRecord, Store as _};

use crate::exchange::{self, Exchange, OWNER};
use crate::store::Store;

const ID_PREFIX: &str = "release.id.";
const TO_PREFIX: &str = "release.to.";
const END_PREFIX: &str = "release.end.";
const SEP: char = '\u{1f}';

/// What the owner sees beside a released piece, wherever it is rendered.
pub const LABEL: &str = "released here by the owner from another conversation";
/// What a withdrawal says.
pub const UNSENT: &str =
    "it is no longer readable there; what Theseus was already shown or sent there cannot be unsent";

/// One release.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Release {
    pub id: String,
    /// The exchange, by its turn.
    pub exchange: String,
    /// The private place it is from, and the place it is released to, as the ledger names places.
    pub from: String,
    pub to: String,
    /// The surface he released it from (`cli`, `web`, `discord`).
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

fn to_key(to: &str, exchange: &str) -> String {
    format!("{TO_PREFIX}{to}{SEP}{exchange}")
}

/// Why `ex` may not be released to `to` today, by the rules a release and every read share; none when it may.
/// `held` is the seal's answer for it (its session or a moved message of it not proven clear).
pub fn refusal(ex: &Exchange, to: &str, held: bool) -> Option<&'static str> {
    if ex.person.as_deref() != Some(OWNER) {
        return Some("only the owner's own words, by his mark, are released");
    }
    if !ex.private {
        return Some("only a private place's exchange is released");
    }
    if ex.place == to || to.trim().is_empty() || ex.place == exchange::rebuild::UNKNOWN {
        return Some("a piece is released to a place other than its own");
    }
    if held {
        return Some("it is in a sealed thread, or its seal cannot be proven clear");
    }
    None
}

/// The release of `exchange` to `to`, when there is one and it is not withdrawn. Fails closed: a record that
/// does not read is no release.
pub fn live(store: &Store, to: &str, exchange: &str) -> Option<Release> {
    let id: String = store.get_meta(&to_key(to, exchange)).ok()??;
    let r: Release = store.get_meta(&format!("{ID_PREFIX}{id}")).ok()??;
    let ended = store.get_meta::<Ended>(&format!("{END_PREFIX}{id}"));
    match ended {
        Ok(None) if r.to == to && r.exchange == exchange => Some(r),
        _ => None,
    }
}

/// Whether `ex` is admissible in `to` by a release: a live record, and the rules again. `held` is the seal's
/// answer for it now.
pub fn admitted(store: &Store, to: &str, ex: &Exchange, held: bool) -> bool {
    refusal(ex, to, held).is_none() && live(store, to, &ex.turn_id).is_some()
}

/// The exchanges released to `to` and not withdrawn, newest release first.
pub fn released_to(store: &Store, to: &str) -> Vec<Release> {
    let prefix = format!("{TO_PREFIX}{to}{SEP}");
    let Ok(recs) = store.inner().first_with_prefix(kinds::META, &prefix, 256) else {
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
        .filter_map(|r| live(store, to, &r.exchange))
        .collect();
    out.sort_by(|a, b| b.at_ms.cmp(&a.at_ms).then(a.id.cmp(&b.id)));
    out
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

/// Writes the owner's release of `exchange` to `to`. The CALLER has proven the owner in person; `held` is the
/// seal's answer for the exchange now.
pub fn grant(
    store: &Store,
    turn: &str,
    to: &str,
    surface: &str,
    held: &dyn Fn(&Exchange) -> bool,
) -> Result<Release> {
    let ex = exchange::exchange(store, turn)?.with_context(|| format!("no exchange {turn}"))?;
    if let Some(why) = refusal(&ex, to, held(&ex)) {
        bail!("not released: {why}");
    }
    if store.get_meta::<String>(&to_key(to, turn))?.is_some() {
        bail!("not released: this piece was released to {to} once already; a release is made once");
    }
    let r = Release {
        id: format!("rel_{}", uuid::Uuid::now_v7().simple()),
        exchange: turn.into(),
        from: ex.place,
        to: to.into(),
        surface: surface.into(),
        at_ms: theseus_protocol::now_unix_ms(),
    };
    store.append(&[
        NewRecord::json(kinds::META, Some(&format!("{ID_PREFIX}{}", r.id)), &r)?,
        NewRecord::json(kinds::META, Some(&to_key(to, turn)), &r.id)?,
    ])?;
    Ok(r)
}

/// Writes the owner's withdrawal of release `id`. The CALLER has proven the owner in person.
pub fn withdraw(store: &Store, id: &str, surface: &str) -> Result<Ended> {
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
