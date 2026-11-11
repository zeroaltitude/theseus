//! An imported session that grew (theseus-pjmj7): the same episode id with
//! another hash, whose messages begin with the ones already stored.
//!
//! - **The prefix is proven by reading the stored message nodes**, not by
//!   a digest kept on the session: the session's nodes in the order they
//!   were written must equal the new episode's first messages, each with the
//!   same `idx` and `sha256`. The read costs the session's nodes (one
//!   cached decode each) and only when an episode's hash changed.
//! - **Only the new messages are written**, as nodes `node_id_of(session,
//!   idx)` at their own times, with the session's record and the tag's
//!   counts in the same frame (an episode's records are never cut across
//!   two frames). The session keeps the tag, place, labels and `file` it
//!   was first imported with; its hash, message count and `as_of.end_ms`
//!   move to the new episode's, and its `last_active_ms` with it.
//! - **A summary** is written again, under the same node id, only when the
//!   new episode has one whose text differs from the stored summary's.
//! - **A prefix that diverged** (a rewrite, a compaction, a resume;
//!   theseus-ectzg) appends the messages after the last stored one whose
//!   digest still matches the episode's message of that `idx`, never a
//!   stored `idx` again, and records N, the first stored `idx` that
//!   differs, once, in the session's `diverged_at`. That is a field of its
//!   own, the one field this step adds to a stored record: `labels.topic` is
//!   the ontology's input, and `import.topics` would make a topic of any
//!   entry in it.
//! - **A divergence with nothing after the last match is taken quietly**: no
//!   node is written, the session's hash moves to the episode's and it takes
//!   the mark, so the next run finds the hash held and skips it. It counts in
//!   the batch's `appended` (a take) but not in `touched` (nothing is new to
//!   the people step). Refused, a session that was rewritten and then went
//!   quiet would be rejected every night.
//! - **An episode that holds exactly the stored messages and differs only in
//!   its tag, place or labels is taken the same quiet way** (the owner's
//!   word, 2026-11-10): the hash moves, no node is written, no mark. The
//!   monthly import tag is in the episode's hash and is the month of the
//!   *run*, so without this a session first imported in one month is offered
//!   and refused every night of the next, forever. Two conditions, both
//!   needed: `e.messages.len() == stored.len()`, never that nothing counted
//!   as added (an episode that gained a message at a stored idx adds nothing
//!   by that count too, and taking it would drop that message); and a source
//!   whose digests this build took (`episode::DIGESTED_SOURCES`), so that
//!   equal digests are provably the same text rather than the client's claim.
//! - **A renumbered episode is refused** (a stored message moved to another
//!   idx: an insertion, or a compaction that renumbers): appending past it
//!   would drop the new message with the shifted ones and write a duplicate.
//!   Refusing is the whole rule: no message is re-mapped.
//! - **Anything else is refused and named**: an episode where no stored
//!   message matches (another episode under a reused id), an episode that is
//!   only the stored session's first messages (an older or cut view: taken,
//!   it would move the hash back), an episode whose prefix matched whole and
//!   which holds more messages than the store, each at a stored idx (nothing
//!   counts as added, and taking it would drop them), or one that adds
//!   nothing and whose digests this build did not take. An erased session is
//!   never appended to.

use std::collections::BTreeMap;

use anyhow::Result;
use theseus_store::{kinds, NewRecord};

use super::anchor;
use super::episode::Episode;
use super::write::Frame;
use super::{node_id_of, summary_id_of, tag_scope};
use crate::node::{Body, Node};
use crate::session::SessionRecord;
use crate::store::Store;

/// What an append would write: where the new messages start in the
/// episode, and its new summary. Nothing at all to write but the session's
/// record is a plan too, when the prefix diverged (`diverged`).
pub(super) struct Plan {
    /// The episode's messages before this index are stored already.
    pub from: usize,
    /// Stored message idxs: never written again.
    pub skip: std::collections::BTreeSet<u32>,
    /// The stored prefix no longer matched: the first stored idx that differs.
    pub diverged: Option<u32>,
    /// A summary to write: the episode has one the store does not.
    pub summary: bool,
    /// A renumbered episode (`anchor`): the idx each of its messages is stored
    /// at, by the episode's idx, the shared ones and the new; else empty.
    pub idx_of: BTreeMap<u32, u32>,
}

/// Where the new messages start, which stored idxs are never written again,
/// the divergence, and the idxs a renumbered episode's new messages take.
struct Route {
    from: usize,
    skip: std::collections::BTreeSet<u32>,
    diverged: Option<u32>,
    idx_of: BTreeMap<u32, u32>,
}

/// Whether `e` extends the stored session `rec`: a plan, or why not.
pub(super) fn plan(
    store: &Store,
    rec: &SessionRecord,
    e: &Episode,
) -> Result<std::result::Result<Plan, String>> {
    let sid = &rec.session_id;
    // The stored messages in the order they were written: idx and digest.
    let mut stored: Vec<(u32, String)> = Vec::new();
    for (_, n) in store.session_nodes(sid)? {
        if let Body::Imported { idx, sha256, .. } = n.body {
            stored.push((idx, sha256));
        }
    }
    let Route {
        from,
        skip,
        diverged,
        idx_of,
    } = match route(&stored, e) {
        Ok(r) => r,
        Err(why) => return Ok(Err(why)),
    };
    let added = e.messages[from..]
        .iter()
        .filter(|m| !skip.contains(&m.idx))
        .count();
    let summary = match e.summary.as_ref().filter(|s| !s.text.trim().is_empty()) {
        None => false,
        Some(s) => match store.get_node(&summary_id_of(sid))? {
            Some((_, n)) => {
                !matches!(&n.body, Body::ImportedSummary { text, .. } if *text == s.text)
            }
            None => true,
        },
    };
    // A diverged episode with nothing to add is taken quietly, not refused:
    // `records` moves the session to its hash and marks it, so the next run
    // finds the hash held and skips it. Refused instead, a session that was
    // rewritten and then went quiet would be rejected every night.
    if added == 0 && !summary && diverged.is_none() {
        // An episode that holds EXACTLY the stored messages and differs only
        // in its tag, place or labels is taken quietly too: the hash moves,
        // no node is written, the next run finds the hash held and skips it.
        // The monthly import tag rolls over — `scripts/openclaw-export.py`
        // tags an episode by the clock of the RUN, not of the session, and
        // the tag is in the hash — so refused, a session first imported in
        // one month was offered and refused every night of the next, forever.
        // The append has always kept the tag the session was first imported
        // with (the head of this file), so nothing is lost by taking it.
        //
        // Two conditions, and it takes both:
        //
        // 1. **The LENGTH, never `added == 0` on its own.** `added` counts
        //    only the episode's messages past the stored prefix whose idx is
        //    not stored already, so an episode that gained a message AT a
        //    stored idx adds nothing by that count as well; taking that would
        //    move the hash while dropping the new message, the silent loss
        //    `moved` refuses in the diverged arm (`moved` is not reached
        //    here, because the prefix matched whole). With
        //    `e.messages.len() == stored.len()` there is nothing past the
        //    prefix at all, so there is nothing to lose — structurally.
        // 2. **This build took the digests** (`episode::DIGESTED_SOURCES`,
        //    `write::digested`). Then both the stored digest and the
        //    episode's are sha256 of that message's own text, so equal
        //    digests are the same text and "holds exactly the stored
        //    messages" is a fact. For any other source the digests are the
        //    client's claim and the import never checks them: an episode
        //    whose text changed while its declared digest did not would be
        //    taken quietly and its new text lost for good. Refusing that is
        //    both the safe answer and the one on main.
        let whole = e.messages.len() == stored.len();
        if whole && super::episode::DIGESTED_SOURCES.contains(&e.source.as_str()) {
            // `idx_of` is `route`'s own: empty on this path, because a
            // matched prefix renumbers nothing. Carried rather than written
            // afresh, so the two `Plan` literals stay one shape.
            return Ok(Ok(Plan {
                from,
                skip,
                summary,
                diverged,
                idx_of,
            }));
        }
        if !whole {
            return Ok(Err(format!(
                "its {from} stored messages are this episode's first ones and each of its {} \
                 further messages is at a stored idx: not appended over",
                e.messages.len() - from
            )));
        }
        return Ok(Err(format!(
            "its {from} messages are the stored ones and it adds nothing (another hash: its tag, \
             place or labels differ): not overwritten"
        )));
    }
    Ok(Ok(Plan {
        from,
        skip,
        summary,
        diverged,
        idx_of,
    }))
}

/// Where the episode's new messages start, or why it is refused.
fn route(stored: &[(u32, String)], e: &Episode) -> std::result::Result<Route, String> {
    let route = |from, diverged, skip, idx_of| Route {
        from,
        skip,
        diverged,
        idx_of,
    };
    let Some(why) = prefix_refusal(stored, e) else {
        let skip = stored.iter().map(|(i, _)| *i).collect();
        return Ok(route(stored.len(), None, skip, BTreeMap::new()));
    };
    // The episode is only the stored session's own first messages: an older
    // or cut view of it, not a rewrite. Taken, it would move the session's
    // hash back, and the episode the source holds now would be refused for
    // adding nothing, for good.
    let cut = e.messages.len() < stored.len()
        && e.messages
            .iter()
            .zip(stored)
            .all(|(m, (i, s))| m.idx == *i && m.sha256 == *s);
    if cut {
        return Err(why);
    }
    let at = first_difference(stored, e);
    if moved(stored, e).is_some() {
        // The source renumbered: the stored end is read from content.
        // Refused with anchor::find's own words when the content does not settle it.
        let a = anchor::find(stored, e)?;
        return Ok(route(a.from, Some(at), Default::default(), a.idx_of));
    }
    // The prefix no longer matches in place (a rewrite): the new turns are
    // those after the last stored message whose digest still matches at its
    // idx. No match at all is another episode under a reused id.
    let last = e
        .messages
        .iter()
        .rposition(|m| stored.iter().any(|(i, s)| *i == m.idx && *s == m.sha256))
        .ok_or(why)?;
    // A stored idx is never written again, whatever its digest.
    let skip = stored.iter().map(|(i, _)| *i).collect();
    Ok(route(last + 1, Some(at), skip, BTreeMap::new()))
}

/// The first stored idx whose message is not the episode's at its position.
fn first_difference(stored: &[(u32, String)], e: &Episode) -> u32 {
    stored
        .iter()
        .enumerate()
        .find(|(i, (idx, sha))| {
            e.messages
                .get(*i)
                .is_none_or(|m| m.idx != *idx || m.sha256 != *sha)
        })
        .map_or(0, |(_, (idx, _))| *idx)
}

/// The new messages' nodes (and a new summary), then the session's record
/// as it stands after them, added to `frame`: how many nodes. `rec` is the
/// stored record, changed in place.
pub(super) fn records(
    e: &Episode,
    rec: &mut SessionRecord,
    p: &Plan,
    frame: &mut Frame,
) -> Result<u64> {
    let sid = rec.session_id.clone();
    let mut nodes = 0;
    let mut written = 0u32;
    // On a renumbered episode (`p.idx_of` non-empty) a pre-anchor message the
    // source added -- a compaction's own summary turn, say -- has no entry:
    // it was never imported, so `unwrap_or(m.idx)` would alias its own local
    // idx onto an unrelated stored message's. Left out of `ids` instead, a
    // cite naming it is dropped below rather than pointed at the wrong node.
    let mut ids: BTreeMap<u32, String> = BTreeMap::new();
    for m in &e.messages {
        let idx = match p.idx_of.get(&m.idx) {
            Some(&i) => i,
            None if p.idx_of.is_empty() => m.idx,
            None => continue,
        };
        ids.insert(m.idx, node_id_of(&sid, idx));
    }
    for m in e.messages[p.from..]
        .iter()
        .filter(|m| !p.skip.contains(&m.idx))
    {
        let idx = p.idx_of.get(&m.idx).copied().unwrap_or(m.idx);
        let n = Node::imported(
            node_id_of(&sid, idx),
            &sid,
            &m.author,
            m.at_ms,
            Body::Imported {
                text: m.text.clone(),
                integrity: m.integrity,
                source: e.source.clone(),
                unit: m.unit.clone(),
                sha256: m.sha256.clone(),
                idx,
            },
        );
        frame.push(n.record()?);
        nodes += 1;
        written += 1;
    }
    if p.summary {
        if let Some(s) = e.summary.as_ref() {
            let n = Node::imported(
                summary_id_of(&sid),
                &sid,
                &format!("summary:{}", s.model),
                e.as_of.end_ms,
                Body::ImportedSummary {
                    text: s.text.clone(),
                    cites: s.cites.iter().filter_map(|c| ids.get(c).cloned()).collect(),
                    model: s.model.clone(),
                },
            );
            frame.push(n.record()?);
            nodes += 1;
        }
    }
    let imp = rec.imported.as_mut().expect("an imported session");
    imp.hash = e.hash.clone();
    imp.messages += written;
    imp.summary |= p.summary;
    if let Some(at) = p.diverged {
        if !imp.diverged_at.contains(&at) {
            imp.diverged_at.push(at);
        }
    }
    imp.as_of.end_ms = imp.as_of.end_ms.max(e.as_of.end_ms);
    let (tag, end) = (imp.tag.clone(), imp.as_of.end_ms);
    rec.last_active_ms = rec.last_active_ms.max(end);
    frame.push(NewRecord::json(kinds::SESSION, Some(&sid), &*rec)?.scoped(&tag_scope(&tag)));
    Ok(nodes)
}

/// A stored message the source renumbered, named: a stored `(i, d)` whose
/// episode message at idx `i` is absent or another digest, while `d` sits in
/// the episode at another idx `j` whose stored digest (if `j` is stored) is
/// not `d`. Keyed by idx, never position: a text kept at its idx is no move.
/// Appending past it would drop the new message with the shifted ones and
/// write a duplicate, so the episode is refused.
fn moved(stored: &[(u32, String)], e: &Episode) -> Option<String> {
    let at: BTreeMap<u32, &str> = stored.iter().map(|(i, d)| (*i, d.as_str())).collect();
    for (i, d) in stored {
        if e.messages.iter().any(|m| m.idx == *i && m.sha256 == *d) {
            continue;
        }
        let Some(j) = e.messages.iter().find(|m| {
            m.idx != *i && m.sha256 == *d && at.get(&m.idx).is_none_or(|s| *s != m.sha256)
        }) else {
            continue;
        };
        return Some(format!(
            "message {} holds what stored message {i} held (the source renumbered): not appended over",
            j.idx
        ));
    }
    None
}

/// Why the stored messages are not the episode's first ones, if they are not.
fn prefix_refusal(stored: &[(u32, String)], e: &Episode) -> Option<String> {
    for (i, (idx, sha)) in stored.iter().enumerate() {
        match e.messages.get(i) {
            None => {
                return Some(format!(
                    "message {idx} is not in this episode (the stored one holds {} messages, \
                     this {}): a removed message is not appended over",
                    stored.len(),
                    e.messages.len()
                ))
            }
            Some(m) if m.idx != *idx => {
                return Some(format!(
                    "message {} is not where the stored episode has it (it holds message {idx} \
                     at position {i}): a changed or reordered message is not appended over",
                    m.idx
                ))
            }
            Some(m) if m.sha256 != *sha => {
                return Some(format!(
                    "message {} changed since it was imported (another digest): a rewritten \
                     message is not appended over",
                    m.idx
                ))
            }
            Some(_) => {}
        }
    }
    None
}
