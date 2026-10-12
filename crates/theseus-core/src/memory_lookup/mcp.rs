//! What the MCP server's `memory_search` and `memory_get` read (theseus-02i1h):
//! the same functions `memory.lookup` runs, with their answers as typed hits
//! rather than a page of text, for a client that is another agent.
//!
//! - **Search** is `by_words`, recall's pipeline with the place rule first,
//!   asked as the CLI asks (a private place) and then kept to private
//!   sessions: a private asker may draw on a shared place's sessions, so
//!   `by_words`'s `private_only` drops each hit whose session
//!   `TurnRunner::place_of` does not read as private. The MCP principal's own
//!   place is private; the memory it reads must be too.
//! - **Get** reads one node by its citation's id with a few of its session's
//!   messages around it, or one session's messages in order, paged by a
//!   cursor. A node of a session that is not private answers as a node that
//!   is not there, so the answer never says that it exists.
//! - **Veiled as `memory.lookup` veils**: `personal` and
//!   `partner-confidential` text, and an imported episode whose labels could
//!   not be read, is never in a hit; the hit says it is veiled.
//! - **Citations**: the node's id, its WAL position, its session and its
//!   as-of minute in the daemon's zone. A client quotes them back.

use std::sync::Arc;

use theseus_memory::recall::Place;
use theseus_protocol::import::ImportSessionsParams;
use theseus_protocol::memory::{MemoryCitation, MemoryHit, MemoryLookupParams, MemoryLookupResult};

use super::read::{clipped, moment, one_line, readable_at, MESSAGE_MAX};
use super::{by_words, catalog, some, veils, Args, Item, BUDGET, PAGE, PAGE_MAX, UNREAD};
use crate::node::{Body, Node};
use crate::Core;

/// The most neighbours a `get` gives on a side, and by default.
const AROUND: u64 = 2;
const AROUND_MAX: u64 = 10;
/// What an answer says of a node or session that is not there, or not
/// private.
const NOT_FOUND: &str = "no remembered node or session is named so: cite an id or session an \
     earlier memory_search answered";
/// A hit's text, never a veiled one's.
fn shown(veiled: bool, sensitivity: Option<&str>, text: &str, cut: usize) -> String {
    match veiled {
        true => format!(
            "[{} · veiled: its text stays veiled here, as on the cockpit's Context page; the owner opens it there]",
            sensitivity.unwrap_or_default().replace('-', " ")
        ),
        false => clipped(&one_line(text), cut),
    }
}

fn as_of(at_ms: u64, fallback: &str) -> String {
    match at_ms {
        0 => fallback.to_string(),
        ms => moment(ms),
    }
}

fn hit_of_item(it: &Item, cut: usize) -> MemoryHit {
    let veiled = veils(it.sensitivity.as_deref());
    MemoryHit {
        citation: MemoryCitation {
            node_id: it.node_id.clone(),
            position: it.position,
            session_id: it.session_id.clone(),
            as_of: as_of(it.at_ms, &it.when),
        },
        origin: origin(&it.session_id),
        harness: it.harness.clone(),
        sensitivity: it.sensitivity.clone(),
        veiled,
        book: it.book.clone(),
        text: shown(veiled, it.sensitivity.as_deref(), &it.text, cut),
        target: false,
    }
}

fn origin(session_id: &str) -> String {
    match crate::import::is_imported(session_id) {
        true => "imported",
        false => "native",
    }
    .into()
}

/// `memory_search`: a page of hits for `words`.
pub(crate) async fn search(
    core: &Arc<Core>,
    p: &MemoryLookupParams,
) -> Result<MemoryLookupResult, String> {
    let Some(words) = some(&p.words) else {
        return Err("memory_search needs words: what to find, as you would say it".into());
    };
    let a = Args {
        words: Some(words.to_string()),
        from: p.since.clone(),
        to: p.until.clone(),
        book: p.book.clone(),
        topic: p.topic.clone(),
        ..Args::default()
    };
    let when = a.span(theseus_protocol::now_unix_ms())?;
    let limit = p.limit.unwrap_or(PAGE).clamp(1, PAGE_MAX);
    let offset = p.cursor.unwrap_or(0);
    // The most the index is asked, whatever the page: a private-only filter
    // may drop some of the best hits, and a total that moved with the page's
    // size would break a cursor.
    let k = 100;
    let (_, items, total) =
        by_words(core, None, words.to_string(), &a, when.as_ref(), (k, true)).await?;
    let cut = MESSAGE_MAX as usize * 4;
    let mut hits = Vec::new();
    let mut used = 0u64;
    for it in items.iter().skip(offset as usize).take(limit as usize) {
        let h = hit_of_item(it, cut);
        let cost = (h.text.len() as u64 + 120) / 4;
        if !hits.is_empty() && used + cost > BUDGET {
            break;
        }
        used += cost;
        hits.push(h);
    }
    let next = offset + hits.len() as u64;
    Ok(MemoryLookupResult {
        next: (next < total).then_some(next),
        total,
        hits,
        left_out: None,
    })
}

/// A session's labels: its harness, sensitivity and book. An imported one's
/// from its episode (labels unread: veiled), the harness's own none.
async fn labels(
    core: &Arc<Core>,
    session: &str,
) -> Result<(String, Option<String>, Option<String>), String> {
    if !crate::import::is_imported(session) {
        return Ok(("theseus".into(), None, None));
    }
    let p = ImportSessionsParams {
        ids: vec![session.to_string()],
        erased: true,
        limit: Some(1),
        ..Default::default()
    };
    let r = catalog(core, p).await?;
    Ok(match r.episodes.into_iter().next() {
        Some(e) => (e.source, Some(e.sensitivity), e.book),
        None => ("imported".into(), Some(UNREAD.into()), None),
    })
}

fn hit_of_node(
    (position, n): (u64, &Node),
    (harness, sensitivity, book): &(String, Option<String>, Option<String>),
    target: bool,
    cut: usize,
) -> MemoryHit {
    let veiled = veils(sensitivity.as_deref());
    MemoryHit {
        citation: MemoryCitation {
            node_id: n.id.clone(),
            position,
            session_id: n.session_id.clone(),
            as_of: moment(n.created_at_ms),
        },
        origin: origin(&n.session_id),
        harness: harness.clone(),
        sensitivity: sensitivity.clone(),
        veiled,
        book: book.clone(),
        text: shown(
            veiled,
            sensitivity.as_deref(),
            &crate::recall::text_of(n),
            cut,
        ),
        target,
    }
}

/// A session's nodes, as `session.history` shows them, if it is a private
/// place's and has any.
async fn nodes_of(core: &Arc<Core>, session: &str) -> Result<Vec<(u64, Node)>, String> {
    if core.runner.place_of(session) != Place::Private {
        return Err(NOT_FOUND.into());
    }
    let c = core.clone();
    let sid = session.to_string();
    let nodes = tokio::task::spawn_blocking(move || {
        let nodes = c.store.session_nodes(&sid)?;
        crate::import::shown(&c.store, nodes)
    })
    .await
    .map_err(|e| e.to_string())?
    .map_err(|e| format!("the session could not be read: {e:#}"))?;
    match nodes.is_empty() {
        true => Err(NOT_FOUND.into()),
        false => Ok(nodes),
    }
}

/// `memory_get`: one node and its neighbours, or a session's page.
pub(crate) async fn get(
    core: &Arc<Core>,
    p: &MemoryLookupParams,
) -> Result<MemoryLookupResult, String> {
    match (some(&p.id), some(&p.session)) {
        (Some(id), None) => by_id(core, id, p).await,
        (None, Some(session)) => by_session(core, session, p).await,
        _ => Err("memory_get takes an id (a citation's node_id) or a session, not both".into()),
    }
}

async fn by_id(
    core: &Arc<Core>,
    id: &str,
    p: &MemoryLookupParams,
) -> Result<MemoryLookupResult, String> {
    let found = core
        .store
        .get_node(id)
        .map_err(|e| format!("the node could not be read: {e:#}"))?;
    let Some((position, node)) = found else {
        return Err(NOT_FOUND.into());
    };
    if matches!(node.body, Body::Erased { .. }) || crate::recall::text_of(&node).trim().is_empty() {
        return Err(NOT_FOUND.into());
    }
    let nodes = nodes_of(core, &node.session_id).await?;
    let labels = labels(core, &node.session_id).await?;
    let (before, after) = (
        p.before.unwrap_or(AROUND).min(AROUND_MAX) as usize,
        p.after.unwrap_or(AROUND).min(AROUND_MAX) as usize,
    );
    let (messages, left_out) = readable_at(&nodes, None);
    // The node itself, a message or not, among the messages by position.
    let mut line: Vec<(u64, &Node)> = messages;
    if !line.iter().any(|(at, _)| *at == position) {
        line.push((position, &node));
        line.sort_by_key(|(at, _)| *at);
    }
    let at = line.iter().position(|(p, _)| *p == position).unwrap_or(0);
    let from = at.saturating_sub(before);
    let cut = MESSAGE_MAX as usize * 4;
    let hits: Vec<MemoryHit> = line[from..(at + after + 1).min(line.len())]
        .iter()
        .map(|(pos, n)| hit_of_node((*pos, n), &labels, *pos == position, cut))
        .collect();
    Ok(MemoryLookupResult {
        total: hits.len() as u64,
        hits,
        next: None,
        left_out,
    })
}

async fn by_session(
    core: &Arc<Core>,
    session: &str,
    p: &MemoryLookupParams,
) -> Result<MemoryLookupResult, String> {
    let nodes = nodes_of(core, session).await?;
    let labels = labels(core, session).await?;
    let (messages, left_out) = readable_at(&nodes, None);
    let total = messages.len() as u64;
    let offset = p.cursor.unwrap_or(0);
    let limit = p.limit.unwrap_or(PAGE).clamp(1, PAGE_MAX);
    let cut = (MESSAGE_MAX as usize * 4).min(BUDGET as usize * 4);
    let mut hits = Vec::new();
    let mut used = 0u64;
    for (pos, n) in messages.iter().skip(offset as usize).take(limit as usize) {
        let h = hit_of_node((*pos, n), &labels, false, cut);
        let cost = (h.text.len() as u64 + 120) / 4;
        if !hits.is_empty() && used + cost > BUDGET {
            break;
        }
        used += cost;
        hits.push(h);
    }
    let next = offset + hits.len() as u64;
    Ok(MemoryLookupResult {
        next: (next < total).then_some(next),
        total,
        hits,
        left_out,
    })
}
