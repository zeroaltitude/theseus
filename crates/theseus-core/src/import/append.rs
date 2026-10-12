//! An imported session that grew (theseus-pjmj7): the same episode id with
//! another hash, whose messages begin with the ones already stored.
//!
//! - **The prefix is proven by reading the stored message nodes**, not by
//!   a digest kept on the session: the session's nodes in the order they
//!   were written must equal the new episode's first messages, each with the
//!   same `idx` and `sha256`. The read costs the session's nodes (one
//!   cached decode each) and only when an episode's hash changed; nothing is
//!   added to a stored record, so the store's format is unchanged.
//! - **Only the new messages are written**, as nodes `node_id_of(session,
//!   idx)` at their own times, with the session's record and the tag's
//!   counts in the same frame (an episode's records are never cut across
//!   two frames). The session keeps the tag, place, labels and `file` it
//!   was first imported with; its hash, message count and `as_of.end_ms`
//!   move to the new episode's, and its `last_active_ms` with it.
//! - **A summary** is written again, under the same node id, only when the
//!   new episode has one whose text differs from the stored summary's.
//! - **Anything else is refused and named**: a changed, removed or
//!   reordered message (the first differing `idx`), or an episode that
//!   adds nothing (the same messages with another hash: its tag, place or
//!   labels changed). An erased session is never appended to.

use std::collections::BTreeMap;

use anyhow::Result;
use theseus_store::{kinds, NewRecord};

use super::episode::Episode;
use super::write::Frame;
use super::{node_id_of, summary_id_of, tag_scope};
use crate::node::{Body, Node};
use crate::session::SessionRecord;
use crate::store::Store;

/// What an append would write: where the new messages start in the
/// episode, and its new summary.
pub(super) struct Plan {
    /// The episode's messages before this index are stored already.
    pub from: usize,
    /// A summary to write: the episode has one the store does not.
    pub summary: bool,
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
    for (i, (idx, sha)) in stored.iter().enumerate() {
        match e.messages.get(i) {
            None => {
                return Ok(Err(format!(
                    "message {idx} is not in this episode (the stored one holds {} messages, \
                     this {}): a removed message is not appended over",
                    stored.len(),
                    e.messages.len()
                )))
            }
            Some(m) if m.idx != *idx => {
                return Ok(Err(format!(
                    "message {} is not where the stored episode has it (it holds message {idx} \
                     at position {i}): a changed or reordered message is not appended over",
                    m.idx
                )))
            }
            Some(m) if m.sha256 != *sha => {
                return Ok(Err(format!(
                    "message {} changed since it was imported (another digest): a rewritten \
                     message is not appended over",
                    m.idx
                )))
            }
            Some(_) => {}
        }
    }
    let from = stored.len();
    let summary = match e.summary.as_ref().filter(|s| !s.text.trim().is_empty()) {
        None => false,
        Some(s) => match store.get_node(&summary_id_of(sid))? {
            Some((_, n)) => {
                !matches!(&n.body, Body::ImportedSummary { text, .. } if *text == s.text)
            }
            None => true,
        },
    };
    if from == e.messages.len() && !summary {
        return Ok(Err(format!(
            "its {from} messages are the stored ones and it adds nothing (another hash: its tag, \
             place or labels differ): not overwritten"
        )));
    }
    Ok(Ok(Plan { from, summary }))
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
    let mut ids: BTreeMap<u32, String> = BTreeMap::new();
    for m in &e.messages {
        ids.insert(m.idx, node_id_of(&sid, m.idx));
    }
    for m in &e.messages[p.from..] {
        let n = Node::imported(
            node_id_of(&sid, m.idx),
            &sid,
            &m.author,
            m.at_ms,
            Body::Imported {
                text: m.text.clone(),
                integrity: m.integrity,
                source: e.source.clone(),
                unit: m.unit.clone(),
                sha256: m.sha256.clone(),
                idx: m.idx,
            },
        );
        frame.push(n.record()?);
        nodes += 1;
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
    imp.messages = e.messages.len() as u32;
    imp.summary |= p.summary;
    imp.as_of.end_ms = imp.as_of.end_ms.max(e.as_of.end_ms);
    let (tag, end) = (imp.tag.clone(), imp.as_of.end_ms);
    rec.last_active_ms = rec.last_active_ms.max(end);
    frame.push(NewRecord::json(kinds::SESSION, Some(&sid), &*rec)?.scoped(&tag_scope(&tag)));
    Ok(nodes)
}
