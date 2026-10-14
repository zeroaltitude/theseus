//! What the scan reads of the imported episodes (theseus-71ib): the sessions
//! of every tag that holds a source starting with `from`, those that ended in
//! the window, and their owner messages and summaries. A read for a person
//! (the dry run), never a turn's path: it walks the tags' scopes and the
//! nodes of the sessions in the window only.

use anyhow::Result;

use super::{Msg, Seen, Summary};
use crate::import::{write, Integrity};
use crate::node::Body;
use crate::store::Store;

/// The place kinds that make streams.
const STREAM_KINDS: &[&str] = &["dm", "slack-channel", "discord-channel"];

/// The imported episodes of `from` (`openclaw` takes `openclaw-store`,
/// `openclaw-sync`, …) that ended at or after `since_ms`, in session-id order.
pub fn scan(store: &Store, from: &str, since_ms: u64) -> Result<Vec<Seen>> {
    let mut out = Vec::new();
    for tag in write::list(store)?.tags {
        if !tag.sources.keys().any(|s| s.starts_with(from)) {
            continue;
        }
        let sessions = write::sessions_of_tag(store, &tag.tag, &|| false)?.unwrap_or_default();
        for (id, rec) in sessions {
            let Some(i) = rec.imported else { continue };
            if i.erased.is_some()
                || !i.source.starts_with(from)
                || i.as_of.end_ms < since_ms
                || !STREAM_KINDS.contains(&i.place.kind.as_str())
            {
                continue;
            }
            let nodes = crate::import::shown(store, store.session_nodes(&id)?)?;
            let mut owner = Vec::new();
            let mut summary = None;
            for (_, n) in nodes {
                match n.body {
                    Body::Imported {
                        text, integrity, ..
                    } if integrity == Integrity::Operator && !text.trim().is_empty() => {
                        owner.push(Msg {
                            node: n.id,
                            at_ms: n.created_at_ms,
                            text,
                        });
                    }
                    Body::ImportedSummary { text, .. } if !text.trim().is_empty() => {
                        summary = Some(Summary { node: n.id, text });
                    }
                    _ => {}
                }
            }
            owner.sort_by_key(|m| m.at_ms);
            out.push(Seen {
                session_id: id,
                agent: i.agent,
                place_kind: i.place.kind,
                place_name: i.place.name,
                place_id: i.place.id,
                topics: i.labels.topic,
                partner: i.labels.sensitivity == "partner-confidential"
                    || i.labels.partner.is_some(),
                sensitivity: i.labels.sensitivity,
                end_ms: i.as_of.end_ms,
                owner,
                summary,
            });
        }
    }
    Ok(out)
}
